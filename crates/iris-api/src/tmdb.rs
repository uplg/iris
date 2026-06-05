//! TMDB metadata client + memory cache.
//!
//! We hit `themoviedb.org` for poster/backdrop/year/overview when a search
//! result carries a `tmdb_id`. Lookups are cached forever in memory (and
//! re-issued on restart — TMDB metadata is essentially static for a given
//! id, so this is harmless).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

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
    typed_cache: RwLock<HashMap<(u64, Option<&'static str>), CacheEntry>>,
    /// Separate cache for season episode lists, keyed by `(tmdb_id, season_number)`.
    /// TMDB rarely retroactively edits aired episodes, so caching forever is fine —
    /// the only mutation we'd miss is air-date corrections on unaired episodes,
    /// and the notify scheduler bursts a fresh request anyway when it finds a
    /// new episode (cache only matters for repeat reads within a single uptime).
    seasons: RwLock<HashMap<(u64, u32), Vec<EpisodeMetadata>>>,
    /// Lowercase-keyed cache for `multi_search` results. Search-page
    /// rendering issues one lookup per unique cleaned title and many
    /// of those repeat across pages / users — caching the raw upstream
    /// response in-memory keeps TMDB calls bounded without paying a
    /// DB round-trip on the hot path. Empty results are cached too so
    /// "no hits" doesn't re-issue.
    searches: RwLock<HashMap<String, Vec<TmdbSuggestion>>>,
    /// Genre taxonomy per kind, keyed by `kind_marker`. Unlike the id
    /// caches above this one carries a fetched-at stamp and expires
    /// after `GENRE_CACHE_TTL`: the taxonomy is near-static but the
    /// onboarding picker should still pick up the rare addition without
    /// a process restart.
    genres_cache: RwLock<HashMap<&'static str, (Instant, Vec<Genre>)>>,
    /// Request-keyed cache for the list endpoints (`recommendations` /
    /// `similar`). Keyed by a string built from the call + params, expires
    /// after `DISCOVER_CACHE_TTL`, so repeated For-You renders don't re-fetch
    /// the same recommendation slice.
    discover_cache: RwLock<HashMap<String, (Instant, Vec<MediaMetadata>)>>,
}

/// TTL for the cached TMDB genre taxonomy. The list changes maybe once
/// a year; a daily refresh costs one request per kind and keeps the
/// onboarding picker current.
const GENRE_CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// TTL for cached list slices (`recommendations` / `similar`). A few hours
/// keeps the "Because you watched" shelf cheap across repeated renders.
const DISCOVER_CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Clone)]
enum CacheEntry {
    // Boxed: MediaMetadata is much larger than the empty NotFound variant.
    Found(Box<MediaMetadata>),
    NotFound,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TmdbKind {
    Movie,
    Tv,
}

#[derive(Debug, Clone, Serialize)]
pub struct TmdbSuggestion {
    pub kind: TmdbKind,
    pub tmdb_id: u64,
    pub title: String,
    pub year: Option<u32>,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
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

#[derive(Debug, Clone, Serialize)]
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
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?;
        Ok(Self {
            inner: Arc::new(Inner {
                api_key,
                http,
                typed_cache: RwLock::new(HashMap::new()),
                seasons: RwLock::new(HashMap::new()),
                searches: RwLock::new(HashMap::new()),
                genres_cache: RwLock::new(HashMap::new()),
                discover_cache: RwLock::new(HashMap::new()),
            }),
        })
    }

}

const fn kind_marker(k: TmdbKind) -> &'static str {
    match k {
        TmdbKind::Movie => "movie",
        TmdbKind::Tv => "tv",
    }
}

impl TmdbClient {

