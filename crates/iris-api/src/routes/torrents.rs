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
use std::time::Duration;
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
        // Capability-negotiated entry point. Clients fetch the manifest
        // once, then pick a decode tier; see docs/SOTA_ARCHITECTURE.md.
        .route(
            "/{infohash}/files/{idx}/manifest.json",
            get(manifest_json),
        )
        // Playhead hint → playhead-priority piece prefetch (Phase 1).
        .route("/{infohash}/files/{idx}/seek", post(seek_hint))
        .route(
            "/{infohash}/files/{idx}/playback-error",
            post(playback_error),
        )
        .route(
            "/{infohash}/files/{idx}/sub/{stream_idx}/track.vtt",
            get(subtitle_vtt),
        )
        // ASS/SSA preserved as-is for client-side libass-wasm overlay
        // rendering. Phase 2d.
        .route(
            "/{infohash}/files/{idx}/sub/{stream_idx}/track.ass",
            get(subtitle_ass),
        )
        // PGS bitmap subtitles copied verbatim for client-side libpgs-js
        // overlay rendering. Phase 2d.
        .route(
            "/{infohash}/files/{idx}/sub/{stream_idx}/track.sup",
            get(subtitle_sup),
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
    /// Whether the player is actively playing (vs paused) at the moment of
    /// this heartbeat. Drives the admin "Now watching" presence state.
    /// Additive + optional so legacy clients (which only heartbeat while
    /// playing anyway) default to `Playing`.
    #[serde(default)]
    pub playing: Option<bool>,
}

