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

async fn for_you(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<reco::ForYou>> {
    Ok(Json(reco::for_you(&state, user.id).await?))
}

async fn for_you_page(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<reco::ForYou>> {
    Ok(Json(reco::for_you_page(&state, user.id).await?))
}

#[derive(Debug, Deserialize)]
struct DismissRequest {
    catalog_id: Uuid,
}

async fn dismiss(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<DismissRequest>,
) -> ApiResult<StatusCode> {
    iris_db::reco_feedback::record(state.db(), user.id, body.catalog_id, "dismissed").await?;
    reco::invalidate(user.id);
    Ok(StatusCode::NO_CONTENT)
}
