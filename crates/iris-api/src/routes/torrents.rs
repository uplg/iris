use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header};
use axum::response::Response;
use axum::routing::{get, post};
use iris_core::ids::TorrentId;
use iris_core::search::TorrentSource;
use iris_torrent::{TorrentPreview, TorrentSnapshot};
use serde::{Deserialize, Serialize};
use std::io::SeekFrom;
use std::process::Stdio;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::process::Command;
use tokio_util::io::ReaderStream;

use crate::error::{ApiError, ApiResult};
use crate::routes::extract::AuthUser;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/preview", post(preview))
        .route("/", post(ingest).get(list))
        .route("/{infohash}", get(get_one).delete(remove))
        .route(
            "/{infohash}/files/{idx}/stream",
            get(stream_file).head(stream_file),
        )
        .route("/{infohash}/files/{idx}/play", get(play_file).head(play_file))
        .route("/{infohash}/files/{idx}/probe", get(probe_file))
        .route(
            "/{infohash}/files/{idx}/hls/status",
            get(hls_status),
        )
        // Wildcard so the master, the per-variant playlists
        // (`stream_0/playlist.m3u8`, `stream_1/playlist.m3u8`, …) and
        // segments (`stream_0/seg_00001.m4s`) all funnel through the same
        // handler. Path traversal is guarded inside `hls_asset`.
        .route(
            "/{infohash}/files/{idx}/hls/{*asset}",
            get(hls_asset),
        )
        .route(
            "/{infohash}/files/{idx}/sub/{stream_idx}/track.vtt",
            get(subtitle_vtt),
        )
        .route(
            "/{infohash}/files/{idx}/progress",
            get(get_progress).put(put_progress),
        )
        .route("/{infohash}/progress", get(get_torrent_progress))
}

#[derive(Debug, Serialize)]
pub struct FileProgressEntry {
    pub file_idx: i64,
    pub position_seconds: f64,
    pub duration_seconds: Option<f64>,
    pub completed: bool,
    pub last_watched_at: chrono::DateTime<chrono::Utc>,
}

async fn get_torrent_progress(
    State(state): State<AppState>,
    user: AuthUser,
    Path(infohash): Path<String>,
) -> ApiResult<Json<Vec<FileProgressEntry>>> {
    let infohash = infohash.to_ascii_lowercase();
    let rows = iris_db::playback::list_for_torrent(state.db(), user.id, &infohash).await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| FileProgressEntry {
                file_idx: r.file_idx,
                position_seconds: r.position_seconds,
                duration_seconds: r.duration_seconds,
                completed: r.completed,
                last_watched_at: r.last_watched_at,
            })
            .collect(),
    ))
}

#[derive(Debug, Serialize)]
pub struct ProgressView {
    pub position_seconds: f64,
    pub duration_seconds: Option<f64>,
    pub audio_track_idx: Option<i64>,
    pub subtitle_track_idx: Option<i64>,
    pub completed: bool,
    pub last_watched_at: chrono::DateTime<chrono::Utc>,
}

async fn get_progress(
    State(state): State<AppState>,
    user: AuthUser,
    Path((infohash, idx)): Path<(String, usize)>,
) -> ApiResult<Json<Option<ProgressView>>> {
    let infohash = infohash.to_ascii_lowercase();
    let row = iris_db::playback::get(state.db(), user.id, &infohash, idx as i64).await?;
    Ok(Json(row.map(|r| ProgressView {
        position_seconds: r.position_seconds,
        duration_seconds: r.duration_seconds,
        audio_track_idx: r.audio_track_idx,
        subtitle_track_idx: r.subtitle_track_idx,
        completed: r.completed,
        last_watched_at: r.last_watched_at,
    })))
}

#[derive(Debug, Deserialize)]
pub struct ProgressUpdate {
    pub position_seconds: f64,
    pub duration_seconds: Option<f64>,
    pub audio_track_idx: Option<i64>,
    pub subtitle_track_idx: Option<i64>,
    #[serde(default)]
    pub completed: bool,
}

