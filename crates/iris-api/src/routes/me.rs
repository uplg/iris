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
}

#[derive(Debug, Serialize)]
struct MeResponse {
    id: Uuid,
    email: String,
    is_admin: bool,
}

async fn me(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<MeResponse>> {
    let u = iris_db::users::find_by_id(state.db(), user.id)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    Ok(Json(MeResponse {
        id: u.id.into(),
        email: u.email,
        is_admin: u.is_admin,
    }))
}

#[derive(Debug, serde::Serialize)]
struct ContinueWatchingItem {
    infohash: String,
    torrent_name: String,
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
