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
        .route("/titles", get(titles))
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
    /// Keep only releases of this TMDB title (a title picked in the Titles
    /// view → its releases).
    pub tmdb_id: Option<u64>,
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
    /// TMDB poster path (`/abc.jpg`). Additive — older clients resolve it
    /// themselves.
    #[serde(default)]
    pub poster_path: Option<String>,
    /// The caller's progress: on the episode asked for when the query named
    /// one, else on the title. Additive.
    #[serde(default)]
    pub watch: Option<crate::routes::library::TitleWatch>,
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

/// A release page: the tracker's own details plus what Iris knows about the
/// release, so the page stands alone (no search results needed). `flatten`
/// keeps the wire shape of `TorrentDetails` for shipped clients.
#[derive(Debug, Serialize, ToSchema)]
pub struct ReleaseDetails {
    #[serde(flatten)]
    pub details: TorrentDetails,
    /// The TMDB title the release name resolves to. Additive.
    #[serde(default)]
    pub title_match: Option<iris_core::search::TitleMatch>,
    /// The title's poster, full URL. Additive.
    #[serde(default)]
    pub poster_url: Option<String>,
    /// The torrent grabbed from this release, when it is on disk. Additive.
    #[serde(default)]
    pub library_infohash: Option<String>,
    /// The file it plays (a movie's main video); `None` for a pack. Additive.
    #[serde(default)]
    pub library_file_idx: Option<i64>,
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
    user: AuthUser,
    Query(params): Query<SearchParams>,
) -> ApiResult<Json<SearchResponse>> {
    // SCENE-style parse of the raw query. `"Classroom of the Elite S04E11"`
    // → `(parsed_title=classroom of the elite, season=4, episode=11)`. We
    // pass these as hints to providers (Torznab can dispatch
    // `t=tvsearch&season=&ep=`; UNIT3D appends `SxxExx` to the name
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
        r.language_tag = crate::ranking::resolve_language_tag(r, state.providers())
            .map(|t| t.as_str().to_string());
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
    match_titles(&state, &mut agg.results).await;
    if let Some(id) = params.tmdb_id {
        agg.results
            .retain(|r| r.title_match.as_ref().is_some_and(|m| m.tmdb_id == id));
    }
    agg.parsed_query = ranking::parsed_query_summary(&q);
    let library_matches = library_matches_for(&state, &q, user.id).await;
    Ok(Json(SearchResponse {
        agg,
        library_matches,
    }))
}

/// Settle the TMDB title and poster each card shows, through the trust
/// gate (`tmdb_trust`): the release's own tracker id cross-checked with
/// the strict SCENE match. `title_match` is set only for a trusted match.
/// The poster:
///
/// 1. trusted match: the tracker's own poster when it ships one (picked by
///    the uploader for this release, resized when a tiny TMDB thumbnail),
///    else the trusted title's TMDB poster;
/// 2. no trusted match: the tracker's poster only when the tracker made no
///    TMDB claim at all — a poster riding on a rejected id goes with it.
pub(crate) async fn match_titles(
    state: &AppState,
    results: &mut [iris_core::search::SearchResult],
) {
    let Some(tmdb) = state.tmdb() else {
        return;
    };
    let wanted: Vec<_> = results
        .iter()
        .map(|r| (r.title.clone(), r.kind, r.tmdb_id))
        .collect();
    let matches = crate::fanout::map_ordered(wanted, |(title, kind, tracker)| async move {
        crate::tmdb_trust::trusted_release_match(
            state.db(),
            tmdb,
            &title,
            kind.map(Into::into),
            tracker,
        )
        .await
    })
    .await;
    for (r, m) in results.iter_mut().zip(matches) {
        let tracker_poster = r
            .poster_url
            .take()
            .map(|url| crate::tmdb::resized_poster(&url, crate::tmdb::POSTER_SIZE));
        r.poster_url = match m.as_ref() {
            Some(m) => tracker_poster.or_else(|| {
                let path = m.poster_path.as_deref()?;
                Some(crate::tmdb::image_url(path, crate::tmdb::POSTER_SIZE))
            }),
            None if r.tmdb_id.is_none() => tracker_poster,
            None => None,
        };
        r.title_match = m.map(title_match_of);
    }
}