async fn put_progress(
    State(state): State<AppState>,
    user: AuthUser,
    Path((infohash, idx)): Path<(String, usize)>,
    Json(body): Json<ProgressUpdate>,
) -> ApiResult<StatusCode> {
    let infohash = infohash.to_ascii_lowercase();
    if !infohash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ApiError::BadRequest("invalid infohash".into()));
    }
    iris_db::playback::upsert(
        state.db(),
        iris_db::playback::UpsertProgress {
            user_id: user.id,
            infohash,
            file_idx: idx as i64,
            position_seconds: body.position_seconds.max(0.0),
            duration_seconds: body.duration_seconds,
            audio_track_idx: body.audio_track_idx,
            subtitle_track_idx: body.subtitle_track_idx,
            completed: body.completed,
        },
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
pub struct ResolveBody {
    pub provider_id: String,
    pub external_id: String,
    /// Optional TMDB id, captured at search time. We persist it on ingest so
    /// the library/continue-watching endpoints can ship posters without a
    /// fuzzy title-year lookup. Frontends are expected to pass this through
    /// from the search hit when available.
    #[serde(default)]
    pub tmdb_id: Option<i64>,
}

async fn preview(
    State(state): State<AppState>,
    _user: AuthUser,
    Json(body): Json<ResolveBody>,
) -> ApiResult<Json<TorrentPreview>> {
    let provider = state
        .providers()
        .get(&body.provider_id)
        .ok_or_else(|| ApiError::BadRequest(format!("unknown provider `{}`", body.provider_id)))?;
    let source = provider
        .resolve(&body.external_id)
        .await
        .map_err(map_provider_err)?;
    let bytes = match source {
        TorrentSource::TorrentFile(b) => b,
        TorrentSource::Magnet(_) => {
            return Err(ApiError::BadRequest(
                "preview: magnet sources need full ingest first".into(),
            ));
        }
    };
    let preview = iris_torrent::parse_preview(&bytes)
        .map_err(|e| ApiError::BadRequest(format!("torrent parse: {e}")))?;
    Ok(Json(preview))
}

#[derive(Debug, Serialize)]
pub struct IngestResponse {
    pub id: uuid::Uuid,
    pub already_managed: bool,
    pub snapshot: TorrentSnapshot,
}

async fn ingest(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<ResolveBody>,
) -> ApiResult<Json<IngestResponse>> {
    let provider = state
        .providers()
        .get(&body.provider_id)
        .ok_or_else(|| ApiError::BadRequest(format!("unknown provider `{}`", body.provider_id)))?;
    let source = provider
        .resolve(&body.external_id)
        .await
        .map_err(map_provider_err)?;

    let result = match source {
        TorrentSource::TorrentFile(bytes) => {
            state.engine().add_from_bytes(bytes).await
        }
        TorrentSource::Magnet(m) => state.engine().add_from_magnet(&m).await,
    }
    .map_err(|e| ApiError::Internal(anyhow::anyhow!("engine: {e}")))?;

    let row = iris_db::torrents::upsert(
        state.db(),
        iris_db::torrents::NewTorrent {
            infohash: result.snapshot.infohash.clone(),
            name: result.snapshot.name.clone().unwrap_or_else(|| "<unnamed>".into()),
            total_size_bytes: result.snapshot.total_size_bytes,
            source_provider: Some(body.provider_id),
            source_external_id: Some(body.external_id),
            tmdb_id: body.tmdb_id,
            added_by: user.id,
        },
    )
    .await?;

    // Pre-warm HLS on a best-effort background task. By the time the user
    // clicks Play, ffmpeg has already started writing segments — first-frame
    // latency drops from "wait for ffmpeg cold-start" to "single segment
    // round-trip". For multi-file torrents we pick the largest video file as
    // the most likely candidate.
    let prewarm_state = state.clone();
    let prewarm_infohash = result.snapshot.infohash.clone();
    tokio::spawn(async move {
        prewarm_default_hls(&prewarm_state, &prewarm_infohash).await;
    });

    Ok(Json(IngestResponse {
        id: row.id,
        already_managed: result.already_managed,
        snapshot: result.snapshot,
    }))
}

async fn prewarm_default_hls(state: &AppState, infohash: &str) {
    use std::time::Duration;
    // Wait for the *whole torrent* to finish downloading before letting
    // ffmpeg touch it. Running on a sparse / partial source produces a
    // truncated HLS pipeline (playlist references segments ffmpeg never
    // got to write), which then 404s in the player. Up to 30 minutes —
    // beyond that the user can trigger a manual play and we'll start
    // ffmpeg synchronously instead.
    let mut chosen_idx: Option<usize> = None;
    for _ in 0..900 {
        if let Some(snap) = state.engine().get_by_infohash(infohash) {
            if !snap.finished {
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
            let largest_video = snap
                .files
                .iter()
                .filter(|f| {
                    let p = std::path::Path::new(&f.path);
                    matches!(
                        p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref(),
                        Some("mkv" | "mp4" | "webm" | "m4v" | "avi" | "mov" | "ts" | "mts" | "m2ts" | "wmv")
                    )
                })
                .max_by_key(|f| f.size_bytes)
                .map(|f| f.index);
            if let Some(idx) = largest_video {
                let path = match state.engine().file_path(infohash, idx) {
                    Ok(p) => p,
                    Err(_) => return,
                };
                if path.exists() {
                    chosen_idx = Some(idx);
                    break;
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    let Some(idx) = chosen_idx else {
        tracing::debug!(infohash, "prewarm: no video file appeared, skipping");
        return;
    };
    let path = match state.engine().file_path(infohash, idx) {
        Ok(p) => p,
        Err(_) => return,
    };
    let probe = match state.probes().get_or_probe(infohash, idx, &path).await {
        Ok(p) => p,
        Err(e) => {
            tracing::debug!(error = %e, "prewarm: probe failed");
            return;
        }
    };
    verify_tmdb_match(state, infohash, probe.duration_seconds).await;
    let audio_tracks = audio_tracks_for_hls(&probe);
    let key = format!("{infohash}_{idx}");
    if let Err(e) = state
        .hls()
        .ensure_job(&key, &path, &audio_tracks, probe.duration_seconds)
        .await
    {
        tracing::debug!(error = %e, "prewarm: hls ensure_job failed");
        return;
    }
    tracing::info!(
        infohash,
        idx,
        audio_count = audio_tracks.len(),
        "prewarmed multi-audio HLS"
    );
}

/// Tolerance used when matching TMDB's declared runtime against the file's
/// probed duration. 15 % covers minor encode-to-encode drift, intro/outro
/// promo additions, and the minute or two of credits that some releases
/// trim. It does NOT cover director's cuts (often +20-30 %) — those
/// stay marked unverified, which is correct: a director's cut isn't the
/// movie TMDB has metadata for.
const TMDB_RUNTIME_TOLERANCE: f64 = 0.15;

/// Confirm or reject a torrent's `tmdb_id` by matching declared runtime
/// against the file's probed duration. Idempotent: once verified, never
/// re-checked. No-op when `tmdb_id` is missing or the runtime is unknown.
async fn verify_tmdb_match(
    state: &AppState,
    infohash: &str,
    probed_duration_secs: Option<f64>,
) {
    let row = match iris_db::torrents::find_by_infohash(state.db(), infohash).await {
        Ok(Some(r)) => r,
        _ => return,
    };
    if row.tmdb_verified {
        return;
    }
    let Some(tmdb_id) = row.tmdb_id.filter(|id| *id > 0) else { return };
    let Some(probed) = probed_duration_secs.filter(|d| *d > 0.0) else { return };
    let Some(tmdb) = state.tmdb() else { return };
    let Some(meta) = tmdb.lookup(tmdb_id as u64).await else { return };
    let Some(tmdb_minutes) = meta.runtime_minutes.filter(|m| *m > 0) else { return };
    let tmdb_secs = f64::from(tmdb_minutes) * 60.0;
    let diff = (probed - tmdb_secs).abs() / tmdb_secs;
    let verified = diff < TMDB_RUNTIME_TOLERANCE;
    if let Err(e) =
        iris_db::torrents::set_tmdb_verified(state.db(), infohash, verified).await
    {
        tracing::warn!(error = %e, infohash, "tmdb verify: db write failed");
        return;
    }
    tracing::info!(
        infohash,
        tmdb_id,
        probed_secs = probed,
        tmdb_secs,
        diff_pct = diff * 100.0,
        verified,
        "tmdb verification result",
    );
}

/// Convert an ffprobe `audio` list into the `AudioTrack` view that
/// [`iris_media::hls::HlsManager`] expects. We carry over codec, language,
/// title, and the source's default flag so the master playlist can label
/// each rendition correctly.
fn audio_tracks_for_hls(probe: &iris_media::MediaProbe) -> Vec<iris_media::hls::AudioTrack> {
    probe
        .audio
        .iter()
        .map(|a| iris_media::hls::AudioTrack {
            track_idx: a.index as u32,
            codec: a.codec.clone(),
            language: a.language.clone(),
            name: a.title.clone(),
            default: a.default,
        })
        .collect()
}

#[derive(Debug, Serialize)]
pub struct TorrentView {
    pub id: uuid::Uuid,
    pub added_by: uuid::Uuid,
    /// Public display name of the uploader (NOT the email — that stays
    /// private). Backfilled from the email's local-part for accounts
    /// that pre-date migration 0006; users edit it from /account.
    pub added_by_name: String,
    pub added_at: chrono::DateTime<chrono::Utc>,
    pub last_played_at: Option<chrono::DateTime<chrono::Utc>>,
    pub source_provider: Option<String>,
    pub source_external_id: Option<String>,
    pub tmdb_id: Option<i64>,
    /// True only when we've matched the TMDB runtime against the file's
    /// probed duration within ±15 %. Frontends use the `(tmdb_id,
    /// tmdb_verified=true)` pair to decide whether to fetch posters /
    /// titles from TMDB; otherwise they stick with the filename.
    pub tmdb_verified: bool,
    #[serde(flatten)]
    pub snapshot: TorrentSnapshot,
}

async fn list(
    State(state): State<AppState>,
    _user: AuthUser,
) -> ApiResult<Json<Vec<TorrentView>>> {
    let rows = iris_db::torrents::list_active(state.db()).await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        if let Some(snapshot) = state.engine().get_by_infohash(&row.infohash) {
            out.push(TorrentView {
                id: row.id,
                added_by: row.added_by,
                added_by_name: row.added_by_name,
                added_at: row.added_at,
                last_played_at: row.last_played_at,
                source_provider: row.source_provider,
                source_external_id: row.source_external_id,
                tmdb_id: row.tmdb_id,
                tmdb_verified: row.tmdb_verified,
                snapshot,
            });
        }
    }
    Ok(Json(out))
}

async fn get_one(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(infohash): Path<String>,
) -> ApiResult<Json<TorrentView>> {
    let row = iris_db::torrents::find_by_infohash(state.db(), &infohash.to_ascii_lowercase())
        .await?
        .ok_or(ApiError::NotFound)?;
    let snapshot = state
        .engine()
        .get_by_infohash(&row.infohash)
        .ok_or(ApiError::NotFound)?;
    Ok(Json(TorrentView {
        id: row.id,
        added_by: row.added_by,
        added_by_name: row.added_by_name,
        added_at: row.added_at,
        last_played_at: row.last_played_at,
        source_provider: row.source_provider,
        source_external_id: row.source_external_id,
        tmdb_id: row.tmdb_id,
        tmdb_verified: row.tmdb_verified,
        snapshot,
    }))
}

async fn remove(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(infohash): Path<String>,
) -> ApiResult<StatusCode> {
    let row = iris_db::torrents::find_by_infohash(state.db(), &infohash.to_ascii_lowercase())
        .await?
        .ok_or(ApiError::NotFound)?;
    state
        .engine()
        .delete_by_infohash(&row.infohash, true)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("engine delete: {e}")))?;
    state.hls().cleanup_for_torrent(&row.infohash).await;
    iris_db::torrents::soft_delete(state.db(), TorrentId::from(row.id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn probe_file(
    State(state): State<AppState>,
    _user: AuthUser,
    Path((infohash, idx)): Path<(String, usize)>,
) -> ApiResult<Json<iris_media::MediaProbe>> {
    let infohash = infohash.to_ascii_lowercase();
    let _row = iris_db::torrents::find_by_infohash(state.db(), &infohash)
        .await?
        .ok_or(ApiError::NotFound)?;
    let path = state.engine().file_path(&infohash, idx).map_err(map_engine_err)?;
    if !path.exists() {
        return Err(ApiError::BadRequest(format!(
            "file not yet on disk: {}",
            path.display()
        )));
    }
    let probe = state
        .probes()
        .get_or_probe(&infohash, idx, &path)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("ffprobe: {e}")))?;
    Ok(Json(probe))
}

fn map_engine_err(e: iris_torrent::EngineError) -> ApiError {
    match e {
        iris_torrent::EngineError::NotFound => ApiError::NotFound,
        iris_torrent::EngineError::FileOutOfRange => {
            ApiError::BadRequest("file index out of range".into())
        }
        iris_torrent::EngineError::Librqbit(e) => ApiError::Internal(e),
    }
}

/// Non-blocking poll endpoint: starts ffmpeg if it's not running yet, then
/// returns the current state on disk (segment count, ENDLIST present, error
/// flag). The frontend polls this while the player is hidden so the user
/// sees real progress instead of a silent black box.
#[derive(Debug, Serialize)]
pub struct HlsStatus {
    pub ffmpeg_running: bool,
    pub segments_produced: u32,
    pub estimated_total_segments: Option<u32>,
    pub endlist_present: bool,
    pub error: Option<String>,
}

async fn hls_status(
    State(state): State<AppState>,
    _user: AuthUser,
    Path((infohash, idx)): Path<(String, usize)>,
) -> ApiResult<Json<HlsStatus>> {
    let infohash = infohash.to_ascii_lowercase();
    let _row = iris_db::torrents::find_by_infohash(state.db(), &infohash)
        .await?
        .ok_or(ApiError::NotFound)?;
    let path = state
        .engine()
        .file_path(&infohash, idx)
        .map_err(map_engine_err)?;
    if !path.exists() {
        return Err(ApiError::BadRequest(format!(
            "file not yet on disk: {}",
            path.display()
        )));
    }
    // Don't kick ffmpeg while the torrent is still downloading — we'd
    // produce a truncated HLS pipeline (playlist with N segments but
    // each shrinking to nothing as ffmpeg hits EOF on the sparse file).
    // The WatchScreen polls this endpoint as soon as it mounts, and
    // without this guard ffmpeg used to start *during* the download.
    if let Some(snap) = state.engine().get_by_infohash(&infohash) {
        if !snap.finished {
            return Ok(Json(HlsStatus {
                ffmpeg_running: false,
                segments_produced: 0,
                estimated_total_segments: None,
                endlist_present: false,
                error: Some("torrent still downloading".into()),
            }));
        }
    }

    // Cached probe powers both audio mapping and the duration estimate.
    let probe = state
        .probes()
        .get_or_probe(&infohash, idx, &path)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("ffprobe: {e}")))?;
    // Opportunistic TMDB verification — runs at most once per torrent
    // (idempotent on `tmdb_verified=true`), so the cost is negligible.
    verify_tmdb_match(&state, &infohash, probe.duration_seconds).await;
    let audio_tracks = audio_tracks_for_hls(&probe);
    let key = format!("{infohash}_{idx}");

    // Idempotent: starts ffmpeg if not yet running, no-op otherwise.
    state
        .hls()
        .ensure_job(&key, &path, &audio_tracks, probe.duration_seconds)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("hls ensure: {e}")))?;

    // Progress is measured by the **video** variant — audio finishes much
    // faster (smaller files). The video sub-playlist reaching ENDLIST means
    // the whole job is done.
    let segments_produced = state.hls().video_segment_count(&key).await;
    // Strict ENDLIST check: every variant (video + each audio rendition)
    // must be done. A previous version only verified `stream_0` and
    // mounted the player while stream_1/stream_2 were still partial,
    // producing the dreaded "tank with no errors" symptom.
    let endlist_present = state
        .hls()
        .all_endlist_present(&key, audio_tracks.len())
        .await;
    let ffmpeg_running = !endlist_present && state.hls().is_job_active(&key).await;

    // Rough estimate of total segments based on probed duration. Actual
    // segment durations vary (keyframe-aligned), but as a progress-bar
    // denominator this is good enough.
    let estimated_total_segments = probe
        .duration_seconds
        .filter(|d| *d > 0.0)
        .map(|d| (d / 6.0).ceil() as u32);

    Ok(Json(HlsStatus {
        ffmpeg_running,
        segments_produced,
        estimated_total_segments,
        endlist_present,
        error: None,
    }))
}

