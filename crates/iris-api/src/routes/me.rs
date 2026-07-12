use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::get;
use chrono::{DateTime, Utc};
use iris_core::search::MediaKind;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::routes::extract::AuthUser;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(me))
        .route("/continue-watching", get(continue_watching))
        .route(
            "/continue-watching/dismiss",
            axum::routing::post(dismiss_continue_watching),
        )
        .route("/history", get(history))
        .route("/gone/dismiss", axum::routing::post(dismiss_gone))
        .route("/watchlist", get(watchlist))
        .route("/password", axum::routing::post(change_password))
        .route("/display-name", axum::routing::post(change_display_name))
}

#[derive(Debug, serde::Deserialize, ToSchema)]
pub(crate) struct ChangePasswordRequest {
    pub old_password: String,
    pub new_password: String,
}

#[utoipa::path(
    post,
    path = "/api/me/password",
    operation_id = "change_password",
    request_body = ChangePasswordRequest,
    responses(
        (status = 204, description = "Password changed; all other sessions revoked"),
        (status = 400, description = "New password too short (min 8 chars)"),
        (status = 401, description = "Old password mismatch / not authenticated"),
    ),
    tag = "me",
)]
pub(crate) async fn change_password(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<ChangePasswordRequest>,
) -> ApiResult<axum::http::StatusCode> {
    if body.new_password.len() < 8 {
        return Err(ApiError::BadRequest(
            "new password too short (min 8 chars)".into(),
        ));
    }
    let current = iris_db::users::get_password_hash(state.db(), user.id)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    let ok = iris_auth::verify_password(&body.old_password, &current)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("verify: {e}")))?;
    if !ok {
        return Err(ApiError::Unauthorized);
    }
    let new_hash = iris_auth::hash_password(&body.new_password)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("hash: {e}")))?;
    iris_db::users::update_password_hash(state.db(), user.id, &new_hash).await?;
    // Force every other session to log back in.
    iris_db::refresh_tokens::revoke_all_for_user(state.db(), user.id).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct MeResponse {
    id: Uuid,
    email: String,
    display_name: String,
    is_admin: bool,
}

#[utoipa::path(
    get,
    path = "/api/me",
    operation_id = "get_me",
    responses(
        (status = 200, description = "The authenticated user's profile", body = MeResponse),
        (status = 401, description = "Not authenticated"),
    ),
    tag = "me",
)]
pub(crate) async fn me(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<MeResponse>> {
    let u = iris_db::users::find_by_id(state.db(), user.id)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    Ok(Json(MeResponse {
        id: u.id.into(),
        email: u.email,
        display_name: u.display_name,
        is_admin: u.is_admin,
    }))
}

#[derive(Debug, serde::Deserialize, ToSchema)]
pub(crate) struct ChangeDisplayNameRequest {
    pub display_name: String,
}

#[utoipa::path(
    post,
    path = "/api/me/display-name",
    operation_id = "change_display_name",
    request_body = ChangeDisplayNameRequest,
    responses(
        (status = 204, description = "Display name updated"),
        (status = 400, description = "Empty / too-long display name (max 64)"),
    ),
    tag = "me",
)]
pub(crate) async fn change_display_name(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<ChangeDisplayNameRequest>,
) -> ApiResult<axum::http::StatusCode> {
    let trimmed = body.display_name.trim();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest("display name cannot be empty".into()));
    }
    if trimmed.len() > 64 {
        return Err(ApiError::BadRequest(
            "display name too long (max 64)".into(),
        ));
    }
    let _ = iris_db::users::update_display_name(state.db(), user.id, trimmed).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub(crate) struct ContinueWatchingItem {
    infohash: String,
    torrent_name: String,
    tmdb_id: Option<i64>,
    tmdb_verified: bool,
    /// `"movie"` / `"tv"` from the parent collection. Clients pass
    /// this to `/api/metadata/tmdb/{id}?kind=` — without it, TMDB's
    /// separate id namespaces collide and the lookup serves a
    /// stranger's poster.
    kind: Option<MediaKind>,
    file_idx: i64,
    file_path: Option<String>,
    position_seconds: f64,
    duration_seconds: Option<f64>,
    last_watched_at: chrono::DateTime<chrono::Utc>,
    completed: bool,
    /// Parent collection id (TV series). Sent back to
    /// `/api/me/continue-watching/dismiss` so "remove" hides the whole
    /// series, not just one episode. Null for movies / standalone.
    collection_id: Option<uuid::Uuid>,
    /// True when this is the NEXT unstarted episode (the previous one is
    /// finished) rather than a mid-way resume. Clients label it "Up next".
    next_up: bool,
}