fn title_match_of(m: crate::tmdb::TmdbSuggestion) -> iris_core::search::TitleMatch {
    iris_core::search::TitleMatch {
        tmdb_id: m.tmdb_id,
        kind: m.kind.into(),
        title: m.title,
        year: m.year,
        poster_path: m.poster_path,
    }
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct TitlesParams {
    pub q: String,
}

/// One title for the Titles view: what the query could mean on TMDB, and
/// whether the library already holds it.
#[derive(Debug, Serialize, ToSchema)]
pub struct TitleCard {
    pub tmdb_id: u64,
    pub kind: MediaKind,
    pub title: String,
    pub year: Option<u32>,
    pub overview: Option<String>,
    pub poster_url: Option<String>,
    /// The library collection holding this title, when there is one.
    pub collection_id: Option<uuid::Uuid>,
}

#[utoipa::path(
    get,
    path = "/api/search/titles",
    params(TitlesParams),
    responses((status = 200, description = "TMDB titles for the query, library ones flagged", body = [TitleCard])),
    tag = "search",
)]
pub(crate) async fn titles(
    State(state): State<AppState>,
    _user: AuthUser,
    Query(params): Query<TitlesParams>,
) -> ApiResult<Json<Vec<TitleCard>>> {
    let Some(tmdb) = state.tmdb() else {
        return Ok(Json(Vec::new()));
    };
    let suggestions = tmdb.multi_search(&params.q).await.unwrap_or_default();
    let state = &state;
    let cards = crate::fanout::map_ordered(suggestions, |s| async move {
        let kind = MediaKind::from(s.kind);
        let collection_id = match i64::try_from(s.tmdb_id) {
            Ok(id) => iris_db::collections::list_by_tmdb(state.db(), id)
                .await
                .unwrap_or_default()
                .into_iter()
                .find(|c| c.kind == kind.as_wire())
                .map(|c| c.id),
            Err(_) => None,
        };
        TitleCard {
            poster_url: s
                .poster_path
                .as_deref()
                .map(|p| crate::tmdb::image_url(p, crate::tmdb::POSTER_SIZE)),
            tmdb_id: s.tmdb_id,
            kind,
            title: s.title,
            year: s.year,
            overview: s.overview,
            collection_id,
        }
    })
    .await;
    Ok(Json(cards))
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
async fn library_matches_for(
    state: &AppState,
    q: &SearchQuery,
    user_id: iris_core::ids::UserId,
) -> Vec<LibraryMatch> {
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
    let wanted = summaries
        .into_iter()
        .filter(|c| q.kind.is_none_or(|k| c.kind == k.as_wire()));
    crate::fanout::map_ordered(wanted, |c| library_match(state, q, c, user_id))
        .await
        .into_iter()
        .flatten()
        .take(5)
        .collect()
}

/// One library row for the query, or `None` when the collection can't
/// answer it (an episode it doesn't own, a movie for an episode query).
async fn library_match(
    state: &AppState,
    query: &SearchQuery,
    summary: iris_db::collections::CollectionSummary,
    user_id: iris_core::ids::UserId,
) -> Option<LibraryMatch> {
    let mut hit = LibraryMatch {
        collection_id: summary.id.to_string(),
        display_title: summary.display_title,
        kind: summary.kind.clone(),
        tmdb_id: summary.tmdb_id,
        is_anime: summary.is_anime,
        torrent_count: summary.torrent_count,
        episode_count: summary.episode_count,
        representative_infohash: summary.representative_infohash,
        episode_season: None,
        episode_number: None,
        episode_infohash: None,
        episode_file_idx: None,
        season_episode_count: None,
        poster_path: None,
        watch: None,
    };
    match (summary.kind.as_str(), query.season, query.episode) {
        ("tv", season, Some(episode)) => {
            let ef = iris_db::episode_files::find_owned_episode(
                state.db(),
                summary.id,
                season.map(i64::from),
                i64::from(episode),
            )
            .await
            .ok()
            .flatten()?;
            hit.episode_season = Some(ef.season);
            hit.episode_number = Some(ef.episode);
            hit.episode_infohash = Some(ef.infohash);
            hit.episode_file_idx = Some(ef.file_idx);
        }
        ("tv", Some(season), None) => {
            let n = iris_db::episode_files::count_owned_in_season(
                state.db(),
                summary.id,
                i64::from(season),
            )
            .await
            .unwrap_or(0);
            if n == 0 {
                return None;
            }
            hit.episode_season = Some(i64::from(season));
            hit.season_episode_count = Some(n);
        }
        ("tv", None, None) => {}
        // A movie can't satisfy an episode-shaped query.
        (_, s, e) if s.is_some() || e.is_some() => return None,
        _ => {}
    }
    hit.poster_path = crate::routes::library::collection_artwork(state, hit.tmdb_id, &hit.kind)
        .await
        .0;
    hit.watch = match (&hit.episode_infohash, hit.episode_file_idx) {
        (Some(infohash), Some(file_idx)) => {
            iris_db::playback::get(state.db(), user_id, infohash, file_idx)
                .await
                .ok()
                .flatten()
                .map(|p| crate::routes::library::TitleWatch {
                    infohash: p.infohash,
                    file_idx: p.file_idx,
                    season: hit.episode_season,
                    episode: hit.episode_number,
                    position_seconds: p.position_seconds,
                    duration_seconds: p.duration_seconds,
                    completed: p.completed,
                    last_watched_at: p.last_watched_at,
                    watched_episodes: i64::from(p.completed),
                })
        }
        _ => crate::routes::library::title_watch(state, user_id, summary.id).await,
    };
    Some(hit)
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
        (status = 200, description = "Normalised torrent detail view", body = ReleaseDetails),
        (status = 400, description = "Unknown provider"),
        (status = 404, description = "Provider exposes no detail page for this id"),
        (status = 502, description = "The tracker's detail endpoint failed"),
    ),
    tag = "search",
)]
pub(crate) async fn details(
    State(state): State<AppState>,
    _user: AuthUser,
    Query(params): Query<DetailsParams>,
) -> ApiResult<Json<ReleaseDetails>> {
    let provider = state.provider(&params.provider)?;
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
            Ok(Json(release_details(&state, d).await))
        }
        // Provider doesn't expose a details endpoint — surface as a 404
        // so the frontend can hide the preview button cleanly.
        Ok(None) => Err(ApiError::NotFound),
        // A refusal is the tracker's answer, not a bug on our side — pass the
        // reason through instead of burying it in a 500.
        Err(iris_core::Error::ProviderRefused(m)) => {
            tracing::warn!(provider = %params.provider, id = %params.id, reason = %m, "details refused");
            Err(ApiError::ProviderRefused(m))
        }
        Err(e) => {
            tracing::warn!(provider = %params.provider, id = %params.id, error = %e, "details fetch failed");
            Err(details_failure(e))
        }
    }
}

