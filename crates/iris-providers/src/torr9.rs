//! Torr9 search provider — private French tracker, JWT-authenticated.
//!
//! Config:
//! ```toml
//! [[providers]]
//! id = "torr9"
//! kind = "torr9"
//! base_url = "https://api.torr9.net"
//! username_env = "TORR9_USERNAME"
//! password_env = "TORR9_PASSWORD"
//! # optional overrides:
//! # referer = "https://torr9.net/"
//! # origin = "https://torr9.net"
//! ```

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use iris_config::ProviderEntry;
use iris_core::Error;
use iris_core::Result;
use iris_core::search::{
    DescriptionFormat, MediaKind, ProviderCapabilities, ProviderPage, SearchQuery, SearchResult,
    SortField, SortOrder, TorrentDetails, TorrentSource,
};

use crate::nfo;
use quick_xml::Reader;
use quick_xml::escape::unescape as xml_unescape;
use quick_xml::events::Event;
use reqwest::header::{
    ACCEPT, ACCEPT_LANGUAGE, AUTHORIZATION, HeaderMap, HeaderValue, ORIGIN, REFERER, USER_AGENT,
};
use reqwest::{Client, RequestBuilder, Response, StatusCode};
use serde::Deserialize;
use tokio::sync::Mutex;
use url::Url;

use crate::SearchProvider;
use crate::util::{extract_year, field_or_env, field_str};

const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:150.0) Gecko/20100101 Firefox/150.0";
const DEFAULT_REFERER: &str = "https://torr9.net/";
const DEFAULT_ORIGIN: &str = "https://torr9.net";
/// Token TTL is 30 days; refresh proactively well before that.
const TOKEN_REFRESH_AFTER: Duration = Duration::from_hours(600);
/// Bencoded torrent files start with a dictionary marker.
const BENCODE_DICT_MARKER: u8 = b'd';
/// Featured carousels are curated server-side and refresh slowly. Caching
/// 30 min keeps the discovery home cheap without going stale on the
/// daily-ish editorial cadence.
const FEATURED_TTL: Duration = Duration::from_mins(30);
/// Torrent details get re-opened when the user shops around the search
/// results. 60s avoids hammering the indexer when they bounce between
/// 5 torrents in 30 seconds, but stays fresh enough for seeders/leechers
/// to be representative.
const DETAILS_TTL: Duration = Duration::from_mins(1);

pub struct Torr9 {
    id: String,
    base_url: Url,
    username: String,
    password: String,
    /// Passkey for the public RSS feeds (`/api/v1/rss/{Films,Séries}`) —
    /// distinct from the JWT used everywhere else. `None` disables the
    /// provider's rolling-window contribution (search/grab still work).
    passkey: Option<String>,
    http: Client,
    token: Mutex<Option<CachedToken>>,
    featured_movies_cache: Mutex<Option<CachedFeatured>>,
    featured_series_cache: Mutex<Option<CachedFeatured>>,
    details_cache: Mutex<std::collections::HashMap<String, CachedDetails>>,
}

struct CachedToken {
    bearer: String,
    fetched_at: Instant,
}

struct CachedFeatured {
    items: Vec<SearchResult>,
    fetched_at: Instant,
}

struct CachedDetails {
    details: TorrentDetails,
    fetched_at: Instant,
}