async fn put_progress(
    State(state): State<AppState>,
    user: AuthUser,
    Path((infohash, idx)): Path<(String, usize)>,
    headers: HeaderMap,
    Json(body): Json<ProgressUpdate>,
) -> ApiResult<StatusCode> {
    let infohash = infohash.to_ascii_lowercase();
    if !infohash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ApiError::BadRequest("invalid infohash".into()));
    }
    let file_idx = file_idx_to_i64(idx);
    let position_seconds = body.position_seconds.max(0.0);
    iris_db::playback::upsert(
        state.db(),
        iris_db::playback::UpsertProgress {
            user_id: user.id,
            infohash: infohash.clone(),
            file_idx,
            position_seconds,
            duration_seconds: body.duration_seconds,
            audio_track_idx: body.audio_track_idx,
            subtitle_track_idx: body.subtitle_track_idx,
            completed: body.completed,
        },
    )
    .await?;

    // Live presence: a completed playback leaves the "now watching" set;
    // any other heartbeat refreshes it.
    if body.completed {
        state.presence().remove(user.id.into()).await;
    } else {
        let parsed = headers
            .get(crate::client_version::CLIENT_HEADER)
            .and_then(|h| h.to_str().ok())
            .and_then(crate::client_version::ClientVersion::parse);
        let client = parsed.as_ref().map(|c| c.kind);
        let client_version = parsed.as_ref().map(|c| c.version.to_string());
        let play_state = if body.playing.unwrap_or(true) {
            crate::presence::PlaybackState::Playing
        } else {
            crate::presence::PlaybackState::Paused
        };
        state
            .presence()
            .touch(crate::presence::Heartbeat {
                user_id: user.id.into(),
                infohash,
                file_idx,
                position_seconds,
                duration_seconds: body.duration_seconds,
                state: play_state,
                client,
                client_version,
            })
            .await;
    }
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

    // Resolve `tmdb_id` from the SCENE-cleaned release name rather than
    // trusting the indexer's value — torr9 mistags Silicon Valley
    // releases with The Burning Bed's id, etc. Server-side cache makes
    // this cheap on repeat ingestions of the same series. Falls back to
    // the indexer's `body.tmdb_id` when the resolver finds nothing.
    let release_name = result
        .snapshot
        .name
        .clone()
        .unwrap_or_else(|| result.snapshot.infohash.clone());
    let resolved_tmdb_id = if let Some(tmdb) = state.tmdb() {
        crate::tmdb_resolve::resolve_release_name(state.db(), tmdb, &release_name, None)
            .await
            .and_then(|r| i64::try_from(r.tmdb_id).ok())
    } else {
        None
    };
    let final_tmdb_id = resolved_tmdb_id.or(body.tmdb_id);

    let row = iris_db::torrents::upsert(
        state.db(),
        iris_db::torrents::NewTorrent {
            infohash: result.snapshot.infohash.clone(),
            name: result.snapshot.name.clone().unwrap_or_else(|| "<unnamed>".into()),
            total_size_bytes: result.snapshot.total_size_bytes,
            source_provider: Some(body.provider_id),
            source_external_id: Some(body.external_id),
            tmdb_id: final_tmdb_id,
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
        let providers = state.providers().clone();
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
                Some(&providers),
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
    // We used to remux to fragmented MP4 here so the web client's
    // Tier F fallback would have a hot cache. With Tier B now
    // handling the vast majority of files via Mediabunny, Tier F is
    // a rare path — and the Android TV client plays the raw
    // `/stream` directly without ever needing the remux. The
    // eager remux on every torrent ingest cost CPU + disk for a
    // cache that was almost always unused.
    //
    // What we KEEP here is the probe + TMDB verification — those
    // are still useful side-effects on ingest (the UI relies on
    // `verify_tmdb_match` for accurate poster / metadata gating
    // when the torrent name's TMDB inference was ambiguous). The
    // remux now runs lazily, only when a client actually requests
    // `/play/master.m3u8` or `/play/status` (see `play_asset` and
    // `play_status` below).
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
    // Both prewarm paths only reach here once the torrent is `finished`
    // (guarded above), so the probe is taken on a complete file.
    let probe = match state.probes().get_or_probe(infohash, idx, &path, true).await {
        Ok(p) => p,
        Err(e) => {
            tracing::debug!(error = %e, "prewarm: probe failed");
            return;
        }
    };
    verify_tmdb_match(state, infohash, probe.duration_seconds).await;
    tracing::info!(infohash, idx, "prewarm: probed (no remux — lazy on first /play hit)");
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
        // Default: stream-copy video. The caps catch-up path flips this
        // to `TranscodeH264` only for clients that genuinely can't
        // decode the source — wired separately (task #4).
        video: iris_media::VideoMode::Copy,
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
    /// `"movie"` / `"tv"` from the parent collection. Clients pass
    /// this to `/api/metadata/tmdb/{id}?kind=` so TMDB's separate
    /// movie / tv namespaces don't collide on poster lookups.
    pub kind: Option<String>,
    /// Parent collection UUID, once `collection_assign` has grouped this
    /// torrent. `None` for orphan torrents (no SCENE identity / not yet
    /// assigned). Additive field — lets a client deep-link a multi-file
    /// torrent straight to its collection page rather than a generic
    /// fallback. Older clients ignore the unknown field.
    pub collection_id: Option<uuid::Uuid>,
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
                kind: row.kind,
                collection_id: row.collection_id,
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
        kind: row.kind,
        collection_id: row.collection_id,
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
    // Cascade the removal into `episode_files`. Soft-deleting the torrent
    // row + dropping the handle + wiping files would otherwise leave the
    // (collection, season, episode) → infohash mappings behind, and the
    // series / collection views would keep listing episodes pointing at a
    // torrent that no longer exists. Re-grabbing the release re-`upsert`s
    // these rows, so a hard delete is safe here.
    if let Err(e) = iris_db::episode_files::delete_for_infohash(state.db(), &row.infohash).await {
        // Best-effort: the self-heal `EXISTS (… deleted_at IS NULL)` filter
        // on the read paths already hides them, so a failed cascade
        // degrades to "invisible but still on disk in the DB", not a
        // user-visible regression. Log and continue with the soft-delete.
        tracing::warn!(error = %e, infohash = %row.infohash, "episode_files cascade delete failed");
    }
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

    // Don't gate playback on the *whole torrent* finishing — a 4 K remux is
    // tens of GB and "click-to-play" must work as soon as the head is here.
    // Mirror `manifest_json`: while the torrent is still downloading, force
    // the bytes ffprobe needs (container header at the front, MKV Cues /
    // MP4 trailing `moov` at the tail) to download now. `prefetch_range`'s
    // sequential priority is per-stream and transient, so this never slows
    // the other torrents (no global first/last-piece priority).
    let snap = state.engine().get_by_infohash(&infohash);
    let torrent_finished = snap.as_ref().is_some_and(|s| s.finished);
    let mut header_bytes: u64 = 0;
    if let Some(snap) = snap.as_ref() {
        if !snap.finished {
            if let Some(file) = snap.files.iter().find(|f| f.index == idx) {
                let header_count: u64 = 1 << 16; // 64 KiB
                let tail_count: u64 = 1 << 20; // 1 MiB
                let tail_start = file.size_bytes.saturating_sub(tail_count);
                let engine = state.engine().clone();
                let ih = infohash.clone();
                let header =
                    engine.prefetch_range(&ih, idx, 0, header_count, Duration::from_secs(30));
                let tail = engine.prefetch_range(
                    &ih,
                    idx,
                    tail_start,
                    tail_count,
                    Duration::from_secs(30),
                );
                let (h, t) = tokio::join!(header, tail);
                match h {
                    Ok(n) => header_bytes = n,
                    Err(e) => tracing::debug!(error = %e, "probe: header prefetch errored"),
                }
                if let Err(e) = t {
                    tracing::debug!(error = %e, "probe: tail prefetch errored");
                }
            }
        }
    }

    // Stalled-swarm guard. If after the prefetch window we still couldn't
    // pull a single header byte AND the torrent isn't finished AND the
    // swarm is dead (no peers, no throughput), there's nothing to read and
    // won't be until a seeder reappears. Returning the retryable
    // "file not yet on disk" here spins the client on
    // "Reading media metadata…" until its retry budget runs out — with no
    // hint *why*. Surface a distinct, non-retryable error that does NOT
    // carry the poll tokens, so the UI can say "no seeders" and stop.
    if !torrent_finished && header_bytes == 0 {
        if let Some(s) = state.engine().get_by_infohash(&infohash) {
            if !s.finished && s.peers == 0 && s.download_speed_bps == 0 {
                return Err(ApiError::Conflict(format!(
                    "stalled: no seeders for this file ({:.0}% downloaded, 0 peers, 0 B/s) — \
                     nothing to read yet",
                    s.progress_pct
                )));
            }
        }
    }

    if !path.exists() {
        return Err(ApiError::BadRequest(format!(
            "file not yet on disk: {}",
            path.display()
        )));
    }
    let probe = state
        .probes()
        .get_or_probe(&infohash, idx, &path, torrent_finished)
        .await
        .map_err(|e| map_probe_err(&e, torrent_finished))?;
    Ok(Json(probe))
}

async fn manifest_json(
    State(state): State<AppState>,
    _user: AuthUser,
    Path((infohash, idx)): Path<(String, usize)>,
) -> ApiResult<Json<iris_media::Manifest>> {
    let infohash = infohash.to_ascii_lowercase();
    if !infohash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ApiError::BadRequest("invalid infohash".into()));
    }
    let _row = iris_db::torrents::find_by_infohash(state.db(), &infohash)
        .await?
        .ok_or(ApiError::NotFound)?;

    let snapshot = state
        .engine()
        .get_by_infohash(&infohash)
        .ok_or(ApiError::NotFound)?;
    let file = snapshot
        .files
        .iter()
        .find(|f| f.index == idx)
        .ok_or_else(|| ApiError::BadRequest("file index out of range".into()))?
        .clone();
    let path = state
        .engine()
        .file_path(&infohash, idx)
        .map_err(map_engine_err)?;

    // Phase 1: probe partial downloads by pre-fetching the byte ranges
    // ffprobe needs (first 8 KiB for any container header, last 1 MiB for
    // MKV Cues / MP4 trailing `moov` / AVI `idx1`). librqbit downloads
    // those pieces to the sparse file on disk, then ffprobe reads real
    // bytes instead of zero-pad. 30s timeout covers the common slow-
    // tracker case; if we time out we still try the probe (it might
    // succeed on the pieces we got, or fail with a useful error).
    let mut header_bytes: u64 = 0;
    if !snapshot.finished {
        let header_count: u64 = 1 << 16; // 64 KiB — generous for any container header
        let tail_count: u64 = 1 << 20;   // 1 MiB
        let tail_start = file.size_bytes.saturating_sub(tail_count);
        let engine = state.engine().clone();
        let infohash_for_prefetch = infohash.clone();
        let header = engine.prefetch_range(
            &infohash_for_prefetch,
            idx,
            0,
            header_count,
            Duration::from_secs(30),
        );
        let tail = engine.prefetch_range(
            &infohash_for_prefetch,
            idx,
            tail_start,
            tail_count,
            Duration::from_secs(30),
        );
        let (h, t) = tokio::join!(header, tail);
        match h {
            Ok(n) => header_bytes = n,
            Err(e) => tracing::debug!(error = %e, "manifest: header prefetch errored"),
        }
        if let Err(e) = t {
            tracing::debug!(error = %e, "manifest: tail prefetch errored");
        }
    }

    // Stalled-swarm guard — same rationale as `probe_file`: a dead torrent
    // (no peers, no throughput, head still unreadable) must surface a
    // distinct non-retryable error instead of an endless not-ready poll.
    if !snapshot.finished && header_bytes == 0 {
        if let Some(s) = state.engine().get_by_infohash(&infohash) {
            if !s.finished && s.peers == 0 && s.download_speed_bps == 0 {
                return Err(ApiError::Conflict(format!(
                    "stalled: no seeders for this file ({:.0}% downloaded, 0 peers, 0 B/s) — \
                     nothing to read yet",
                    s.progress_pct
                )));
            }
        }
    }
    if !path.exists() {
        return Err(ApiError::BadRequest(format!(
            "file not yet on disk: {}",
            path.display()
        )));
    }

    let probe = state
        .probes()
        .get_or_probe(&infohash, idx, &path, snapshot.finished)
        .await
        .map_err(|e| map_probe_err(&e, snapshot.finished))?;

    let file_idx_u32 = u32::try_from(idx)
        .map_err(|_| ApiError::BadRequest("file index too large".into()))?;
    let progress = if snapshot.total_size_bytes > 0 {
        #[allow(clippy::cast_precision_loss)]
        let p = snapshot.progress_bytes as f64 / snapshot.total_size_bytes as f64;
        p.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let manifest = iris_media::build_manifest(
        &probe,
        iris_media::ManifestInputs {
            infohash: &infohash,
            file_idx: file_idx_u32,
            filename: &file.path,
            size_bytes: file.size_bytes,
            download_progress: progress,
            // ranges_complete reporting needs librqbit's piece bitmap;
            // out of scope for Phase 1. The download_progress + bytes
            // figures cover what the UI needs today.
            ranges_complete: Vec::new(),
            bytes_complete: snapshot.progress_bytes.min(file.size_bytes),
        },
        Some(&path),
    )
    .await;

    Ok(Json(manifest))
}

