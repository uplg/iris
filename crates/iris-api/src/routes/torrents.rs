use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header};
use axum::response::Response;
use axum::routing::{get, post};
use iris_core::ids::TorrentId;
use iris_core::search::{MediaKind, TorrentSource};
use iris_torrent::{TorrentPreview, TorrentSnapshot};
use serde::{Deserialize, Serialize};
use std::io::SeekFrom;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;
use utoipa::ToSchema;

/// Chunk size for streamed response bodies. `ReaderStream::new` defaults
/// to 4 KiB reads — far too small for video: a 4K REMUX needs sustained
/// tens of Mbps and the per-chunk overhead (librqbit piece lookup +
/// hyper frame + TLS record per 4 KiB) caps real throughput below the
/// film's bitrate, so the player drains its buffer, stalls, refills and
/// resumes in a loop. 256 KiB amortises that overhead and stays within
/// a single typical torrent piece.
const STREAM_CHUNK_SIZE: usize = 256 * 1024;

use crate::error::{ApiError, ApiResult};
use crate::routes::extract::{AuthUser, Infohash, Path};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/preview", post(preview))
        .route("/", post(ingest).get(list))
        .route("/{infohash}", get(get_one).delete(remove))
        .route("/{infohash}/pause", post(pause))
        .route("/{infohash}/resume", post(resume))
        .route("/{infohash}/regrab", post(regrab))
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
        .route("/{infohash}/files/{idx}/manifest.json", get(manifest_json))
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
            get(get_progress)
                .put(put_progress)
                .post(put_progress)
                .delete(delete_progress),
        )
        .route(
            "/{infohash}/files/{idx}/progress/complete",
            post(mark_watched),
        )
        .route("/{infohash}/progress", get(get_torrent_progress))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct FileProgressEntry {
    pub file_idx: i64,
    pub position_seconds: f64,
    pub duration_seconds: Option<f64>,
    pub completed: bool,
    pub last_watched_at: chrono::DateTime<chrono::Utc>,
}

#[utoipa::path(
    get,
    path = "/api/torrents/{infohash}/progress",
    params(("infohash" = String, Path)),
    responses((status = 200, description = "Per-file watch progress for the caller", body = [FileProgressEntry])),
    tag = "torrents",
)]
pub(crate) async fn get_torrent_progress(
    State(state): State<AppState>,
    user: AuthUser,
    Path(infohash): Path<Infohash>,
) -> ApiResult<Json<Vec<FileProgressEntry>>> {
    let infohash = infohash.into_inner();
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

#[derive(Debug, Serialize, ToSchema)]
pub struct ProgressView {
    pub position_seconds: f64,
    pub duration_seconds: Option<f64>,
    pub audio_track_idx: Option<i64>,
    pub subtitle_track_idx: Option<i64>,
    pub completed: bool,
    pub last_watched_at: chrono::DateTime<chrono::Utc>,
}

#[utoipa::path(
    get,
    path = "/api/torrents/{infohash}/files/{idx}/progress",
    params(("infohash" = String, Path), ("idx" = i32, Path)),
    responses((status = 200, description = "Saved playback position for this file (null if none)", body = ProgressView)),
    tag = "torrents",
)]
pub(crate) async fn get_progress(
    State(state): State<AppState>,
    user: AuthUser,
    Path((infohash, idx)): Path<(Infohash, usize)>,
) -> ApiResult<Json<Option<ProgressView>>> {
    let infohash = infohash.into_inner();
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

#[derive(Debug, Deserialize, ToSchema)]
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
    /// The player is waiting for data (buffering, seeking) while meant to
    /// play. Additive: clients that never say so read as playing/paused.
    #[serde(default)]
    pub buffering: Option<bool>,
    /// True when this update follows a deliberate user seek. Lets the
    /// reset guard below distinguish "user restarted the film from 0"
    /// (persist it) from "player error-recovered at position 0" (must
    /// NOT clobber the real progress). Additive — legacy clients omit
    /// it and get the guard's conservative behavior.
    #[serde(default)]
    pub seek: bool,
}

/// Reset-guard window: a save below `NEW_MAX` landing on stored progress
/// above `PREV_MIN` without a user seek is treated as an error-recovery
/// artifact, not a real position.
const PROGRESS_RESET_GUARD_NEW_MAX_SECS: f64 = 60.0;
const PROGRESS_RESET_GUARD_PREV_MIN_SECS: f64 = 300.0;

#[utoipa::path(
    put,
    path = "/api/torrents/{infohash}/files/{idx}/progress",
    params(("infohash" = String, Path), ("idx" = i32, Path)),
    request_body = ProgressUpdate,
    responses((status = 204, description = "Progress saved")),
    tag = "torrents",
)]
pub(crate) async fn put_progress(
    State(state): State<AppState>,
    user: AuthUser,
    Path((infohash, idx)): Path<(Infohash, usize)>,
    headers: HeaderMap,
    Json(body): Json<ProgressUpdate>,
) -> ApiResult<StatusCode> {
    let infohash = infohash.into_inner();
    let file_idx = file_idx_to_i64(idx);
    let position_seconds = body.position_seconds.max(0.0);

    // Reset guard: progress must never be lost without a voluntary seek.
    // A player recovering from a mid-play error (network cut, engine
    // remount) can come back up at position 0 and heartbeat from there —
    // persisting that would destroy an hour of real progress. A near-zero
    // position over substantial stored progress is only honored when the
    // client flags it as a deliberate seek (rewatch of a completed file is
    // exempt: `prev.completed` short-circuits the guard).
    let reset_guarded = !body.seek
        && !body.completed
        && position_seconds < PROGRESS_RESET_GUARD_NEW_MAX_SECS
        && match iris_db::playback::get(state.db(), user.id, &infohash, file_idx).await {
            Ok(Some(prev)) => {
                !prev.completed && prev.position_seconds >= PROGRESS_RESET_GUARD_PREV_MIN_SECS
            }
            _ => false,
        };
    if reset_guarded {
        tracing::info!(
            %infohash,
            file_idx,
            position_seconds,
            "progress reset guard: ignoring near-zero save over substantial progress"
        );
    } else {
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
    }

    // Keep both eviction clocks warm for the whole session, not just its
    // first second. `touch_played` (DB row → disk-GC active window) and the
    // remux `.last_played` sentinel (cache LRU + protection window) were
    // historically only bumped on the `master.m3u8` fetch at playback start,
    // so a viewer one hour into a film looked "cold" to both evictors and
    // could lose the cache — or the whole torrent — mid-play. The heartbeat
    // is the proof someone still has the player open (paused included).
    let _ = iris_db::torrents::touch_played(state.db(), &infohash).await;
    state.remuxer().touch_played_file(&infohash, idx).await;

    // "Moved on to the next episode" ⇒ the one before it is done. Skipping the
    // credits and jumping to the next episode otherwise leaves the prior one
    // stuck at e.g. 97 % (the `position >= duration - 30s` completion rule
    // never fires for a long outro). Server-side so web + TV both get it.
    // Best-effort: a failure here must never fail the progress save.
    if let Err(e) =
        iris_db::playback::complete_previous_episode(state.db(), user.id, &infohash, file_idx).await
    {
        tracing::debug!(error = %e, "complete_previous_episode failed");
    }

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
        let play_state = if !body.playing.unwrap_or(true) {
            crate::presence::PlaybackState::Paused
        } else if body.buffering.unwrap_or(false) {
            crate::presence::PlaybackState::Buffering
        } else {
            crate::presence::PlaybackState::Playing
        };
        let browser = (client == Some(crate::client_version::ClientKind::Web))
            .then(|| {
                headers
                    .get(axum::http::header::USER_AGENT)
                    .and_then(|h| h.to_str().ok())
                    .and_then(crate::presence::browser_of)
            })
            .flatten();
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
                browser,
            })
            .await;
    }
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    delete,
    path = "/api/torrents/{infohash}/files/{idx}/progress",
    params(("infohash" = String, Path), ("idx" = i32, Path)),
    responses((status = 204, description = "Removed from the caller's Continue Watching / history")),
    tag = "torrents",
)]
pub(crate) async fn delete_progress(
    State(state): State<AppState>,
    user: AuthUser,
    Path((infohash, idx)): Path<(Infohash, usize)>,
) -> ApiResult<StatusCode> {
    let infohash = infohash.into_inner();
    iris_db::playback::delete(state.db(), user.id, &infohash, file_idx_to_i64(idx)).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/torrents/{infohash}/files/{idx}/progress/complete",
    params(("infohash" = String, Path), ("idx" = i32, Path)),
    responses((status = 204, description = "Marked watched for the caller")),
    tag = "torrents",
)]
pub(crate) async fn mark_watched(
    State(state): State<AppState>,
    user: AuthUser,
    Path((infohash, idx)): Path<(Infohash, usize)>,
) -> ApiResult<StatusCode> {
    let infohash = infohash.into_inner();
    iris_db::playback::mark_completed(state.db(), user.id, &infohash, file_idx_to_i64(idx)).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ResolveBody {
    pub provider_id: String,
    pub external_id: String,
    /// Ignored, kept for shipped clients. Clients send their own guess
    /// here, so the server records the tracker's id from the search it
    /// served instead (`ProviderRegistry::tracker_tmdb_id`).
    #[serde(default)]
    pub tmdb_id: Option<i64>,
    /// Explicit consent to ingest a movie whose collection already holds a
    /// live copy. Absent/false → the duplicate guard answers
    /// `409 duplicate_in_library`; the client confirms with the user and
    /// retries with `true`. Additive — old clients simply hit the guard.
    #[serde(default)]
    pub allow_duplicate: bool,
}

#[utoipa::path(
    post,
    path = "/api/torrents/preview",
    request_body = ResolveBody,
    responses(
        (status = 200, description = "Parsed torrent metadata (no ingest)", body = TorrentPreview),
        (status = 400, description = "Unknown provider, magnet source, or parse error"),
    ),
    tag = "torrents",
)]
pub(crate) async fn preview(
    State(state): State<AppState>,
    _user: AuthUser,
    Json(body): Json<ResolveBody>,
) -> ApiResult<Json<TorrentPreview>> {
    let provider = state.provider(&body.provider_id)?;
    // The preview needs the `.torrent` too, so a full slot breaks the dialog
    // exactly like it breaks the grab — same guard, same message.
    check_leech_slots(&state, &body.provider_id).await?;
    let source = resolve_release(&state, &provider, &body.provider_id, &body.external_id).await?;
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

#[derive(Debug, Serialize, ToSchema)]
pub struct IngestResponse {
    pub id: uuid::Uuid,
    pub already_managed: bool,
    pub snapshot: TorrentSnapshot,
}

#[utoipa::path(
    post,
    path = "/api/torrents",
    operation_id = "ingest_torrent",
    request_body = ResolveBody,
    responses((status = 200, description = "Ingested or already-managed torrent", body = IngestResponse)),
    tag = "torrents",
)]
pub(crate) async fn ingest(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<ResolveBody>,
) -> ApiResult<Json<IngestResponse>> {
    Ok(Json(
        ingest_core(
            &state,
            user.id,
            body.provider_id,
            body.external_id,
            body.allow_duplicate,
        )
        .await?,
    ))
}

