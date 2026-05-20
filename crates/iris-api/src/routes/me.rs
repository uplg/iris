use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::get;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::routes::extract::AuthUser;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(me))
        .route("/continue-watching", get(continue_watching))
        .route("/watchlist", get(watchlist))
        .route("/password", axum::routing::post(change_password))
        .route("/display-name", axum::routing::post(change_display_name))
}

#[derive(Debug, serde::Deserialize)]
pub struct ChangePasswordRequest {
    pub old_password: String,
    pub new_password: String,
}

async fn change_password(
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

#[derive(Debug, Serialize)]
struct MeResponse {
    id: Uuid,
    email: String,
    display_name: String,
    is_admin: bool,
}

async fn me(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<MeResponse>> {
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

#[derive(Debug, serde::Deserialize)]
pub struct ChangeDisplayNameRequest {
    pub display_name: String,
}

async fn change_display_name(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<ChangeDisplayNameRequest>,
) -> ApiResult<axum::http::StatusCode> {
    let trimmed = body.display_name.trim();
    if trimmed.is_empty() {
        return Err(ApiError::BadRequest("display name cannot be empty".into()));
    }
    if trimmed.len() > 64 {
        return Err(ApiError::BadRequest("display name too long (max 64)".into()));
    }
    let _ = iris_db::users::update_display_name(state.db(), user.id, trimmed).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[derive(Debug, serde::Serialize)]
struct ContinueWatchingItem {
    infohash: String,
    torrent_name: String,
    tmdb_id: Option<i64>,
    tmdb_verified: bool,
    /// `"movie"` / `"tv"` from the parent collection. Clients pass
    /// this to `/api/metadata/tmdb/{id}?kind=` — without it, TMDB's
    /// separate id namespaces collide and the lookup serves a
    /// stranger's poster.
    kind: Option<String>,
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
#[derive(Debug, Serialize)]
struct WatchlistItem {
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

async fn watchlist(
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

async fn continue_watching(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<Vec<ContinueWatchingItem>>> {
    let rows = iris_db::playback::continue_watching(state.db(), user.id, 12).await?;
    let out = rows
        .into_iter()
        .map(|r| {
            let file_path = state
                .engine()
                .get_by_infohash(&r.infohash)
                .and_then(|s| {
                    let file_idx = usize::try_from(r.file_idx).ok()?;
                    s.files.into_iter().find(|f| f.index == file_idx).map(|f| f.path)
                });
            ContinueWatchingItem {
                infohash: r.infohash,
                torrent_name: r.torrent_name,
                tmdb_id: r.tmdb_id,
                tmdb_verified: r.tmdb_verified,
                kind: r.kind,
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
