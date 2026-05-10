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
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

use crate::error::{ApiError, ApiResult};
use crate::routes::extract::AuthUser;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/preview", post(preview))
        .route("/", post(ingest).get(list))
        .route("/{infohash}", get(get_one).delete(remove))
        // `/stream` serves the *raw* source file (range-supported). Used by
        // the download button and as the URL for native MKV players.
        .route(
            "/{infohash}/files/{idx}/stream",
            get(stream_file).head(stream_file),
        )
        // Inflight status of the per-file remux job, used by the player UI
        // to render a "preparing…" overlay before the first byte. Listed
        // BEFORE the wildcard `/play/{asset}` so axum routes the literal
        // path here instead of treating "status" as an asset.
        .route("/{infohash}/files/{idx}/play/status", get(play_status))
        // `/play/{asset}` is the HLS-CMAF cache. The player asks for
        // `master.m3u8` first; that call ensures ffmpeg is running and
        // blocks until the master + first fragments are on disk. Every
        // other asset (variant playlists, init segments, `.m4s`) is
        // served as a static file with byte-range support.
        .route(
            "/{infohash}/files/{idx}/play/{asset}",
            get(play_asset).head(play_asset),
        )
        .route("/{infohash}/files/{idx}/probe", get(probe_file))
        .route(
            "/{infohash}/files/{idx}/sub/{stream_idx}/track.vtt",
            get(subtitle_vtt),
        )
        .route(
            // PUT for normal calls; POST is accepted because
            // `navigator.sendBeacon` (used at unload to flush the last
            // playback position) is hard-wired to POST.
            "/{infohash}/files/{idx}/progress",
            get(get_progress).put(put_progress).post(put_progress),
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
    let row = iris_db::playback::get(state.db(), user.id, &infohash, file_idx_to_i64(idx)).await?;
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
            file_idx: file_idx_to_i64(idx),
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

    // Pre-warm the remuxer cache on a best-effort background task. By the
    // time the user clicks Play, the `.fmp4` file is already on disk — the
    // first request hits the cached file directly instead of waiting for
    // a cold ffmpeg run. For multi-file torrents we pick the largest video
    // file as the most likely candidate.
    let prewarm_state = state.clone();
    let prewarm_infohash = result.snapshot.infohash.clone();
    tokio::spawn(async move {
        prewarm_default_remux(&prewarm_state, &prewarm_infohash).await;
    });

    // Collection assignment — pick / create the right `collections`
    // row, attach the torrent, and (for TV) populate `episode_files`
    // from any SCENE-parseable filename. Best-effort, runs in the
    // background so the ingest response isn't blocked.
    {
        let pool = state.db().clone();
        let tmdb = state.tmdb().cloned();
        let infohash = result.snapshot.infohash.clone();
        let name = result.snapshot.name.clone().unwrap_or_default();
        let tmdb_id = body.tmdb_id;
        let files: Vec<(usize, String)> = result
            .snapshot
            .files
            .iter()
            .map(|f| (f.index, f.path.clone()))
            .collect();
        tokio::spawn(async move {
            crate::collection_assign::assign_after_ingest(
                &pool,
                tmdb.as_ref(),
                &infohash,
                &name,
                tmdb_id,
                &files,
            )
            .await;
        });
    }

    Ok(Json(IngestResponse {
        id: row.id,
        already_managed: result.already_managed,
        snapshot: result.snapshot,
    }))
}