/// Re-ingest a release the engine no longer serves, from the provenance
/// recorded at first grab (`source_provider` + `source_external_id`).
/// Same release → same infohash, so saved playback positions apply
/// again untouched. Deliberately reads the torrent row WITHOUT a
/// `deleted_at` filter — resurrecting a GC-reclaimed release is the
/// whole point, and `torrents::upsert` inside the ingest clears the
/// soft-delete. The dead player page ("no longer on disk") calls this
/// so the user can grab & play in place instead of hunting the release
/// down through the library.
#[utoipa::path(
    post,
    path = "/api/torrents/{infohash}/regrab",
    operation_id = "regrab_torrent",
    params(("infohash" = String, Path)),
    responses(
        (status = 200, description = "Re-ingested (or already-managed) release", body = IngestResponse),
        (status = 400, description = "No provenance recorded — re-grab via search instead"),
        (status = 404, description = "Unknown infohash"),
    ),
    tag = "torrents",
)]
pub(crate) async fn regrab(
    State(state): State<AppState>,
    user: AuthUser,
    Path(infohash): Path<Infohash>,
) -> ApiResult<Json<IngestResponse>> {
    let row = torrent_or_404(&state, &infohash).await?;
    let (Some(provider_id), Some(external_id)) = (row.source_provider, row.source_external_id)
    else {
        return Err(ApiError::BadRequest(
            "release has no recorded provenance — re-grab it from search".into(),
        ));
    };
    // Resurrecting a specific known release is explicit intent — the
    // duplicate guard must not block a ghost-resume next to a live copy.
    Ok(Json(
        ingest_core(&state, user.id, provider_id, external_id, true).await?,
    ))
}

/// Is this release a dead torrent (a confirmed 0 seeders)? Re-checks live via
/// the provider's details endpoint right before a grab. Only a confirmed 0
/// blocks — 1 seeder is often plenty, and providers that don't expose
/// `details()` / seeders can't be verified so they're allowed through.
async fn is_dead(provider: &Arc<dyn iris_providers::SearchProvider>, external_id: &str) -> bool {
    matches!(provider.details(external_id).await, Ok(Some(d)) if d.seeders == Some(0))
}

/// Refuse a grab the tracker would refuse anyway, before asking it.
///
/// A tracker that caps concurrent downloads (see
/// [`iris_providers::registry::ProviderPolicy::leech_slots`]) answers a
/// `.torrent` request over the cap with a redirect our JSON `Accept` turns
/// into `401 Unauthenticated` — unreadable, and it lands on the preview
/// dialog, which needs the `.torrent` to list files. Counting our own
/// in-progress torrents for that provider gets the user a message naming
/// what holds the slot instead.
///
/// Only *live* unfinished torrents count: a paused one has announced
/// `stopped` on the way out, so the tracker no longer counts it either.
/// [`check_leech_slots`] for a grab, holding the provider's slot lock: the
/// caller keeps the guard until the torrent has its library row, so two
/// grabs can't both see a free slot before either one counts. `None` for an
/// uncapped provider.
pub(crate) async fn take_leech_slot(
    state: &AppState,
    provider_id: &str,
) -> ApiResult<Option<tokio::sync::OwnedMutexGuard<()>>> {
    static LOCKS: std::sync::LazyLock<
        std::sync::Mutex<std::collections::HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    > = std::sync::LazyLock::new(Default::default);
    if state.providers().leech_slots(provider_id).is_none() {
        return Ok(None);
    }
    let lock = LOCKS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entry(provider_id.to_owned())
        .or_default()
        .clone();
    let guard = lock.lock_owned().await;
    check_leech_slots(state, provider_id).await?;
    Ok(Some(guard))
}

pub(crate) async fn check_leech_slots(state: &AppState, provider_id: &str) -> ApiResult<()> {
    let Some(slots) = state.providers().leech_slots(provider_id) else {
        return Ok(());
    };
    let unfinished: std::collections::HashMap<String, iris_torrent::TorrentSnapshot> = state
        .engine()
        .list()
        .into_iter()
        .filter(|s| !s.finished && s.state != iris_torrent::TorrentState::Paused)
        .map(|s| (s.infohash.clone(), s))
        .collect();
    if unfinished.is_empty() {
        return Ok(());
    }
    let holding: Vec<String> = iris_db::torrents::list_active(state.db())
        .await?
        .into_iter()
        .filter(|row| row.source_provider.as_deref() == Some(provider_id))
        // Stamped finished: complete on disk, whatever the snapshot says
        // during the post-restart re-check.
        .filter(|row| row.finished_at.is_none())
        .filter_map(|row| unfinished.get(&row.infohash).map(|snap| (row, snap)))
        .map(|(row, snap)| format!("{} ({:.0}%)", row.name, snap.progress_pct))
        .collect();
    if u32::try_from(holding.len()).unwrap_or(u32::MAX) < slots {
        return Ok(());
    }
    let plural = if slots == 1 { "" } else { "s" };
    Err(ApiError::ProviderSlotLimit(format!(
        "`{provider_id}` allows {slots} download{plural} at a time. In progress: {}. Finish or remove it before grabbing another one.",
        holding.join(", "),
    )))
}

/// Resolve a release to a [`TorrentSource`], preferring the persisted
/// pre-signed `.torrent` URL recorded in the discovery catalogue
/// (restart/cache-proof) over the provider's in-memory link cache. The link
/// cache is cold after a restart and FIFO-evicts under load, which broke
/// c411/torznab catalogue previews + grabs ("no cached download URL …").
/// Search hits (not in the catalogue) fall straight through to `resolve()`,
/// whose cache is hot from the search the user just ran.
async fn resolve_release(
    state: &AppState,
    provider: &Arc<dyn iris_providers::SearchProvider>,
    provider_id: &str,
    external_id: &str,
) -> ApiResult<TorrentSource> {
    if let Some(url) =
        iris_db::catalog::download_url_for(state.db(), provider_id, external_id).await?
    {
        match provider.fetch_source(&url).await {
            Ok(source) => return Ok(source),
            Err(e) => tracing::warn!(
                url = %iris_providers::url_origin(&url),
                provider = provider_id,
                error = %e,
                "catalog download_url fetch failed; falling back to resolve()",
            ),
        }
    }
    provider
        .resolve(external_id)
        .await
        .map_err(map_provider_err)
}

/// Pre-engine guards, on the parsed `.torrent` (magnets skip both —
/// their file list only exists after metadata exchange, and every
/// private-tracker provider hands us file bytes):
///
/// * RAR'd scene release — nothing Iris could ever stream. Refuse
///   outright: it never enters the engine, so no download, no seed.
/// * Same movie already in the library under another live release —
///   `409 duplicate_in_library` unless the caller consented via
///   `allow_duplicate`, so copies never stack silently.
async fn pre_engine_guards(
    state: &AppState,
    source: &TorrentSource,
    allow_duplicate: bool,
) -> ApiResult<()> {
    let TorrentSource::TorrentFile(bytes) = source else {
        return Ok(());
    };
    let Ok(preview) = iris_torrent::parse_preview(bytes) else {
        return Ok(());
    };
    if !preview.streamable {
        return Err(ApiError::ArchiveOnly);
    }
    if allow_duplicate {
        return Ok(());
    }
    let files: Vec<(usize, String)> = preview
        .files
        .iter()
        .map(|f| (f.index, f.path.clone()))
        .collect();
    if let Some(col) =
        crate::collection_assign::peek_movie_collection(state.db(), &preview.name, &files).await
        // Re-adding a release the engine already manages is a no-op
        // resume, not a new copy.
        && !state.engine().contains(&preview.infohash)
    {
        let copies = iris_db::torrents::list_in_collection(state.db(), col.id)
            .await?
            .into_iter()
            .filter(|t| t.infohash != preview.infohash)
            .map(|t| t.name)
            .collect::<Vec<_>>();
        if !copies.is_empty() {
            return Err(ApiError::DuplicateInLibrary(format!(
                "\"{}\" is already in the library ({})",
                col.display_title,
                copies.join(", "),
            )));
        }
    }
    Ok(())
}

