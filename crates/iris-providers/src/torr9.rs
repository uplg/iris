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
    ProviderCapabilities, ProviderPage, SearchQuery, SearchResult, SortField, SortOrder,
    TorrentSource,
};
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
const TOKEN_REFRESH_AFTER: Duration = Duration::from_secs(25 * 24 * 60 * 60);
/// Bencoded torrent files start with a dictionary marker.
const BENCODE_DICT_MARKER: u8 = b'd';

pub struct Torr9 {
    id: String,
    base_url: Url,
    username: String,
    password: String,
    http: Client,
    token: Mutex<Option<CachedToken>>,
}

struct CachedToken {
    bearer: String,
    fetched_at: Instant,
}

impl Torr9 {
    pub fn from_config(entry: &ProviderEntry) -> Result<Arc<Self>> {
        let base_url_str = field_str(entry, "base_url")?;
        let base_url = Url::parse(base_url_str)
            .map_err(|e| Error::Provider(format!("torr9 base_url invalid: {e}")))?;
        let username = field_or_env(entry, "username")?;
        let password = field_or_env(entry, "password")?;
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
            http,
            token: Mutex::new(None),
        }))
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
        if let Some(t) = guard.as_ref() {
            if t.fetched_at.elapsed() < TOKEN_REFRESH_AFTER {
                return Ok(t.bearer.clone());
            }
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

        let mut qs: Vec<(&'static str, String)> = vec![
            ("q", q.q.clone()),
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

#[derive(Debug, Deserialize)]
struct LoginResponse {
    token: String,
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
}

impl Torrent {
    fn into_search_result(self, provider_id: &str) -> SearchResult {
        let category = match (self.category_name, self.parent_category_name) {
            (Some(c), Some(p)) if c != p => Some(format!("{p} / {c}")),
            (Some(c), _) => Some(c),
            (None, Some(p)) => Some(p),
            (None, None) => None,
        };
        let year = extract_year(&self.title);
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
        }
    }
}