impl Torr9 {
    pub fn from_config(entry: &ProviderEntry) -> Result<Arc<Self>> {
        let base_url_str = field_str(entry, "base_url")?;
        let base_url = Url::parse(base_url_str)
            .map_err(|e| Error::Provider(format!("torr9 base_url invalid: {e}")))?;
        let username = field_or_env(entry, "username")?;
        let password = field_or_env(entry, "password")?;
        // Optional: only the RSS rolling-window feeds need it. Any failure
        // (unset env, missing field) just disables that contribution.
        let passkey = field_or_env(entry, "passkey").ok();
        let referer = entry
            .fields
            .get("referer")
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_REFERER);
        let origin = entry
            .fields
            .get("origin")
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_ORIGIN);

        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));
        headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
        headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));
        headers.insert(
            REFERER,
            HeaderValue::from_str(referer)
                .map_err(|e| Error::Provider(format!("torr9 referer invalid: {e}")))?,
        );
        headers.insert(
            ORIGIN,
            HeaderValue::from_str(origin)
                .map_err(|e| Error::Provider(format!("torr9 origin invalid: {e}")))?,
        );

        let http = Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| Error::Provider(format!("torr9 http client: {e}")))?;

        Ok(Arc::new(Self {
            id: entry.id.clone(),
            base_url,
            username,
            password,
            passkey,
            http,
            token: Mutex::new(None),
            featured_movies_cache: Mutex::new(None),
            featured_series_cache: Mutex::new(None),
            details_cache: Mutex::new(std::collections::HashMap::new()),
        }))
    }

    /// Shared body for `featured_movies` / `featured_series`. The torr9
    /// endpoint returns the same shape on both routes (a list of torrents)
    /// — only the path differs.
    async fn featured(&self, kind: FeaturedKind) -> Result<Vec<SearchResult>> {
        let cache_slot = match kind {
            FeaturedKind::Movies => &self.featured_movies_cache,
            FeaturedKind::Series => &self.featured_series_cache,
        };
        if let Some(c) = cache_slot.lock().await.as_ref()
            && c.fetched_at.elapsed() < FEATURED_TTL
        {
            return Ok(c.items.clone());
        }

        let path = match kind {
            FeaturedKind::Movies => "/api/v1/featured/movies",
            FeaturedKind::Series => "/api/v1/featured/series",
        };
        let url = self
            .base_url
            .join(path)
            .map_err(|e| Error::Provider(format!("torr9 join featured url: {e}")))?;

        // Buffer the body so we can log it on a deserialise failure —
        // the torr9 featured endpoints aren't documented and the shape
        // can change without warning.
        let body = self
            .authed_get(|http| http.get(url.clone()))
            .await?
            .text()
            .await
            .map_err(|e| Error::Provider(format!("torr9 featured body: {e}")))?;

        let resp: FeaturedResponse = serde_json::from_str(&body).map_err(|e| {
            tracing::warn!(
                provider = %self.id,
                ?kind,
                body_preview = %body.chars().take(300).collect::<String>(),
                error = %e,
                "torr9 featured decode failed — unexpected shape",
            );
            Error::Provider(format!("torr9 featured decode: {e}"))
        })?;

        let id = self.id.clone();
        let items: Vec<SearchResult> = resp
            .items
            .into_iter()
            .map(|t| t.into_search_result(&id))
            .collect();

        tracing::info!(
            provider = %self.id,
            ?kind,
            count = items.len(),
            "torr9 featured fetched",
        );

        *cache_slot.lock().await = Some(CachedFeatured {
            items: items.clone(),
            fetched_at: Instant::now(),
        });
        Ok(items)
    }

    async fn login(&self) -> Result<String> {
        let url = self
            .base_url
            .join("/api/v1/auth/login")
            .map_err(|e| Error::Provider(format!("torr9 join login url: {e}")))?;

        let res = self
            .http
            .post(url)
            .json(&serde_json::json!({
                "username": self.username,
                "password": self.password,
                "remember_me": true,
            }))
            .send()
            .await
            .map_err(|e| Error::Provider(format!("torr9 login: {e}")))?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(Error::Provider(format!(
                "torr9 login failed: HTTP {status} — {body}"
            )));
        }

        let parsed: LoginResponse = res
            .json()
            .await
            .map_err(|e| Error::Provider(format!("torr9 login decode: {e}")))?;
        Ok(parsed.token)
    }

    async fn token(&self) -> Result<String> {
        let mut guard = self.token.lock().await;
        if let Some(t) = guard.as_ref()
            && t.fetched_at.elapsed() < TOKEN_REFRESH_AFTER
        {
            return Ok(t.bearer.clone());
        }
        tracing::debug!(provider = %self.id, "torr9 (re)logging in");
        let bearer = self.login().await?;
        *guard = Some(CachedToken {
            bearer: bearer.clone(),
            fetched_at: Instant::now(),
        });
        Ok(bearer)
    }

    async fn invalidate(&self) {
        *self.token.lock().await = None;
    }

    /// Issue an authenticated GET, retrying once with a fresh token on 401.
    async fn authed_get<F>(&self, build: F) -> Result<Response>
    where
        F: Fn(&Client) -> RequestBuilder,
    {
        let mut attempt = 0u8;
        loop {
            let token = self.token().await?;
            let res = build(&self.http)
                .header(AUTHORIZATION, format!("Bearer {token}"))
                .send()
                .await
                .map_err(|e| Error::Provider(format!("torr9 request: {e}")))?;
            if res.status() == StatusCode::UNAUTHORIZED && attempt == 0 {
                attempt += 1;
                self.invalidate().await;
                continue;
            }
            if !res.status().is_success() {
                let status = res.status();
                let body = res.text().await.unwrap_or_default();
                return Err(Error::Provider(format!(
                    "torr9 request failed: HTTP {status} — {body}"
                )));
            }
            return Ok(res);
        }
    }
}

