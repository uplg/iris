//! TMDB metadata client + memory cache.
//!
//! We hit `themoviedb.org` for poster/backdrop/year/overview when a search
//! result carries a `tmdb_id`. Every response is cached in memory through a
//! bounded [`TtlCache`]: TMDB metadata barely changes for a given id, so the
//! TTLs are long, but the maps can't grow without bound and a failed fetch
//! is retried rather than remembered.

use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::ttl_cache::TtlCache;

#[derive(Clone)]
pub struct TmdbClient {
    inner: Arc<Inner>,
}

struct Inner {
    api_key: String,
    http: reqwest::Client,
    /// Kind-aware cache. Key is `(id, kind?)` so `/movie/X` and
    /// `/tv/X` get separate cache slots — TMDB uses separate id
    /// namespaces and a flat-id cache served the wrong entry
    /// whenever the two namespaces collided (Silicon Valley TV id =
    /// some unrelated movie id, etc.).
    typed_cache: TtlCache<(u64, Option<&'static str>), CacheEntry>,
    /// Season episode lists, keyed by `(tmdb_id, season_number)`. Shorter
    /// TTL than ids: air dates of upcoming episodes do get corrected.
    seasons: TtlCache<(u64, u32), Vec<EpisodeMetadata>>,
    /// Lowercase-keyed `multi_search` / `search_typed` results. The
    /// typeahead and the SCENE resolver repeat the same queries across
    /// pages and users; empty results are cached too so "no hits" doesn't
    /// re-issue.
    searches: TtlCache<String, Vec<TmdbSuggestion>>,
    /// Genre taxonomy per kind. Near-static, refreshed daily so the
    /// onboarding picker still picks up the rare addition.
    genres: TtlCache<&'static str, Vec<Genre>>,
    /// List endpoints (trending / discover / on the air), keyed by path +
    /// query. Shorter than the pulse cycle so each pass sees fresh lists.
    lists: TtlCache<String, Vec<MediaMetadata>>,
}

const ID_TTL: Duration = Duration::from_hours(24);
const SEASON_TTL: Duration = Duration::from_hours(12);
const SEARCH_TTL: Duration = Duration::from_hours(1);
const GENRE_TTL: Duration = Duration::from_hours(24);
const LIST_TTL: Duration = Duration::from_hours(2);

#[derive(Clone)]
enum CacheEntry {
    // Boxed: MediaMetadata is much larger than the empty NotFound variant.
    Found(Box<MediaMetadata>),
    /// TMDB answered 404 for both namespaces. Only a definitive answer is
    /// cached this way; a network error caches nothing.
    NotFound,
}

/// Why a TMDB request produced no value.
enum Miss {
    /// TMDB answered 404: the id or path doesn't exist.
    NotFound,
    /// Network error, refusal or unparsable body: worth retrying later.
    Failed,
}

/// Filters for a mood's [`TmdbClient::discover`] query. Id lists are in
/// TMDB syntax: `,` = AND, `|` = OR (the query encoder escapes it).
#[derive(Debug, Clone)]
pub struct DiscoverFilter {
    pub with_genres: String,
    pub without_genres: String,
    /// Earliest (first) release date, `YYYY-MM-DD`.
    pub since: chrono::NaiveDate,
    pub min_votes: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum TmdbKind {
    Movie,
    Tv,
}

impl TmdbKind {
    /// Parse the `"movie"` / `"tv"` wire form used by collections, query
    /// params and TMDB's own `media_type`.
    pub fn from_wire(s: &str) -> Option<Self> {
        iris_core::search::MediaKind::from_wire(s).map(Self::from)
    }

