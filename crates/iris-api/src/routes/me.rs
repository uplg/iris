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
        .route("/history", get(history))
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
    let rows = iris_db::playback::continue_watching(state.db(), user.id, 12).await?;
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
            }
        })
        .collect();
    Ok(Json(out))
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
            }
        })
        .collect();
    Ok(Json(out))
}