#[async_trait]
impl SearchProvider for Torr9 {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            returns_magnet: false,
            returns_torrent_file: true,
            returns_infohash: true,
        }
    }

    async fn search(&self, q: &SearchQuery) -> Result<ProviderPage> {
        let url = self
            .base_url
            .join("/api/v1/torrents/search")
            .map_err(|e| Error::Provider(format!("torr9 join search url: {e}")))?;
        let limit = q.limit.unwrap_or(25).clamp(1, 100);
        let page = q.page.unwrap_or(1).max(1);

        // Torr9's `q=` is a substring match. When the SCENE parser
        // pulled a clean title + season/episode out of the raw query
        // we rebuild a SCENE-form filter so the indexer narrows
        // exactly to the requested release line.
        let q_param = build_torr9_q(q);

        let mut qs: Vec<(&'static str, String)> = vec![
            ("q", q_param),
            ("page", page.to_string()),
            ("limit", limit.to_string()),
        ];
        if let Some(field) = q.sort_by {
            qs.push(("sort_by", torr9_sort_field(field).to_string()));
        }
        if let Some(order) = q.order {
            qs.push((
                "order",
                match order {
                    SortOrder::Asc => "asc",
                    SortOrder::Desc => "desc",
                }
                .to_string(),
            ));
        }
        // An SxxExx hint in the raw query is an unambiguous TV signal —
        // tag the request even if the caller didn't pass an explicit kind.
        let inferred_kind = q.kind.or_else(|| q.season.map(|_| MediaKind::Tv));
        if let Some(kind) = inferred_kind {
            qs.push((
                "category",
                match kind {
                    MediaKind::Movie => "film".to_string(),
                    MediaKind::Tv => "tv".to_string(),
                },
            ));
        }

        let resp: SearchResponse = self
            .authed_get(|http| http.get(url.clone()).query(&qs))
            .await?
            .json()
            .await
            .map_err(|e| Error::Provider(format!("torr9 search decode: {e}")))?;

        let id = self.id.clone();
        let results = resp
            .torrents
            .into_iter()
            .map(|t| t.into_search_result(&id))
            .collect();

        Ok(ProviderPage {
            results,
            current_page: resp.current_page.unwrap_or(page),
            limit: resp.limit.unwrap_or(limit),
            total_count: resp.total_count,
            total_pages: resp.total_pages,
        })
    }

    async fn latest(&self, kind: Option<MediaKind>, page: u32) -> Result<ProviderPage> {
        // The RSS feeds are a single un-paginated snapshot of the newest
        // items; page > 1 has nothing more to give.
        let empty = || ProviderPage {
            results: Vec::new(),
            current_page: page.max(1),
            limit: 0,
            total_count: Some(0),
            total_pages: Some(1),
        };
        if page > 1 {
            return Ok(empty());
        }
        let Some(passkey) = self.passkey.as_deref() else {
            return Ok(empty()); // RSS not configured — no contribution.
        };

        // One feed per category. `Séries` percent-encodes through Url::join.
        let feeds: &[(&str, MediaKind)] = match kind {
            Some(MediaKind::Movie) => &[("Films", MediaKind::Movie)],
            Some(MediaKind::Tv) => &[("Séries", MediaKind::Tv)],
            None => &[("Films", MediaKind::Movie), ("Séries", MediaKind::Tv)],
        };

        let mut results = Vec::new();
        for (feed, feed_kind) in feeds {
            let url = self
                .base_url
                .join(&format!("/api/v1/rss/{feed}"))
                .map_err(|e| Error::Provider(format!("torr9 join rss url: {e}")))?;
            let body = self
                .http
                .get(url)
                .query(&[("passkey", passkey)])
                .send()
                .await
                .map_err(|e| Error::Provider(format!("torr9 rss request: {e}")))?
                .error_for_status()
                .map_err(|e| Error::Provider(format!("torr9 rss status: {e}")))?
                .text()
                .await
                .map_err(|e| Error::Provider(format!("torr9 rss body: {e}")))?;

            let items = parse_torr9_rss(&body, &self.id, *feed_kind);
            if items.is_empty() {
                tracing::warn!(
                    provider = %self.id,
                    feed,
                    body_preview = %body.chars().take(200).collect::<String>(),
                    "torr9 rss yielded no items — shape may have changed",
                );
            }
            results.extend(items);
        }

        let limit = u32::try_from(results.len()).unwrap_or(u32::MAX);
        Ok(ProviderPage {
            results,
            current_page: 1,
            limit,
            total_count: Some(u64::from(limit)),
            total_pages: Some(1),
        })
    }

    async fn featured_movies(&self) -> Result<Vec<SearchResult>> {
        self.featured(FeaturedKind::Movies).await
    }

    async fn featured_series(&self) -> Result<Vec<SearchResult>> {
        self.featured(FeaturedKind::Series).await
    }

    async fn details(&self, external_id: &str) -> Result<Option<TorrentDetails>> {
        if external_id.is_empty() || !external_id.chars().all(|c| c.is_ascii_digit()) {
            return Err(Error::InvalidInput(format!(
                "torr9 external_id must be a numeric id, got `{external_id}`"
            )));
        }

        // Cache hit?
        {
            let cache = self.details_cache.lock().await;
            if let Some(c) = cache.get(external_id)
                && c.fetched_at.elapsed() < DETAILS_TTL
            {
                return Ok(Some(c.details.clone()));
            }
        }

        let url = self
            .base_url
            .join(&format!("/api/v1/torrents/{external_id}"))
            .map_err(|e| Error::Provider(format!("torr9 join details url: {e}")))?;
        let raw: TorrentDetailsRaw = self
            .authed_get(|http| http.get(url.clone()))
            .await?
            .json()
            .await
            .map_err(|e| Error::Provider(format!("torr9 details decode: {e}")))?;

        let category = match (raw.category_name.clone(), raw.parent_category_name.clone()) {
            (Some(c), Some(p)) if c != p => Some(format!("{p} / {c}")),
            (Some(c), _) => Some(c),
            (None, Some(p)) => Some(p),
            (None, None) => None,
        };

        let media_info = raw.nfo.as_deref().and_then(nfo::parse);

        let details = TorrentDetails {
            provider_id: self.id.clone(),
            external_id: external_id.to_string(),
            title: raw.title,
            description: raw.description.filter(|s| !s.is_empty()),
            description_format: DescriptionFormat::Bbcode,
            nfo: raw.nfo.filter(|s| !s.is_empty()),
            media_info,
            tags: raw.tags,
            category,
            uploader: raw.uploader_name,
            uploaded_at: raw.upload_date,
            age: raw.age,
            seeders: raw.seeders,
            leechers: raw.leechers,
            times_completed: raw.times_completed,
            views: raw.views,
            freeleech: raw.is_freeleech,
            exclusive: raw.is_exclu,
            file_count: raw.file_count,
            file_size_bytes: raw.file_size_bytes,
        };

        // Store in cache for the next click.
        self.details_cache.lock().await.insert(
            external_id.to_string(),
            CachedDetails {
                details: details.clone(),
                fetched_at: Instant::now(),
            },
        );

        Ok(Some(details))
    }

    async fn resolve(&self, external_id: &str) -> Result<TorrentSource> {
        if external_id.is_empty() || !external_id.chars().all(|c| c.is_ascii_digit()) {
            return Err(Error::InvalidInput(format!(
                "torr9 external_id must be a numeric id, got `{external_id}`"
            )));
        }
        let url = self
            .base_url
            .join(&format!("/api/v1/torrents/{external_id}/download"))
            .map_err(|e| Error::Provider(format!("torr9 join download url: {e}")))?;

        let res = self.authed_get(|http| http.get(url.clone())).await?;
        let bytes = res
            .bytes()
            .await
            .map_err(|e| Error::Provider(format!("torr9 download body: {e}")))?;

        if bytes.first().copied() != Some(BENCODE_DICT_MARKER) {
            return Err(Error::Provider(format!(
                "torr9 download for {external_id} returned non-bencoded body \
                (first byte: {:?})",
                bytes.first()
            )));
        }
        Ok(TorrentSource::TorrentFile(bytes.to_vec()))
    }
}