async fn hls_asset(
    State(state): State<AppState>,
    _user: AuthUser,
    Path((infohash, idx, asset)): Path<(String, usize, String)>,
) -> ApiResult<Response> {
    let infohash = infohash.to_ascii_lowercase();
    tracing::debug!(infohash = %infohash, idx, asset = %asset, "hls_asset enter");

    if !is_safe_hls_asset(&asset) {
        return Err(ApiError::BadRequest("invalid HLS asset path".into()));
    }
    let _row = iris_db::torrents::find_by_infohash(state.db(), &infohash)
        .await?
        .ok_or(ApiError::NotFound)?;
    let path = state
        .engine()
        .file_path(&infohash, idx)
        .map_err(map_engine_err)?;
    let path_exists = tokio::fs::try_exists(&path).await.unwrap_or(false);
    if !path_exists {
        return Err(ApiError::BadRequest(format!(
            "file not yet on disk: {}",
            path.display()
        )));
    }
    // Guard against running ffmpeg on a still-downloading torrent:
    // sparse files segment into nonsense (playlist references seg_00009
    // when only 0..7 are actually written on disk → 404 in the player).
    // We only block the cold path here — if the HLS dir is already
    // clean, [`hls_asset`]'s hot path skipped this check entirely and
    // serves segments straight off disk.
    if let Some(snap) = state.engine().get_by_infohash(&infohash) {
        if !snap.finished {
            return Err(ApiError::BadRequest(
                "torrent still downloading — wait until it's complete to play".into(),
            ));
        }
    }

    let key = format!("{infohash}_{idx}");
    let dir = state.hls().segment_dir_for(&key);
    let asset_path = dir.join(&asset);

    let mime = if asset.ends_with(".m4s") {
        "video/iso.segment"
    } else if asset.ends_with(".mp4") {
        "video/mp4"
    } else if asset.ends_with(".m3u8") {
        "application/vnd.apple.mpegurl"
    } else if asset.ends_with(".ts") {
        "video/mp2t"
    } else {
        "application/octet-stream"
    };

    // Hot path: serve the asset directly when it's on disk and we're
    // not at a validation entry point. The master.m3u8 is the entry
    // point — players fetch it once per session and we use that single
    // request to (re)validate the dir + self-heal corruption.
    // Segments stay hot because by the time the player requests one
    // the dir has already been validated (or freshly re-spawned).
    let is_validation_entry = asset == iris_media::hls::MASTER_PLAYLIST;
    if !is_validation_entry {
        if let Ok(bytes) = tokio::fs::read(&asset_path).await {
            tracing::debug!(asset = %asset, size = bytes.len(), "hls_asset hot serve");
            return Ok(Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime)
                .header(header::CACHE_CONTROL, "no-store")
                .header(header::CONTENT_LENGTH, bytes.len())
                .body(Body::from(bytes))
                .unwrap());
        }
    }
    tracing::debug!(asset = %asset, "hls_asset cold path (validation/missing)");

    // Cold path: file isn't there yet. Kick the ffmpeg job (idempotent if
    // already running) and wait for the asset to appear.
    let probe = state
        .probes()
        .get_or_probe(&infohash, idx, &path)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("ffprobe: {e}")))?;
    let audio_tracks = audio_tracks_for_hls(&probe);
    state
        .hls()
        .ensure_job(&key, &path, &audio_tracks, probe.duration_seconds)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("hls ensure: {e}")))?;

    if asset == iris_media::hls::MASTER_PLAYLIST {
        let _ = iris_db::torrents::touch_played(state.db(), &infohash).await;
        // Master is hand-written by `ensure_job` before ffmpeg spawns,
        // so by this point it's there.
        return serve_static_file(&asset_path, mime).await;
    }

    state
        .hls()
        .wait_for_segment(&key, &asset_path, std::time::Duration::from_secs(120))
        .await
        .map_err(|e| {
            tracing::warn!(asset = %asset, error = %e, "hls segment unavailable");
            ApiError::NotFound
        })?;
    serve_static_file(&asset_path, mime).await
}

