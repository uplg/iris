//! `GET /api/me/moods`       — the curated mood board (tiles, taste-ordered).
//! `GET /api/me/moods/{id}`  — results for a mood, `?kind=movie|tv`.
//!
//! Moods are curated TMDB genre rules (`pulse::MOODS`); results are the
//! titles our trackers can serve, ranked by popularity, swarm and recency.
//! See `reco::mood_board` / `reco::mood_results`.

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use serde::Deserialize;
use utoipa::IntoParams;

use crate::error::ApiResult;
use crate::reco;
use crate::routes::extract::AuthUser;
use crate::state::AppState;
use crate::tmdb::TmdbKind;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(board))
        .route("/{id}", get(results))
}

#[derive(Debug, Deserialize, IntoParams)]
pub(crate) struct MoodQuery {
    /// `movie` (default) or `tv`.
    kind: Option<String>,
}

fn kind_of(q: &MoodQuery) -> TmdbKind {
    q.kind
        .as_deref()
        .and_then(TmdbKind::from_wire)
        .unwrap_or(TmdbKind::Movie)
}

#[utoipa::path(
    get,
    path = "/api/me/moods",
    operation_id = "get_mood_board",
    params(MoodQuery),
    responses((status = 200, description = "Curated mood board for the kind", body = reco::MoodBoard)),
    tag = "moods",
)]
pub(crate) async fn board(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<MoodQuery>,
) -> ApiResult<Json<reco::MoodBoard>> {
    Ok(Json(reco::mood_board(&state, user.id, kind_of(&q)).await?))
}

#[utoipa::path(
    get,
    path = "/api/me/moods/{id}",
    operation_id = "get_mood_results",
    params(("id" = String, Path, description = "Mood id (e.g. `scary`)"), MoodQuery),
    responses((status = 200, description = "Mood results", body = reco::MoodResults)),
    tag = "moods",
)]
pub(crate) async fn results(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    Query(q): Query<MoodQuery>,
) -> ApiResult<Json<reco::MoodResults>> {
    Ok(Json(
        reco::mood_results(&state, user.id, &id, kind_of(&q)).await?,
    ))
}