/// Map a probe failure to an HTTP error. While the torrent is still
/// downloading, ffprobe choking on librqbit's zero-filled head (EBML /
/// "invalid as first byte" / "Invalid data found") just means the bytes
/// aren't here yet — surface it as a retryable 400 carrying the
/// "file not yet on disk" token the frontend retry-policy keys on, so
/// click-to-play keeps polling instead of dying on a 500.
///
/// Once the torrent is **finished**, the same ffprobe failure is a
/// genuinely corrupt file — surface it as a real 500 rather than masking
/// it behind an infinite poll (see memory `feedback_no_hide_bad_data`).
fn map_probe_err(e: &iris_media::ProbeError, torrent_finished: bool) -> ApiError {
    let msg = e.to_string();
    let not_ready = msg.contains("file not yet on disk")
        || msg.contains("EBML header parsing failed")
        || msg.contains("Invalid data found when processing input")
        || msg.contains("invalid as first byte");
    // The "not ready, keep polling" state is ONLY valid while the torrent
    // is still downloading. Once it's finished, the same ffprobe failure
    // (zero head / bad EBML) is a genuinely corrupt file — gating the
    // retryable branch on `!torrent_finished` stops the client polling
    // "Reading media metadata…" forever and surfaces the real error
    // (see memory `feedback_no_hide_bad_data`).
    if !torrent_finished && not_ready {
        // Carry BOTH "file not yet on disk" and "download in progress" so
        // any client retry-policy that keys on either phrasing keeps
        // polling (web regex + older shipped APKs — strict no-break-APK
        // discipline, see CLAUDE.md backward-compat).
        ApiError::BadRequest(format!(
            "file not yet on disk: download in progress ({msg})"
        ))
    } else {
        ApiError::Internal(anyhow::anyhow!("ffprobe: {msg}"))
    }
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

/// Playhead hint sent by the client on every user-initiated seek. Phase 1
/// will use it to bias librqbit's piece priority. Phase 0 only logs.
#[derive(Debug, Deserialize)]
pub struct SeekHint {
    pub byte_offset: u64,
    pub playhead_s: Option<f64>,
}

async fn seek_hint(
    State(state): State<AppState>,
    _user: AuthUser,
    Path((infohash, idx)): Path<(String, usize)>,
    Json(body): Json<SeekHint>,
) -> ApiResult<StatusCode> {
    let infohash = infohash.to_ascii_lowercase();
    if !infohash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ApiError::BadRequest("invalid infohash".into()));
    }
    tracing::debug!(
        infohash,
        file_idx = idx,
        byte_offset = body.byte_offset,
        playhead_s = body.playhead_s,
        "seek hint",
    );
    // Spawn the prefetch in the background so the client gets its 204 back
    // immediately; librqbit picks up the priority bias as soon as the read
    // starts. We aim for ~30 seconds of playback ahead — derived from the
    // probed bitrate when we have one, falling back to a flat 64 MiB cap.
    let engine = state.engine().clone();
    let probes = state.probes().clone();
    let infohash_clone = infohash.clone();
    let byte_offset = body.byte_offset;
    tokio::spawn(async move {
        let bytes_ahead = playhead_window_bytes(&engine, &probes, &infohash_clone, idx).await;
        if let Err(e) = engine
            .prefetch_range(
                &infohash_clone,
                idx,
                byte_offset,
                bytes_ahead,
                Duration::from_secs(60),
            )
            .await
        {
            tracing::debug!(error = %e, "seek hint prefetch errored");
        }
    });
    Ok(StatusCode::NO_CONTENT)
}