/// Validate the relative HLS asset path. The wildcard route captures things
/// like `master.m3u8`, `stream_0/playlist.m3u8`, `stream_0/seg_00001.m4s`.
/// We require:
///   * non-empty
///   * no `..` (path traversal)
///   * no leading `/` (no absolute paths)
///   * no backslashes (Windows-style traversal)
///   * each path segment matches `[A-Za-z0-9._-]+`
///   * at most two path segments deep (master + stream_X dirs)
fn is_safe_hls_asset(asset: &str) -> bool {
    if asset.is_empty() || asset.starts_with('/') || asset.contains("..") || asset.contains('\\') {
        return false;
    }
    let parts: Vec<&str> = asset.split('/').collect();
    if parts.len() > 2 {
        return false;
    }
    parts.iter().all(|seg| {
        !seg.is_empty()
            && seg.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    })
}

async fn serve_static_file(path: &std::path::Path, mime: &str) -> ApiResult<Response> {
    if !path.exists() {
        return Err(ApiError::NotFound);
    }
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("read {}: {e}", path.display())))?;
    let mut resp = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_LENGTH, bytes.len().to_string())
        .header(header::CACHE_CONTROL, "public, max-age=31536000")
        .body(Body::from(bytes))
        .unwrap();
    if mime.ends_with("mpegurl") {
        resp.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    Ok(resp)
}