    /// The wire form, which is also TMDB's path segment (`/movie/…`, `/tv/…`).
    pub const fn as_wire(self) -> &'static str {
        match self {
            Self::Movie => "movie",
            Self::Tv => "tv",
        }
    }
}

impl From<iris_core::search::MediaKind> for TmdbKind {
    fn from(kind: iris_core::search::MediaKind) -> Self {
        match kind {
            iris_core::search::MediaKind::Movie => Self::Movie,
            iris_core::search::MediaKind::Tv => Self::Tv,
        }
    }
}

impl From<TmdbKind> for iris_core::search::MediaKind {
    fn from(kind: TmdbKind) -> Self {
        match kind {
            TmdbKind::Movie => Self::Movie,
            TmdbKind::Tv => Self::Tv,
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TmdbSuggestion {
    pub kind: TmdbKind,
    pub tmdb_id: u64,
    pub title: String,
    /// TMDB's `original_title` / `original_name`, when it differs from
    /// `title`. The strict SCENE match accepts either. Additive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_title: Option<String>,
    pub year: Option<u32>,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    /// TMDB's vote count: the strict SCENE match's tie-break between exact
    /// homonyms. Server-side only, never serialised.
    #[serde(skip)]
    pub vote_count: Option<u32>,
}

/// One entry of TMDB's genre taxonomy (`/genre/{movie,tv}/list`). Powers
/// the onboarding genre picker; the `id` is what we persist in a user's
/// `genres` preference and later feed to `/discover` as `with_genres`.
#[derive(Debug, Clone, Serialize)]
pub struct Genre {
    pub id: u32,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EpisodeMetadata {
    pub season: u32,
    pub episode: u32,
    pub name: Option<String>,
    pub overview: Option<String>,
    /// `YYYY-MM-DD` per TMDB. Kept as a string — callers rarely need a real
    /// `Date` and the only branching we do is "in the past?" which is a
    /// trivial lex-compare against today.
    pub air_date: Option<String>,
    pub runtime_minutes: Option<u32>,
    pub still_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct MediaMetadata {
    pub kind: TmdbKind,
    pub tmdb_id: u64,
    pub title: String,
    pub overview: Option<String>,
    pub year: Option<u32>,
    /// Path component, e.g. `/abc.jpg`. Combine with size + base URL to form
    /// the actual image URL: `https://image.tmdb.org/t/p/<size><poster_path>`.
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    /// 0..1 (TMDB's `vote_average` is /10, normalized here for the UI).
    pub vote_score: Option<f64>,
    pub vote_count: Option<u32>,
    pub genres: Vec<String>,
    /// Movie runtime in minutes (TMDB `runtime`). For TV shows we expose
    /// the *typical* episode runtime here when one is published; both
    /// can drift wildly from the actual file's duration so callers do
    /// the verification themselves.
    pub runtime_minutes: Option<u32>,
    /// TV shows only — total seasons published. Used at follow time to
    /// snapshot how many seasons we expect, so the Series page can pre-
    /// render season tabs without waiting on a fresh TMDB lookup.
    pub number_of_seasons: Option<u32>,
    /// TMDB popularity score (relative, unbounded). Ranks catalogue
    /// candidates in the recommendation pipeline.
    pub popularity: Option<f64>,
    /// ISO 639-1 original language ("fr" / "en" / …). Drives per-user
    /// language filtering of the catalogue.
    pub original_language: Option<String>,
    /// TMDB genre ids. Always present from list endpoints (discover /
    /// trending); derived from the full genre objects on detail lookups.
    pub genre_ids: Vec<u32>,
    /// `YYYY-MM-DD` release / first-air date, kept raw alongside the
    /// parsed `year`.
    pub release_date: Option<String>,
}

impl TmdbClient {
    pub fn new(api_key: String) -> anyhow::Result<Self> {
        let http = iris_providers::tls::client_builder()
            .timeout(Duration::from_secs(10))
            .build()?;
        Ok(Self {
            inner: Arc::new(Inner {
                api_key,
                http,
                typed_cache: TtlCache::new(ID_TTL, 4096),
                seasons: TtlCache::new(SEASON_TTL, 2048),
                searches: TtlCache::new(SEARCH_TTL, 2048),
                genres: TtlCache::new(GENRE_TTL, 4),
                lists: TtlCache::new(LIST_TTL, 256),
            }),
        })
    }

    /// GET `https://api.themoviedb.org/3/{path}` with the API key added to
    /// `query`. `what` names the call in logs; the URL never is (it carries
    /// the key), so errors are logged through `iris_providers::redact`.
    async fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
        what: &str,
    ) -> Result<T, Miss> {
        let res = self
            .inner
            .http
            .get(format!("https://api.themoviedb.org/3/{path}"))
            .query(&[("api_key", self.inner.api_key.as_str())])
            .query(query)
            .send()
            .await
            .map_err(|e| {
                tracing::warn!(error = %iris_providers::redact(e), what, "tmdb request failed");
                Miss::Failed
            })?;
        if res.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(Miss::NotFound);
        }
        if !res.status().is_success() {
            tracing::warn!(status = %res.status(), what, "tmdb request refused");
            return Err(Miss::Failed);
        }
        res.json().await.map_err(|e| {
            tracing::warn!(error = %iris_providers::redact(e), what, "tmdb response unparsable");
            Miss::Failed
        })
    }
}

/// Poster width for cards (search, library): sharp at TV distance, light
/// on phones.
pub const POSTER_SIZE: &str = "w342";
/// Backdrop width for hero banners and wide cards.
pub const BACKDROP_SIZE: &str = "w780";
const IMAGE_BASE: &str = "https://image.tmdb.org/t/p/";

/// Full URL of a TMDB image path (`/abc.jpg`) at `size`.
pub fn image_url(path: &str, size: &str) -> String {
    format!("{IMAGE_BASE}{size}{path}")
}

/// A poster URL at `size` when it is a TMDB image (trackers often ship
/// `w92`, unreadable on a card); any other URL is kept as is.
pub fn resized_poster(url: &str, size: &str) -> String {
    url.strip_prefix(IMAGE_BASE)
        .and_then(|rest| rest.split_once('/'))
        .map_or_else(
            || url.to_owned(),
            |(_, path)| image_url(&format!("/{path}"), size),
        )
}

/// Year of a TMDB `YYYY-MM-DD` date.
fn year_of(date: Option<&str>) -> Option<u32> {
    date?.split('-').next()?.parse().ok()
}

impl TmdbMultiResult {
    /// The suggestion for this result, typed as `kind`.
    fn into_suggestion(self, kind: TmdbKind) -> Option<TmdbSuggestion> {
        let title = self.title.or(self.name)?;
        let original_title = self
            .original_title
            .or(self.original_name)
            .filter(|o| *o != title);
        let date = self.release_date.or(self.first_air_date);
        Some(TmdbSuggestion {
            kind,
            tmdb_id: self.id,
            title,
            original_title,
            year: year_of(date.as_deref()),
            overview: self.overview.filter(|s| !s.is_empty()),
            poster_path: self.poster_path,
            vote_count: self.vote_count,
        })
    }
}

impl TmdbClient {
    /// Multi-search across movies + TV shows. Powers the search-page
    /// typeahead — the user types a few characters, we surface "did you
    /// mean X (2024)" suggestions tied to a TMDB id so a click runs an
    /// indexer search with the cleaned title (and optionally remembers
    /// the tmdb id for the eventual ingest). People results filtered out.
    /// `None` when TMDB couldn't be asked (an empty list is a real answer).
    pub async fn multi_search(&self, query: &str) -> Option<Vec<TmdbSuggestion>> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Some(Vec::new());
        }
        self.inner
            .searches
            .get_or_fetch(trimmed.to_lowercase(), || async {
                let raw: TmdbMultiRaw = self
                    .get_json(
                        "search/multi",
                        &[
                            ("query", trimmed),
                            ("include_adult", "false"),
                            ("page", "1"),
                        ],
                        "multi-search",
                    )
                    .await
                    .ok()?;
                Some(
                    raw.results
                        .into_iter()
                        .filter_map(|r| {
                            // People and unknown media types are skipped.
                            let kind = r.media_type.as_deref().and_then(TmdbKind::from_wire)?;
                            r.into_suggestion(kind)
                        })
                        .take(10)
                        .collect(),
                )
            })
            .await
    }

    /// Typed, year-targeted search. `/search/multi` ranks by raw
    /// popularity across both media kinds, so a common title gets
    /// drowned: searching "Midnight" returns nine unrelated TV shows
    /// plus "Midnight Matinee" (1988) within the page-1 cutoff and the
    /// actual "Midnight" (2021) movie never appears at all. Hitting the
    /// typed endpoint (`/search/{movie,tv}`) with TMDB's own year filter
    /// returns year-correct candidates with the exact title ranked
    /// first. Used by the SCENE → TMDB resolver, which always knows the
    /// kind (decided at ingest) and usually the year (from the release
    /// name). Shares the `multi_search` cache, namespaced by kind + year
    /// so the keyspaces never collide. `None` when TMDB couldn't be asked.
    pub async fn search_typed(
        &self,
        query: &str,
        kind: TmdbKind,
        year: Option<u32>,
    ) -> Option<Vec<TmdbSuggestion>> {
        self.search_typed_in(query, kind, year, None).await
    }

    /// [`Self::search_typed`] with TMDB's titles in `language` (`fr-FR`):
    /// the match is the same, only `title` is the localized one.
    pub async fn search_typed_in(
        &self,
        query: &str,
        kind: TmdbKind,
        year: Option<u32>,
        language: Option<&str>,
    ) -> Option<Vec<TmdbSuggestion>> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Some(Vec::new());
        }
        let marker = kind.as_wire();
        let cache_key = format!(
            "{marker}:{}:{}:{}",
            language.unwrap_or(""),
            year.unwrap_or(0),
            trimmed.to_lowercase()
        );
        self.inner
            .searches
            .get_or_fetch(cache_key, || async {
                // TMDB's canonical year filters for the typed search endpoints.
                let year_param = match kind {
                    TmdbKind::Movie => "primary_release_year",
                    TmdbKind::Tv => "first_air_date_year",
                };
                let year_str = year.map(|y| y.to_string());
                let mut params = vec![
                    ("query", trimmed),
                    ("include_adult", "false"),
                    ("page", "1"),
                ];
                if let Some(y) = year_str.as_deref() {
                    params.push((year_param, y));
                }
                if let Some(lang) = language {
                    params.push(("language", lang));
                }
                let raw: TmdbMultiRaw = self
                    .get_json(&format!("search/{marker}"), &params, "typed-search")
                    .await
                    .ok()?;
                // Typed endpoints omit `media_type` — the kind is the one we
                // asked for, so stamp it directly rather than reading the field.
                Some(
                    raw.results
                        .into_iter()
                        .filter_map(|r| r.into_suggestion(kind))
                        .take(10)
                        .collect(),
                )
            })
            .await
    }

