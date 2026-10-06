//! AniList catalogue client (GraphQL, keyless).
//!
//! AniList defines the anime candidate universe — precise anime genres,
//! status, airing schedule, and romaji/english titles that TMDB's
//! `/discover` can't surface as "anime" specifically. This is a
//! *catalogue* client (like [`crate::tmdb`]), not a `SearchProvider`:
//! the reco scheduler reconciles each AniList title to TMDB (via
//! `pick_best`) and falls back to an AniList-only catalogue row when
//! there's no TMDB match — it never drops a title.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::ttl_cache::TtlCache;
use serde::Deserialize;

/// AniList changes day-to-day at most; a 6 h cache matches the scheduler
/// cadence and keeps the keyless public endpoint happy.
const ANILIST_CACHE_TTL: Duration = Duration::from_hours(6);
/// Failed lookups (403 outage/ban, network error, bad payload) are cached
/// too — otherwise every scheduler cycle re-hammers the same titles, which
/// is exactly what keeps a Cloudflare ban alive. Short TTL so recovery is
/// picked up quickly once the endpoint is healthy again.
const ANILIST_FAILURE_TTL: Duration = Duration::from_mins(15);
/// AniList's keyless rate limit is 30 req/min in degraded mode; spacing
/// outgoing requests ≥ 2 s keeps scheduler batches from bursting.
const ANILIST_MIN_INTERVAL: Duration = Duration::from_secs(2);
const ENDPOINT: &str = "https://graphql.anilist.co";

#[derive(Clone)]
pub struct AniListClient {
    inner: Arc<Inner>,
}

struct Inner {
    http: reqwest::Client,
    endpoint: String,
    /// Answers by `cache_key`, single-flight per key.
    cache: TtlCache<String, Vec<AniListMedia>>,
    /// Keys whose last fetch failed, under the shorter failure TTL.
    failed: TtlCache<String, ()>,
    last_request: tokio::sync::Mutex<Option<Instant>>,
}

/// Distinct titles and recommendation ids kept per cache.
const ANILIST_CACHE_CAPACITY: usize = 2048;

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

/// The result that IS `title`: same series key (never AniList's fuzzy first
/// hit, which can be another show), same format (series or movie), the one
/// from `year` when several match.
pub fn strict_match<'a>(
    results: &'a [AniListMedia],
    title: &str,
    is_movie: bool,
    year: Option<u32>,
) -> Option<&'a AniListMedia> {
    let want = iris_media::filename::series_key(title);
    if want.is_empty() {
        return None;
    }
    let mut matching = results
        .iter()
        .filter(|m| m.is_movie == is_movie && iris_media::filename::series_key(&m.title) == want);
    let first = matching.next()?;
    let year = year.and_then(|y| u16::try_from(y).ok());
    if first.year == year || year.is_none() {
        return Some(first);
    }
    Some(matching.find(|m| m.year == year).unwrap_or(first))
}

const SEARCH_QUERY: &str = "\
query ($search: String) {
  Page(page: 1, perPage: 10) {
    media(search: $search, type: ANIME, sort: SEARCH_MATCH) {
      id title { romaji english } startDate { year month day } coverImage { large } bannerImage
      description(asHtml: false) genres popularity averageScore format
    }
  }
}";

const RECOMMENDATIONS_QUERY: &str = "\
query ($mediaId: Int) {
  Page(page: 1, perPage: 25) {
    recommendations(mediaId: $mediaId, sort: RATING_DESC) {
      mediaRecommendation {
        id title { romaji english } startDate { year month day } coverImage { large } bannerImage
        description(asHtml: false) genres popularity averageScore format
      }
    }
  }
}";

impl AniListClient {
    pub fn new() -> anyhow::Result<Self> {
        Self::with_endpoint(ENDPOINT)
    }

    fn with_endpoint(endpoint: &str) -> anyhow::Result<Self> {
        let http = iris_providers::tls::client_builder()
            .timeout(Duration::from_secs(15))
            .user_agent("iris/1.0 (+https://uplg.xyz)")
            .build()?;
        Ok(Self {
            inner: Arc::new(Inner {
                http,
                endpoint: endpoint.to_owned(),
                cache: TtlCache::new(ANILIST_CACHE_TTL, ANILIST_CACHE_CAPACITY),
                failed: TtlCache::new(ANILIST_FAILURE_TTL, ANILIST_CACHE_CAPACITY),
                last_request: tokio::sync::Mutex::new(None),
            }),
        })
    }

    /// Search anime by title — reconciles a tracker release (or a watched
    /// title) to its AniList entry for a precise anime poster + `is_anime`.
    pub async fn search(&self, title: &str) -> Vec<AniListMedia> {
        let trimmed = title.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        let key = format!("anisearch:{}", trimmed.to_lowercase());
        let vars = serde_json::json!({ "search": trimmed });
        self.fetch(key, SEARCH_QUERY, vars).await
    }

    /// Anime AniList recommends for `anilist_id` — the anime counterpart of
    /// TMDB's `recommendations`, for the "Because you watched X" shelf.
    pub async fn recommendations(&self, anilist_id: i64) -> Vec<AniListMedia> {
        let key = format!("anirec:{anilist_id}");
        let vars = serde_json::json!({ "mediaId": anilist_id });
        self.fetch(key, RECOMMENDATIONS_QUERY, vars).await
    }

