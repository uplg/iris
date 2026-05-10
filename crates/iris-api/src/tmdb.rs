//! TMDB metadata client + memory cache.
//!
//! We hit `themoviedb.org` for poster/backdrop/year/overview when a search
//! result carries a `tmdb_id`. Lookups are cached forever in memory (and
//! re-issued on restart — TMDB metadata is essentially static for a given
//! id, so this is harmless).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct TmdbClient {
    inner: Arc<Inner>,
}

struct Inner {
    api_key: String,
    http: reqwest::Client,
    cache: RwLock<HashMap<u64, CacheEntry>>,
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
}

#[derive(Clone)]
enum CacheEntry {
    Found(MediaMetadata),
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
                cache: RwLock::new(HashMap::new()),
                seasons: RwLock::new(HashMap::new()),
                searches: RwLock::new(HashMap::new()),
            }),
        })
    }

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
    pub async fn lookup(&self, tmdb_id: u64) -> Option<MediaMetadata> {
        if let Some(hit) = self.inner.cache.read().await.get(&tmdb_id).cloned() {
            return match hit {
                CacheEntry::Found(m) => Some(m),
                CacheEntry::NotFound => None,
            };
        }
        if let Some(m) = self.fetch(tmdb_id, TmdbKind::Movie).await {
            self.inner
                .cache
                .write()
                .await
                .insert(tmdb_id, CacheEntry::Found(m.clone()));
            return Some(m);
        }
        if let Some(m) = self.fetch(tmdb_id, TmdbKind::Tv).await {
            self.inner
                .cache
                .write()
                .await
                .insert(tmdb_id, CacheEntry::Found(m.clone()));
            return Some(m);
        }
        self.inner
            .cache
            .write()
            .await
            .insert(tmdb_id, CacheEntry::NotFound);
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
        let genres = raw
            .genres
            .unwrap_or_default()
            .into_iter()
            .map(|g| g.name)
            .collect();
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
}

#[derive(Deserialize)]
struct TmdbGenre {
    name: String,
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
