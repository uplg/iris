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
}

#[derive(Clone)]
enum CacheEntry {
    Found(MediaMetadata),
    NotFound,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TmdbKind {
    Movie,
    Tv,
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
            }),
        })
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
}

#[derive(Deserialize)]
struct TmdbGenre {
    name: String,
}

impl std::fmt::Debug for TmdbKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TmdbKind::Movie => f.write_str("movie"),
            TmdbKind::Tv => f.write_str("tv"),
        }
    }
}