async fn prewarm_default_remux(state: &AppState, infohash: &str) {
    use std::time::Duration;
    // Wait for the torrent to finish downloading before letting ffmpeg
    // touch it. Remuxing a sparse source produces a truncated cache
    // that's worse than waiting. Up to 30 minutes — beyond that the
    // user's first manual Play will trigger the remux synchronously.
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
                let Ok(path) = state.engine().file_path(infohash, idx) else {
                    return;
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
    let Ok(path) = state.engine().file_path(infohash, idx) else {
        return;
    };
    // Probe runs the TMDB verification side-effect — useful regardless of
    // whether the remux below succeeds.
    let probe = match state.probes().get_or_probe(infohash, idx, &path).await {
        Ok(p) => p,
        Err(e) => {
            tracing::debug!(error = %e, "prewarm: probe failed");
            return;
        }
    };
    verify_tmdb_match(state, infohash, probe.duration_seconds).await;
    let key = format!("{infohash}_{idx}");
    let plan = build_remux_plan(&probe);
    if let Err(e) = state.remuxer().ensure_remuxed(&key, &path, plan).await {
        tracing::warn!(error = %e, infohash, "prewarm: remux failed");
        return;
    }
    tracing::info!(infohash, idx, "prewarmed fragmented MP4");
}

/// Build the HLS audio-rendition plan from probe data.
///
/// One rendition per source audio. Browser-compatible codecs (AAC / MP3
/// / Opus / Vorbis) are kept as `Copy`; everything else (DTS / AC-3 /
/// E-AC-3 / FLAC / `TrueHD` / PCM / …) is transcoded to stereo AAC. We
/// don't emit "copy + AAC fallback" pairs for the same source — the
/// HLS rendition list IS the user-visible audio menu, so duplicates
/// would confuse without adding signal (browsers wouldn't decode the
/// copy anyway).
///
/// Names disambiguate sources sharing a language tag: `fre`, `fre2`, …
fn build_remux_plan(probe: &iris_media::MediaProbe) -> iris_media::RemuxPlan {
    use iris_media::{AudioCodec, AudioRendition};
    let mut renditions: Vec<AudioRendition> = Vec::new();
    let mut lang_count: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for a in &probe.audio {
        let language = a
            .language
            .as_deref()
            .filter(|l| !l.is_empty())
            .unwrap_or("und")
            .to_ascii_lowercase();
        let n = lang_count.entry(language.clone()).or_insert(0);
        *n += 1;
        let name = if *n == 1 {
            language.clone()
        } else {
            format!("{language}{n}")
        };
        let codec = if a.browser_compatible {
            AudioCodec::Copy
        } else {
            AudioCodec::Aac
        };
        renditions.push(AudioRendition {
            source_idx: a.index,
            codec,
            name,
            language,
            default: false, // set below
        });
    }
    // Mark the first browser-compatible rendition as default; otherwise
    // the first one (any). hls.js / Vidstack pick this on initial load.
    if let Some(first_compat) = renditions
        .iter_mut()
        .find(|r| matches!(r.codec, iris_media::AudioCodec::Copy))
    {
        first_compat.default = true;
    } else if let Some(first) = renditions.first_mut() {
        first.default = true;
    }
    iris_media::RemuxPlan {
        audio: renditions,
        source_video_codec: probe.video.first().map(|v| v.codec.clone()),
        source_duration_secs: probe.duration_seconds,
    }
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
    let Ok(Some(row)) = iris_db::torrents::find_by_infohash(state.db(), infohash).await else {
        return;
    };
    if row.tmdb_verified {
        return;
    }
    let Some(tmdb_id) = row.tmdb_id.filter(|id| *id > 0) else { return };
    let Some(probed) = probed_duration_secs.filter(|d| *d > 0.0) else { return };
    let Some(tmdb) = state.tmdb() else { return };
    // tmdb_id is a positive i64 from the DB; u64::try_from cannot fail here.
    let Ok(tmdb_id_u64) = u64::try_from(tmdb_id) else { return };
    let Some(meta) = tmdb.lookup(tmdb_id_u64).await else { return };
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
    // On successful verification, propagate the now-trusted tmdb_id
    // to the torrent's collection so the UI can pull poster /
    // synopsis. Failures are logged inside enrich_after_verify.
    if verified {
        crate::collection_assign::enrich_after_verify(state.db(), infohash).await;
    }
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
    /// Lifetime upload counter — survives session restarts and GC
    /// evictions, unlike `snapshot.uploaded_bytes`.
    pub uploaded_bytes_total: u64,
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
                uploaded_bytes_total: u64::try_from(row.uploaded_bytes_total).unwrap_or(0),
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
        uploaded_bytes_total: u64::try_from(row.uploaded_bytes_total).unwrap_or(0),
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
    // Capture the final upload delta before the engine drops the torrent —
    // otherwise the bytes uploaded since the last 30 s reconcile tick are
    // lost forever.
    if let Some(snap) = state.engine().get_by_infohash(&row.infohash) {
        let _ = iris_db::torrents::reconcile_uploaded(
            state.db(),
            &row.infohash,
            snap.uploaded_bytes,
        )
        .await;
    }
    state
        .engine()
        .delete_by_infohash(&row.infohash, true)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("engine delete: {e}")))?;
    // Drop every cached fragmented MP4 for this torrent. We don't know the
    // file count from here without going back to the engine snapshot — the
    // GC callback wired up in `iris-api::lib` already does this prefix
    // sweep on the cache dir, so it's enough to soft-delete the row and
    // let the next eviction tick clean up the leftovers.
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
    // librqbit pre-allocates the full target size with zeros and fills
    // in pieces as they arrive. So a freshly added torrent has a path
    // that exists at full size but contains all-zero bytes — ffprobe
    // chokes on this with "EBML header parsing failed". Gate the probe
    // on the engine's `finished` flag so we mirror what `play_status`
    // already does, and return a "not yet on disk" message so the
    // frontend's retry loop keeps polling.
    if let Some(snap) = state.engine().get_by_infohash(&infohash) {
        if !snap.finished {
            return Err(ApiError::BadRequest(format!(
                "file not yet on disk: download in progress ({:.0}%)",
                snap.progress_pct
            )));
        }
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

/// State of the per-file remux job exposed to the player UI. Polled
/// before mounting the `<video>` so we can render a meaningful loading
/// step ("downloading 47 %", "remuxing", "ready").
#[derive(Debug, Serialize)]
pub struct PlayStatus {
    pub ready: bool,
    /// `"downloading"` / `"remuxing"` / `null` when ready or when an
    /// `error` is set instead.
    pub reason: Option<String>,
    /// 0..1. Populated when `reason == "downloading"` (torrent
    /// progress) or when `reason == "remuxing"` (ffmpeg's encoded
    /// position over total duration). Null until the relevant source
    /// has produced its first measurement.
    pub progress: Option<f64>,
    pub error: Option<String>,
}

async fn play_status(
    State(state): State<AppState>,
    _user: AuthUser,
    Path((infohash, idx)): Path<(String, usize)>,
) -> ApiResult<Json<PlayStatus>> {
    let infohash = infohash.to_ascii_lowercase();
    iris_db::torrents::find_by_infohash(state.db(), &infohash)
        .await?
        .ok_or(ApiError::NotFound)?;
    let path = state.engine().file_path(&infohash, idx).map_err(map_engine_err)?;

    if let Some(snap) = state.engine().get_by_infohash(&infohash) {
        if !snap.finished {
            return Ok(Json(PlayStatus {
                ready: false,
                reason: Some("downloading".into()),
                progress: Some((snap.progress_pct / 100.0).clamp(0.0, 1.0)),
                error: None,
            }));
        }
    }

    let key = format!("{infohash}_{idx}");
    let master = state.remuxer().master_path(&key);
    if let Ok(meta) = tokio::fs::metadata(&master).await {
        if meta.is_file() && meta.len() > 0 {
            return Ok(Json(PlayStatus {
                ready: true,
                reason: None,
                progress: None,
                error: None,
            }));
        }
    }

    // Sticky failure short-circuit: once ffmpeg has failed for this source
    // we surface the error and STOP polling-driven respawns. The user can
    // wipe the entry from `/admin` to retry, otherwise the cooldown clears
    // it after a few minutes.
    if let Some(msg) = state.remuxer().recent_failure(&key).await {
        return Ok(Json(PlayStatus {
            ready: false,
            reason: None,
            progress: None,
            error: Some(msg),
        }));
    }

    // No cache yet, no recorded failure. If nothing's running for this key,
    // kick off the remux in the background — otherwise the UI would gate
    // `/play` on `ready: true` forever, and `/play` is the only place that
    // triggers ffmpeg. `ensure_remuxed` deduplicates internally, so even if
    // this poll races with another caller, only one ffmpeg ever runs.
    let in_flight = state
        .remuxer()
        .list_jobs()
        .await
        .iter()
        .any(|j| j.key == key && j.in_flight);
    if !in_flight {
        // Probe is needed to know whether a browser-compatible audio track
        // exists; it's cached per (infohash, idx) so re-running on each
        // status poll only hits ffprobe the first time.
        let remuxer = state.remuxer().clone();
        let probes = state.probes().clone();
        let infohash_owned = infohash.clone();
        let key_owned = key.clone();
        tokio::spawn(async move {
            let probe = match probes.get_or_probe(&infohash_owned, idx, &path).await {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(error = %e, key = %key_owned, "play_status: probe failed");
                    return;
                }
            };
            let plan = build_remux_plan(&probe);
            if let Err(e) = remuxer.ensure_remuxed(&key_owned, &path, plan).await {
                tracing::warn!(error = %e, key = %key_owned, "play_status: background remux failed");
            }
        });
    }
    // Surface ffmpeg's encoded-so-far / total-duration as a 0..1
    // fraction so the loading overlay can show a real progress bar
    // instead of an indeterminate spinner.
    let progress = state.remuxer().progress(&key).await;
    Ok(Json(PlayStatus {
        ready: false,
        reason: Some("remuxing".into()),
        progress,
        error: None,
    }))
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
        if let Some((start, end)) = parse_range(rh, total) {
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
        let mut resp = Response::new(Body::from("invalid range"));
        *resp.status_mut() = StatusCode::RANGE_NOT_SATISFIABLE;
        resp.headers_mut().insert(
            header::CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes */{total}")).unwrap(),
        );
        return Ok(resp);
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

/// Serve any file from the per-source HLS-CMAF cache directory.
///
/// On the master playlist request specifically (`asset == "master.m3u8"`)
/// we ensure the cache is built — probe the source, compute the rendition
/// plan, and block on the remuxer until the master + first fragment of
/// every variant exist on disk. Subsequent asset requests (variant
/// playlists, init segments, `.m4s`) are pure static-file serving with
/// byte-range support; the player only asks for them after parsing the
/// master, by which point everything has been observed by the watcher.
async fn play_asset(
    State(state): State<AppState>,
    _user: AuthUser,
    Path((infohash, idx, asset)): Path<(String, usize, String)>,
    req: Request<Body>,
) -> ApiResult<Response> {
    let infohash = infohash.to_ascii_lowercase();
    let _row = iris_db::torrents::find_by_infohash(state.db(), &infohash)
        .await?
        .ok_or(ApiError::NotFound)?;

    let path = state
        .engine()
        .file_path(&infohash, idx)
        .map_err(map_engine_err)?;

    if let Some(snap) = state.engine().get_by_infohash(&infohash) {
        if !snap.finished {
            return Err(ApiError::BadRequest(
                "torrent still downloading — wait until it's complete to play".into(),
            ));
        }
    }
    if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
        return Err(ApiError::BadRequest(format!(
            "file not on disk: {}",
            path.display()
        )));
    }

    let key = format!("{infohash}_{idx}");

    if asset == iris_media::MASTER_PLAYLIST {
        // Probe runs the TMDB-runtime verification side-effect that the UI
        // relies on for poster / metadata gating. Cached, so cheap on repeat.
        let probe = state
            .probes()
            .get_or_probe(&infohash, idx, &path)
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("ffprobe: {e}")))?;
        verify_tmdb_match(&state, &infohash, probe.duration_seconds).await;
        let plan = build_remux_plan(&probe);
        state
            .remuxer()
            .ensure_remuxed(&key, &path, plan)
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("remux: {e}")))?;
        let _ = iris_db::torrents::touch_played(state.db(), &infohash).await;
    }

    let asset_path = state
        .remuxer()
        .asset_path(&key, &asset)
        .ok_or_else(|| ApiError::BadRequest("invalid asset name".into()))?;
    if !tokio::fs::try_exists(&asset_path).await.unwrap_or(false) {
        return Err(ApiError::NotFound);
    }

    let mime = guess_hls_mime(&asset);
    let method = req.method().clone();
    let range = req.headers().get(header::RANGE).cloned();
    // expected_total = 0 → use actual file size; HLS players ask only for
    // ranges already advertised in the playlist, no need to lie about
    // the total like the old single-file progressive setup did.
    let mut resp = serve_file_with_range(&asset_path, mime, method, range, 0).await?;
    // Cache policy. Playlists MUST NOT be cached — in EVENT mode the
    // master + variant playlists are rewritten as ffmpeg appends new
    // segments, and a cached stale master broke us once already
    // (browser served an old `CODECS="hvc1.2.4.L120.B01,..."` even
    // after we'd simplified the on-disk file). Segments / init
    // segments are content-addressed by name and never change once
    // produced, so they're safe to cache aggressively.
    let is_playlist = std::path::Path::new(&asset)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("m3u8"));
    let cache_control = if is_playlist {
        "no-store"
    } else {
        "public, max-age=604800, immutable"
    };
    resp.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(cache_control));
    Ok(resp)
}