    /// Fetch TMDB's canonical genre taxonomy for `kind` (movies or TV).
    /// Powers the onboarding genre picker. Returns an empty list on any
    /// error (the caller renders an empty picker rather than failing
    /// onboarding).
    pub async fn genre_list(&self, kind: TmdbKind) -> Vec<Genre> {
        let marker = kind.as_wire();
        self.inner
            .genres
            .get_or_fetch(marker, || async {
                let raw: TmdbGenreListRaw = self
                    .get_json(
                        &format!("genre/{marker}/list"),
                        &[("language", "en-US")],
                        "genre list",
                    )
                    .await
                    .ok()?;
                Some(
                    raw.genres
                        .into_iter()
                        .map(|g| Genre {
                            id: g.id,
                            name: g.name,
                        })
                        .collect(),
                )
            })
            .await
            .unwrap_or_default()
    }

    /// `/trending/{movie,tv}/week` — TMDB's short-window activity
    /// ranking (page views, votes, watchlist adds), the always-available
    /// "what's hot now" signal. French titles (`fr-FR`) so the pulse job can
    /// also search francophone trackers by their local title.
    pub async fn trending(&self, kind: TmdbKind, page: u32) -> Vec<MediaMetadata> {
        let endpoint = kind.as_wire();
        let page = page.to_string();
        self.fetch_list(
            &format!("trending/{endpoint}/week"),
            &[("language", "fr-FR"), ("page", &page)],
            kind,
        )
        .await
    }