async fn subtitle_vtt(
    State(state): State<AppState>,
    _user: AuthUser,
    Path((infohash, idx, stream_idx)): Path<(String, usize, u32)>,
) -> ApiResult<Response> {
    let infohash = infohash.to_ascii_lowercase();
    let _row = iris_db::torrents::find_by_infohash(state.db(), &infohash)
        .await?
        .ok_or(ApiError::NotFound)?;
    let path = state
        .engine()
        .file_path(&infohash, idx)
        .map_err(map_engine_err)?;
    if !path.exists() {
        return Err(ApiError::BadRequest("file not yet on disk".into()));
    }

    let cache_dir = state.cfg().storage.data_dir.join("subs");
    let cache_path = iris_media::subtitle_cache_path(&cache_dir, &infohash, idx, stream_idx);

    // Cache hit: serve as static file (instant for the player).
    if cache_path.exists() {
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/vtt; charset=utf-8")
            .header(header::CACHE_CONTROL, "public, max-age=86400")
            .body(Body::from(tokio::fs::read(&cache_path).await.map_err(
                |e| ApiError::Internal(anyhow::anyhow!("read cache: {e}")),
            )?))
            .unwrap());
    }

    // Miss: stream chunks from ffmpeg as it produces them (so Firefox doesn't
    // abort the <track> request) and tee into the cache for next time.
    let stream = iris_media::stream_webvtt(&path, stream_idx, cache_path)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("webvtt: {e}")))?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/vtt; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::TRANSFER_ENCODING, "chunked")
        .body(Body::from_stream(stream))
        .unwrap())
}

