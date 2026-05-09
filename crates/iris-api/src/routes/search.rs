use axum::Json;
use axum::Router;
use axum::extract::{Query, State};
use axum::routing::get;
use iris_core::search::{MediaKind, SearchQuery, SortField, SortOrder, TorrentDetails};
use iris_providers::registry::AggregatedResults;
use serde::Deserialize;

use crate::error::{ApiError, ApiResult};
use crate::routes::extract::AuthUser;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(search))
        .route("/details", get(details))
}

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    pub q: String,
    pub page: Option<u32>,
    pub limit: Option<u32>,
    pub sort_by: Option<SortField>,
    pub order: Option<SortOrder>,
    pub kind: Option<MediaKind>,
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
        kind: params.kind,
    };
    let agg = state.providers().search_all(&q).await;
    Ok(Json(agg))
}

#[derive(Debug, Deserialize)]
pub struct DetailsParams {
    /// Provider id from the search hit (`provider_id` field).
    pub provider: String,
    /// Provider-specific opaque id from the search hit (`external_id`).
    pub id: String,
}

/// Rich preview for a single torrent. Powers the search-result preview
/// dialog. Provider-specific shape is normalised to a single
/// `TorrentDetails` so web + TV consume one structure.
async fn details(
    State(state): State<AppState>,
    _user: AuthUser,
    Query(params): Query<DetailsParams>,
) -> ApiResult<Json<TorrentDetails>> {
    let provider = state
        .providers()
        .get(&params.provider)
        .ok_or_else(|| ApiError::BadRequest(format!("unknown provider `{}`", params.provider)))?;
    match provider.details(&params.id).await {
        Ok(Some(d)) => Ok(Json(d)),
        // Provider doesn't expose a details endpoint — surface as a 404
        // so the frontend can hide the preview button cleanly.
        Ok(None) => Err(ApiError::NotFound),
        Err(e) => {
            tracing::warn!(provider = %params.provider, id = %params.id, error = %e, "details fetch failed");
            Err(ApiError::Internal(anyhow::anyhow!("details: {e}")))
        }
    }
}