/// Window of bytes to mark high-priority ahead of the playhead. Targets
/// ~30 seconds of playback by deriving bytes-per-second from the cached
/// probe (total size ÷ duration); falls back to a flat 64 MiB ceiling
/// when the probe is unavailable. Capped at 256 MiB so a long seek on a
/// 4 K HEVC remux doesn't lock librqbit into an impossibly wide window.
async fn playhead_window_bytes(
    engine: &std::sync::Arc<iris_torrent::Engine>,
    probes: &iris_media::ProbeCache,
    infohash: &str,
    idx: usize,
) -> u64 {
    const FALLBACK: u64 = 64 * 1024 * 1024;
    const CAP: u64 = 256 * 1024 * 1024;
    const SECONDS_AHEAD: f64 = 30.0;

    let Some(snapshot) = engine.get_by_infohash(infohash) else {
        return FALLBACK;
    };
    let file_size = snapshot
        .files
        .iter()
        .find(|f| f.index == idx)
        .map_or(0, |f| f.size_bytes);
    if file_size == 0 {
        return FALLBACK;
    }
    let Ok(path) = engine.file_path(infohash, idx) else {
        return FALLBACK;
    };
    let Ok(probe) = probes.get_or_probe(infohash, idx, &path, snapshot.finished).await else {
        return FALLBACK;
    };
    let Some(duration) = probe.duration_seconds.filter(|d| *d > 0.0) else {
        return FALLBACK;
    };
    #[allow(clippy::cast_precision_loss)]
    let bps = file_size as f64 / duration;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let bytes = (bps * SECONDS_AHEAD).round() as u64;
    bytes.clamp(8 * 1024 * 1024, CAP)
}