async fn stream_file(
    State(state): State<AppState>,
    _user: AuthUser,
    Path((infohash, idx)): Path<(String, usize)>,
    req: Request<Body>,
) -> ApiResult<Response> {
    let infohash = infohash.to_ascii_lowercase();
    let _row = iris_db::torrents::find_by_infohash(state.db(), &infohash)
        .await?
        .ok_or(ApiError::NotFound)?;
    let stream = state
        .engine()
        .open_stream(&infohash, idx)
        .map_err(|e| match e {
            iris_torrent::EngineError::NotFound => ApiError::NotFound,
            iris_torrent::EngineError::FileOutOfRange => {
                ApiError::BadRequest("file index out of range".into())
            }
            iris_torrent::EngineError::Librqbit(e) => ApiError::Internal(e),
        })?;

    // Best-effort: bump the played timestamp.
    let _ = iris_db::torrents::touch_played(state.db(), &infohash).await;

    let mime = guess_mime(&infohash, idx, state.engine());
    let total = stream.file_size();
    let mut reader = stream.into_reader();
    let range = req.headers().get(header::RANGE).cloned();
    let head_only = req.method() == Method::HEAD;

    if let Some(rh) = range.as_ref() {
        match parse_range(rh, total) {
            Some((start, end)) => {
                let len = end - start + 1;
                if head_only {
                    return Ok(build_headers(StatusCode::PARTIAL_CONTENT, len, &mime, Some((start, end, total)))
                        .body(Body::empty())
                        .unwrap());
                }
                if let Err(e) = reader.seek(SeekFrom::Start(start)).await {
                    return Err(ApiError::Internal(anyhow::anyhow!("seek: {e}")));
                }
                let limited = reader.take(len);
                let body = Body::from_stream(ReaderStream::new(limited));
                return Ok(build_headers(StatusCode::PARTIAL_CONTENT, len, &mime, Some((start, end, total)))
                    .body(body)
                    .unwrap());
            }
            None => {
                let mut resp =
                    Response::new(Body::from("invalid range")) ;
                *resp.status_mut() = StatusCode::RANGE_NOT_SATISFIABLE;
                resp.headers_mut().insert(
                    header::CONTENT_RANGE,
                    HeaderValue::from_str(&format!("bytes */{total}")).unwrap(),
                );
                return Ok(resp);
            }
        }
    }

    if head_only {
        return Ok(build_headers(StatusCode::OK, total, &mime, None)
            .body(Body::empty())
            .unwrap());
    }
    let body = Body::from_stream(ReaderStream::new(reader));
    Ok(build_headers(StatusCode::OK, total, &mime, None)
        .body(body)
        .unwrap())
}