    /// Films whose French digital release (VOD / streaming, release type 4)
    /// falls in `[since, until]` — what just became watchable at home, and so
    /// what the trackers are about to carry or already do.
    pub async fn digital_releases_fr(
        &self,
        since: chrono::NaiveDate,
        until: chrono::NaiveDate,
        page: u32,
    ) -> Vec<MediaMetadata> {
        let (since, until, page) = (since.to_string(), until.to_string(), page.to_string());
        self.fetch_list(
            "discover/movie",
            &[
                ("language", "fr-FR"),
                ("region", "FR"),
                ("with_release_type", "4"),
                ("release_date.gte", &since),
                ("release_date.lte", &until),
                ("sort_by", "popularity.desc"),
                ("include_adult", "false"),
                ("vote_count.gte", "5"),
                ("page", &page),
            ],
            TmdbKind::Movie,
        )
        .await
    }

    /// `/tv/on_the_air` — series with an episode airing in the next week.
    pub async fn on_the_air(&self, page: u32) -> Vec<MediaMetadata> {
        let page = page.to_string();
        self.fetch_list(
            "tv/on_the_air",
            &[
                ("language", "fr-FR"),
                ("timezone", "Europe/Paris"),
                ("page", &page),
            ],
            TmdbKind::Tv,
        )
        .await
    }

