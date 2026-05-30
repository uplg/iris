use axum::Json;
use axum::Router;
use axum::extract::{Query, State};
use axum::routing::get;
use iris_core::search::{MediaKind, SearchQuery, SortField, SortOrder, TorrentDetails};
use iris_providers::registry::AggregatedResults;
use serde::Deserialize;

use crate::error::{ApiError, ApiResult};
use crate::routes::extract::AuthUser;
use crate::ranking;
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
    // SCENE-style parse of the raw query. `"Classroom of the Elite S04E11"`
    // → `(parsed_title=classroom of the elite, season=4, episode=11)`. We
    // pass these as hints to providers (Torznab can dispatch
    // `t=tvsearch&season=&ep=`; UNIT3D/Torr9 append `SxxExx` to the name
    // filter) and use them again post-aggregation for relevance ranking.
    let parsed = iris_media::filename::parse(&params.q);
    let parsed_title = parsed
        .as_ref()
        .map(|p| iris_media::filename::series_key(&p.title));
    let q = SearchQuery {
        q: params.q,
        page: params.page,
        limit: params.limit,
        sort_by: params.sort_by,
        order: params.order,
        kind: params.kind,
        parsed_title,
        season: parsed.as_ref().and_then(|p| p.season),
        episode: parsed.as_ref().and_then(|p| p.episode),
        year: parsed.as_ref().and_then(|p| p.year),
    };
    let mut agg = state.providers().search_all(&q).await;
    // Drop non-video releases (games, music, books, software) — Iris can't
    // play them, so they're noise on the search page.
    agg.results
        .retain(iris_core::search::SearchResult::is_probably_video);
    // Library dedup index is a single round-trip per /api/search call;
    // if the DB hiccups we degrade gracefully (no dedup flags rather
    // than failing the search).
    let lib = ranking::LibraryIndex::load(state.db())
        .await
        .unwrap_or_else(|e| {
            tracing::warn!(error = %e, "search: failed to load library dedup index");
            ranking::LibraryIndex::empty()
        });
    ranking::rerank_results(&mut agg, &q, &lib);
    // Tag every result with its detected language so the UI can
    // render an FR / EN / MULTi badge per card. Cheap (a few token
    // scans per row) and the same detector the scheduler already
    // applies, so badges match what gets cached in
    // `available_episodes`.
    //
    // Falls back to the provider's configured `default_language`
    // when the filename carried no explicit tag — Seedpool ships
    // English implicitly (no marker) and an `Unknown` badge in
    // the UI would mean "no signal at all" instead of the real
    // "this is English by provider convention".
    for r in &mut agg.results {
        let detected = iris_media::filename::detect_language(&r.title);
        let resolved = if detected == iris_media::filename::Language::Unknown {
            state
                .providers()
                .default_language(&r.provider_id)
                .map_or(detected, iris_media::filename::Language::parse_tag)
        } else {
            detected
        };
        r.language = Some(resolved.as_str().to_string());
    }
    agg.parsed_query = ranking::parsed_query_summary(&q);
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