/// Core grab path shared by the search ingest endpoint and the For-You preview
/// dialog (which ingests the catalogue card's recommended-best release through
/// the same path): refuse a dead torrent, resolve the release, add it to the
/// engine, and persist + classify it.
pub(crate) async fn ingest_core(
    state: &AppState,
    user_id: iris_core::ids::UserId,
    provider_id: String,
    external_id: String,
    allow_duplicate: bool,
) -> ApiResult<IngestResponse> {
    let provider = state.provider(&provider_id)?;

    // Dead-torrent guard: a 0-seeder release can never assemble all its
    // pieces. Block only a confirmed-0 (see `is_dead`).
    if is_dead(&provider, &external_id).await {
        return Err(ApiError::DeadTorrent);
    }

    let _slot = take_leech_slot(state, &provider_id).await?;
    let source = resolve_release(state, &provider, &provider_id, &external_id).await?;
    pre_engine_guards(state, &source, allow_duplicate).await?;
    let result = match source {
        TorrentSource::TorrentFile(bytes) => state.engine().add_from_bytes(bytes).await,
        TorrentSource::Magnet(m) => state.engine().add_from_magnet(&m).await,
    }
    .map_err(map_engine_err)?;
    reject_unstreamable(state, &result).await?;

    // The tracker's own TMDB id is recorded on the torrent (trust signal T1);
    // the collection's id is settled by the trust gate once it is assigned.
    let row = match record_ingest(
        state,
        &result.snapshot,
        || "<unnamed>".into(),
        provider_id,
        external_id,
        user_id,
    )
    .await
    {
        Ok(row) => row,
        Err(e) => {
            discard_unrecorded(state, &result).await;
            return Err(e.into());
        }
    };

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
        let anilist = state.anilist().cloned();
        let providers = state.providers().clone();
        let infohash = result.snapshot.infohash.clone();
        let name = result.snapshot.name.clone().unwrap_or_default();
        let files: Vec<(usize, String)> = result
            .snapshot
            .files
            .iter()
            .map(|f| (f.index, f.path.clone()))
            .collect();
        tokio::spawn(async move {
            crate::collection_assign::assign_after_ingest(
                &pool,
                crate::collection_assign::EnrichDeps {
                    tmdb: tmdb.as_ref(),
                    anilist: anilist.as_ref(),
                    providers: Some(&providers),
                },
                &infohash,
                &name,
                &files,
            )
            .await;
        });
    }

    Ok(IngestResponse {
        id: row.id,
        already_managed: result.already_managed,
        snapshot: result.snapshot,
    })
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
                .filter(|f| iris_torrent::is_video_path(&f.path))
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
    let probe = match state
        .probes()
        .get_or_probe(infohash, idx, &path, true)
        .await
    {
        Ok(p) => p,
        Err(e) => {
            tracing::debug!(error = %e, "prewarm: probe failed");
            return;
        }
    };
    verify_tmdb_match(state, infohash, probe.duration_seconds).await;
    tracing::info!(
        infohash,
        idx,
        "prewarm: probed (no remux — lazy on first /play hit)"
    );
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
/// Decide how to treat the video stream for `key`. Re-encode (capped at
/// 1080p) fires in two cases; everything else stream-copies.
///
/// 1. **Resolution downscale.** A client only ever requests the remux
///    *after* it failed to direct-play the source (the TV flips to the HLS
///    fallback on `ERROR_CODE_DECODING_FORMAT_EXCEEDS_CAPABILITIES`; the web
///    client uses it as its last-resort tier). So a source taller than 1080p
///    means the client's hardware decoder rejected that frame size — a
///    level-4.2 H.264 decoder (common on TV boxes) chokes on a 1440p/4K
///    stream even though it plays the 1080p siblings fine. Stream-copying
///    would hand it back the exact size it just refused → it fails again, and
///    the TV's one-shot fallback won't re-fire → the remux loader spins
///    forever. Re-encode down to the universal 1080p H.264 baseline. Gated on
///    a *real* client (non-empty caps) so the speculative, caps-less prewarm
///    keeps its bare `infohash_idx` stream-copy key.
/// 2. **AV1 software-decode catch-up.** A box with no AV1 silicon (e.g.
///    Amlogic S905X2) stutters on a 10-bit AV1 in software; when it
///    *explicitly* advertises `av1-sw` we re-encode proactively to the
///    operator's configured codec. Legacy clients (bare `vdec=…,av1`) and the
///    caps-less prewarm are never transcoded for this.
fn decide_video_mode(
    probe: &iris_media::MediaProbe,
    caps: &iris_caps::ClientCapabilities,
    transcode: &iris_config::TranscodeConfig,
) -> iris_media::VideoMode {
    let Some(v) = probe.video.first() else {
        return iris_media::VideoMode::Copy;
    };
    let codec = v.codec.to_ascii_lowercase();
    let ten_bit_source = v.bit_depth.is_some_and(|d| d >= 10);

    // Only a real client (TV / browser) advertises decoders; the speculative
    // prewarm passes default (empty) caps. Re-encoding only earns its CPU for
    // a client that actually reached `/play` because it couldn't direct-play.
    let live_client = !caps.video_decoders.is_empty();
    let over_1080p = v.height.is_some_and(|h| h > 1080) || v.width.is_some_and(|w| w > 1920);
    let downscale = live_client && over_1080p;

    let av1_sw_catchup = codec == "av1" && caps.has_video_decoder("av1-sw") && ten_bit_source;

    if !downscale && !av1_sw_catchup {
        return iris_media::VideoMode::Copy;
    }

    // The AV1 catch-up honours the operator's configured codec; a pure
    // resolution downscale targets H.264 — the universal baseline every client
    // (and certainly this one, which direct-played the 1080p siblings)
    // hardware-decodes at 1080p.
    let target = if av1_sw_catchup && transcode.codec.eq_ignore_ascii_case("hevc") {
        iris_media::VideoCodec::Hevc
    } else {
        iris_media::VideoCodec::H264
    };
    // H.264 is 8-bit only; HEVC keeps 10-bit only when the operator opts in.
    let ten_bit = matches!(target, iris_media::VideoCodec::Hevc) && transcode.ten_bit;
    // We never carry HDR metadata through the transcode, so flatten any HDR
    // (PQ/HLG) source to BT.709 SDR — the box gets a clean, correct picture.
    let tonemap = !matches!(v.hdr, iris_media::HdrKind::None);
    iris_media::VideoMode::Transcode {
        codec: target,
        ten_bit,
        tonemap,
    }
}

fn build_remux_plan(
    probe: &iris_media::MediaProbe,
    caps: &iris_caps::ClientCapabilities,
    transcode: &iris_config::TranscodeConfig,
) -> iris_media::RemuxPlan {
    use iris_media::{AudioCodec, AudioRendition};
    let mut renditions: Vec<AudioRendition> = Vec::new();
    let mut lang_count: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
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
            source_stream: a.absolute_index,
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
        video: decide_video_mode(probe, caps, transcode),
    }
}

/// Tolerance used when matching TMDB's declared runtime against the file's
/// probed duration. 15 % covers minor encode-to-encode drift, intro/outro
/// promo additions, and the minute or two of credits that some releases
/// trim. It does NOT cover director's cuts (often +20-30 %) — those
/// stay marked unverified, which is correct: a director's cut isn't the
/// movie TMDB has metadata for.
const TMDB_RUNTIME_TOLERANCE: f64 = 0.15;

/// The lookup hint for a collection's `tmdb_id`: TMDB's movie and TV ids are
/// separate namespaces, so the same number can be a film and a series. An
/// unknown kind gives no hint (movie first, then TV).
fn tmdb_kind_hint(collection_kind: Option<&str>) -> Option<crate::tmdb::TmdbKind> {
    collection_kind.and_then(crate::tmdb::TmdbKind::from_wire)
}

/// The legacy runtime check: confirm or reject a LEGACY collection's
/// `tmdb_id` (one `tmdb-trust --apply` hasn't evaluated yet) by matching its
/// declared runtime against the file's probed duration, so such rows keep
/// today's behaviour until the apply step. It is no gate for a trusted
/// collection — `tmdb_verified` is derived from the trust there — and no
/// veto either: kind is already part of every trust signal, and episode
/// runtimes on TMDB are too loose to overrule one. Idempotent: once
/// verified, never re-checked.
async fn verify_tmdb_match(state: &AppState, infohash: &str, probed_duration_secs: Option<f64>) {
    let Ok(Some(row)) = iris_db::torrents::find_by_infohash(state.db(), infohash).await else {
        return;
    };
    if row.tmdb_verified || row.collection_tmdb_trust.is_some() {
        return;
    }
    let Some(tmdb_id) = row.collection_tmdb_id.filter(|id| *id > 0) else {
        return;
    };
    let Some(probed) = probed_duration_secs.filter(|d| *d > 0.0) else {
        return;
    };
    let Some(tmdb) = state.tmdb() else { return };
    // tmdb_id is a positive i64 from the DB; u64::try_from cannot fail here.
    let Ok(tmdb_id_u64) = u64::try_from(tmdb_id) else {
        return;
    };
    let Some(meta) = tmdb
        .lookup_with_kind(tmdb_id_u64, tmdb_kind_hint(row.kind.as_deref()))
        .await
    else {
        return;
    };
    let Some(tmdb_minutes) = meta.runtime_minutes.filter(|m| *m > 0) else {
        return;
    };
    let tmdb_secs = f64::from(tmdb_minutes) * 60.0;
    let diff = (probed - tmdb_secs).abs() / tmdb_secs;
    let verified = diff < TMDB_RUNTIME_TOLERANCE;
    if let Err(e) = iris_db::torrents::set_tmdb_verified(state.db(), infohash, verified).await {
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

#[derive(Debug, Serialize, ToSchema)]
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
    /// True when the collection's TMDB match is trusted (see `tmdb_trust`);
    /// for a legacy collection not yet re-evaluated, the old runtime check.
    /// Frontends use the `(tmdb_id, tmdb_verified=true)` pair to decide
    /// whether to fetch posters / titles from TMDB; otherwise they stick
    /// with the filename.
    pub tmdb_verified: bool,
    /// `"movie"` / `"tv"` from the parent collection. Clients pass
    /// this to `/api/metadata/tmdb/{id}?kind=` so TMDB's separate
    /// movie / tv namespaces don't collide on poster lookups.
    pub kind: Option<MediaKind>,
    /// Parent collection UUID, once `collection_assign` has grouped this
    /// torrent. `None` for orphan torrents (no SCENE identity / not yet
    /// assigned). Additive field — lets a client deep-link a multi-file
    /// torrent straight to its collection page rather than a generic
    /// fallback. Older clients ignore the unknown field.
    pub collection_id: Option<uuid::Uuid>,
    /// Lifetime upload counter — survives session restarts and GC
    /// evictions, unlike `snapshot.uploaded_bytes`.
    pub uploaded_bytes_total: u64,
    /// Lifetime download counter (max on-disk progress ever observed) —
    /// the ratio denominator matching `uploaded_bytes_total`'s horizon.
    /// Additive field; older clients ignore it.
    #[serde(default)]
    pub downloaded_bytes_total: u64,
    /// Whether the caller may delete this torrent: admins, and whoever
    /// added it. Additive — older clients ignore it (a refused delete
    /// answers 403).
    #[serde(default)]
    pub can_delete: bool,
    #[serde(flatten)]
    pub snapshot: TorrentSnapshot,
}

/// Who may delete a torrent: an admin, or the user who added it. Everyone
/// else in the household plays and seeds it but can't take it away.
pub(crate) fn may_delete(viewer: &AuthUser, added_by: uuid::Uuid) -> bool {
    viewer.is_admin || uuid::Uuid::from(viewer.id) == added_by
}

impl TorrentView {
    pub(crate) fn new(
        viewer: &AuthUser,
        row: iris_db::torrents::TorrentRow,
        snapshot: TorrentSnapshot,
    ) -> Self {
        Self {
            can_delete: may_delete(viewer, row.added_by),
            tmdb_id: row.effective_tmdb_id(),
            id: row.id,
            added_by: row.added_by,
            added_by_name: row.added_by_name,
            added_at: row.added_at,
            last_played_at: row.last_played_at,
            source_provider: row.source_provider,
            source_external_id: row.source_external_id,
            tmdb_verified: row.tmdb_verified,
            kind: row.kind.as_deref().and_then(MediaKind::from_wire),
            collection_id: row.collection_id,
            uploaded_bytes_total: u64::try_from(row.uploaded_bytes_total).unwrap_or(0),
            downloaded_bytes_total: u64::try_from(row.downloaded_bytes_total).unwrap_or(0),
            snapshot,
        }
    }

    /// The view of a row whose torrent the engine still manages; `None` once
    /// the engine has dropped it.
    pub(crate) fn live(
        state: &AppState,
        viewer: &AuthUser,
        row: iris_db::torrents::TorrentRow,
    ) -> Option<Self> {
        let snapshot = state.engine().get_by_infohash(&row.infohash)?;
        Some(Self::new(viewer, row, snapshot))
    }
}

#[utoipa::path(
    get,
    path = "/api/torrents",
    operation_id = "list_torrents",
    responses((status = 200, description = "All active torrents", body = [TorrentView])),
    tag = "torrents",
)]
pub(crate) async fn list(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<Vec<TorrentView>>> {
    let rows = iris_db::torrents::list_active(state.db()).await?;
    let out = rows
        .into_iter()
        .filter_map(|row| TorrentView::live(&state, &user, row))
        .collect();
    Ok(Json(out))
}

#[utoipa::path(
    get,
    path = "/api/torrents/{infohash}",
    params(("infohash" = String, Path)),
    responses((status = 200, body = TorrentView), (status = 404, description = "Unknown infohash")),
    tag = "torrents",
)]
pub(crate) async fn get_one(
    State(state): State<AppState>,
    user: AuthUser,
    Path(infohash): Path<Infohash>,
) -> ApiResult<Json<TorrentView>> {
    let row = torrent_or_404(&state, &infohash).await?;
    TorrentView::live(&state, &user, row)
        .map(Json)
        .ok_or(ApiError::NotFound)
}