/// A tracker that timed out or answered garbage is a 502, not a server bug.
fn details_failure(e: iris_core::Error) -> ApiError {
    match e {
        iris_core::Error::Provider(m) => ApiError::Upstream(m),
        e => ApiError::Internal(anyhow::anyhow!("details: {e}")),
    }
}

async fn release_details(state: &AppState, details: TorrentDetails) -> ReleaseDetails {
    let matched = match state.tmdb() {
        Some(tmdb) => {
            let tracker = state
                .providers()
                .tracker_tmdb_id(&details.provider_id, &details.external_id);
            crate::tmdb_trust::trusted_release_match(
                state.db(),
                tmdb,
                &details.title,
                None,
                tracker,
            )
            .await
        }
        None => None,
    };
    let poster_url = matched
        .as_ref()
        .and_then(|m| m.poster_path.as_deref())
        .map(|p| crate::tmdb::image_url(p, crate::tmdb::POSTER_SIZE));
    let owned = iris_db::torrents::find_live_by_source(
        state.db(),
        &details.provider_id,
        &details.external_id,
    )
    .await
    .ok()
    .flatten()
    .filter(|t| state.engine().contains(&t.infohash));
    let library_file_idx = owned.as_ref().and_then(|t| {
        let snap = state.engine().get_by_infohash(&t.infohash)?;
        let videos = snap
            .files
            .iter()
            .filter(|f| iris_torrent::is_main_video(&f.path))
            .count();
        (videos == 1)
            .then(|| iris_torrent::main_video_index(&snap.files))
            .flatten()
            .and_then(|i| i64::try_from(i).ok())
    });
    ReleaseDetails {
        details,
        title_match: matched.map(title_match_of),
        poster_url,
        library_infohash: owned.map(|t| t.infohash),
        library_file_idx,
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

#[cfg(test)]
mod tests {
    use axum::response::IntoResponse;
    use http::StatusCode;

    use super::details_failure;

    #[test]
    fn a_failing_tracker_reads_as_bad_gateway() {
        let res = details_failure(iris_core::Error::Provider("timed out".into())).into_response();
        assert_eq!(res.status(), StatusCode::BAD_GATEWAY);
        let res = details_failure(iris_core::Error::InvalidInput("x".into())).into_response();
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
