use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::routing::get;

use crate::error::{ApiError, ApiResult};
use crate::routes::extract::AuthUser;
use crate::state::AppState;
use crate::tmdb::MediaMetadata;

pub fn router() -> Router<AppState> {
    Router::new().route("/tmdb/{id}", get(tmdb_lookup))
}

async fn tmdb_lookup(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(id): Path<u64>,
) -> ApiResult<Json<MediaMetadata>> {
    let client = state.tmdb().ok_or_else(|| {
        ApiError::BadRequest(
            "TMDB enrichment is not configured (set [tmdb].api_key)".into(),
        )
    })?;
    client
        .lookup(id)
        .await
        .map(Json)
        .ok_or(ApiError::NotFound)
}
