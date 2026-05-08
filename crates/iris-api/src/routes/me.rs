use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::get;
use serde::Serialize;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::routes::extract::AuthUser;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(me))
        .route("/continue-watching", get(continue_watching))
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
    file_idx: i64,
    file_path: Option<String>,
    position_seconds: f64,
    duration_seconds: Option<f64>,
    last_watched_at: chrono::DateTime<chrono::Utc>,
    completed: bool,
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
                    s.files
                        .into_iter()
                        .find(|f| f.index == r.file_idx as usize)
                        .map(|f| f.path)
                });
            ContinueWatchingItem {
                infohash: r.infohash,
                torrent_name: r.torrent_name,
                tmdb_id: r.tmdb_id,
                tmdb_verified: r.tmdb_verified,
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