    /// `/discover/{movie,tv}` for a mood: genre filters inside a
    /// recency window, popularity-sorted, with a vote floor against noise.
    pub async fn discover(
        &self,
        kind: TmdbKind,
        filter: &DiscoverFilter,
        page: u32,
    ) -> Vec<MediaMetadata> {
        let endpoint = kind.as_wire();
        let date_param = match kind {
            TmdbKind::Movie => "primary_release_date.gte",
            TmdbKind::Tv => "first_air_date.gte",
        };
        let (min_votes, since, page) = (
            filter.min_votes.to_string(),
            filter.since.to_string(),
            page.to_string(),
        );
        let mut query = vec![
            ("language", "fr-FR"),
            ("sort_by", "popularity.desc"),
            ("include_adult", "false"),
            ("vote_count.gte", min_votes.as_str()),
            (date_param, since.as_str()),
            ("page", page.as_str()),
        ];
        if !filter.with_genres.is_empty() {
            query.push(("with_genres", filter.with_genres.as_str()));
        }
        if !filter.without_genres.is_empty() {
            query.push(("without_genres", filter.without_genres.as_str()));
        }
        self.fetch_list(&format!("discover/{endpoint}"), &query, kind)
            .await
    }

    /// Shared cached fetch for the paged `{ results: [...] }` list
    /// endpoints. Path + query double as the cache key. Returns empty on
    /// any error.
    async fn fetch_list(
        &self,
        path: &str,
        query: &[(&str, &str)],
        kind: TmdbKind,
    ) -> Vec<MediaMetadata> {
        let key = std::iter::once(path.to_owned())
            .chain(query.iter().map(|(k, v)| format!("{k}={v}")))
            .collect::<Vec<_>>()
            .join("&");
        self.inner
            .lists
            .get_or_fetch(key, || async {
                let raw: TmdbDiscoverRaw = self.get_json(path, query, "list").await.ok()?;
                Some(raw.results.into_iter().map(|r| r.into_meta(kind)).collect())
            })
            .await
            .unwrap_or_default()
    }

    /// List the episodes TMDB has on file for a given TV season. Used by the
    /// notify scheduler to know what episodes to expect (and by the Series
    /// detail page to render the season layout). Returns an empty Vec on
    /// any error or for invalid `(tmdb_id, season)` combos — the caller can't
    /// usefully distinguish "doesn't exist" from "TMDB is down" and treats
    /// both as "no expected episodes right now".
    pub async fn tv_season_episodes(&self, tmdb_id: u64, season: u32) -> Vec<EpisodeMetadata> {
        self.inner
            .seasons
            .get_or_fetch((tmdb_id, season), || async {
                let raw: TmdbSeasonRaw = match self
                    .get_json(&format!("tv/{tmdb_id}/season/{season}"), &[], "season")
                    .await
                {
                    Ok(raw) => raw,
                    // A season TMDB doesn't know is a definitive empty list.
                    Err(Miss::NotFound) => return Some(Vec::new()),
                    Err(Miss::Failed) => return None,
                };
                Some(
                    raw.episodes
                        .unwrap_or_default()
                        .into_iter()
                        .map(|e| EpisodeMetadata {
                            season: e.season_number.unwrap_or(season),
                            episode: e.episode_number,
                            name: e.name.filter(|s| !s.is_empty()),
                            overview: e.overview.filter(|s| !s.is_empty()),
                            air_date: e.air_date.filter(|s| !s.is_empty()),
                            runtime_minutes: e.runtime,
                            still_path: e.still_path,
                        })
                        .collect(),
                )
            })
            .await
            .unwrap_or_default()
    }

    /// Look up `tmdb_id` as a movie, then as a TV show. Cached.
    ///
    /// Without a `kind_hint` the disambiguation order (movie → tv) is
    /// arbitrary, and TMDB uses *separate id namespaces* for movies
    /// and TV shows: `/movie/60573` and `/tv/60573` resolve to two
    /// completely different entries. Picking blindly returns the
    /// wrong metadata for whichever id collides — Silicon Valley
    /// (tv, 60573) is masked by an unrelated movie at the same
    /// numerical id. Always pass a hint when the caller knows the
    /// kind (collection.kind, search-result kind, etc.).
    pub async fn lookup(&self, tmdb_id: u64) -> Option<MediaMetadata> {
        self.lookup_with_kind(tmdb_id, None).await
    }

