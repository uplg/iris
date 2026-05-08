use axum::Json;
use axum::Router;
use axum::extract::{Query, State};
use axum::routing::get;
use iris_core::search::{SearchQuery, SortField, SortOrder};
use iris_providers::registry::AggregatedResults;
use serde::Deserialize;

use crate::error::ApiResult;
use crate::routes::extract::AuthUser;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(search))
}

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    pub q: String,
    pub page: Option<u32>,
    pub limit: Option<u32>,
    pub sort_by: Option<SortField>,
    pub order: Option<SortOrder>,
}

async fn search(
    State(state): State<AppState>,
    _user: AuthUser,
    Query(params): Query<SearchParams>,
) -> ApiResult<Json<AggregatedResults>> {
    let q = SearchQuery {
        q: params.q,
        page: params.page,
        limit: params.limit,
        sort_by: params.sort_by,
        order: params.order,
    };
    let agg = state.providers().search_all(&q).await;
    Ok(Json(agg))
}
