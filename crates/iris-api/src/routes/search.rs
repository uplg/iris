use axum::Json;
use axum::Router;
use axum::extract::{Query, State};
use axum::routing::get;
use iris_core::search::{
    DescriptionFormat, MediaKind, SearchQuery, SortField, SortOrder, TorrentDetails,
};
use iris_media::filename::{DubiousSource, detect_dubious_source};
use iris_providers::registry::AggregatedResults;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::error::{ApiError, ApiResult};
use crate::ranking;
use crate::routes::extract::AuthUser;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(search))
        .route("/details", get(details))
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct SearchParams {
    pub q: String,
    pub page: Option<u32>,
    pub limit: Option<u32>,
    pub sort_by: Option<SortField>,
    pub order: Option<SortOrder>,
    pub kind: Option<MediaKind>,
}

/// A library item matching the search query, surfaced ABOVE tracker
/// results by the clients ("you already have this"). Built from the
/// SCENE-normalised collection key, so a different release of the same
/// work matches — unlike the infohash-keyed `already_in_library` flag
/// on individual results, which deliberately only marks the exact
/// release (see `ranking.rs`: other languages/cuts must stay grabbable).
#[derive(Debug, Serialize, ToSchema)]
pub struct LibraryMatch {
    pub collection_id: String,
    pub display_title: String,
    /// `"movie"` | `"tv"` — same vocabulary as `MediaKind`.
    pub kind: String,
    pub tmdb_id: Option<i64>,
    pub is_anime: bool,
    pub torrent_count: i64,
    pub episode_count: i64,
    /// Fallback navigation target (most recently played torrent).
    pub representative_infohash: Option<String>,
    /// Set when the query named one specific episode the library owns:
    /// the exact owned file, ready for a direct `/watch` deep-link.
    pub episode_season: Option<i64>,
    pub episode_number: Option<i64>,
    pub episode_infohash: Option<String>,
    pub episode_file_idx: Option<i64>,
    /// Set when the query was season-scoped ("vikings s03"): how many
    /// episodes of that season the library holds.
    pub season_episode_count: Option<i64>,
}

/// `AggregatedResults` + the library rows. `flatten` keeps the wire
/// shape byte-compatible for deployed clients — `library_matches` is
/// purely additive (TV ignores unknown keys, TS fields are optional).
#[derive(Debug, Serialize, ToSchema)]
pub struct SearchResponse {
    #[serde(flatten)]
    pub agg: AggregatedResults,
    pub library_matches: Vec<LibraryMatch>,
}

#[utoipa::path(
    get,
    path = "/api/search",
    params(SearchParams),
    responses(
        (status = 200, description = "Aggregated tracker results + library matches", body = SearchResponse),
    ),
    tag = "search",
)]
pub(crate) async fn search(
    State(state): State<AppState>,
    _user: AuthUser,
    Query(params): Query<SearchParams>,
) -> ApiResult<Json<SearchResponse>> {
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
        let resolved = crate::ranking::resolve_language(r, state.providers());
        r.language = Some(resolved.as_str().to_string());
        r.codec = Some(
            iris_media::filename::detect_codec(&r.title)
                .as_str()
                .to_string(),
        );
        // CAM / TS / TC / screener warning. Rides the existing `tags`
        // field (front position so the web card's 4-chip cap keeps it)
        // — deliberately not a new response field, so already-shipped
        // clients surface the warning without an update.
        if let Some(src) = detect_dubious_source(&r.title) {
            r.tags.insert(0, dubious_tag(src));
        }
    }
    agg.parsed_query = ranking::parsed_query_summary(&q);
    let library_matches = library_matches_for(&state, &q).await;
    Ok(Json(SearchResponse {
        agg,
        library_matches,
    }))
}

