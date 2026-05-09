use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use serde::Deserialize;

use crate::error::{ApiError, ApiResult};
use crate::routes::extract::AuthUser;
use crate::state::AppState;
use crate::tmdb::{MediaMetadata, TmdbSuggestion};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tmdb/{id}", get(tmdb_lookup))
        .route("/tmdb/search", get(tmdb_search))
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

#[derive(Debug, Deserialize)]
struct TmdbSearchParams {
    q: String,
}

/// Typeahead suggestions for the search page. Proxies TMDB's
/// `/search/multi` so the API key stays server-side. Empty array on no
/// results, missing TMDB config, or upstream failure (typeahead is best-
/// effort: never block typing on a flaky network).
async fn tmdb_search(
    State(state): State<AppState>,
    _user: AuthUser,
    Query(params): Query<TmdbSearchParams>,
) -> ApiResult<Json<Vec<TmdbSuggestion>>> {
    let Some(client) = state.tmdb() else {
        return Ok(Json(Vec::new()));
    };
    Ok(Json(client.multi_search(&params.q).await))
}