/// Post-0.4 "My Watchlist" payload. Derived from TV collections
/// that have at least one ingested episode — the household
/// auto-tracks every show they're watching, no Follow button. The
/// shape mirrors what the legacy `/api/me/follows` façade returns
/// so the web client can flip endpoints without rewriting card
/// rendering. Old APK 0.3.1 keeps calling `/api/me/follows`.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct WatchlistItem {
    /// Collection id — clients route to `/collection/:id`.
    id: Uuid,
    /// SCENE-normalised name. Clients use this to detect "is this
    /// search result already on my Watchlist?" without having to
    /// run the same normaliser themselves.
    normalized_name: String,
    name: String,
    tmdb_id: Option<i64>,
    poster_path: Option<String>,
    backdrop_path: Option<String>,
    /// Distinct (season, episode) the indexer has surfaced since
    /// the requesting user last opened this collection. Drives the
    /// "X new" tile badge.
    new_count: i64,
    last_visited_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

#[utoipa::path(
    get,
    path = "/api/me/watchlist",
    operation_id = "list_watchlist",
    responses(
        (status = 200, description = "The caller's per-user Watchlist (TV collections)", body = [WatchlistItem]),
        (status = 401, description = "Not authenticated"),
    ),
    tag = "me",
)]
pub(crate) async fn watchlist(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<Vec<WatchlistItem>>> {
    // Per-user: this household now has ~10 viewers from different
    // families and "what's on the Watchlist" is personal. The shared
    // surface is the library (episode_files / available_episodes —
    // disk content is one copy for everyone); the per-user state
    // lives in `series_follows`, auto-created when the user grabs
    // or plays an episode (see `grab_episode_core`). The collection
    // it joins through is shared, so we still surface the same
    // poster + display title for everyone.
    let follows = iris_db::follows::list_for_user(state.db(), user.id).await?;
    let mut out = Vec::with_capacity(follows.len());
    for f in follows {
        // Each follow joins through its normalised name to a
        // (maybe present, maybe verified) TV collection so the tile
        // can borrow the canonical title + poster. Missing collection
        // = the user has a follow but no episodes ingested yet —
        // surface the follow's own name and let the poster slot stay
        // empty.
        let collection = iris_db::collections::find_by_parsed_title(
            state.db(),
            &f.normalized_name,
            iris_db::collections::Kind::Tv,
        )
        .await
        .unwrap_or(None);
        let (display_title, tmdb_id, collection_id) = match collection {
            Some(c) => (c.display_title, c.tmdb_id.or(f.tmdb_id), c.id),
            // No collection yet → route the tile to a hypothetical
            // collection path. The user will see the empty-state
            // until first ingest; this stays consistent with the
            // collection routing the rest of the UI uses.
            None => (f.name.clone(), f.tmdb_id, f.id),
        };
        // Watchlist is TV-only by construction (we derive it from
        // `series_follows`). Hint the TMDB namespace so the same
        // numerical id can't collide with an unrelated movie.
        let (poster_path, backdrop_path) = match (state.tmdb(), tmdb_id) {
            (Some(client), Some(tid)) => {
                #[allow(clippy::cast_sign_loss)]
                let meta = client
                    .lookup_with_kind(tid as u64, Some(crate::tmdb::TmdbKind::Tv))
                    .await;
                meta.map_or((None, None), |m| (m.poster_path, m.backdrop_path))
            }
            _ => (None, None),
        };
        let new_count = iris_db::available_episodes::count_new_for_series(
            state.db(),
            &f.normalized_name,
            f.last_visited_at,
        )
        .await
        .unwrap_or(0);
        out.push(WatchlistItem {
            id: collection_id,
            normalized_name: f.normalized_name,
            name: display_title,
            tmdb_id,
            poster_path,
            backdrop_path,
            new_count,
            last_visited_at: f.last_visited_at,
            created_at: f.created_at,
        });
    }
    Ok(Json(out))
}

#[utoipa::path(
    get,
    path = "/api/me/continue-watching",
    operation_id = "continue_watching",
    responses(
        (status = 200, description = "Recently-watched, not-yet-finished items for resume", body = [ContinueWatchingItem]),
        (status = 401, description = "Not authenticated"),
    ),
    tag = "me",
)]
pub(crate) async fn continue_watching(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<Vec<ContinueWatchingItem>>> {
    // Two sources: episodes/movies the user paused mid-way (resume), and the
    // NEXT owned episode for any series whose most-recent episode is finished
    // (next-up). Merge them into one shelf, one tile per series — a resume
    // tile wins over a next-up tile, and within a series the most-recently
    // active wins — then trim to the shelf size.
    let resume = iris_db::playback::continue_watching(state.db(), user.id, 24).await?;
    let next_up = iris_db::playback::continue_watching_next_up(state.db(), user.id, 24).await?;

    let mut merged: Vec<iris_db::playback::ContinueWatchingRow> = Vec::new();
    // collection_id → index into `merged`, so a series appears once.
    let mut by_collection: std::collections::HashMap<uuid::Uuid, usize> =
        std::collections::HashMap::new();
    // Resume rows first so they take precedence over a next-up tile for the
    // same series; both lists are already newest-first.
    for r in resume.into_iter().chain(next_up) {
        if let Some(cid) = r.collection_id {
            if let Some(&i) = by_collection.get(&cid) {
                // Keep whichever is more recent for this series.
                if r.last_watched_at > merged[i].last_watched_at {
                    merged[i] = r;
                }
                continue;
            }
            by_collection.insert(cid, merged.len());
        }
        merged.push(r);
    }
    merged.sort_by_key(|r| std::cmp::Reverse(r.last_watched_at));
    merged.truncate(12);

    let out = merged
        .into_iter()
        .map(|r| {
            let file_path = state.engine().get_by_infohash(&r.infohash).and_then(|s| {
                let file_idx = usize::try_from(r.file_idx).ok()?;
                s.files
                    .into_iter()
                    .find(|f| f.index == file_idx)
                    .map(|f| f.path)
            });
            ContinueWatchingItem {
                infohash: r.infohash,
                torrent_name: r.torrent_name,
                tmdb_id: r.tmdb_id,
                tmdb_verified: r.tmdb_verified,
                kind: r.kind.as_deref().and_then(MediaKind::from_wire),
                file_idx: r.file_idx,
                file_path,
                position_seconds: r.position_seconds,
                duration_seconds: r.duration_seconds,
                last_watched_at: r.last_watched_at,
                completed: r.completed,
                collection_id: r.collection_id,
                next_up: r.next_up,
            }
        })
        .collect();
    Ok(Json(out))
}

#[derive(Debug, serde::Deserialize, ToSchema)]
pub(crate) struct DismissCwRequest {
    /// Series to hide (TV). When set, the whole collection is removed from
    /// Continue Watching until the user plays a newer episode.
    collection_id: Option<uuid::Uuid>,
    /// Movie / standalone fallback: the exact file whose progress row is
    /// deleted. Ignored when `collection_id` is present.
    infohash: Option<String>,
    file_idx: Option<i64>,
}

#[utoipa::path(
    post,
    path = "/api/me/continue-watching/dismiss",
    request_body = DismissCwRequest,
    responses((status = 204, description = "Removed from the caller's Continue Watching")),
    tag = "me",
)]
pub(crate) async fn dismiss_continue_watching(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<DismissCwRequest>,
) -> ApiResult<axum::http::StatusCode> {
    if let Some(cid) = body.collection_id {
        // TV series — hide the whole collection (survives frontier regen).
        // Guard on existence: a stale id (series GC'd between the shelf fetch
        // and this call) has nothing to hide, and inserting it would trip the
        // `cw_dismissed` → `collections` foreign key. No-op is the right answer.
        if iris_db::collections::get(state.db(), cid).await?.is_some() {
            iris_db::playback::dismiss_collection(state.db(), user.id, cid).await?;
        }
    } else if let (Some(infohash), Some(file_idx)) = (body.infohash, body.file_idx) {
        // Movie / standalone — just drop the one progress row.
        iris_db::playback::delete(
            state.db(),
            user.id,
            &infohash.to_ascii_lowercase(),
            file_idx,
        )
        .await?;
    } else {
        return Err(ApiError::BadRequest(
            "need collection_id or (infohash, file_idx)".into(),
        ));
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[derive(Debug, serde::Deserialize, ToSchema)]
pub(crate) struct DismissGoneRequest {
    /// Whole ghost collection to hide from the caller's Library grid
    /// (the greyed-out "GONE" card). When set, `infohash` is ignored.
    collection_id: Option<uuid::Uuid>,
    /// One reclaimed release to hide from the caller's gone rows on the
    /// collection page (both the per-episode ghost rows and the raw
    /// release row).
    infohash: Option<String>,
}

/// Per-user, timestamped, never destructive: `playback_progress` (and
/// therefore the History page) is untouched, and newer activity makes
/// the dismissal stale so the entry returns — a release re-reclaimed
/// after a fresh download reappears, and a ghost collection the user
/// re-watches comes back to their Library.
#[utoipa::path(
    post,
    path = "/api/me/gone/dismiss",
    request_body = DismissGoneRequest,
    responses(
        (status = 204, description = "Hidden from the caller's Gone surfaces"),
        (status = 400, description = "Neither collection_id nor infohash provided"),
        (status = 401, description = "Not authenticated"),
    ),
    tag = "me",
)]
pub(crate) async fn dismiss_gone(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<DismissGoneRequest>,
) -> ApiResult<axum::http::StatusCode> {
    if let Some(cid) = body.collection_id {
        // Existence guard: a stale id has nothing to hide, and inserting
        // it would trip the `ghost_dismissed` → `collections` foreign key.
        if iris_db::collections::get(state.db(), cid).await?.is_some() {
            iris_db::collections::dismiss_ghost(state.db(), user.id, cid).await?;
        }
    } else if let Some(infohash) = body.infohash {
        let infohash = infohash.to_ascii_lowercase();
        // Same guard for the `gone_release_dismissed` → `torrents` FK.
        if iris_db::torrents::find_by_infohash(state.db(), &infohash)
            .await?
            .is_some()
        {
            iris_db::torrents::dismiss_gone_release(state.db(), user.id, &infohash).await?;
        }
    } else {
        return Err(ApiError::BadRequest(
            "need collection_id or infohash".into(),
        ));
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// One row of the caller's full watch history (in-progress AND completed),
/// including items whose source torrent has since been deleted
/// (disk-reclaim GC, admin cleanup) — unlike [`ContinueWatchingItem`], an
/// entry never just vanishes; `deleted` flags it instead so the client can
/// show "no longer available" rather than a dead resume link.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct HistoryItem {
    infohash: String,
    torrent_name: String,
    tmdb_id: Option<i64>,
    tmdb_verified: bool,
    kind: Option<MediaKind>,
    file_idx: i64,
    file_path: Option<String>,
    position_seconds: f64,
    duration_seconds: Option<f64>,
    last_watched_at: chrono::DateTime<chrono::Utc>,
    completed: bool,
    deleted: bool,
    /// Parent collection — survives GC (collections are never dropped),
    /// so clients group history under the show/movie and can route to
    /// the collection page even when every torrent was reclaimed (the
    /// "ghost collection" resume path). Additive — old clients ignore it.
    #[serde(default)]
    collection_id: Option<uuid::Uuid>,
    /// The collection's clean display title — the readable label to
    /// render instead of the raw SCENE torrent name.
    #[serde(default)]
    collection_title: Option<String>,
    /// SCENE-derived episode coordinates of the exact file watched
    /// (`S01E03`). Survive GC; a manual per-torrent remove drops them.
    #[serde(default)]
    season: Option<i64>,
    #[serde(default)]
    episode: Option<i64>,
    /// Absolute episode number for fleuve anime (render "Episode N").
    #[serde(default)]
    absolute_episode: Option<i64>,
    /// Source-torrent provenance: with both set, clients can offer
    /// "Download again" on a deleted row — re-resolving the same
    /// release yields the same infohash, so the stored resume
    /// position applies untouched.
    #[serde(default)]
    source_provider: Option<String>,
    #[serde(default)]
    source_external_id: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct HistoryQuery {
    /// Max rows to return (clamped 1..=200, defaults to 50).
    limit: Option<i64>,
    /// Pagination offset (defaults to 0).
    offset: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/api/me/history",
    operation_id = "list_my_history",
    params(HistoryQuery),
    responses(
        (status = 200, description = "Caller's full watch history, including deleted-source items", body = [HistoryItem]),
        (status = 401, description = "Not authenticated"),
    ),
    tag = "me",
)]
pub(crate) async fn history(
    State(state): State<AppState>,
    user: AuthUser,
    axum::extract::Query(q): axum::extract::Query<HistoryQuery>,
) -> ApiResult<Json<Vec<HistoryItem>>> {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let offset = q.offset.unwrap_or(0).max(0);
    let rows = iris_db::playback::user_history(state.db(), user.id, limit, offset).await?;
    let out = rows
        .into_iter()
        .map(|r| {
            let file_path = state.engine().get_by_infohash(&r.infohash).and_then(|s| {
                let file_idx = usize::try_from(r.file_idx).ok()?;
                s.files
                    .into_iter()
                    .find(|f| f.index == file_idx)
                    .map(|f| f.path)
            });
            HistoryItem {
                infohash: r.infohash,
                torrent_name: r.torrent_name,
                tmdb_id: r.tmdb_id,
                tmdb_verified: r.tmdb_verified,
                kind: r.kind.as_deref().and_then(MediaKind::from_wire),
                file_idx: r.file_idx,
                file_path,
                position_seconds: r.position_seconds,
                duration_seconds: r.duration_seconds,
                last_watched_at: r.last_watched_at,
                completed: r.completed,
                deleted: r.deleted,
                collection_id: r.collection_id,
                collection_title: r.collection_title,
                season: r.season,
                episode: r.episode,
                absolute_episode: r.absolute_episode,
                source_provider: r.source_provider,
                source_external_id: r.source_external_id,
            }
        })
        .collect();
    Ok(Json(out))
}