/// Library rows relevant to this query — pertinence rules, so a series
/// card never drowns an episode-level search:
///
/// - bare title → every matching collection (movie or series);
/// - title + `SxxEyy` → ONLY collections owning that exact episode
///   (seasonal or anime-absolute), carrying the watch deep-link;
/// - title + season → ONLY collections owning ≥ 1 episode of that
///   season, with the honest per-season count;
/// - an episode-shaped query never surfaces movie collections.
///
/// Best-effort: any DB error degrades to "no library rows" rather than
/// failing the tracker search.
async fn library_matches_for(state: &AppState, q: &SearchQuery) -> Vec<LibraryMatch> {
    let Some(key) = q.parsed_title.as_deref().filter(|k| k.len() >= 2) else {
        return Vec::new();
    };
    let summaries = match iris_db::collections::search_summaries(state.db(), key, 8).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "search: library match lookup failed");
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    for c in summaries {
        if let Some(kind) = q.kind {
            let want = match kind {
                MediaKind::Movie => "movie",
                MediaKind::Tv => "tv",
            };
            if c.kind != want {
                continue;
            }
        }
        let mut m = LibraryMatch {
            collection_id: c.id.to_string(),
            display_title: c.display_title,
            kind: c.kind.clone(),
            tmdb_id: c.tmdb_id,
            is_anime: c.is_anime,
            torrent_count: c.torrent_count,
            episode_count: c.episode_count,
            representative_infohash: c.representative_infohash,
            episode_season: None,
            episode_number: None,
            episode_infohash: None,
            episode_file_idx: None,
            season_episode_count: None,
        };
        match (c.kind.as_str(), q.season, q.episode) {
            ("tv", season, Some(episode)) => {
                let hit = iris_db::episode_files::find_owned_episode(
                    state.db(),
                    c.id,
                    season.map(i64::from),
                    i64::from(episode),
                )
                .await
                .ok()
                .flatten();
                let Some(ef) = hit else { continue };
                m.episode_season = Some(ef.season);
                m.episode_number = Some(ef.episode);
                m.episode_infohash = Some(ef.infohash);
                m.episode_file_idx = Some(ef.file_idx);
            }
            ("tv", Some(season), None) => {
                let n = iris_db::episode_files::count_owned_in_season(
                    state.db(),
                    c.id,
                    i64::from(season),
                )
                .await
                .unwrap_or(0);
                if n == 0 {
                    continue;
                }
                m.episode_season = Some(i64::from(season));
                m.season_episode_count = Some(n);
            }
            ("tv", None, None) => {}
            // A movie can't satisfy an episode-shaped query.
            (_, s, e) if s.is_some() || e.is_some() => continue,
            _ => {}
        }
        out.push(m);
        if out.len() >= 5 {
            break;
        }
    }
    out
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct DetailsParams {
    /// Provider id from the search hit (`provider_id` field).
    pub provider: String,
    /// Provider-specific opaque id from the search hit (`external_id`).
    pub id: String,
}

/// Rich preview for a single torrent. Powers the search-result preview
/// dialog. Provider-specific shape is normalised to a single
/// `TorrentDetails` so web + TV consume one structure.
#[utoipa::path(
    get,
    path = "/api/search/details",
    params(DetailsParams),
    responses(
        (status = 200, description = "Normalised torrent detail view", body = TorrentDetails),
        (status = 400, description = "Unknown provider"),
        (status = 404, description = "Provider exposes no detail page for this id"),
    ),
    tag = "search",
)]
pub(crate) async fn details(
    State(state): State<AppState>,
    _user: AuthUser,
    Query(params): Query<DetailsParams>,
) -> ApiResult<Json<TorrentDetails>> {
    let provider = state
        .providers()
        .get(&params.provider)
        .ok_or_else(|| ApiError::BadRequest(format!("unknown provider `{}`", params.provider)))?;
    match provider.details(&params.id).await {
        Ok(Some(mut d)) => {
            // Same server-authored warning as the search cards, plus an
            // explanation prepended to the description — both clients
            // already render detail tags and the description, so shipped
            // APKs get the full "why is this dubious" context for free.
            if let Some(src) = detect_dubious_source(&d.title) {
                d.tags.insert(0, dubious_tag(src));
                d.description = Some(prepend_dubious_warning(
                    d.description.as_deref(),
                    d.description_format,
                    src,
                ));
            }
            Ok(Json(d))
        }
        // Provider doesn't expose a details endpoint — surface as a 404
        // so the frontend can hide the preview button cleanly.
        Ok(None) => Err(ApiError::NotFound),
        Err(e) => {
            tracing::warn!(provider = %params.provider, id = %params.id, error = %e, "details fetch failed");
            Err(ApiError::Internal(anyhow::anyhow!("details: {e}")))
        }
    }
}

fn dubious_tag(src: DubiousSource) -> String {
    format!("⚠️ Dubious: {}", src.label())
}

/// Prepend the dubious-source explanation to a detail description,
/// speaking the description's own markup dialect so every renderer
/// (web BBCode/HTML/plain, TV tag-stripper) shows it as styled text.
fn prepend_dubious_warning(
    desc: Option<&str>,
    format: DescriptionFormat,
    src: DubiousSource,
) -> String {
    let expl = src.explanation();
    let warning = match format {
        DescriptionFormat::Bbcode | DescriptionFormat::Plain => {
            format!("⚠️ Dubious quality — {expl}.")
        }
        DescriptionFormat::Html => {
            format!("<p><strong>⚠️ Dubious quality</strong> — {expl}.</p>")
        }
    };
    match desc {
        Some(body) if !body.trim().is_empty() => match format {
            DescriptionFormat::Bbcode | DescriptionFormat::Plain => {
                format!("{warning}\n\n{body}")
            }
            DescriptionFormat::Html => format!("{warning}{body}"),
        },
        _ => warning,
    }
}