    /// Multi-search across movies + TV shows. Powers the search-page
    /// typeahead — the user types a few characters, we surface "did you
    /// mean X (2024)" suggestions tied to a TMDB id so a click runs an
    /// indexer search with the cleaned title (and optionally remembers
    /// the tmdb id for the eventual ingest). People results filtered out.
    /// NOT cached — the typeahead is short-lived and `TanStack` Query
    /// already debounces / caches client-side.
    pub async fn multi_search(&self, query: &str) -> Vec<TmdbSuggestion> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        let cache_key = trimmed.to_lowercase();
        if let Some(hit) = self.inner.searches.read().await.get(&cache_key).cloned() {
            return hit;
        }
        let res = match self
            .inner
            .http
            .get("https://api.themoviedb.org/3/search/multi")
            .query(&[
                ("api_key", self.inner.api_key.as_str()),
                ("query", query),
                ("include_adult", "false"),
                ("page", "1"),
            ])
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, query, "tmdb multi-search failed");
                return Vec::new();
            }
        };
        if !res.status().is_success() {
            return Vec::new();
        }
        let raw: TmdbMultiRaw = match res.json().await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, query, "tmdb multi-search parse failed");
                return Vec::new();
            }
        };
        let out: Vec<TmdbSuggestion> = raw
            .results
            .into_iter()
            .filter_map(|r| {
                let kind = match r.media_type.as_deref() {
                    Some("movie") => TmdbKind::Movie,
                    Some("tv") => TmdbKind::Tv,
                    _ => return None, // skip people + unknowns
                };
                let title = r.title.or(r.name)?;
                let date = r.release_date.or(r.first_air_date);
                let year = date
                    .as_deref()
                    .and_then(|d| d.split('-').next())
                    .and_then(|y| y.parse().ok());
                Some(TmdbSuggestion {
                    kind,
                    tmdb_id: r.id,
                    title,
                    year,
                    overview: r.overview.filter(|s| !s.is_empty()),
                    poster_path: r.poster_path,
                })
            })
            .take(10)
            .collect();
        self.inner
            .searches
            .write()
            .await
            .insert(cache_key, out.clone());
        out
    }

    /// Fetch TMDB's canonical genre taxonomy for `kind` (movies or TV).
    /// Powers the onboarding genre picker. Cached in-memory per kind for
    /// `GENRE_CACHE_TTL`; returns an empty list on any error or when the
    /// client is unconfigured (the caller renders an empty picker rather
    /// than failing onboarding).
    pub async fn genre_list(&self, kind: TmdbKind) -> Vec<Genre> {
        let marker = kind_marker(kind);
        if let Some((fetched, genres)) = self.inner.genres_cache.read().await.get(marker).cloned() {
            if fetched.elapsed() < GENRE_CACHE_TTL {
                return genres;
            }
        }
        let url = format!(
            "https://api.themoviedb.org/3/genre/{marker}/list?api_key={}&language=en-US",
            self.inner.api_key
        );
        let res = match self.inner.http.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, ?kind, "tmdb genre list fetch failed");
                return Vec::new();
            }
        };
        if !res.status().is_success() {
            return Vec::new();
        }
        let raw: TmdbGenreListRaw = match res.json().await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, ?kind, "tmdb genre list parse failed");
                return Vec::new();
            }
        };
        let genres: Vec<Genre> = raw
            .genres
            .into_iter()
            .map(|g| Genre { id: g.id, name: g.name })
            .collect();
        self.inner
            .genres_cache
            .write()
            .await
            .insert(marker, (Instant::now(), genres.clone()));
        genres
    }

    /// `/{movie,tv}/{id}/recommendations` — TMDB's collaborative "people who
    /// liked this also liked" list. Powers the taste-based "Because you
    /// watched X" shelf; an older recommendation is itself a strong
    /// availability signal (it has had time to be indexed + seeded).
    pub async fn recommendations(&self, kind: TmdbKind, tmdb_id: u64) -> Vec<MediaMetadata> {
        let endpoint = kind_marker(kind);
        let url = format!(
            "https://api.themoviedb.org/3/{endpoint}/{tmdb_id}/recommendations?api_key={}&page=1",
            self.inner.api_key
        );
        self.fetch_list(format!("recommendations:{endpoint}:{tmdb_id}"), url, kind)
            .await
    }

    /// `/{movie,tv}/{id}/similar` — content-based (keyword/genre) neighbours.
    /// Complements [`Self::recommendations`] for the same shelf.
    pub async fn similar(&self, kind: TmdbKind, tmdb_id: u64) -> Vec<MediaMetadata> {
        let endpoint = kind_marker(kind);
        let url = format!(
            "https://api.themoviedb.org/3/{endpoint}/{tmdb_id}/similar?api_key={}&page=1",
            self.inner.api_key
        );
        self.fetch_list(format!("similar:{endpoint}:{tmdb_id}"), url, kind)
            .await
    }

    /// Shared cached fetch for the list endpoints. `recommendations` /
    /// `similar` return the same paged `{ results: [...] }` envelope of list
    /// items, so they share one cache + parse path. Returns empty on any error.
    async fn fetch_list(
        &self,
        cache_key: String,
        url: String,
        kind: TmdbKind,
    ) -> Vec<MediaMetadata> {
        if let Some((fetched, items)) = self.inner.discover_cache.read().await.get(&cache_key).cloned()
        {
            if fetched.elapsed() < DISCOVER_CACHE_TTL {
                return items;
            }
        }
        let res = match self.inner.http.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, cache_key, "tmdb list fetch failed");
                return Vec::new();
            }
        };
        if !res.status().is_success() {
            return Vec::new();
        }
        let raw: TmdbDiscoverRaw = match res.json().await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, cache_key, "tmdb list parse failed");
                return Vec::new();
            }
        };
        let items: Vec<MediaMetadata> =
            raw.results.into_iter().map(|r| r.into_meta(kind)).collect();
        self.inner
            .discover_cache
            .write()
            .await
            .insert(cache_key, (Instant::now(), items.clone()));
        items
    }

    /// List the episodes TMDB has on file for a given TV season. Used by the
    /// notify scheduler to know what episodes to expect (and by the Series
    /// detail page to render the season layout). Returns an empty Vec on
    /// any error or for invalid `(tmdb_id, season)` combos — the caller can't
    /// usefully distinguish "doesn't exist" from "TMDB is down" and treats
    /// both as "no expected episodes right now".
    pub async fn tv_season_episodes(
        &self,
        tmdb_id: u64,
        season: u32,
    ) -> Vec<EpisodeMetadata> {
        let key = (tmdb_id, season);
        if let Some(hit) = self.inner.seasons.read().await.get(&key).cloned() {
            return hit;
        }
        let url = format!(
            "https://api.themoviedb.org/3/tv/{tmdb_id}/season/{season}?api_key={}",
            self.inner.api_key
        );
        let res = match self.inner.http.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, tmdb_id, season, "tmdb season fetch failed");
                return Vec::new();
            }
        };
        if !res.status().is_success() {
            return Vec::new();
        }
        let raw: TmdbSeasonRaw = match res.json().await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, tmdb_id, season, "tmdb season parse failed");
                return Vec::new();
            }
        };
        let episodes: Vec<EpisodeMetadata> = raw
            .episodes
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
            .collect();
        self.inner.seasons.write().await.insert(key, episodes.clone());
        episodes
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

    pub async fn lookup_with_kind(
        &self,
        tmdb_id: u64,
        kind_hint: Option<TmdbKind>,
    ) -> Option<MediaMetadata> {
        // Cache key includes the kind so a /movie/X lookup doesn't
        // serve a stale /tv/X entry from a previous call.
        let cache_key = (tmdb_id, kind_hint.map(kind_marker));
        if let Some(hit) = self
            .inner
            .typed_cache
            .read()
            .await
            .get(&cache_key)
            .cloned()
        {
            return match hit {
                CacheEntry::Found(m) => Some(*m),
                CacheEntry::NotFound => None,
            };
        }
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
            if let Some(m) = self.fetch(tmdb_id, k).await {
                self.inner
                    .typed_cache
                    .write()
                    .await
                    .insert(cache_key, CacheEntry::Found(Box::new(m.clone())));
                return Some(m);
            }
        }
        self.inner
            .typed_cache
            .write()
            .await
            .insert(cache_key, CacheEntry::NotFound);
        None
    }

    async fn fetch(&self, tmdb_id: u64, kind: TmdbKind) -> Option<MediaMetadata> {
        let endpoint = match kind {
            TmdbKind::Movie => "movie",
            TmdbKind::Tv => "tv",
        };
        let url = format!(
            "https://api.themoviedb.org/3/{endpoint}/{tmdb_id}?api_key={}",
            self.inner.api_key
        );
        let res = match self.inner.http.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, tmdb_id, ?kind, "tmdb fetch failed");
                return None;
            }
        };
        if !res.status().is_success() {
            return None;
        }
        let raw: TmdbRaw = res.json().await.ok()?;
        let date = raw.release_date.or(raw.first_air_date);
        let year = date
            .as_deref()
            .and_then(|d| d.split('-').next())
            .and_then(|y| y.parse().ok());
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
        let runtime_minutes = raw
            .runtime
            .or_else(|| raw.episode_run_time.as_ref().and_then(|v| v.first().copied()));
        Some(MediaMetadata {
            kind,
            tmdb_id,
            title,
            overview: raw.overview.filter(|s| !s.is_empty()),
            year,
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
        let year = date
            .as_deref()
            .and_then(|d| d.split('-').next())
            .and_then(|y| y.parse().ok());
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
    title: Option<String>,        // movies
    name: Option<String>,         // tv
    release_date: Option<String>, // movies
    first_air_date: Option<String>, // tv
    overview: Option<String>,
    poster_path: Option<String>,
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
        match self {
            TmdbKind::Movie => f.write_str("movie"),
            TmdbKind::Tv => f.write_str("tv"),
        }
    }
}
