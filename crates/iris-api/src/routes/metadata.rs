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

#[derive(Debug, Deserialize)]
struct TmdbLookupParams {
    /// `"movie"` | `"tv"`. Without a kind, the lookup tries movie
    /// first then TV — but TMDB uses separate id namespaces, so a
    /// numerical id can collide between a movie and an unrelated TV
    /// show. Pass the kind whenever the caller knows it
    /// (collection.kind, search-result kind, etc.) to disambiguate.
    kind: Option<String>,
}

async fn tmdb_lookup(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(id): Path<u64>,
    Query(params): Query<TmdbLookupParams>,
) -> ApiResult<Json<MediaMetadata>> {
    let client = state.tmdb().ok_or_else(|| {
        ApiError::BadRequest(
            "TMDB enrichment is not configured (set [tmdb].api_key)".into(),
        )
    })?;
    let kind_hint = match params.kind.as_deref() {
        Some("tv") => Some(crate::tmdb::TmdbKind::Tv),
        Some("movie") => Some(crate::tmdb::TmdbKind::Movie),
        _ => None,
    };
    client
        .lookup_with_kind(id, kind_hint)
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