fn guess_hls_mime(asset: &str) -> &'static str {
    let ext = std::path::Path::new(asset)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("m3u8") => "application/vnd.apple.mpegurl",
        Some("ts") => "video/mp2t",
        // CMAF fragment + init segments — `video/iso.segment` is the
        // spec MIME for `.m4s`, `video/mp4` is what every player we
        // care about expects.
        Some("m4s" | "mp4") => "video/mp4",
        _ => "application/octet-stream",
    }
}

/// Open `path`, honour `Range`/`HEAD`, and stream the response body.
/// Used for both the raw source (`/stream`) and the cached fMP4
/// (`/play`). Read-ahead is implicit through [`tokio::fs::File`] +
/// [`ReaderStream`].
///
/// `expected_total` lets the caller advertise a `Content-Length` /
/// `Content-Range` total larger than what's currently on disk — needed
/// for the cached fMP4 path where ffmpeg keeps appending while we
/// serve. Pass `0` to fall back to the actual file size (used by the
/// raw-source `/stream` route, which always serves complete files).
async fn serve_file_with_range(
    path: &std::path::Path,
    mime: &str,
    method: Method,
    range: Option<HeaderValue>,
    expected_total: u64,
) -> ApiResult<Response> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("open {}: {e}", path.display())))?;
    let actual = file
        .metadata()
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("stat {}: {e}", path.display())))?
        .len();
    // Advertise the larger of the estimate and the actual size — the
    // browser uses this for the timeline, so reporting the partial size
    // would freeze the seekable region at the moment-of-first-request.
    let total = expected_total.max(actual);
    let head_only = method == Method::HEAD;

    if let Some(rh) = range.as_ref() {
        if let Some((start, end)) = parse_range(rh, total) {
            // If the requested range starts past what's currently on disk
            // (typical when the player resumes at a saved time deep into
            // a still-encoding file), long-poll for ffmpeg to catch up
            // rather than 416-ing — the browser treats an immediate 416
            // on the initial GET as a fatal media error.
            //
            // Cap the wait at 60s so we never hold the connection longer
            // than typical fetch timeouts. If ffmpeg can't catch up in
            // time the client is told via 416 and can retry; meanwhile
            // playback from earlier positions continues to work.
            let mut actual = actual;
            if start >= actual {
                actual = wait_for_size(path, start + 1, std::time::Duration::from_secs(60))
                    .await
                    .unwrap_or(actual);
            }
            if start >= actual {
                let mut resp = Response::new(Body::from("range past current EOF"));
                *resp.status_mut() = StatusCode::RANGE_NOT_SATISFIABLE;
                resp.headers_mut().insert(
                    header::CONTENT_RANGE,
                    HeaderValue::from_str(&format!("bytes */{total}")).unwrap(),
                );
                return Ok(resp);
            }
            // Clip the response to what's actually written. The player
            // will issue a follow-up range for the remaining bytes once
            // ffmpeg has written more.
            let effective_end = end.min(actual - 1);
            let len = effective_end - start + 1;
            if head_only {
                return Ok(build_headers(
                    StatusCode::PARTIAL_CONTENT,
                    len,
                    mime,
                    Some((start, effective_end, total)),
                )
                .body(Body::empty())
                .unwrap());
            }
            if let Err(e) = file.seek(SeekFrom::Start(start)).await {
                return Err(ApiError::Internal(anyhow::anyhow!("seek: {e}")));
            }
            let body = Body::from_stream(ReaderStream::new(file.take(len)));
            return Ok(build_headers(
                StatusCode::PARTIAL_CONTENT,
                len,
                mime,
                Some((start, effective_end, total)),
            )
            .body(body)
            .unwrap());
        }
        let mut resp = Response::new(Body::from("invalid range"));
        *resp.status_mut() = StatusCode::RANGE_NOT_SATISFIABLE;
        resp.headers_mut().insert(
            header::CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes */{total}")).unwrap(),
        );
        return Ok(resp);
    }

    if head_only {
        return Ok(build_headers(StatusCode::OK, total, mime, None)
            .body(Body::empty())
            .unwrap());
    }
    let body = Body::from_stream(ReaderStream::new(file));
    Ok(build_headers(StatusCode::OK, total, mime, None)
        .body(body)
        .unwrap())
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
        for (k, v) in &headers {
            map.insert(k.clone(), v.clone());
        }
    }
    builder
}

