//! `GET  /api/me/for-you`        — the home blended shelf.
//! `GET  /api/me/for-you/page`   — the organized "see all" page (sections).
//! `POST /api/me/for-you/dismiss`— hide a candidate from future shelves.
//!
//! Grabbing a card is NOT a dedicated endpoint: the client opens the same
//! preview dialog as a search hit (a rolling-window card carries its
//! recommended-best release's `provider_id`/`external_id`; a lazy
//! recommendation falls back to a title search), then grabs via the normal
//! `/api/torrents` ingest path — so the dead-torrent guard + recommended
//! selection are shared with search.

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::ApiResult;
use crate::reco;
use crate::routes::extract::AuthUser;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(for_you))
        .route("/page", get(for_you_page))
        .route("/dismiss", post(dismiss))
}

#[utoipa::path(
    get,
    path = "/api/me/for-you",
    operation_id = "get_for_you",
    responses((status = 200, description = "The home blended For You shelf", body = reco::ForYou)),
    tag = "for-you",
)]
pub(crate) async fn for_you(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<reco::ForYou>> {
    Ok(Json(reco::for_you(&state, user.id).await?))
}

#[utoipa::path(
    get,
    path = "/api/me/for-you/page",
    operation_id = "get_for_you_page",
    responses((status = 200, description = "The organized For You page (sectioned shelves)", body = reco::ForYou)),
    tag = "for-you",
)]
pub(crate) async fn for_you_page(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<reco::ForYou>> {
    Ok(Json(reco::for_you_page(&state, user.id).await?))
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct DismissRequest {
    catalog_id: Uuid,
}

#[utoipa::path(
    post,
    path = "/api/me/for-you/dismiss",
    operation_id = "dismiss_for_you",
    request_body = DismissRequest,
    responses((status = 204, description = "Candidate hidden from future shelves")),
    tag = "for-you",
)]
pub(crate) async fn dismiss(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<DismissRequest>,
) -> ApiResult<StatusCode> {
    iris_db::reco_feedback::record(state.db(), user.id, body.catalog_id, "dismissed").await?;
    reco::invalidate(user.id);
    Ok(StatusCode::NO_CONTENT)
}