#[utoipa::path(
    post,
    path = "/api/torrents/{infohash}/pause",
    params(("infohash" = String, Path)),
    responses(
        (status = 204, description = "Left the swarm; files stay on disk"),
        (status = 403, description = "Only an admin or whoever added it"),
        (status = 404, description = "Unknown infohash"),
    ),
    tag = "torrents",
)]
pub(crate) async fn pause(
    State(state): State<AppState>,
    user: AuthUser,
    Path(infohash): Path<Infohash>,
) -> ApiResult<StatusCode> {
    let row = owned_row(&state, &user, &infohash).await?;
    state
        .engine()
        .pause_by_infohash(&row.infohash)
        .await
        .map_err(map_engine_err)?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/torrents/{infohash}/resume",
    params(("infohash" = String, Path)),
    responses(
        (status = 204, description = "Back in the swarm"),
        (status = 403, description = "Only an admin or whoever added it"),
        (status = 404, description = "Unknown infohash"),
        (status = 409, description = "The tracker's download slots are full"),
    ),
    tag = "torrents",
)]
pub(crate) async fn resume(
    State(state): State<AppState>,
    user: AuthUser,
    Path(infohash): Path<Infohash>,
) -> ApiResult<StatusCode> {
    let row = owned_row(&state, &user, &infohash).await?;
    // An unfinished torrent downloads again: it takes one of the tracker's
    // download slots, like a grab does.
    let unfinished = state
        .engine()
        .get_by_infohash(&row.infohash)
        .is_some_and(|s| !s.finished);
    // Held through the resume, as a grab holds it through its add.
    let _slot = match row.source_provider.as_deref() {
        Some(provider) if unfinished => take_leech_slot(&state, provider).await?,
        _ => None,
    };
    state
        .engine()
        .resume_by_infohash(&row.infohash)
        .await
        .map_err(map_engine_err)?;
    Ok(StatusCode::NO_CONTENT)
}

/// The torrent's row, once the caller may act on it (pause, resume,
/// delete): an admin, or whoever added it.
async fn owned_row(
    state: &AppState,
    user: &AuthUser,
    infohash: &Infohash,
) -> ApiResult<iris_db::torrents::TorrentRow> {
    let row = torrent_or_404(state, infohash).await?;
    if row.deleted_at.is_some() {
        return Err(ApiError::NotFound);
    }
    if !may_delete(user, row.added_by) {
        return Err(ApiError::Forbidden);
    }
    Ok(row)
}

#[utoipa::path(
    delete,
    path = "/api/torrents/{infohash}",
    operation_id = "remove_torrent",
    params(("infohash" = String, Path)),
    responses(
        (status = 204, description = "Soft-deleted"),
        (status = 404, description = "Unknown infohash"),
    ),
    tag = "torrents",
)]
pub(crate) async fn remove(
    State(state): State<AppState>,
    user: AuthUser,
    Path(infohash): Path<Infohash>,
) -> ApiResult<StatusCode> {
    let row = owned_row(&state, &user, &infohash).await?;
    // Capture the final upload delta before the engine drops the torrent —
    // otherwise the bytes uploaded since the last 30 s reconcile tick are
    // lost forever.
    if let Some(snap) = state.engine().get_by_infohash(&row.infohash) {
        let _ =
            iris_db::torrents::reconcile_uploaded(state.db(), &row.infohash, snap.uploaded_bytes)
                .await;
    }
    match state.engine().delete_by_infohash(&row.infohash, true).await {
        // Not in the engine (it failed to restore after a restart): the row
        // must still be removable.
        Ok(()) | Err(iris_torrent::EngineError::NotFound) => {}
        Err(e) => return Err(ApiError::Internal(anyhow::anyhow!("engine delete: {e}"))),
    }
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
    wipe_derived(
        state.remuxer(),
        &subtitle_cache_dir(state.cfg()),
        &row.infohash,
    )
    .await;
    iris_db::torrents::soft_delete(state.db(), TorrentId::from(row.id)).await?;
    super::audit(
        &state,
        user.id,
        "torrent.delete",
        "torrent",
        Some(&row.infohash),
        Some(&row.name),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/api/torrents/{infohash}/files/{idx}/probe",
    params(("infohash" = String, Path), ("idx" = u32, Path)),
    responses(
        (status = 200, description = "ffprobe stream summary", body = iris_media::MediaProbe),
        (status = 404, description = "Unknown infohash / file index"),
    ),
    tag = "torrents",
)]
pub(crate) async fn probe_file(
    State(state): State<AppState>,
    _user: AuthUser,
    Path((infohash, idx)): Path<(Infohash, usize)>,
) -> ApiResult<Json<iris_media::MediaProbe>> {
    let infohash = infohash.into_inner();
    let row = torrent_or_404(&state, &infohash).await?;
    let path = state
        .engine()
        .file_path(&infohash, idx)
        .map_err(map_engine_err)?;

    // Don't gate playback on the *whole torrent* finishing — a 4 K remux is
    // tens of GB and "click-to-play" must work as soon as the head is here.
    // Mirror `manifest_json`: while the torrent is still downloading, force
    // the bytes ffprobe needs (container header at the front, MKV Cues /
    // MP4 trailing `moov` at the tail) to download now. `prefetch_range`'s
    // sequential priority is per-stream and transient, so this never slows
    // the other torrents (no global first/last-piece priority).
    //
    // `finished_at` from the DB outranks the snapshot: during the
    // post-deploy `initializing` re-check the snapshot reports finished =
    // false (and 0 peers / 0 B/s — which used to trip the stalled-swarm
    // guard below) for torrents whose bytes are complete on disk.
    let snap = state.engine().get_by_infohash(&infohash);
    let torrent_finished = row.finished_at.is_some() || snap.as_ref().is_some_and(|s| s.finished);
    let mut header_bytes: u64 = 0;
    if let Some(snap) = snap.as_ref()
        && !torrent_finished
        && let Some(file) = snap.files.iter().find(|f| f.index == idx)
    {
        header_bytes =
            prefetch_probe_ranges(&state, &infohash, idx, file.size_bytes, "probe").await;
    }
    ensure_probe_readable(&state, &infohash, &path, torrent_finished, header_bytes)?;
    let probe = state
        .probes()
        .get_or_probe(&infohash, idx, &path, torrent_finished)
        .await
        .map_err(|e| map_probe_err(&e, torrent_finished))?;
    Ok(Json(probe))
}

// Cohesive partial-download manifest handler (validate → prefetch header/tail
// → stalled-swarm guard → probe → assemble); 2 lines over the pedantic
// heuristic after the `collapsible_if` → let-chain cleanup. Same inline-allow
// pattern as `clippy::cast_precision_loss` below.
#[allow(clippy::too_many_lines)]
#[utoipa::path(
    get,
    path = "/api/torrents/{infohash}/files/{idx}/manifest.json",
    params(("infohash" = String, Path), ("idx" = u32, Path)),
    responses(
        (status = 200, description = "Per-file playback manifest", body = iris_media::Manifest),
        (status = 400, description = "Invalid infohash or file not yet on disk"),
        (status = 404, description = "Unknown infohash / file index"),
    ),
    tag = "torrents",
)]
pub(crate) async fn manifest_json(
    State(state): State<AppState>,
    _user: AuthUser,
    Path((infohash, idx)): Path<(Infohash, usize)>,
) -> ApiResult<Json<iris_media::Manifest>> {
    let infohash = infohash.into_inner();
    let row = torrent_or_404(&state, &infohash).await?;

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
    // Restart-proof "finished": during the post-deploy `initializing`
    // re-check the snapshot reports finished = false + 0 peers + 0 B/s
    // for fully-downloaded torrents — without the DB stamp this route
    // wrongly answered "stalled: no seeders" and probed as incomplete.
    let torrent_finished = row.finished_at.is_some() || snapshot.finished;

    let header_bytes = if torrent_finished {
        0
    } else {
        prefetch_probe_ranges(&state, &infohash, idx, file.size_bytes, "manifest").await
    };
    ensure_probe_readable(&state, &infohash, &path, torrent_finished, header_bytes)?;

    let probe = state
        .probes()
        .get_or_probe(&infohash, idx, &path, torrent_finished)
        .await
        .map_err(|e| map_probe_err(&e, torrent_finished))?;

    let file_idx_u32 =
        u32::try_from(idx).map_err(|_| ApiError::BadRequest("file index too large".into()))?;
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

pub(crate) fn map_engine_err(e: iris_torrent::EngineError) -> ApiError {
    match e {
        iris_torrent::EngineError::NotFound => ApiError::NotFound,
        iris_torrent::EngineError::FileOutOfRange => {
            ApiError::BadRequest("file index out of range".into())
        }
        iris_torrent::EngineError::MetadataTimeout => ApiError::Upstream(e.to_string()),
        iris_torrent::EngineError::Librqbit(e) => ApiError::Internal(e),
    }
}

/// Playhead hint sent by the client on every user-initiated seek. Phase 1
/// will use it to bias librqbit's piece priority. Phase 0 only logs.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SeekHint {
    pub byte_offset: u64,
    pub playhead_s: Option<f64>,
}

