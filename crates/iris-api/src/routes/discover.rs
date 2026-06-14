//! Discovery endpoints — featured carousels powering the Sorties shelves.
//!
//! Iterates every registered provider's `featured_movies` / `featured_series`
//! method and concatenates the results. Providers that don't implement
//! featured (default trait impls return empty) silently contribute nothing.
//! Provider failures are logged and skipped — discovery shouldn't block
//! the page on a flaky tracker.

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::get;
use iris_core::search::SearchResult;
use serde::Serialize;
use utoipa::ToSchema;

use crate::error::ApiResult;
use crate::routes::extract::AuthUser;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/featured", get(featured))
}

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct FeaturedResponse {
    movies: Vec<SearchResult>,
    series: Vec<SearchResult>,
}

#[utoipa::path(
    get,
    path = "/api/discover/featured",
    operation_id = "get_featured",
    responses((status = 200, description = "Featured movie + series carousels aggregated across providers", body = FeaturedResponse)),
    tag = "discover",
)]
pub(crate) async fn featured(
    State(state): State<AppState>,
    _user: AuthUser,
) -> ApiResult<Json<FeaturedResponse>> {
    let mut movies = Vec::new();
    let mut series = Vec::new();
    for id in state.providers().ids() {
        let Some(p) = state.providers().get(&id) else {
            continue;
        };
        match p.featured_movies().await {
            Ok(items) => movies.extend(items),
            Err(e) => tracing::warn!(provider = %id, error = %e, "featured_movies failed"),
        }
        match p.featured_series().await {
            Ok(items) => series.extend(items),
            Err(e) => tracing::warn!(provider = %id, error = %e, "featured_series failed"),
        }
    }
    Ok(Json(FeaturedResponse { movies, series }))
}