#[derive(Debug, Clone, Copy)]
enum FeaturedKind {
    Movies,
    Series,
}

/// torr9's `/featured/{movies,series}` returns a different shape than
/// the search endpoint — it's editorial-curated entries that POINT at
/// torrents rather than torrent rows themselves:
///
/// ```json
/// {"items": [
///   {"id":96,"type":"movie","tmdb_id":1159559,"torrent_id":210100,
///    "title":"Scream 7","poster_url":"https://image.tmdb.org/..."},
///   ...
/// ]}
/// ```
///
/// The `torrent_id` is what we need to feed back into the ingest path
/// (it's the `external_id` for `provider.resolve()`). `info_hash` /
/// `file_size_bytes` / seeders aren't in the featured payload — they
/// come back through `resolve()` if the user actually grabs.
#[derive(Debug, Deserialize)]
struct FeaturedResponse {
    #[serde(default)]
    items: Vec<FeaturedItem>,
}

#[derive(Debug, Deserialize)]
struct FeaturedItem {
    /// Editorial entry id (NOT the torrent id). Kept around for logs
    /// / future debugging only.
    #[allow(dead_code)]
    id: u64,
    /// `"movie"` / `"series"` — drives the home shelf split.
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    tmdb_id: Option<u64>,
    /// torr9 torrent id — what we pass to `Torr9::resolve` when the
    /// user clicks Préparer.
    torrent_id: u64,
    title: String,
    /// Pre-resolved poster URL. Frontend uses it directly — saves a
    /// TMDB lookup on the discovery shelf and avoids the
    /// strict-tmdb-id gate that would hide everything otherwise.
    #[serde(default)]
    poster_url: Option<String>,
}