#[utoipa::path(
    post,
    path = "/api/torrents/{infohash}/files/{idx}/seek",
    params(("infohash" = String, Path), ("idx" = u32, Path)),
    request_body = SeekHint,
    responses(
        (status = 204, description = "Hint accepted (prefetch scheduled)"),
        (status = 400, description = "Invalid infohash"),
    ),
    tag = "torrents",
)]
pub(crate) async fn seek_hint(
    State(state): State<AppState>,
    _user: AuthUser,
    Path((infohash, idx)): Path<(Infohash, usize)>,
    Json(body): Json<SeekHint>,
) -> ApiResult<StatusCode> {
    let infohash = infohash.into_inner();
    tracing::debug!(
        infohash,
        file_idx = idx,
        byte_offset = body.byte_offset,
        playhead_s = body.playhead_s,
        "seek hint",
    );
    // A file on disk has nothing to fetch: the read would only compete with
    // the real playback read.
    let row = torrent_or_404(&state, &infohash).await?;
    if torrent_finished(&state, &row) {
        return Ok(StatusCode::NO_CONTENT);
    }
    // Spawn the prefetch in the background so the client gets its 204 back
    // immediately; librqbit picks up the priority bias as soon as the read
    // starts. We aim for ~30 seconds of playback ahead — derived from the
    // probed bitrate when we have one, falling back to a flat 64 MiB cap.
    // One prefetch per file: a new seek aborts the previous one, whose
    // position the player has already left.
    let key = format!("{infohash}_{idx}");
    let mut prefetches = SEEK_PREFETCHES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let entry = prefetches.entry(key.clone()).or_default();
    if let Some(task) = entry.task.take() {
        task.abort();
    }
    let window = entry.window;
    let engine = state.engine().clone();
    let probes = state.probes().clone();
    let byte_offset = body.byte_offset;
    let task_key = key;
    let task = tokio::spawn(async move {
        let bytes_ahead = if let Some(w) = window {
            w
        } else {
            let w = playhead_window_bytes(&engine, &probes, &infohash, idx).await;
            if let Some(w) = w
                && let Some(entry) = SEEK_PREFETCHES
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get_mut(&task_key)
            {
                entry.window = Some(w);
            }
            w.unwrap_or(PREFETCH_FALLBACK)
        };
        if let Err(e) = engine
            .prefetch_range(
                &infohash,
                idx,
                byte_offset,
                bytes_ahead,
                Duration::from_mins(1),
            )
            .await
        {
            tracing::debug!(error = %e, "seek hint prefetch errored");
        }
    });
    entry.task = Some(task.abort_handle());
    drop(prefetches);
    Ok(StatusCode::NO_CONTENT)
}

/// Per `{infohash}_{idx}`: the prefetch in flight, and the playhead window
/// once measured (one ffprobe per file, not one per seek).
#[derive(Default)]
struct SeekPrefetch {
    task: Option<tokio::task::AbortHandle>,
    window: Option<u64>,
}

static SEEK_PREFETCHES: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, SeekPrefetch>>,
> = std::sync::LazyLock::new(Default::default);

const PREFETCH_FALLBACK: u64 = 64 * 1024 * 1024;

/// Window of bytes to mark high-priority ahead of the playhead. Targets
/// ~30 seconds of playback by deriving bytes-per-second from the cached
/// probe (total size ÷ duration); `None` when the probe can't tell yet (the
/// caller then uses [`PREFETCH_FALLBACK`]). Capped at 256 MiB so a long
/// seek on a 4 K HEVC remux doesn't lock librqbit into an impossibly wide
/// window.
async fn playhead_window_bytes(
    engine: &std::sync::Arc<iris_torrent::Engine>,
    probes: &iris_media::ProbeCache,
    infohash: &str,
    idx: usize,
) -> Option<u64> {
    const CAP: u64 = 256 * 1024 * 1024;
    const SECONDS_AHEAD: f64 = 30.0;

    let snapshot = engine.get_by_infohash(infohash)?;
    let file_size = snapshot
        .files
        .iter()
        .find(|f| f.index == idx)
        .map(|f| f.size_bytes)
        .filter(|s| *s > 0)?;
    let path = engine.file_path(infohash, idx).ok()?;
    let probe = probes
        .get_or_probe(infohash, idx, &path, snapshot.finished)
        .await
        .ok()?;
    let duration = probe.duration_seconds.filter(|d| *d > 0.0)?;
    #[allow(clippy::cast_precision_loss)]
    let bps = file_size as f64 / duration;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let bytes = (bps * SECONDS_AHEAD).round() as u64;
    Some(bytes.clamp(8 * 1024 * 1024, CAP))
}

/// Playback-error report sent by clients when a decode tier fails.
/// Echoes the legacy HLS URL as the fallback and pre-warms the server-
/// side ffmpeg + shaka remux in the background so the client's next
/// request to `/play/master.m3u8` lands on a hot cache.
#[derive(Debug, Deserialize, ToSchema)]
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

#[derive(Debug, Serialize, ToSchema)]
pub struct PlaybackErrorResponse {
    pub fallback_tier: &'static str,
    pub fallback_url: String,
}

#[utoipa::path(
    post,
    path = "/api/torrents/{infohash}/files/{idx}/playback-error",
    params(("infohash" = String, Path), ("idx" = u32, Path)),
    request_body = PlaybackErrorBody,
    responses(
        (status = 200, description = "Fallback tier + URL; server pre-warms the HLS remux", body = PlaybackErrorResponse),
        (status = 400, description = "Invalid infohash"),
    ),
    tag = "torrents",
)]
pub(crate) async fn playback_error(
    State(state): State<AppState>,
    _user: AuthUser,
    Path((infohash, idx)): Path<(Infohash, usize)>,
    Json(body): Json<PlaybackErrorBody>,
) -> ApiResult<Json<PlaybackErrorResponse>> {
    let infohash = infohash.into_inner();
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
    if let Some(snap) = state.engine().get_by_infohash(infohash)
        && !snap.finished
    {
        tracing::debug!(
            infohash,
            idx,
            pct = snap.progress_pct,
            "fallback prewarm: deferring until torrent finishes"
        );
        return;
    }
    // Both prewarm paths only reach here once the torrent is `finished`
    // (guarded above), so the probe is taken on a complete file.
    let probe = match state
        .probes()
        .get_or_probe(infohash, idx, &path, true)
        .await
    {
        Ok(p) => p,
        Err(e) => {
            tracing::debug!(error = %e, "fallback prewarm: probe failed");
            return;
        }
    };
    let key = format!("{infohash}_{idx}");
    // Prewarm is speculative + caps-less, so it always builds the
    // stream-copy variant (no transcode). The real `/play` request carries
    // `Iris-Caps` and builds the transcoded variant under its own cache key.
    let plan = build_remux_plan(
        &probe,
        &iris_caps::ClientCapabilities::default(),
        &state.cfg().transcode,
    );
    if let Err(e) = state.remuxer().ensure_remuxed(&key, &path, plan).await {
        tracing::warn!(error = %e, infohash, idx, "fallback prewarm: remux failed");
        return;
    }
    tracing::info!(infohash, idx, "fallback prewarm: HLS cache hot");
}

/// State of the per-file remux job exposed to the player UI. Polled
/// before mounting the `<video>` so we can render a meaningful loading
/// step ("downloading 47 %", "remuxing", "ready").
#[derive(Debug, Serialize, ToSchema)]
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