/// Playback-error report sent by clients when a decode tier fails.
/// Echoes the legacy HLS URL as the fallback and pre-warms the server-
/// side ffmpeg + shaka remux in the background so the client's next
/// request to `/play/master.m3u8` lands on a hot cache.
#[derive(Debug, Deserialize)]
pub struct PlaybackErrorBody {
    pub tier: String,
    pub reason: String,
    #[serde(default)]
    pub codec: Option<String>,
    #[serde(default)]
    pub browser: Option<String>,
    #[serde(default)]
    pub details: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PlaybackErrorResponse {
    pub fallback_tier: &'static str,
    pub fallback_url: String,
}

async fn playback_error(
    State(state): State<AppState>,
    _user: AuthUser,
    Path((infohash, idx)): Path<(String, usize)>,
    Json(body): Json<PlaybackErrorBody>,
) -> ApiResult<Json<PlaybackErrorResponse>> {
    let infohash = infohash.to_ascii_lowercase();
    if !infohash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ApiError::BadRequest("invalid infohash".into()));
    }
    tracing::warn!(
        infohash,
        file_idx = idx,
        tier = %body.tier,
        reason = %body.reason,
        codec = ?body.codec,
        browser = ?body.browser,
        details = ?body.details,
        "client-side playback error → falling back to legacy HLS"
    );

    // Fire-and-forget: prewarm the legacy HLS cache for THIS specific
    // file so the client's `/play/master.m3u8` request lands on a hot
    // ffmpeg+shaka output. Idempotent — `RemuxManager::ensure_remuxed`
    // returns immediately when the cache already exists. Best-effort:
    // we still return the fallback URL even if the prewarm task errors.
    let prewarm_state = state.clone();
    let prewarm_infohash = infohash.clone();
    tokio::spawn(async move {
        prewarm_remux_file(&prewarm_state, &prewarm_infohash, idx).await;
    });

    Ok(Json(PlaybackErrorResponse {
        fallback_tier: "F",
        fallback_url: format!("/api/torrents/{infohash}/files/{idx}/play/master.m3u8"),
    }))
}