impl FeaturedItem {
    fn into_search_result(self, provider_id: &str) -> SearchResult {
        let kind = match self.kind.as_deref() {
            Some("movie") => Some(MediaKind::Movie),
            // torr9 emits "series"; tolerate "tv" if a future variant
            // shows up.
            Some("series" | "tv") => Some(MediaKind::Tv),
            _ => None,
        };
        SearchResult {
            provider_id: provider_id.to_string(),
            external_id: self.torrent_id.to_string(),
            title: self.title,
            year: None,
            size_bytes: None,
            seeders: None,
            leechers: None,
            infohash: None,
            magnet: None,
            category: None,
            tags: Vec::new(),
            freeleech: false,
            uploader: None,
            uploaded_at: None,
            tmdb_id: self.tmdb_id.filter(|id| *id > 0),
            kind,
            poster_url: self.poster_url,
            already_in_library: false,
            library_infohash: None,
            library_file_idx: None,
            language: None,
            // torr9 fetches the `.torrent` bytes on demand via a
            // per-id authenticated endpoint — no URL to persist
            // ahead of time, no in-memory cache to lose at
            // restart.
            download_url: None,
            parsed_season: None,
            parsed_episode: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct LoginResponse {
    token: String,
}

/// Single-torrent detail payload from `GET /api/v1/torrents/:id`. Shape
/// matches torr9 v1 closely; everything optional so a future field
/// rename / removal doesn't break existing builds.
#[derive(Debug, Deserialize)]
struct TorrentDetailsRaw {
    #[allow(dead_code)]
    id: u64,
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    nfo: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    category_name: Option<String>,
    #[serde(default)]
    parent_category_name: Option<String>,
    #[serde(default)]
    uploader_name: Option<String>,
    #[serde(default)]
    upload_date: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    age: Option<String>,
    #[serde(default)]
    seeders: Option<u32>,
    #[serde(default)]
    leechers: Option<u32>,
    #[serde(default)]
    times_completed: Option<u64>,
    #[serde(default)]
    views: Option<u64>,
    #[serde(default)]
    is_freeleech: bool,
    #[serde(default)]
    is_exclu: bool,
    #[serde(default)]
    file_count: Option<u32>,
    #[serde(default)]
    file_size_bytes: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    torrents: Vec<Torrent>,
    #[serde(default)]
    current_page: Option<u32>,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    total_count: Option<u64>,
    #[serde(default)]
    total_pages: Option<u32>,
}

fn torr9_sort_field(f: SortField) -> &'static str {
    match f {
        SortField::Title => "title",
        SortField::Size => "size",
        SortField::Seeders => "seeders",
        SortField::Leechers => "leechers",
        SortField::Uploaded => "upload_date",
    }
}

/// Compose torr9's `q=` substring filter from the parsed query
/// hints. SCENE-form `<title> SxxExx` when both are known; otherwise
/// fall through to the raw user input — no regression for free text.
fn build_torr9_q(q: &SearchQuery) -> String {
    let parsed = match q.parsed_title.as_deref() {
        Some(t) if !t.is_empty() => t,
        _ => return q.q.clone(),
    };
    match (q.season, q.episode) {
        (Some(s), Some(e)) if e > 0 => format!("{parsed} S{s:02}E{e:02}"),
        (Some(s), _) => format!("{parsed} S{s:02}"),
        _ => q.q.clone(),
    }
}

#[derive(Debug, Deserialize)]
struct Torrent {
    id: u64,
    title: String,
    info_hash: String,
    file_size_bytes: Option<u64>,
    #[serde(default)]
    upload_date: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    is_freeleech: bool,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    category_name: Option<String>,
    #[serde(default)]
    parent_category_name: Option<String>,
    #[serde(default)]
    uploader_name: Option<String>,
    #[serde(default)]
    seeders: Option<u32>,
    #[serde(default)]
    leechers: Option<u32>,
    #[serde(default)]
    tmdb_id: Option<u64>,
}

impl Torrent {
    fn into_search_result(self, provider_id: &str) -> SearchResult {
        let category = match (
            self.category_name.clone(),
            self.parent_category_name.clone(),
        ) {
            (Some(c), Some(p)) if c != p => Some(format!("{p} / {c}")),
            (Some(c), _) => Some(c),
            (None, Some(p)) => Some(p),
            (None, None) => None,
        };
        let year = extract_year(&self.title);
        let kind = derive_kind(
            self.parent_category_name.as_deref(),
            self.category_name.as_deref(),
        );
        let tmdb_id = self.tmdb_id.filter(|id| *id > 0);
        SearchResult {
            provider_id: provider_id.to_string(),
            external_id: self.id.to_string(),
            title: self.title,
            year,
            size_bytes: self.file_size_bytes,
            seeders: self.seeders,
            leechers: self.leechers,
            infohash: Some(self.info_hash),
            magnet: None,
            category,
            tags: self.tags,
            freeleech: self.is_freeleech,
            uploader: self.uploader_name,
            uploaded_at: self.upload_date,
            tmdb_id,
            kind,
            poster_url: None,
            already_in_library: false,
            library_infohash: None,
            library_file_idx: None,
            language: None,
            // torr9 fetches the `.torrent` bytes on demand via a
            // per-id authenticated endpoint — no URL to persist
            // ahead of time, no in-memory cache to lose at
            // restart.
            download_url: None,
            parsed_season: None,
            parsed_episode: None,
        }
    }
}

/// Best-effort mapping from torr9's localized category labels to our coarse
/// `MediaKind`. We look at the parent first (broadest signal) then the leaf.
fn derive_kind(parent: Option<&str>, leaf: Option<&str>) -> Option<MediaKind> {
    let test = |s: &str| {
        let lower = s.to_ascii_lowercase();
        if lower.contains("films") || lower == "film" || lower.contains("animation") {
            return Some(MediaKind::Movie);
        }
        if lower.contains("séries")
            || lower.contains("series")
            || lower.contains("anime")
            || lower.contains("manga")
        {
            return Some(MediaKind::Tv);
        }
        None
    };
    parent.and_then(test).or_else(|| leaf.and_then(test))
}

// ===========================================================================
// RSS rolling-window feed parsing (`/api/v1/rss/{Films,Séries}`).
//
// Plain RSS 2.0 — one `<item>` per release. Sample:
//   <title>Scream.7.2026.MULTi.2160p...-SURCODE</title>
//   <link>https://torr9.net/torrents/293422</link>          ← numeric id
//   <description>Uploaded by Knroad | Size: 60.9 GB | Category: Films</description>
//   <pubDate>Fri, 05 Jun 2026 18:09:46 +0000</pubDate>      ← RFC2822
//   <category>Films</category>
//   <guid>https://torr9.net/torrents/293422</guid>
//   <enclosure url="…/293422/download?passkey=…" length="65443659776"
//              type="application/x-bittorrent"></enclosure>  ← signed .torrent
//
// Seeders are NOT in the feed — the freshness scheduler backfills them via
// details() for the release it actually keeps, then drops dead (0-seeder) ones.
// ===========================================================================

#[derive(Default)]
struct RssItem {
    title: Option<String>,
    /// `<link>`/`<guid>` URL — we extract the trailing numeric id from it.
    page_url: Option<String>,
    /// `<enclosure url=…>` — the passkey-signed .torrent (default
    /// `fetch_bytes` downloads it directly; the passkey lives in the URL).
    enclosure_url: Option<String>,
    /// `<enclosure length=…>` — exact size in bytes.
    length: Option<u64>,
    pub_date: Option<String>,
}

#[derive(Clone, Copy)]
enum RssTag {
    Title,
    Link,
    Guid,
    PubDate,
}

impl RssItem {
    fn into_search_result(self, provider_id: &str, kind: MediaKind) -> Option<SearchResult> {
        // The numeric torrent id (from the link/guid path) is the external_id
        // torr9's resolve()/details() expect; without it we can't grab.
        let external_id = self.page_url.as_deref().and_then(torr9_id_from_url)?;
        let title = self.title.filter(|s| !s.is_empty())?;
        let year = extract_year(&title);
        let uploaded_at = self
            .pub_date
            .as_deref()
            .and_then(|s| chrono::DateTime::parse_from_rfc2822(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc));
        Some(SearchResult {
            provider_id: provider_id.to_string(),
            external_id,
            title,
            year,
            size_bytes: self.length,
            // Not in the RSS — backfilled by the scheduler via details().
            seeders: None,
            leechers: None,
            infohash: None,
            magnet: None,
            category: Some(match kind {
                MediaKind::Movie => "Films".to_string(),
                MediaKind::Tv => "Séries".to_string(),
            }),
            tags: Vec::new(),
            freeleech: false,
            uploader: None,
            uploaded_at,
            tmdb_id: None,
            kind: Some(kind),
            poster_url: None,
            already_in_library: false,
            library_infohash: None,
            library_file_idx: None,
            language: None,
            // The signed .torrent URL — restart-safe grab via fetch_bytes.
            download_url: self.enclosure_url,
            parsed_season: None,
            parsed_episode: None,
        })
    }
}

/// Extract the trailing numeric id from a torr9 torrent URL
/// (`https://torr9.net/torrents/293489` → `"293489"`). `None` for any URL
/// whose last path segment isn't all digits.
fn torr9_id_from_url(url: &str) -> Option<String> {
    let last = url.trim_end_matches('/').rsplit('/').next()?;
    (!last.is_empty() && last.bytes().all(|b| b.is_ascii_digit())).then(|| last.to_string())
}

/// Parse a torr9 RSS feed body into search results, stamping every item with
/// the feed's `kind`. Tolerant: unknown elements are ignored and a malformed
/// item is skipped rather than failing the whole feed.
fn parse_torr9_rss(body: &str, provider_id: &str, kind: MediaKind) -> Vec<SearchResult> {
    let mut reader = Reader::from_str(body);
    reader.config_mut().trim_text(true);
    let mut out = Vec::new();
    let mut buf = Vec::new();
    let mut cur: Option<RssItem> = None;
    let mut tag: Option<RssTag> = None;

    let read_enclosure = |e: &quick_xml::events::BytesStart, item: &mut RssItem| {
        for attr in e.attributes().flatten() {
            let Ok(raw) = std::str::from_utf8(&attr.value) else {
                continue;
            };
            let val =
                xml_unescape(raw).map_or_else(|_| raw.to_string(), std::borrow::Cow::into_owned);
            match attr.key.as_ref() {
                b"url" => item.enclosure_url = Some(val),
                b"length" => item.length = val.parse().ok(),
                _ => {}
            }
        }
    };

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match e.name().as_ref() {
                b"item" => cur = Some(RssItem::default()),
                b"title" => tag = Some(RssTag::Title),
                b"link" => tag = Some(RssTag::Link),
                b"guid" => tag = Some(RssTag::Guid),
                b"pubDate" => tag = Some(RssTag::PubDate),
                b"enclosure" => {
                    if let Some(item) = cur.as_mut() {
                        read_enclosure(&e, item);
                    }
                    tag = None;
                }
                _ => tag = None,
            },
            Ok(Event::Empty(e)) => {
                if e.name().as_ref() == b"enclosure"
                    && let Some(item) = cur.as_mut()
                {
                    read_enclosure(&e, item);
                }
            }
            Ok(Event::Text(t)) => {
                if let (Some(item), Some(tg)) = (cur.as_mut(), tag)
                    && let Ok(decoded) = t.decode()
                {
                    let text = xml_unescape(decoded.as_ref())
                        .map_or_else(|_| decoded.to_string(), std::borrow::Cow::into_owned);
                    let text = text.trim().to_string();
                    if !text.is_empty() {
                        match tg {
                            RssTag::Title => item.title = Some(text),
                            // link wins, but accept guid as a fallback id.
                            RssTag::Link => item.page_url = Some(text),
                            RssTag::Guid => {
                                item.page_url.get_or_insert(text);
                            }
                            RssTag::PubDate => item.pub_date = Some(text),
                        }
                    }
                }
            }
            Ok(Event::End(e)) => {
                if e.name().as_ref() == b"item"
                    && let Some(r) = cur
                        .take()
                        .and_then(|i| i.into_search_result(provider_id, kind))
                {
                    out.push(r);
                }
                tag = None;
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0"><channel>
  <title>Torr9 - Films</title>
  <item>
    <title>Scream.7.2026.MULTi.2160p.Full.BluRay.DV.HDR.HEVC.TrueHD.Atmos.7.1-SURCODE</title>
    <link>https://torr9.net/torrents/293422</link>
    <description>Uploaded by Knroad | Size: 60.9 GB | Category: Films</description>
    <pubDate>Fri, 05 Jun 2026 18:09:46 +0000</pubDate>
    <category>Films</category>
    <guid>https://torr9.net/torrents/293422</guid>
    <enclosure url="https://api.torr9.net/api/v1/rss/torrents/293422/download?passkey=ad9c" length="65443659776" type="application/x-bittorrent"></enclosure>
  </item>
  <item>
    <title>Miss.Boots.2024.FRENCH.1080p.WEB.x264-BULiTT</title>
    <link>https://torr9.net/torrents/293467</link>
    <pubDate>Fri, 05 Jun 2026 18:37:15 +0000</pubDate>
    <guid>https://torr9.net/torrents/293467</guid>
    <enclosure url="https://api.torr9.net/api/v1/rss/torrents/293467/download?passkey=ad9c" length="2387674349" type="application/x-bittorrent"/>
  </item>
</channel></rss>"#;

    #[test]
    fn parses_torr9_rss_items() {
        let items = parse_torr9_rss(SAMPLE, "torr9", MediaKind::Movie);
        assert_eq!(items.len(), 2);

        let first = &items[0];
        assert_eq!(first.external_id, "293422");
        assert_eq!(first.provider_id, "torr9");
        assert_eq!(first.year, Some(2026));
        assert_eq!(first.size_bytes, Some(65_443_659_776));
        assert_eq!(first.kind, Some(MediaKind::Movie));
        assert!(first.seeders.is_none());
        assert_eq!(
            first.download_url.as_deref(),
            Some("https://api.torr9.net/api/v1/rss/torrents/293422/download?passkey=ad9c")
        );
        assert!(first.uploaded_at.is_some());

        // Self-closing <enclosure/> form is handled too.
        assert_eq!(items[1].external_id, "293467");
        assert!(items[1].download_url.is_some());
    }

    #[test]
    fn id_from_url_rejects_non_numeric() {
        assert_eq!(
            torr9_id_from_url("https://torr9.net/torrents/42"),
            Some("42".to_string())
        );
        assert_eq!(
            torr9_id_from_url("https://torr9.net/torrents/42/"),
            Some("42".to_string())
        );
        assert_eq!(torr9_id_from_url("https://torr9.net/about"), None);
    }
}