#[utoipa::path(
    get,
    path = "/api/torrents/{infohash}/files/{idx}/play/status",
    params(("infohash" = String, Path), ("idx" = u32, Path)),
    responses(
        (status = 200, description = "Remux-job readiness for the player loader", body = PlayStatus),
        (status = 404, description = "Unknown infohash / file index"),
    ),
    tag = "torrents",
)]
#[allow(clippy::too_many_lines)] // linear status state-machine; clearer inline
pub(crate) async fn play_status(
    State(state): State<AppState>,
    _user: AuthUser,
    Path((infohash, idx)): Path<(Infohash, usize)>,
    req: Request<Body>,
) -> ApiResult<Json<PlayStatus>> {
    // Caps are OPTIONAL: the `Iris-Caps` middleware only inserts the extension
    // when the client actually sent the header, and the web player polls
    // `/status` WITHOUT it. A required `Extension` extractor 500s ("Missing
    // request extension") on every header-less request — i.e. it breaks the
    // web client. Pull it defensively and fall back to default caps (no
    // transcode), exactly like `play_asset` does. We never break a client.
    let caps = crate::middleware::IrisCaps::of(&req);
    let infohash = infohash.into_inner();
    let row = torrent_or_404(&state, &infohash).await?;
    let path = state
        .engine()
        .file_path(&infohash, idx)
        .map_err(map_engine_err)?;

    // The DB stamp first: during the post-deploy re-check the snapshot says
    // unfinished for minutes about a file `play_asset` already serves.
    if let Some(snap) = state.engine().get_by_infohash(&infohash)
        && !torrent_finished(&state, &row)
    {
        return Ok(Json(PlayStatus {
            ready: false,
            reason: Some("downloading".into()),
            progress: Some((snap.progress_pct / 100.0).clamp(0.0, 1.0)),
            error: None,
        }));
    }

    // Caps-aware cache key + whether THIS client must wait on a server-side
    // build. A software-AV1 box polls the progress of the transcoded variant
    // (`..._h264`) it will actually play, not the never-built stream-copy one.
    // `needs_build` is true only for a Transcode plan: that client routes to
    // `/play/master.m3u8` and the player blocks until ffmpeg has built the
    // head, so `/status` must report real progress rather than a premature
    // `ready: true`. A Copy plan (web, or TV playing raw `/stream`) has
    // nothing to wait for. Falls back to the bare key + no-build if the probe
    // isn't available yet (the master.m3u8 request will probe + build).
    // `build_plan` is `Some` only for a Transcode plan: that client plays the
    // server variant via `/play/master.m3u8`, so `/status` drives its progress
    // (and kicks the build off so the percentage advances even before the
    // player asks). A Copy plan (web, or TV on raw `/stream`) has nothing to
    // wait for. Falls back to the bare key + no-build if the probe isn't ready
    // yet (the master.m3u8 request will probe + build).
    let (key, build_plan) = match state
        .probes()
        .get_or_probe(&infohash, idx, &path, true)
        .await
    {
        Ok(probe) => {
            let plan = build_remux_plan(&probe, &caps, &state.cfg().transcode);
            let key = format!("{infohash}_{idx}{}", plan.cache_suffix());
            let build_plan =
                matches!(plan.video, iris_media::VideoMode::Transcode { .. }).then_some(plan);
            (key, build_plan)
        }
        Err(_) => (format!("{infohash}_{idx}"), None),
    };
    // Transcode plan: the variant STREAMS while ffmpeg keeps encoding ahead of
    // the playhead. The master playlist lands on disk early (after the first
    // segment), so a plain "master exists → ready" check would flip ready the
    // instant the head is built — long before the encoder reaches, say, a
    // 30-min resume position. The TV instead shows a determinate progress bar
    // and only starts playback once the encode has passed its resume point, so
    // `/status` must surface the live encode fraction for as long as the job
    // is in flight, BEFORE the master-exists shortcut.
    if let Some(plan) = build_plan {
        // A failed job isn't in flight, so check the sticky failure first.
        if let Some(msg) = state.remuxer().recent_failure(&key).await {
            return Ok(Json(PlayStatus {
                ready: false,
                reason: None,
                progress: None,
                error: Some(msg),
            }));
        }
        // In flight (and past ffmpeg's first progress tick) → live fraction.
        if let Some(p) = state.remuxer().progress(&key).await {
            return Ok(Json(PlayStatus {
                ready: false,
                reason: Some("remuxing".into()),
                progress: Some(p),
                error: None,
            }));
        }
        // Not in flight: either the encode finished (master on disk → playable
        // + fully seekable) or it hasn't been kicked off yet.
        let master = state.remuxer().master_path(&key);
        let master_ready = matches!(
            tokio::fs::metadata(&master).await,
            Ok(m) if m.is_file() && m.len() > 0
        );
        if master_ready {
            return Ok(Json(PlayStatus {
                ready: true,
                reason: None,
                progress: None,
                error: None,
            }));
        }
        // Kick off the build so the percentage starts moving without the
        // player having to request `master.m3u8` (it's holding back until we
        // report the resume portion encoded). `ensure_remuxed` is idempotent —
        // a no-op once a job is in flight or the cache is built.
        let remuxer = state.remuxer().clone();
        let key_owned = key.clone();
        let source = path.clone();
        tokio::spawn(async move {
            let _ = remuxer.ensure_remuxed(&key_owned, &source, plan).await;
        });
        return Ok(Json(PlayStatus {
            ready: false,
            reason: Some("remuxing".into()),
            progress: None,
            error: None,
        }));
    }

    // Raw-stream path (Copy plan). The web client defaults to Tier B
    // (Mediabunny remux in-browser) and the Android TV client plays raw
    // `/stream` directly, so 99 % of sessions never touch the server remux;
    // it's strictly lazy, firing only when someone requests
    // `/play/master.m3u8`. Report ready as soon as the master exists, else
    // fall through to ready — there's nothing to wait on.
    let master = state.remuxer().master_path(&key);
    if let Ok(meta) = tokio::fs::metadata(&master).await
        && meta.is_file()
        && meta.len() > 0
    {
        return Ok(Json(PlayStatus {
            ready: true,
            reason: None,
            progress: None,
            error: None,
        }));
    }
    if let Some(msg) = state.remuxer().recent_failure(&key).await {
        return Ok(Json(PlayStatus {
            ready: false,
            reason: None,
            progress: None,
            error: Some(msg),
        }));
    }
    // A copy remux writes the master only once the whole file is remuxed: a
    // job in flight is not ready yet.
    if state.remuxer().is_in_flight(&key).await {
        return Ok(Json(PlayStatus {
            ready: false,
            reason: Some("remuxing".into()),
            progress: state.remuxer().progress(&key).await,
            error: None,
        }));
    }
    Ok(Json(PlayStatus {
        ready: true,
        reason: None,
        progress: None,
        error: None,
    }))
}

#[utoipa::path(
    get,
    path = "/api/torrents/{infohash}/files/{idx}/sub/{stream_idx}/track.vtt",
    operation_id = "subtitle_vtt",
    params(("infohash" = String, Path), ("idx" = u32, Path), ("stream_idx" = u32, Path)),
    responses(
        (status = 200, description = "WebVTT track (text-based subs transcoded to VTT)", body = String, content_type = "text/vtt"),
        (status = 404, description = "Unknown infohash / file / stream index"),
    ),
    tag = "torrents",
)]
pub(crate) async fn subtitle_vtt(
    State(state): State<AppState>,
    _user: AuthUser,
    Path((infohash, idx, stream_idx)): Path<(Infohash, usize, u32)>,
) -> ApiResult<Response> {
    serve_subtitle(
        &state,
        &infohash,
        idx,
        stream_idx,
        iris_media::SubtitleFormat::WebVtt,
    )
    .await
}

#[utoipa::path(
    get,
    path = "/api/torrents/{infohash}/files/{idx}/sub/{stream_idx}/track.ass",
    operation_id = "subtitle_ass",
    params(("infohash" = String, Path), ("idx" = u32, Path), ("stream_idx" = u32, Path)),
    responses(
        (status = 200, description = "Raw ASS/SSA subtitle for the libass overlay path", body = String, content_type = "text/plain"),
        (status = 404, description = "Unknown infohash / file / stream index"),
    ),
    tag = "torrents",
)]
pub(crate) async fn subtitle_ass(
    State(state): State<AppState>,
    _user: AuthUser,
    Path((infohash, idx, stream_idx)): Path<(Infohash, usize, u32)>,
) -> ApiResult<Response> {
    serve_subtitle(
        &state,
        &infohash,
        idx,
        stream_idx,
        iris_media::SubtitleFormat::Ass,
    )
    .await
}

#[utoipa::path(
    get,
    path = "/api/torrents/{infohash}/files/{idx}/sub/{stream_idx}/track.sup",
    operation_id = "subtitle_sup",
    params(("infohash" = String, Path), ("idx" = u32, Path), ("stream_idx" = u32, Path)),
    responses(
        (status = 200, description = "PGS bitmap subtitle stream for the libpgs overlay path", body = String, content_type = "application/octet-stream"),
        (status = 404, description = "Unknown infohash / file / stream index"),
    ),
    tag = "torrents",
)]
pub(crate) async fn subtitle_sup(
    State(state): State<AppState>,
    _user: AuthUser,
    Path((infohash, idx, stream_idx)): Path<(Infohash, usize, u32)>,
) -> ApiResult<Response> {
    serve_subtitle(
        &state,
        &infohash,
        idx,
        stream_idx,
        iris_media::SubtitleFormat::Sup,
    )
    .await
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
    infohash: &Infohash,
    idx: usize,
    stream_idx: u32,
    format: iris_media::SubtitleFormat,
) -> ApiResult<Response> {
    let row = torrent_or_404(state, infohash).await?;
    let path = state
        .engine()
        .file_path(infohash, idx)
        .map_err(map_engine_err)?;
    if !path.exists() {
        return Err(ApiError::BadRequest("file not yet on disk".into()));
    }

    // DB stamp first — the snapshot can't answer "finished" during the
    // post-deploy re-check, and `torrent_finished` gates whether the
    // extracted subtitle may be promoted to the permanent cache.
    let torrent_finished = torrent_finished(state, &row);

    let cache_dir = subtitle_cache_dir(state.cfg());
    let cache_path = iris_media::subtitle_cache_path(&cache_dir, infohash, idx, stream_idx, format);
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

    let stream =
        iris_media::stream_subtitle(&path, stream_idx, format, cache_path, torrent_finished)
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("subtitle extract: {e}")))?;
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, format.mime())
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from_stream(stream))
        .unwrap())
}