/// Prewarm the legacy HLS remux for a specific (`infohash`, `file_idx`).
/// Differs from [`prewarm_default_remux`] in that it targets the file
/// the client just failed on — no need to guess which video is the
/// "main" one. Called on `POST /playback-error`.
async fn prewarm_remux_file(state: &AppState, infohash: &str, idx: usize) {
    let Ok(path) = state.engine().file_path(infohash, idx) else {
        return;
    };
    if !path.exists() {
        tracing::debug!(infohash, idx, "fallback prewarm: file not on disk yet");
        return;
    }
    // If the torrent isn't finished, the client's playback was
    // sparse-streaming. The remux pipeline can't handle partial files,
    // so defer until completion.
    if let Some(snap) = state.engine().get_by_infohash(infohash) {
        if !snap.finished {
            tracing::debug!(
                infohash,
                idx,
                pct = snap.progress_pct,
                "fallback prewarm: deferring until torrent finishes"
            );
            return;
        }
    }
    // Both prewarm paths only reach here once the torrent is `finished`
    // (guarded above), so the probe is taken on a complete file.
    let probe = match state.probes().get_or_probe(infohash, idx, &path, true).await {
        Ok(p) => p,
        Err(e) => {
            tracing::debug!(error = %e, "fallback prewarm: probe failed");
            return;
        }
    };
    let key = format!("{infohash}_{idx}");
    let plan = build_remux_plan(&probe);
    if let Err(e) = state.remuxer().ensure_remuxed(&key, &path, plan).await {
        tracing::warn!(error = %e, infohash, idx, "fallback prewarm: remux failed");
        return;
    }
    tracing::info!(infohash, idx, "fallback prewarm: HLS cache hot");
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
    // Validate we know about this file even though we no longer touch
    // the raw bytes here (the lazy-remux path makes them irrelevant
    // until /play/master.m3u8 is hit).
    let _ = state.engine().file_path(&infohash, idx).map_err(map_engine_err)?;

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

    // `play_status` used to eagerly kick off a background remux here
    // so the web UI's "Tier F ready" gate would flip without the user
    // having to request `/play/master.m3u8`. With the web client now
    // defaulting to Tier B (Mediabunny remux in-browser) and the
    // Android TV client playing raw `/stream` directly, 99 % of
    // sessions never touch Tier F — eagerly remuxing on every status
    // poll was burning server CPU for a fallback path that's rarely
    // exercised. The remux is now strictly lazy: it fires the first
    // time someone actually requests `/play/master.m3u8` (see
    // `play_asset`).
    //
    // We fall through here when the torrent is finished, the master
    // playlist doesn't exist yet, and there's no sticky failure
    // recorded. That's the new `ready: true` for the raw-stream path
    // — Tier A/B/TV can start playing immediately. A client that
    // needs Tier F will hit `/play/master.m3u8` and the remux will
    // spin up at that moment.
    Ok(Json(PlayStatus {
        ready: true,
        reason: None,
        progress: None,
        error: None,
    }))
}