/// Browser-friendly playback endpoint.
///
/// - For natively-supported containers (MP4 / WebM) we just delegate to the
///   raw `/stream` handler so HTTP-Range and seeking work as expected.
/// - For MKV (and other "unfriendly" containers carrying browser-friendly
///   codecs — H.264/H.265/VP9/AV1 + AAC/Opus), we remux on the fly with
///   `ffmpeg -c copy` into fragmented MP4 piped to the response body. No
///   re-encode, near-zero CPU. Range/seek isn't honored on the remuxed stream
///   yet — that arrives with the proper M4 ffprobe pipeline.
async fn play_file(
    State(state): State<AppState>,
    user: AuthUser,
    Path((infohash, idx)): Path<(String, usize)>,
    req: Request<Body>,
) -> ApiResult<Response> {
    let infohash = infohash.to_ascii_lowercase();
    let _row = iris_db::torrents::find_by_infohash(state.db(), &infohash)
        .await?
        .ok_or(ApiError::NotFound)?;

    let path = state
        .engine()
        .file_path(&infohash, idx)
        .map_err(|e| match e {
            iris_torrent::EngineError::NotFound => ApiError::NotFound,
            iris_torrent::EngineError::FileOutOfRange => {
                ApiError::BadRequest("file index out of range".into())
            }
            iris_torrent::EngineError::Librqbit(e) => ApiError::Internal(e),
        })?;

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());

    match ext.as_deref() {
        Some("mp4") | Some("m4v") | Some("webm") | Some("ogv") | Some("ogg") => {
            // Browser plays it directly. Reuse the Range-capable streaming path.
            return stream_file(State(state), user, Path((infohash, idx)), req).await;
        }
        Some("mkv") | Some("avi") | Some("mov") | Some("wmv") | Some("ts")
        | Some("mts") | Some("m2ts") => {
            // Remuxable container.
        }
        other => {
            return Err(ApiError::BadRequest(format!(
                "unsupported file extension: {:?}",
                other
            )));
        }
    }

    if !path.exists() {
        return Err(ApiError::BadRequest(format!(
            "file not on disk yet ({}). Wait a moment for the download to start.",
            path.display()
        )));
    }

    let _ = iris_db::torrents::touch_played(state.db(), &infohash).await;

    let head_only = req.method() == Method::HEAD;

    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "video/mp4")
        .header(header::ACCEPT_RANGES, "none")
        .header(header::CACHE_CONTROL, "no-store");

    if head_only {
        return Ok(builder.body(Body::empty()).unwrap());
    }

    let path_str = path.to_string_lossy().into_owned();
    tracing::info!(file = %path_str, "remuxing on the fly via ffmpeg");

    let mut child = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "warning"])
        .args(["-i", &path_str])
        // Map first video + first audio (if present); skip subtitles for now
        // (M4 will expose them as separate WebVTT tracks).
        .args(["-map", "0:v:0", "-map", "0:a:0?"])
        .args(["-c:v", "copy", "-c:a", "copy"])
        // Some MKV-wrapped AAC streams use ADTS framing; this filter normalizes
        // them to ASC so they fit in MP4.
        .args(["-bsf:a", "aac_adtstoasc"])
        .args([
            "-movflags",
            "frag_keyframe+empty_moov+default_base_moof",
        ])
        .args(["-f", "mp4", "pipe:1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("spawn ffmpeg: {e}")))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("ffmpeg stdout missing")))?;
    let stderr = child.stderr.take();

    // Drain stderr to logs without blocking the response.
    if let Some(mut stderr) = stderr {
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut reader = tokio::io::BufReader::new(&mut stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                tracing::debug!(target: "ffmpeg", "{line}");
            }
        });
    }
    // Reap the child when it exits so we don't leak zombies.
    tokio::spawn(async move {
        match child.wait().await {
            Ok(status) if status.success() => tracing::debug!("ffmpeg finished cleanly"),
            Ok(status) => tracing::warn!(?status, "ffmpeg exited non-zero"),
            Err(e) => tracing::warn!(error = %e, "ffmpeg wait failed"),
        }
    });

    if let Some(headers) = builder.headers_mut() {
        headers.insert(
            header::TRANSFER_ENCODING,
            HeaderValue::from_static("chunked"),
        );
    }
    let body = Body::from_stream(ReaderStream::new(stdout));
    Ok(builder.body(body).unwrap())
}