/// Poll `path` until its size is at least `min_size` or `timeout` elapses.
/// Returns the latest observed size (which may still be below `min_size`
/// if the wait timed out — caller decides what to do then). Used to let
/// byte-range requests for forward seeks long-poll while ffmpeg is still
/// writing the cache file.
async fn wait_for_size(
    path: &std::path::Path,
    min_size: u64,
    timeout: std::time::Duration,
) -> std::io::Result<u64> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let size = tokio::fs::metadata(path).await?.len();
        if size >= min_size {
            return Ok(size);
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(size);
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
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
        .map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("mp4" | "m4v") => "video/mp4",
        Some("mkv") => "video/x-matroska",
        Some("webm") => "video/webm",
        Some("avi") => "video/x-msvideo",
        Some("mov") => "video/quicktime",
        Some("ts" | "mts" | "m2ts") => "video/mp2t",
        Some("mp3") => "audio/mpeg",
        Some("flac") => "audio/flac",
        Some("aac") => "audio/aac",
        Some("ogg" | "oga") => "audio/ogg",
        Some("srt") => "application/x-subrip",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// Cast a usize file index from the URL path to the i64 the DB stores.
/// Saturates at `i64::MAX` for absurd inputs — the matching DB lookup
/// will then miss, which is the right behaviour.
fn file_idx_to_i64(idx: usize) -> i64 {
    i64::try_from(idx).unwrap_or(i64::MAX)
}

fn map_provider_err(e: iris_core::Error) -> ApiError {
    match e {
        iris_core::Error::NotFound(m) | iris_core::Error::Provider(m) => {
            ApiError::BadRequest(format!("provider: {m}"))
        }
        iris_core::Error::InvalidInput(m) => ApiError::BadRequest(m),
        other => ApiError::Internal(anyhow::anyhow!(other)),
    }
}