#[utoipa::path(
    get,
    path = "/api/torrents/{infohash}/files/{idx}/stream",
    operation_id = "stream_file",
    params(("infohash" = String, Path), ("idx" = u32, Path)),
    responses(
        (status = 200, description = "Raw source file bytes (used by Tier A/B direct playback + download)", body = String, content_type = "application/octet-stream"),
        (status = 206, description = "Partial content for an HTTP `Range` request"),
        (status = 404, description = "Unknown infohash / file index"),
    ),
    tag = "torrents",
)]
pub(crate) async fn stream_file(
    State(state): State<AppState>,
    _user: AuthUser,
    Path((infohash, idx)): Path<(Infohash, usize)>,
    req: Request<Body>,
) -> ApiResult<Response> {
    let infohash = infohash.into_inner();
    let row = torrent_or_404(&state, &infohash).await?;

    // Fully-downloaded torrents stream straight from disk. The bytes are
    // final, so playback must not depend on librqbit's session state:
    // after a deploy the restored session re-checks every torrent
    // (`initializing`) and `handle.stream()` errors with
    // "invalid state: initializing" for minutes — even though the file
    // is perfectly readable. The FileStream path below is only needed
    // while pieces may still be missing (its reads wait for them).
    let finished = torrent_finished(&state, &row);
    if finished
        && let Ok(path) = state.engine().file_path(&infohash, idx)
        && tokio::fs::try_exists(&path).await.unwrap_or(false)
    {
        let _ = iris_db::torrents::touch_played(state.db(), &infohash).await;
        let mime = mime_for_filename(&path.to_string_lossy());
        let method = req.method().clone();
        let range = byte_range(req.headers());
        return serve_file_with_range(&path, &mime, method, range).await;
    }

    let stream = state
        .engine()
        .open_stream(&infohash, idx)
        .await
        .map_err(map_engine_err)?;

    // Best-effort: bump the played timestamp.
    let _ = iris_db::torrents::touch_played(state.db(), &infohash).await;

    let mime = guess_mime(&infohash, idx, state.engine());
    let total = stream.file_size();
    let mut reader = stream.into_reader();
    let range = byte_range(req.headers());
    let head_only = req.method() == Method::HEAD;

    if let Some(rh) = range.as_ref() {
        if let Some((start, end)) = parse_range(rh, total) {
            let len = end - start + 1;
            if head_only {
                return Ok(build_headers(
                    StatusCode::PARTIAL_CONTENT,
                    len,
                    &mime,
                    Some((start, end, total)),
                )
                .body(Body::empty())
                .unwrap());
            }
            if let Err(e) = reader.seek(SeekFrom::Start(start)).await {
                return Err(ApiError::Internal(anyhow::anyhow!("seek: {e}")));
            }
            let limited = reader.take(len);
            let body = Body::from_stream(ReaderStream::with_capacity(limited, STREAM_CHUNK_SIZE));
            return Ok(build_headers(
                StatusCode::PARTIAL_CONTENT,
                len,
                &mime,
                Some((start, end, total)),
            )
            .body(body)
            .unwrap());
        }
        return Ok(range_not_satisfiable("invalid range", total));
    }

    if head_only {
        return Ok(build_headers(StatusCode::OK, total, &mime, None)
            .body(Body::empty())
            .unwrap());
    }
    let body = Body::from_stream(ReaderStream::with_capacity(reader, STREAM_CHUNK_SIZE));
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
#[utoipa::path(
    get,
    path = "/api/torrents/{infohash}/files/{idx}/play/{asset}",
    operation_id = "play_asset",
    params(
        ("infohash" = String, Path),
        ("idx" = u32, Path),
        ("asset" = String, Path, description = "`master.m3u8`, a variant playlist, an init segment, or an `.m4s` fragment"),
    ),
    responses(
        (status = 200, description = "HLS-CMAF asset; `master.m3u8` blocks until ffmpeg has built the head", body = String, content_type = "application/octet-stream"),
        (status = 206, description = "Partial content for an HTTP `Range` request on a segment"),
        (status = 404, description = "Unknown infohash / file / asset"),
    ),
    tag = "torrents",
)]
pub(crate) async fn play_asset(
    State(state): State<AppState>,
    _user: AuthUser,
    Path((infohash, idx, asset)): Path<(Infohash, usize, String)>,
    req: Request<Body>,
) -> ApiResult<Response> {
    let infohash = infohash.into_inner();
    let row = torrent_or_404(&state, &infohash).await?;

    let path = state
        .engine()
        .file_path(&infohash, idx)
        .map_err(map_engine_err)?;

    // DB `finished_at` outranks the snapshot: during the post-deploy
    // `initializing` re-check the snapshot reports finished = false for
    // fully-downloaded torrents — this gate used to break Tier F for
    // the whole re-check window. A *missing* snapshot with no
    // `finished_at` is the opposite case (grab so fresh the engine
    // hasn't registered it yet) and must count as still downloading,
    // not fall through to probing a zero-filled preallocation.
    if !torrent_finished(&state, &row) {
        return Err(ApiError::BadRequest(
            "torrent still downloading — wait until it's complete to play".into(),
        ));
    }
    if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
        return Err(ApiError::BadRequest(format!(
            "file not on disk: {}",
            path.display()
        )));
    }

    // The cache variant depends on the client's caps: a software-AV1 box gets
    // a transcoded variant under a distinct key (`..._hevc`). Probe + plan are
    // computed for EVERY asset request, not just the master, so the segment
    // requests that follow resolve the SAME suffixed cache dir. The probe is
    // cached, so this stays cheap after the first hit.
    let caps = crate::middleware::IrisCaps::of(&req);
    // `play_asset` already rejected unfinished torrents above, so the remux
    // always probes a complete file.
    let probe = state
        .probes()
        .get_or_probe(&infohash, idx, &path, true)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("ffprobe: {e}")))?;
    let plan = build_remux_plan(&probe, &caps, &state.cfg().transcode);
    let key = format!("{infohash}_{idx}{}", plan.cache_suffix());

    if asset == iris_media::MASTER_PLAYLIST {
        // The probe above also drives the TMDB-runtime verification side-effect
        // the UI relies on for poster / metadata gating.
        verify_tmdb_match(&state, &infohash, probe.duration_seconds).await;
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
    let range = byte_range(req.headers());
    let mut resp = serve_file_with_range(&asset_path, mime, method, range).await?;
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
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_control),
    );
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
async fn serve_file_with_range(
    path: &std::path::Path,
    mime: &str,
    method: Method,
    range: Option<HeaderValue>,
) -> ApiResult<Response> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("open {}: {e}", path.display())))?;
    let total = file
        .metadata()
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("stat {}: {e}", path.display())))?
        .len();
    let head_only = method == Method::HEAD;

    if let Some(rh) = range.as_ref() {
        if let Some((start, effective_end)) = parse_range(rh, total) {
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
            let body = Body::from_stream(ReaderStream::with_capacity(
                file.take(len),
                STREAM_CHUNK_SIZE,
            ));
            return Ok(build_headers(
                StatusCode::PARTIAL_CONTENT,
                len,
                mime,
                Some((start, effective_end, total)),
            )
            .body(body)
            .unwrap());
        }
        return Ok(range_not_satisfiable("invalid range", total));
    }

    if head_only {
        return Ok(build_headers(StatusCode::OK, total, mime, None)
            .body(Body::empty())
            .unwrap());
    }
    let body = Body::from_stream(ReaderStream::with_capacity(file, STREAM_CHUNK_SIZE));
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
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&len.to_string()).unwrap(),
    );
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    // Origin-side cache opt-out. Streamed media bodies are multi-GB and
    // per-user; an edge proxy that decides they're cacheable (e.g. a
    // Cloudflare "cache everything" rule) will abort the transfer when
    // the body exceeds its cacheable-object size limit — which kills
    // playback of exactly the biggest files, mid-stream. `play_asset`
    // overrides this per-asset for the small immutable HLS segments.
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
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

/// The request's `Range` header, when it is in bytes. RFC 9110: a range in
/// another unit is ignored (a full 200), not refused with a 416.
fn byte_range(headers: &HeaderMap) -> Option<HeaderValue> {
    headers
        .get(header::RANGE)
        .filter(|rh| rh.to_str().is_ok_and(|s| s.starts_with("bytes=")))
        .cloned()
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
    mime_for_filename(&path)
}

/// Extension → MIME, shared by the engine-snapshot path (`guess_mime`)
/// and the direct-from-disk fast path in `stream_file` (which has the
/// real on-disk path and must not depend on a live snapshot).
fn mime_for_filename(path: &str) -> String {
    let ext = std::path::Path::new(path)
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
        iris_core::Error::ProviderRefused(m) => ApiError::ProviderRefused(m),
        iris_core::Error::InvalidInput(m) => ApiError::BadRequest(m),
        other => ApiError::Internal(anyhow::anyhow!(other)),
    }
}

/// Probe a partial download by pre-fetching the byte ranges ffprobe needs:
/// the first 64 KiB (any container header) and the last 1 MiB (MKV Cues,
/// MP4 trailing `moov`, AVI `idx1`). librqbit writes those pieces to the
/// sparse file, so ffprobe reads real bytes instead of zero-pad.
/// `prefetch_range`'s sequential priority is per-stream and transient, so
/// this never slows the other torrents. A 30 s timeout covers a slow swarm;
/// on timeout the probe still runs on what arrived. Returns the header
/// bytes fetched (the stalled-swarm signal).
async fn prefetch_probe_ranges(
    state: &AppState,
    infohash: &str,
    idx: usize,
    size_bytes: u64,
    route: &'static str,
) -> u64 {
    const HEADER_BYTES: u64 = 1 << 16;
    const TAIL_BYTES: u64 = 1 << 20;
    const TIMEOUT: Duration = Duration::from_secs(30);
    let engine = state.engine();
    let tail_start = size_bytes.saturating_sub(TAIL_BYTES);
    let (h, t) = tokio::join!(
        engine.prefetch_range(infohash, idx, 0, HEADER_BYTES, TIMEOUT),
        engine.prefetch_range(infohash, idx, tail_start, TAIL_BYTES, TIMEOUT),
    );
    if let Err(e) = t {
        tracing::debug!(error = %e, route, "tail prefetch errored");
    }
    h.unwrap_or_else(|e| {
        tracing::debug!(error = %e, route, "header prefetch errored");
        0
    })
}

/// Refuse to probe what can't be read yet. Stalled swarm first: when the
/// prefetch couldn't pull a single header byte of an unfinished torrent
/// whose swarm is dead (no peers, no throughput), the retryable "not yet on
/// disk" would spin the client on "Reading media metadata…" with no hint
/// why — so a distinct, non-retryable `409` without the poll tokens lets
/// the UI say "no seeders" and stop.
fn ensure_probe_readable(
    state: &AppState,
    infohash: &str,
    path: &std::path::Path,
    torrent_finished: bool,
    header_bytes: u64,
) -> ApiResult<()> {
    if !torrent_finished
        && header_bytes == 0
        && let Some(s) = state.engine().get_by_infohash(infohash)
        && !s.finished
        && s.peers == 0
        && s.download_speed_bps == 0
    {
        return Err(ApiError::Conflict(format!(
            "stalled: no seeders for this file ({:.0}% downloaded, 0 peers, 0 B/s) — \
                     nothing to read yet",
            s.progress_pct
        )));
    }
    if !path.exists() {
        return Err(ApiError::BadRequest(format!(
            "file not yet on disk: {}",
            path.display()
        )));
    }
    Ok(())
}

