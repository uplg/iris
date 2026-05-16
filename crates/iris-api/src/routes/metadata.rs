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
        .route("/tmdb/resolve", get(tmdb_resolve))
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

#[derive(Debug, Deserialize)]
struct TmdbResolveParams {
    /// Raw SCENE release title, untouched
    /// (e.g. `Pride.2014.1080p.BluRay.x264-AMIABLE`). The backend parses
    /// title / year / kind out of it — clients send the release name
    /// verbatim so the cleaning + scoring logic lives in exactly one
    /// place instead of being reimplemented per client.
    title: String,
    /// Optional stronger kind hint from the search result's own
    /// classification (`movie` | `tv`); overrides the kind parsed from
    /// the release name.
    kind: Option<String>,
}

/// Resolve a release name to its single best TMDB match, scored by
/// **kind + year** (not raw popularity) and served from the persistent
/// 30-day resolve cache shared with the ingest/backfill pipeline.
///
/// This is the poster-resolution path for search-result cards. It
/// replaces the old client-side `extractSceneTitle` + `/tmdb/search` +
/// "take popularity #1" logic that was duplicated (and subtly fragile —
/// short titles like "Pride" collided with more-popular namesakes) in
/// both the web and TV clients. `/tmdb/search` stays a raw popularity
/// proxy for the live typeahead dropdown, which legitimately wants the
/// unfiltered suggestion list.
async fn tmdb_resolve(
    State(state): State<AppState>,
    _user: AuthUser,
    Query(params): Query<TmdbResolveParams>,
) -> ApiResult<Json<Option<TmdbSuggestion>>> {
    let Some(client) = state.tmdb() else {
        return Ok(Json(None));
    };
    let kind_hint = match params.kind.as_deref() {
        Some("tv") => Some(crate::tmdb::TmdbKind::Tv),
        Some("movie") => Some(crate::tmdb::TmdbKind::Movie),
        _ => None,
    };
    let resolved = crate::tmdb_resolve::resolve_release_name(
        state.db(),
        client,
        &params.title,
        kind_hint,
    )
    .await;
    Ok(Json(resolved.map(|r| TmdbSuggestion {
        kind: r.kind,
        tmdb_id: r.tmdb_id,
        title: r.title,
        year: r.year,
        overview: r.overview,
        poster_path: r.poster_path,
    })))
}