fn build_headers(
    status: StatusCode,
    len: u64,
    mime: &str,
    range: Option<(u64, u64, u64)>,
) -> http::response::Builder {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_str(mime).unwrap());
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from_str(&len.to_string()).unwrap());
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    if let Some((start, end, total)) = range {
        headers.insert(
            header::CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes {start}-{end}/{total}")).unwrap(),
        );
    }
    let mut builder = Response::builder().status(status);
    if let Some(map) = builder.headers_mut() {
        for (k, v) in headers.iter() {
            map.insert(k.clone(), v.clone());
        }
    }
    builder
}

fn parse_range(value: &HeaderValue, total: u64) -> Option<(u64, u64)> {
    if total == 0 {
        return None;
    }
    let s = value.to_str().ok()?;
    let s = s.strip_prefix("bytes=")?;
    // We only honor the first range — most browsers send a single one.
    let part = s.split(',').next()?;
    let (start_s, end_s) = part.split_once('-')?;
    let start_s = start_s.trim();
    let end_s = end_s.trim();
    if start_s.is_empty() && !end_s.is_empty() {
        // "bytes=-N" -> last N bytes
        let n: u64 = end_s.parse().ok()?;
        if n == 0 {
            return None;
        }
        let n = n.min(total);
        return Some((total - n, total - 1));
    }
    let start: u64 = start_s.parse().ok()?;
    let end: u64 = if end_s.is_empty() {
        total - 1
    } else {
        end_s.parse().ok()?
    };
    if start > end || start >= total {
        return None;
    }
    Some((start, end.min(total - 1)))
}

fn guess_mime(infohash: &str, idx: usize, engine: &iris_torrent::Engine) -> String {
    let snapshot = engine.get_by_infohash(infohash);
    let path = snapshot
        .and_then(|s| s.files.into_iter().find(|f| f.index == idx))
        .map(|f| f.path)
        .unwrap_or_default();
    let ext = std::path::Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("mp4") | Some("m4v") => "video/mp4",
        Some("mkv") => "video/x-matroska",
        Some("webm") => "video/webm",
        Some("avi") => "video/x-msvideo",
        Some("mov") => "video/quicktime",
        Some("ts") | Some("mts") | Some("m2ts") => "video/mp2t",
        Some("mp3") => "audio/mpeg",
        Some("flac") => "audio/flac",
        Some("aac") => "audio/aac",
        Some("ogg") | Some("oga") => "audio/ogg",
        Some("srt") => "application/x-subrip",
        _ => "application/octet-stream",
    }
    .to_string()
}

fn map_provider_err(e: iris_core::Error) -> ApiError {
    match e {
        iris_core::Error::NotFound(m) => ApiError::BadRequest(format!("provider: {m}")),
        iris_core::Error::InvalidInput(m) => ApiError::BadRequest(m),
        iris_core::Error::Provider(m) => ApiError::BadRequest(format!("provider: {m}")),
        other => ApiError::Internal(anyhow::anyhow!(other)),
    }
}