/// `416` with the `bytes */{total}` the RFC asks for.
fn range_not_satisfiable(msg: &'static str, total: u64) -> Response {
    let mut resp = Response::new(Body::from(msg));
    *resp.status_mut() = StatusCode::RANGE_NOT_SATISFIABLE;
    resp.headers_mut().insert(
        header::CONTENT_RANGE,
        HeaderValue::from_str(&format!("bytes */{total}")).unwrap(),
    );
    resp
}

/// Bytes on disk are final: the DB stamp first, since during the post-deploy
/// `initializing` re-check the snapshot reports `finished = false` for
/// fully-downloaded torrents; a missing snapshot without the stamp (a grab
/// the engine hasn't registered yet) is still downloading.
fn torrent_finished(state: &AppState, row: &iris_db::torrents::TorrentRow) -> bool {
    row.finished_at.is_some()
        || state
            .engine()
            .get_by_infohash(&row.infohash)
            .is_some_and(|s| s.finished)
}

/// Record a torrent the engine just added, `fallback_name` standing in
/// until its metadata names it.
pub(crate) async fn record_ingest(
    state: &AppState,
    snapshot: &iris_torrent::TorrentSnapshot,
    fallback_name: impl FnOnce() -> String,
    source_provider: String,
    source_external_id: String,
    added_by: iris_core::ids::UserId,
) -> Result<iris_db::torrents::TorrentRow, sqlx::Error> {
    iris_db::torrents::upsert(
        state.db(),
        iris_db::torrents::NewTorrent {
            infohash: snapshot.infohash.clone(),
            name: snapshot.name.clone().unwrap_or_else(fallback_name),
            total_size_bytes: snapshot.total_size_bytes,
            tracker_tmdb_id: state
                .providers()
                .tracker_tmdb_id(&source_provider, &source_external_id)
                .and_then(|id| i64::try_from(id).ok()),
            source_provider: Some(source_provider),
            source_external_id: Some(source_external_id),
            added_by,
        },
    )
    .await
}

/// Take back an engine add whose grab failed before the torrent got its
/// library row: with no row, GC, delete and the leech-slot guard can't see
/// it, yet it downloads, seeds and comes back on every boot. A torrent the
/// engine already managed is left alone.
pub(crate) async fn discard_unrecorded(state: &AppState, result: &iris_torrent::IngestResult) {
    if result.already_managed {
        return;
    }
    if let Err(e) = state
        .engine()
        .delete_by_infohash(&result.snapshot.infohash, true)
        .await
    {
        tracing::warn!(
            infohash = %result.snapshot.infohash,
            error = %e,
            "grab: could not remove the torrent of a failed grab"
        );
    }
}

pub(crate) fn subtitle_cache_dir(cfg: &iris_config::AppConfig) -> std::path::PathBuf {
    cfg.storage.data_dir.join("subs")
}

/// Drop everything derived from a torrent's files: remux variants and
/// extracted subtitle tracks.
pub(crate) async fn wipe_derived(
    remuxer: &iris_media::RemuxManager,
    subs_dir: &std::path::Path,
    infohash: &str,
) {
    remuxer.wipe_torrent(infohash).await;
    iris_media::subtitles::wipe_torrent(subs_dir, infohash).await;
}

/// The archive gate for a torrent the engine just added: `.torrent` bytes
/// were checked before the add, a magnet only gets its file list now.
pub(crate) async fn reject_unstreamable(
    state: &AppState,
    result: &iris_torrent::IngestResult,
) -> ApiResult<()> {
    if iris_torrent::is_streamable(&result.snapshot.files) {
        return Ok(());
    }
    discard_unrecorded(state, result).await;
    Err(ApiError::ArchiveOnly)
}

/// The torrent row for an (already lowercased) infohash, or 404.
pub(crate) async fn torrent_or_404(
    state: &AppState,
    infohash: &str,
) -> ApiResult<iris_db::torrents::TorrentRow> {
    iris_db::torrents::find_by_infohash(state.db(), infohash)
        .await?
        .ok_or(ApiError::NotFound)
}

#[cfg(test)]
mod range_tests {
    use super::{byte_range, parse_range};
    use http::{HeaderMap, HeaderValue, header};

    fn headers(range: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::RANGE, HeaderValue::from_str(range).unwrap());
        h
    }

    #[test]
    fn a_non_byte_range_is_ignored() {
        assert!(byte_range(&headers("items=0-9")).is_none());
        assert!(byte_range(&HeaderMap::new()).is_none());
        let bytes = byte_range(&headers("bytes=0-9")).unwrap();
        assert_eq!(parse_range(&bytes, 100), Some((0, 9)));
    }

    #[test]
    fn byte_ranges_clamp_to_the_file() {
        let r = |s| HeaderValue::from_static(s);
        assert_eq!(parse_range(&r("bytes=90-200"), 100), Some((90, 99)));
        assert_eq!(parse_range(&r("bytes=-10"), 100), Some((90, 99)));
        assert_eq!(parse_range(&r("bytes=100-"), 100), None);
        assert_eq!(parse_range(&r("bytes=-0"), 100), None);
    }
}

#[cfg(test)]
mod engine_err_tests {
    use super::map_engine_err;
    use crate::error::ApiError;
    use iris_torrent::EngineError;

    #[test]
    fn engine_refusals_are_not_server_errors() {
        assert!(matches!(
            map_engine_err(EngineError::FileOutOfRange),
            ApiError::BadRequest(_)
        ));
        assert!(matches!(
            map_engine_err(EngineError::MetadataTimeout),
            ApiError::Upstream(_)
        ));
        assert!(matches!(
            map_engine_err(EngineError::NotFound),
            ApiError::NotFound
        ));
    }
}

#[cfg(test)]
mod tmdb_verify_tests {
    use super::tmdb_kind_hint;
    use crate::tmdb::TmdbKind;

    #[test]
    fn runtime_check_looks_the_id_up_in_the_collection_kind() {
        assert_eq!(tmdb_kind_hint(Some("tv")), Some(TmdbKind::Tv));
        assert_eq!(tmdb_kind_hint(Some("movie")), Some(TmdbKind::Movie));
        assert_eq!(tmdb_kind_hint(Some("anime")), None);
        assert_eq!(tmdb_kind_hint(None), None);
    }
}

#[cfg(test)]
mod video_mode_tests {
    use super::decide_video_mode;
    use iris_caps::ClientCapabilities;
    use iris_config::TranscodeConfig;
    use iris_media::{HdrKind, MediaProbe, VideoMode, VideoStream};

    fn probe(codec: &str, width: u32, height: u32, bit_depth: u8) -> MediaProbe {
        MediaProbe {
            container: "matroska".into(),
            duration_seconds: Some(1200.0),
            size_bytes: None,
            bit_rate: None,
            video: vec![VideoStream {
                index: 0,
                absolute_index: 0,
                codec: codec.into(),
                profile: None,
                level: None,
                width: Some(width),
                height: Some(height),
                bit_rate: None,
                frame_rate: None,
                frame_rate_num: None,
                frame_rate_den: None,
                bit_depth: Some(bit_depth),
                color_primaries: None,
                color_transfer: None,
                color_space: None,
                hdr: HdrKind::None,
                max_cll: None,
                max_fall: None,
            }],
            audio: Vec::new(),
            subtitle: Vec::new(),
        }
    }

    #[test]
    fn audio_renditions_map_the_file_stream_not_the_probe_position() {
        let audio = |index, absolute_index, language: &str| iris_media::AudioStream {
            index,
            absolute_index,
            codec: "aac".into(),
            channels: 2,
            channel_layout: None,
            sample_rate: None,
            language: Some(language.into()),
            title: None,
            default: false,
            forced: false,
            browser_compatible: true,
        };
        // Streams 0:1 jpn, 0:2 jpn (duplicate, dropped by the probe), 0:3 fre.
        let mut p = probe("h264", 1920, 1080, 8);
        p.audio = vec![audio(0, 1, "jpn"), audio(1, 3, "fre")];
        let plan = super::build_remux_plan(
            &p,
            &ClientCapabilities::default(),
            &TranscodeConfig::default(),
        );
        let streams: Vec<_> = plan
            .audio
            .iter()
            .map(|a| (a.language.as_str(), a.source_stream))
            .collect();
        assert_eq!(streams, [("jpn", 1), ("fre", 3)]);
    }

    // A 1440p H.264 file a TV-class decoder (level 4.2 = 1080p) rejects: the
    // client only reaches /play after direct play already failed, so a
    // stream-copy would hand back the exact frame size it refused → loop. Must
    // re-encode (the Transcode path caps output at 1080p).
    #[test]
    fn downscales_high_res_h264_for_a_real_client() {
        let caps = ClientCapabilities::parse("vdec=h264,hevc-hw; platform=android-tv-14");
        let mode = decide_video_mode(
            &probe("h264", 2560, 1440, 8),
            &caps,
            &TranscodeConfig::default(),
        );
        assert!(
            matches!(mode, VideoMode::Transcode { .. }),
            "1440p must downscale, got {mode:?}",
        );
    }

    #[test]
    fn copies_1080p_h264_directly() {
        let caps = ClientCapabilities::parse("vdec=h264,hevc-hw");
        let mode = decide_video_mode(
            &probe("h264", 1920, 1080, 8),
            &caps,
            &TranscodeConfig::default(),
        );
        assert_eq!(mode, VideoMode::Copy, "1080p H.264 must stream-copy");
    }

    // The caps-less prewarm hardcodes the bare `infohash_idx` (Copy) key, so it
    // MUST resolve to Copy even for a 1440p source — only a real client's
    // request triggers the downscale (under its own `_h264` key).
    #[test]
    fn prewarm_never_transcodes_high_res() {
        let mode = decide_video_mode(
            &probe("h264", 2560, 1440, 8),
            &ClientCapabilities::default(),
            &TranscodeConfig::default(),
        );
        assert_eq!(mode, VideoMode::Copy, "caps-less prewarm stays Copy");
    }

    #[test]
    fn av1_sw_10bit_still_transcodes() {
        let caps = ClientCapabilities::parse("vdec=h264,av1-sw");
        let mode = decide_video_mode(
            &probe("av1", 1920, 1080, 10),
            &caps,
            &TranscodeConfig::default(),
        );
        assert!(
            matches!(mode, VideoMode::Transcode { .. }),
            "10-bit AV1 on a software-only box still transcodes, got {mode:?}",
        );
    }
}