    async fn fetch(
        &self,
        cache_key: String,
        query: &str,
        variables: serde_json::Value,
    ) -> Vec<AniListMedia> {
        let recently_failed = self
            .inner
            .failed
            .get_or_fetch(cache_key.clone(), || async { None })
            .await
            .is_some();
        if recently_failed {
            return Vec::new();
        }
        let fetched = self
            .inner
            .cache
            .get_or_fetch(cache_key.clone(), || {
                self.fetch_uncached(&cache_key, query, variables)
            })
            .await;
        if let Some(items) = fetched {
            return items;
        }
        self.inner
            .failed
            .get_or_fetch(cache_key, || async { Some(()) })
            .await;
        Vec::new()
    }

    /// `None`: the request, its status or its payload failed.
    async fn fetch_uncached(
        &self,
        cache_key: &str,
        query: &str,
        variables: serde_json::Value,
    ) -> Option<Vec<AniListMedia>> {
        self.throttle().await;
        let body = serde_json::json!({ "query": query, "variables": variables });
        let res = match self
            .inner
            .http
            .post(&self.inner.endpoint)
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, cache_key, "anilist fetch failed");
                return None;
            }
        };
        if !res.status().is_success() {
            tracing::warn!(status = %res.status(), cache_key, "anilist non-success");
            return None;
        }
        let parsed: GqlResponse = match res.json().await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, cache_key, "anilist parse failed");
                return None;
            }
        };
        // GraphQL-level error (`data: null` with 200): same as a transport
        // failure.
        let page = parsed.data.and_then(|d| d.page)?;
        let mut out: Vec<AniListMedia> = Vec::new();
        let mut seen: HashSet<i64> = HashSet::new();
        for m in page.media {
            push_unique(&mut out, &mut seen, m.into_media());
        }
        for r in page.recommendations {
            push_unique(
                &mut out,
                &mut seen,
                r.media_recommendation.and_then(RawMedia::into_media),
            );
        }
        Some(out)
    }

    /// Space outgoing requests ≥ [`ANILIST_MIN_INTERVAL`] apart. The lock is
    /// held through the sleep so concurrent callers queue instead of all
    /// firing together once the interval elapses.
    async fn throttle(&self) {
        let mut last = self.inner.last_request.lock().await;
        if let Some(prev) = *last {
            let wait = ANILIST_MIN_INTERVAL.saturating_sub(prev.elapsed());
            if !wait.is_zero() {
                tokio::time::sleep(wait).await;
            }
        }
        *last = Some(Instant::now());
    }
}

fn push_unique(out: &mut Vec<AniListMedia>, seen: &mut HashSet<i64>, media: Option<AniListMedia>) {
    if let Some(m) = media
        && seen.insert(m.anilist_id)
    {
        out.push(m);
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
    #[serde(default)]
    recommendations: Vec<RawRecommendation>,
}

#[derive(Deserialize)]
struct RawRecommendation {
    #[serde(rename = "mediaRecommendation")]
    media_recommendation: Option<RawMedia>,
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

#[cfg(test)]
mod strict_match_tests {
    use super::{AniListMedia, strict_match};

    fn media(id: i64, title: &str, year: u16, is_movie: bool) -> AniListMedia {
        AniListMedia {
            anilist_id: id,
            title: title.into(),
            year: Some(year),
            release_date: None,
            cover_image: None,
            banner_image: None,
            description: None,
            genres: Vec::new(),
            popularity: None,
            average_score: None,
            is_movie,
        }
    }

    #[test]
    fn only_the_same_title_and_format_match() {
        let results = [
            media(1, "Dr. Stone: New World", 2023, false),
            media(2, "Dr. Stone", 2019, true),
            media(3, "Dr. Stone", 2019, false),
            media(4, "Dr. Stone", 2025, false),
        ];
        let id = |title, is_movie, year| {
            strict_match(&results, title, is_movie, year).map(|m| m.anilist_id)
        };
        assert_eq!(
            id("Dr. Stone", false, None),
            Some(3),
            "the fuzzy first hit is another show"
        );
        assert_eq!(id("Dr Stone", false, Some(2025)), Some(4));
        assert_eq!(id("Dr. Stone", false, Some(1999)), Some(3));
        assert_eq!(id("Dr. Stone", true, None), Some(2));
        assert_eq!(id("One Piece", false, None), None);
    }
}

#[cfg(test)]
mod cache_tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::AniListClient;

    #[tokio::test]
    async fn a_failed_lookup_is_not_retried_inside_the_failure_window() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&hits);
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                counted.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 4096];
                let _ = sock.read(&mut buf).await;
                let _ = sock
                    .write_all(b"HTTP/1.1 500 Internal Server Error\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
                    .await;
            }
        });
        let client = AniListClient::with_endpoint(&format!("http://127.0.0.1:{port}")).unwrap();
        assert!(client.search("bleach").await.is_empty());
        assert!(client.search("Bleach").await.is_empty());
        assert_eq!(hits.load(Ordering::SeqCst), 1, "the failure is remembered");
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    #[tokio::test]
    #[ignore = "hits the live AniList API"]
    async fn failure_cache_and_throttle() {
        let client = AniListClient::new().unwrap();
        let t0 = Instant::now();
        let first = client.search("bleach").await;
        let d1 = t0.elapsed();
        eprintln!("first call: {d1:?} → {} items", first.len());

        let t1 = Instant::now();
        let second = client.search("bleach").await;
        let d2 = t1.elapsed();
        eprintln!("second call (same key): {d2:?} → {} items", second.len());
        assert!(
            d2 < Duration::from_millis(100),
            "same key must hit cache, took {d2:?}"
        );

        let t2 = Instant::now();
        let third = client.search("naruto").await;
        let d3 = t2.elapsed();
        eprintln!("third call (new key): {d3:?} → {} items", third.len());
        assert!(
            t0.elapsed() >= ANILIST_MIN_INTERVAL,
            "new key must be throttled ≥ {ANILIST_MIN_INTERVAL:?} after first request"
        );
    }
}
