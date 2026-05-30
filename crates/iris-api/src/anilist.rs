//! AniList catalogue client (GraphQL, keyless).
//!
//! AniList defines the anime candidate universe — precise anime genres,
//! status, airing schedule, and romaji/english titles that TMDB's
//! `/discover` can't surface as "anime" specifically. This is a
//! *catalogue* client (like [`crate::tmdb`]), not a `SearchProvider`:
//! the reco scheduler reconciles each AniList title to TMDB (via
//! `pick_best`) and falls back to an AniList-only catalogue row when
//! there's no TMDB match — it never drops a title.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use tokio::sync::RwLock;

/// AniList changes day-to-day at most; a 6 h cache matches the scheduler
/// cadence and keeps the keyless public endpoint happy.
const ANILIST_CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60);
const ENDPOINT: &str = "https://graphql.anilist.co";

#[derive(Clone)]
pub struct AniListClient {
    inner: Arc<Inner>,
}

struct Inner {
    http: reqwest::Client,
    cache: RwLock<HashMap<String, (Instant, Vec<AniListMedia>)>>,
}

/// The four broadcast seasons AniList buckets anime into.
#[derive(Debug, Clone, Copy)]
pub enum AniSeason {
    Winter,
    Spring,
    Summer,
    Fall,
}

impl AniSeason {
    fn as_str(self) -> &'static str {
        match self {
            AniSeason::Winter => "WINTER",
            AniSeason::Spring => "SPRING",
            AniSeason::Summer => "SUMMER",
            AniSeason::Fall => "FALL",
        }
    }

    /// The broadcast season covering a calendar month (1–12).
    pub fn from_month(month: u32) -> Self {
        match month {
            12 | 1 | 2 => AniSeason::Winter,
            3..=5 => AniSeason::Spring,
            6..=8 => AniSeason::Summer,
            _ => AniSeason::Fall,
        }
    }
}

/// A normalized AniList title — ready to reconcile to TMDB or fall back
/// to an AniList-only catalogue row.
#[derive(Debug, Clone)]
pub struct AniListMedia {
    pub anilist_id: i64,
    /// English title when present, else romaji.
    pub title: String,
    pub year: Option<u16>,
    /// Full start date (`YYYY-MM-DD`) when known — drives "freshest first"
    /// ordering of the anime shelf.
    pub release_date: Option<String>,
    /// AniList cover image URL — the poster fallback when there's no TMDB
    /// match.
    pub cover_image: Option<String>,
    /// AniList banner image URL — the backdrop fallback (TMDB suggestions
    /// don't carry a backdrop, so anime backdrops come from here).
    pub banner_image: Option<String>,
    pub description: Option<String>,
    pub genres: Vec<String>,
    pub popularity: Option<f64>,
    /// 0..1 (the AniList `averageScore` is /100, normalized here).
    pub average_score: Option<f64>,
    /// `true` for `format: MOVIE`; everything else (TV/ONA/OVA/…) is a
    /// series.
    pub is_movie: bool,
}

const SEASONAL_QUERY: &str = "\
query ($season: MediaSeason, $seasonYear: Int) {
  Page(page: 1, perPage: 50) {
    media(season: $season, seasonYear: $seasonYear, type: ANIME, sort: POPULARITY_DESC) {
      id title { romaji english } startDate { year month day } coverImage { large } bannerImage
      description(asHtml: false) genres popularity averageScore format
    }
  }
}";

const AIRING_QUERY: &str = "\
query ($from: Int, $to: Int) {
  Page(page: 1, perPage: 50) {
    airingSchedules(airingAt_greater: $from, airingAt_lesser: $to, sort: TIME) {
      media {
        id title { romaji english } startDate { year month day } coverImage { large } bannerImage
        description(asHtml: false) genres popularity averageScore format
      }
    }
  }
}";