async fn subtitle_vtt(
    State(state): State<AppState>,
    _user: AuthUser,
    Path((infohash, idx, stream_idx)): Path<(String, usize, u32)>,
) -> ApiResult<Response> {
    serve_subtitle(&state, &infohash, idx, stream_idx, iris_media::SubtitleFormat::WebVtt).await
}

async fn subtitle_ass(
    State(state): State<AppState>,
    _user: AuthUser,
    Path((infohash, idx, stream_idx)): Path<(String, usize, u32)>,
) -> ApiResult<Response> {
    serve_subtitle(&state, &infohash, idx, stream_idx, iris_media::SubtitleFormat::Ass).await
}

async fn subtitle_sup(
    State(state): State<AppState>,
    _user: AuthUser,
    Path((infohash, idx, stream_idx)): Path<(String, usize, u32)>,
) -> ApiResult<Response> {
    serve_subtitle(&state, &infohash, idx, stream_idx, iris_media::SubtitleFormat::Sup).await
}

/// Shared subtitle handler used by `track.{vtt,ass,sup}`. Caches per-
/// (`infohash`, `file_idx`, `stream_idx`, format) tuple so the three formats
/// coexist without overwriting each other.
///
/// Two states:
/// - **Torrent finished** + `.ok` sidecar present → serve permanent
///   cache (`max-age=86400`).
/// - **Torrent still downloading** OR no `.ok` sidecar → re-extract
///   fresh, stream to client, do NOT promote to cache. Web client
///   bumps the URL's `?v=` query param on torrent-progress milestones
///   to drive `libass.setTrackByUrl` re-fetches without remounting.
async fn serve_subtitle(
    state: &AppState,
    infohash: &str,
    idx: usize,
    stream_idx: u32,
    format: iris_media::SubtitleFormat,
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

    let torrent_finished = state
        .engine()
        .get_by_infohash(&infohash)
        .is_some_and(|s| s.finished);

    let cache_dir = state.cfg().storage.data_dir.join("subs");
    let cache_path =
        iris_media::subtitle_cache_path(&cache_dir, &infohash, idx, stream_idx, format);
    let marker_path = cache_path.with_extension(format!("{}.ok", format.extension()));

    // Cache hit requires BOTH the file AND its completion marker. The
    // marker was introduced when we discovered ffmpeg silently produces
    // truncated `.ass` outputs on librqbit's sparse source files (exits
    // 0 at the first un-downloaded piece) — pre-marker caches are
    // assumed unsafe and re-extracted.
    if cache_path.exists() && marker_path.exists() {
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, format.mime())
            .header(header::CACHE_CONTROL, "public, max-age=86400")
            .body(Body::from(tokio::fs::read(&cache_path).await.map_err(
                |e| ApiError::Internal(anyhow::anyhow!("read cache: {e}")),
            )?))
            .unwrap());
    }

    let stream = iris_media::stream_subtitle(
        &path,
        stream_idx,
        format,
        cache_path,
        torrent_finished,
    )
    .await
    .map_err(|e| ApiError::Internal(anyhow::anyhow!("subtitle extract: {e}")))?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, format.mime())
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
        .await
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
        // `play_asset` already rejected unfinished torrents above, so
        // the remux always probes a complete file.
        let probe = state
            .probes()
            .get_or_probe(&infohash, idx, &path, true)
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("ffprobe: {e}")))?;
        verify_tmdb_match(&state, &infohash, probe.duration_seconds).await;
        let plan = build_remux_plan(&probe);
        state
            .remuxer()
            .ensure_remuxed(&key, &path, plan)
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("remux: {e}")))?;
        // Two-layer "last play" tracking: the DB row drives torrent-level
        // LRU (which entire seed to evict), the remux sentinel drives
        // cache-entry LRU (which (infohash, file_idx) to trim first when
        // the cache itself is over budget). They serve different windows
        // — a season's torrent might be hot while only the latest episode
        // cache is — so we touch both.
        let _ = iris_db::torrents::touch_played(state.db(), &infohash).await;
        state.remuxer().touch_played(&key).await;
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