    /// [`Self::lookup_with_kind`] for an id read from the database (`i64`).
    pub async fn lookup_db_id(
        &self,
        tmdb_id: i64,
        kind_hint: Option<TmdbKind>,
    ) -> Option<MediaMetadata> {
        let id = u64::try_from(tmdb_id).ok()?;
        self.lookup_with_kind(id, kind_hint).await
    }

    pub async fn lookup_with_kind(
        &self,
        tmdb_id: u64,
        kind_hint: Option<TmdbKind>,
    ) -> Option<MediaMetadata> {
        self.try_lookup_with_kind(tmdb_id, kind_hint)
            .await
            .flatten()
    }

    /// [`Self::lookup_with_kind`] telling the two misses apart: `None` when
    /// TMDB couldn't be asked, `Some(None)` when it answered that the id
    /// exists in neither namespace.
    pub async fn try_lookup_with_kind(
        &self,
        tmdb_id: u64,
        kind_hint: Option<TmdbKind>,
    ) -> Option<Option<MediaMetadata>> {
        // Cache key includes the kind so a /movie/X lookup doesn't
        // serve a stale /tv/X entry from a previous call.
        let cache_key = (tmdb_id, kind_hint.map(TmdbKind::as_wire));
        let entry = self
            .inner
            .typed_cache
            .get_or_fetch(cache_key, || async {
                // Try the hinted kind first, fall back to the other one if
                // nothing comes back. The fallback matters because some
                // collections were misclassified by an older parser
                // (`Silicon.Valley.S01.MULTI` → kind=movie, but the actual
                // tmdb_id points at the TV show), and a strict-only lookup
                // would 404 in that case and serve no poster at all.
                let order: &[TmdbKind] = match kind_hint {
                    Some(TmdbKind::Tv) => &[TmdbKind::Tv, TmdbKind::Movie],
                    Some(TmdbKind::Movie) | None => &[TmdbKind::Movie, TmdbKind::Tv],
                };
                for &k in order {
                    match self.fetch(tmdb_id, k).await {
                        Ok(m) => return Some(CacheEntry::Found(Box::new(m))),
                        Err(Miss::NotFound) => {}
                        Err(Miss::Failed) => return None,
                    }
                }
                Some(CacheEntry::NotFound)
            })
            .await?;
        Some(match entry {
            CacheEntry::Found(m) => Some(*m),
            CacheEntry::NotFound => None,
        })
    }

    async fn fetch(&self, tmdb_id: u64, kind: TmdbKind) -> Result<MediaMetadata, Miss> {
        let raw: TmdbRaw = self
            .get_json(&format!("{}/{tmdb_id}", kind.as_wire()), &[], "lookup")
            .await?;
        let date = raw.release_date.or(raw.first_air_date);
        let title = raw.title.or(raw.name).unwrap_or_default();
        // Detail endpoints return full genre objects; keep both the names
        // (for display) and the ids (for catalogue filtering).
        let raw_genres = raw.genres.unwrap_or_default();
        let genre_ids = raw_genres.iter().map(|g| g.id).collect();
        let genres = raw_genres.into_iter().map(|g| g.name).collect();
        // For movies TMDB returns a single `runtime` (minutes); for TV
        // shows it's `episode_run_time: [N, N, …]` — we pick the first
        // entry as a representative episode length. Either way the
        // caller is expected to compare against the file's real probed
        // duration before trusting the metadata.
        let runtime_minutes = raw.runtime.or_else(|| {
            raw.episode_run_time
                .as_ref()
                .and_then(|v| v.first().copied())
        });
        Ok(MediaMetadata {
            kind,
            tmdb_id,
            title,
            overview: raw.overview.filter(|s| !s.is_empty()),
            year: year_of(date.as_deref()),
            poster_path: raw.poster_path,
            backdrop_path: raw.backdrop_path,
            vote_score: raw.vote_average.map(|v| v / 10.0),
            vote_count: raw.vote_count,
            genres,
            runtime_minutes,
            number_of_seasons: raw.number_of_seasons,
            popularity: raw.popularity,
            original_language: raw.original_language,
            genre_ids,
            release_date: date,
        })
    }
}