impl AniListClient {
    pub fn new() -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent("iris/1.0 (+https://uplg.xyz)")
            .build()?;
        Ok(Self {
            inner: Arc::new(Inner {
                http,
                cache: RwLock::new(HashMap::new()),
            }),
        })
    }

    /// The season's most popular anime.
    pub async fn seasonal(&self, season: AniSeason, year: i32) -> Vec<AniListMedia> {
        let key = format!("seasonal:{}:{year}", season.as_str());
        let vars = serde_json::json!({ "season": season.as_str(), "seasonYear": year });
        self.fetch(key, SEASONAL_QUERY, vars).await
    }

    /// Anime with an episode airing in `[from, to]` (unix seconds) —
    /// upcoming + just-released. Deduped to one entry per series.
    pub async fn airing_window(&self, from_unix: i64, to_unix: i64) -> Vec<AniListMedia> {
        let key = format!("airing:{from_unix}:{to_unix}");
        let vars = serde_json::json!({ "from": from_unix, "to": to_unix });
        self.fetch(key, AIRING_QUERY, vars).await
    }

    async fn fetch(
        &self,
        cache_key: String,
        query: &str,
        variables: serde_json::Value,
    ) -> Vec<AniListMedia> {
        if let Some((at, items)) = self.inner.cache.read().await.get(&cache_key).cloned() {
            if at.elapsed() < ANILIST_CACHE_TTL {
                return items;
            }
        }
        let body = serde_json::json!({ "query": query, "variables": variables });
        let res = match self.inner.http.post(ENDPOINT).json(&body).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, cache_key, "anilist fetch failed");
                return Vec::new();
            }
        };
        if !res.status().is_success() {
            tracing::warn!(status = %res.status(), cache_key, "anilist non-success");
            return Vec::new();
        }
        let parsed: GqlResponse = match res.json().await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, cache_key, "anilist parse failed");
                return Vec::new();
            }
        };
        let Some(page) = parsed.data.and_then(|d| d.page) else {
            return Vec::new();
        };
        let mut out: Vec<AniListMedia> = Vec::new();
        let mut seen: HashSet<i64> = HashSet::new();
        for m in page.media {
            push_unique(&mut out, &mut seen, m.into_media());
        }
        for a in page.airing_schedules {
            push_unique(&mut out, &mut seen, a.media.and_then(RawMedia::into_media));
        }
        self.inner
            .cache
            .write()
            .await
            .insert(cache_key, (Instant::now(), out.clone()));
        out
    }
}

fn push_unique(out: &mut Vec<AniListMedia>, seen: &mut HashSet<i64>, media: Option<AniListMedia>) {
    if let Some(m) = media {
        if seen.insert(m.anilist_id) {
            out.push(m);
        }
    }
}

#[derive(Deserialize)]
struct GqlResponse {
    data: Option<GqlData>,
}

#[derive(Deserialize)]
struct GqlData {
    #[serde(rename = "Page")]
    page: Option<GqlPage>,
}

#[derive(Deserialize)]
struct GqlPage {
    #[serde(default)]
    media: Vec<RawMedia>,
    #[serde(default, rename = "airingSchedules")]
    airing_schedules: Vec<RawAiring>,
}

#[derive(Deserialize)]
struct RawAiring {
    media: Option<RawMedia>,
}

#[derive(Deserialize)]
struct RawMedia {
    id: i64,
    title: Option<RawTitle>,
    #[serde(rename = "startDate")]
    start_date: Option<RawDate>,
    #[serde(rename = "coverImage")]
    cover_image: Option<RawCover>,
    #[serde(rename = "bannerImage")]
    banner_image: Option<String>,
    description: Option<String>,
    #[serde(default)]
    genres: Vec<String>,
    popularity: Option<f64>,
    #[serde(rename = "averageScore")]
    average_score: Option<f64>,
    format: Option<String>,
}

#[derive(Deserialize)]
struct RawTitle {
    romaji: Option<String>,
    english: Option<String>,
}

#[derive(Deserialize)]
struct RawDate {
    year: Option<i64>,
    month: Option<i64>,
    day: Option<i64>,
}

#[derive(Deserialize)]
struct RawCover {
    large: Option<String>,
}

impl RawMedia {
    fn into_media(self) -> Option<AniListMedia> {
        let title = self
            .title
            .and_then(|t| t.english.or(t.romaji))
            .filter(|s| !s.trim().is_empty())?;
        let (year, release_date) = match self.start_date {
            Some(d) => {
                let year = d.year.and_then(|y| u16::try_from(y).ok());
                let release_date = d.year.map(|y| {
                    let m = d.month.unwrap_or(1).clamp(1, 12);
                    let day = d.day.unwrap_or(1).clamp(1, 31);
                    format!("{y:04}-{m:02}-{day:02}")
                });
                (year, release_date)
            }
            None => (None, None),
        };
        let is_movie = matches!(self.format.as_deref(), Some("MOVIE"));
        Some(AniListMedia {
            anilist_id: self.id,
            title,
            year,
            release_date,
            cover_image: self.cover_image.and_then(|c| c.large),
            banner_image: self.banner_image.filter(|s| !s.is_empty()),
            description: self.description.filter(|s| !s.is_empty()),
            genres: self.genres,
            popularity: self.popularity,
            average_score: self.average_score.map(|s| s / 100.0),
            is_movie,
        })
    }
}