#[derive(Deserialize)]
struct TmdbRaw {
    title: Option<String>,
    name: Option<String>,
    overview: Option<String>,
    release_date: Option<String>,
    first_air_date: Option<String>,
    poster_path: Option<String>,
    backdrop_path: Option<String>,
    vote_average: Option<f64>,
    vote_count: Option<u32>,
    genres: Option<Vec<TmdbGenre>>,
    /// Movies only.
    runtime: Option<u32>,
    /// TV shows only — array of typical episode durations (minutes).
    episode_run_time: Option<Vec<u32>>,
    /// TV shows only.
    number_of_seasons: Option<u32>,
    popularity: Option<f64>,
    original_language: Option<String>,
}

#[derive(Deserialize)]
struct TmdbGenre {
    id: u32,
    name: String,
}

#[derive(Deserialize)]
struct TmdbGenreListRaw {
    #[serde(default)]
    genres: Vec<TmdbGenreListEntry>,
}

#[derive(Deserialize)]
struct TmdbGenreListEntry {
    id: u32,
    name: String,
}

#[derive(Deserialize)]
struct TmdbDiscoverRaw {
    #[serde(default)]
    results: Vec<TmdbDiscoverResult>,
}

/// A single list item from discover / trending / `now_playing` / `on_the_air`.
/// These carry `genre_ids` (ints) + `popularity` + `original_language`
/// directly, but no full genre objects / runtime / season counts.
#[derive(Deserialize)]
struct TmdbDiscoverResult {
    id: u64,
    title: Option<String>,
    name: Option<String>,
    overview: Option<String>,
    release_date: Option<String>,
    first_air_date: Option<String>,
    poster_path: Option<String>,
    backdrop_path: Option<String>,
    vote_average: Option<f64>,
    vote_count: Option<u32>,
    popularity: Option<f64>,
    original_language: Option<String>,
    #[serde(default)]
    genre_ids: Vec<u32>,
}

impl TmdbDiscoverResult {
    fn into_meta(self, kind: TmdbKind) -> MediaMetadata {
        let date = self.release_date.or(self.first_air_date);
        let year = year_of(date.as_deref());
        let title = self.title.or(self.name).unwrap_or_default();
        MediaMetadata {
            kind,
            tmdb_id: self.id,
            title,
            overview: self.overview.filter(|s| !s.is_empty()),
            year,
            poster_path: self.poster_path,
            backdrop_path: self.backdrop_path,
            vote_score: self.vote_average.map(|v| v / 10.0),
            vote_count: self.vote_count,
            // List endpoints don't return genre names or runtime/season counts.
            genres: Vec::new(),
            runtime_minutes: None,
            number_of_seasons: None,
            popularity: self.popularity,
            original_language: self.original_language,
            genre_ids: self.genre_ids,
            release_date: date,
        }
    }
}

#[derive(Deserialize)]
struct TmdbMultiRaw {
    #[serde(default)]
    results: Vec<TmdbMultiResult>,
}

#[derive(Deserialize)]
struct TmdbMultiResult {
    id: u64,
    media_type: Option<String>,
    title: Option<String>,          // movies
    name: Option<String>,           // tv
    original_title: Option<String>, // movies
    original_name: Option<String>,  // tv
    release_date: Option<String>,   // movies
    first_air_date: Option<String>, // tv
    overview: Option<String>,
    poster_path: Option<String>,
    vote_count: Option<u32>,
}

#[derive(Deserialize)]
struct TmdbSeasonRaw {
    episodes: Option<Vec<TmdbEpisodeRaw>>,
}

#[derive(Deserialize)]
struct TmdbEpisodeRaw {
    episode_number: u32,
    season_number: Option<u32>,
    name: Option<String>,
    overview: Option<String>,
    air_date: Option<String>,
    runtime: Option<u32>,
    still_path: Option<String>,
}

impl std::fmt::Debug for TmdbKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_wire())
    }
}

#[cfg(test)]
mod tests {
    use super::{POSTER_SIZE, image_url, resized_poster};

    #[test]
    fn tracker_posters_on_tmdb_are_resized() {
        assert_eq!(
            resized_poster("https://image.tmdb.org/t/p/w92/gw.jpg", POSTER_SIZE),
            "https://image.tmdb.org/t/p/w342/gw.jpg"
        );
        assert_eq!(
            resized_poster("https://cdn.tracker.example/p/123.jpg", POSTER_SIZE),
            "https://cdn.tracker.example/p/123.jpg"
        );
        assert_eq!(
            image_url("/x.jpg", "w185"),
            "https://image.tmdb.org/t/p/w185/x.jpg"
        );
    }
}
