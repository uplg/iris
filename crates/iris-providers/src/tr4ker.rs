//! TR4KER — francophone private tracker.
//!
//! Search & download go through its clean Torznab 1.3 endpoint, so we
//! delegate to [`TorznabProvider`] for `search()` / `latest()` /
//! `resolve()`. What the spec doesn't cover is the torrent-detail page:
//! TR4KER's SPA reads `/api/torrents/{slug}` (JSON, authenticated by the
//! same key sent as `X-Api-Key`), which carries the full release page —
//! HTML description with synopsis / release notes, NFO, seeders, category
//! names, uploader, freeleech flags. The Torznab `<guid>` is a permalink
//! whose last path segment IS the slug, so `external_id` routes straight
//! into the detail endpoint.
//!
//! Config:
//! ```toml
//! [[providers]]
//! id = "tr4ker"
//! kind = "tr4ker"
//! enabled = true
//! base_url = "https://tr4ker.net"
//! api_path = "/api/"
//! api_key_env = "TR4KER_API_KEY"
//! api_key_header = "X-Api-Key"   # forwarded to the Torznab layer too
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use iris_config::ProviderEntry;
use iris_core::Error;
use iris_core::Result;
use iris_core::search::{
    DescriptionFormat, MediaKind, ProviderCapabilities, ProviderPage, SearchQuery, TorrentDetails,
    TorrentSource,
};
use reqwest::Client;
use reqwest::StatusCode;
use reqwest::header::{ACCEPT, ACCEPT_LANGUAGE, HeaderMap, HeaderName, HeaderValue, USER_AGENT};
use serde::Deserialize;
use tokio::sync::Mutex;
use url::Url;

use crate::SearchProvider;
use crate::nfo;
use crate::torznab::TorznabProvider;
use crate::util::{field_or_env, field_str};

const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:150.0) Gecko/20100101 Firefox/150.0";
/// Same rationale as c411: the user shopping the preview dialog bounces
/// between torrents; 60 s spares the indexer without letting the
/// seeder counts go meaningfully stale.
const DETAILS_TTL: Duration = Duration::from_mins(1);

pub struct Tr4ker {
    id: String,
    base_url: Url,
    http: Client,
    torznab: Arc<TorznabProvider>,
    /// `slug` -> details from the JSON API.
    details_cache: Mutex<HashMap<String, CachedDetails>>,
}

struct CachedDetails {
    details: TorrentDetails,
    fetched_at: Instant,
}

impl Tr4ker {
    pub fn from_config(entry: &ProviderEntry) -> Result<Arc<Self>> {
        let base_url_str = field_str(entry, "base_url")?;
        let base_url = Url::parse(base_url_str)
            .map_err(|e| Error::Provider(format!("tr4ker base_url invalid: {e}")))?;
        let api_key = field_or_env(entry, "api_key")?;
        // The REST detail route only authenticates via header (the
        // `apikey=` query param is a Torznab-endpoint-only affordance).
        let header_name = entry
            .fields
            .get("api_key_header")
            .and_then(|v| v.as_str())
            .unwrap_or("X-Api-Key");

        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));
        headers.insert(ACCEPT, HeaderValue::from_static("application/json, */*"));
        headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("fr,en;q=0.7"));
        let key_header = HeaderName::from_bytes(header_name.as_bytes())
            .map_err(|e| Error::Provider(format!("tr4ker api_key_header invalid: {e}")))?;
        let mut key_value = HeaderValue::from_str(&api_key)
            .map_err(|e| Error::Provider(format!("tr4ker api_key not header-safe: {e}")))?;
        key_value.set_sensitive(true);
        headers.insert(key_header, key_value);

        let http = crate::tls::client_builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| Error::Provider(format!("tr4ker http client: {e}")))?;

        let torznab = TorznabProvider::from_config(entry)?;

        Ok(Arc::new(Self {
            id: entry.id.clone(),
            base_url,
            http,
            torznab,
            details_cache: Mutex::new(HashMap::new()),
        }))
    }

    async fn fetch_details(&self, slug: &str) -> Result<Option<TorrentDetails>> {
        if slug.is_empty() || slug.contains('/') {
            // `external_id` is the guid's last path segment — anything with
            // a separator would rewrite the endpoint path.
            return Ok(None);
        }
        {
            let cache = self.details_cache.lock().await;
            if let Some(c) = cache.get(slug)
                && c.fetched_at.elapsed() < DETAILS_TTL
            {
                return Ok(Some(c.details.clone()));
            }
        }

        let url = self
            .base_url
            .join(&format!("/api/torrents/{slug}"))
            .map_err(|e| Error::Provider(format!("tr4ker join details: {e}")))?;
        let res = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| Error::Provider(format!("tr4ker details request: {e}")))?;
        if res.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(Error::Provider(format!(
                "tr4ker details HTTP {status} — {}",
                body.chars().take(200).collect::<String>(),
            )));
        }
        let body = res
            .text()
            .await
            .map_err(|e| Error::Provider(format!("tr4ker details body: {e}")))?;
        let raw: TorrentDetailRaw = serde_json::from_str(&body).map_err(|e| {
            tracing::warn!(
                provider = %self.id,
                slug,
                error = %e,
                body_preview = %body.chars().take(300).collect::<String>(),
                "tr4ker details decode failed",
            );
            Error::Provider(format!("tr4ker details decode: {e}"))
        })?;

        let details = raw.into_torrent_details(&self.id, slug);
        self.details_cache.lock().await.insert(
            slug.to_string(),
            CachedDetails {
                details: details.clone(),
                fetched_at: Instant::now(),
            },
        );
        Ok(Some(details))
    }
}

#[async_trait]
impl SearchProvider for Tr4ker {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> ProviderCapabilities {
        self.torznab.capabilities()
    }

    async fn search(&self, q: &SearchQuery) -> Result<ProviderPage> {
        self.torznab.search(q).await
    }

    async fn latest(&self, kind: Option<MediaKind>, page: u32) -> Result<ProviderPage> {
        self.torznab.latest(kind, page).await
    }

    async fn resolve(&self, external_id: &str) -> Result<TorrentSource> {
        self.torznab.resolve(external_id).await
    }

    async fn details(&self, external_id: &str) -> Result<Option<TorrentDetails>> {
        // Failures degrade to "no details" (the preview dialog then shows
        // the search-result fields only) rather than erroring the dialog.
        match self.fetch_details(external_id).await {
            Ok(d) => Ok(d),
            Err(e) => {
                tracing::warn!(
                    provider = %self.id,
                    external_id,
                    error = %e,
                    "tr4ker details failed",
                );
                Ok(None)
            }
        }
    }
}

// /api/torrents/{slug} response (the SPA's own detail payload).

#[derive(Debug, Deserialize)]
struct TorrentDetailRaw {
    name: String,
    /// Native description for tr4ker-era uploads (no format marker).
    #[serde(default)]
    description: Option<String>,
    /// Secondary free-text blob, format in `extra_info_format`.
    #[serde(default)]
    extra_info: Option<String>,
    #[serde(default)]
    extra_info_format: Option<String>,
    /// Full imported release page (YGG-era torrents), format in
    /// `classic_description_format` — `"html"` in every observed payload.
    #[serde(default)]
    classic_description: Option<String>,
    #[serde(default)]
    classic_description_format: Option<String>,
    #[serde(default)]
    nfo: Option<String>,
    #[serde(default)]
    size_bytes: Option<u64>,
    #[serde(default)]
    file_count: Option<u32>,
    #[serde(default)]
    seeders: Option<u32>,
    #[serde(default)]
    leechers: Option<u32>,
    #[serde(default)]
    times_completed: Option<u64>,
    #[serde(default)]
    views: Option<u64>,
    #[serde(default)]
    created_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    is_freeleech: bool,
    #[serde(default)]
    is_exclusive: bool,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    cat_name: Option<String>,
    #[serde(default)]
    sub_cat_name: Option<String>,
    #[serde(default)]
    uploader: Option<String>,
}

/// TR4KER's `*_format` markers vs our renderer dialects. Unknown /
/// absent formats degrade to [`DescriptionFormat::Plain`] (verbatim
/// `pre` block) — notably `"markdown"`, which we have no renderer for;
/// unrendered markdown is still readable, unrendered HTML is not.
fn map_format(fmt: Option<&str>) -> DescriptionFormat {
    match fmt {
        Some("html") => DescriptionFormat::Html,
        Some("bbcode") => DescriptionFormat::Bbcode,
        _ => DescriptionFormat::Plain,
    }
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.filter(|s| !s.trim().is_empty())
}

impl TorrentDetailRaw {
    fn into_torrent_details(self, provider_id: &str, external_id: &str) -> TorrentDetails {
        // Precedence: the imported classic page is the rich, complete one
        // (synopsis, cast, audio/subs tables, release notes); the native
        // fields exist on newer uploads where classic is null.
        let (description, description_format) =
            if let Some(body) = non_empty(self.classic_description) {
                (
                    Some(body),
                    map_format(self.classic_description_format.as_deref()),
                )
            } else if let Some(body) = non_empty(self.description) {
                (Some(body), DescriptionFormat::Plain)
            } else if let Some(body) = non_empty(self.extra_info) {
                (Some(body), map_format(self.extra_info_format.as_deref()))
            } else {
                (None, DescriptionFormat::Plain)
            };

        let nfo = non_empty(self.nfo);
        let media_info = nfo.as_deref().and_then(nfo::parse);
        let category = match (self.cat_name, self.sub_cat_name) {
            (Some(p), Some(s)) if p != s => Some(format!("{p} / {s}")),
            (Some(p), _) => Some(p),
            (None, s) => s,
        };

        TorrentDetails {
            provider_id: provider_id.to_string(),
            external_id: external_id.to_string(),
            title: self.name,
            description,
            description_format,
            nfo,
            media_info,
            tags: self.tags,
            category,
            uploader: non_empty(self.uploader),
            uploaded_at: self.created_at,
            age: None,
            seeders: self.seeders,
            leechers: self.leechers,
            times_completed: self.times_completed,
            views: self.views,
            freeleech: self.is_freeleech,
            exclusive: self.is_exclusive,
            file_count: self.file_count,
            file_size_bytes: self.size_bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Field subset of the real `/api/torrents/{slug}` payload observed
    /// 2026-07-20 (YGG-imported release: native description null, full
    /// page in `classic_description` as HTML).
    #[test]
    fn maps_classic_html_details() {
        let sample = r#"{
            "id": 885969,
            "slug": "avatar-la-voie-de-leau-2022-b67b853b",
            "info_hash": "07a56ef410040b9cbeb5994d9f059e0529ebbb42",
            "name": "Avatar : La Voie de l'eau (2022) 2160p x265-QTZ",
            "size_bytes": 20214514254,
            "file_count": 1,
            "seeders": 18,
            "leechers": 0,
            "times_completed": 13,
            "views": 11,
            "created_at": "2026-06-03T01:52:19.408676Z",
            "description": null,
            "extra_info": null,
            "extra_info_format": "markdown",
            "classic_description": "<div><h1>Avatar</h1><p>Synopsis…</p></div>",
            "classic_description_format": "html",
            "nfo": null,
            "tags": [],
            "is_freeleech": false,
            "is_exclusive": false,
            "cat_name": "Films",
            "sub_cat_name": "Film",
            "uploader": "YggTorrent",
            "source": "ygg",
            "status": 5
        }"#;
        let raw: TorrentDetailRaw = serde_json::from_str(sample).expect("parse");
        let d = raw.into_torrent_details("tr4ker", "avatar-la-voie-de-leau-2022-b67b853b");

        assert_eq!(d.external_id, "avatar-la-voie-de-leau-2022-b67b853b");
        assert_eq!(d.title, "Avatar : La Voie de l'eau (2022) 2160p x265-QTZ");
        assert_eq!(
            d.description.as_deref(),
            Some("<div><h1>Avatar</h1><p>Synopsis…</p></div>"),
        );
        assert_eq!(d.description_format, DescriptionFormat::Html);
        assert!(d.nfo.is_none());
        assert_eq!(d.category.as_deref(), Some("Films / Film"));
        assert_eq!(d.uploader.as_deref(), Some("YggTorrent"));
        assert_eq!(d.seeders, Some(18));
        assert_eq!(d.leechers, Some(0));
        assert_eq!(d.times_completed, Some(13));
        assert_eq!(d.views, Some(11));
        assert_eq!(d.file_size_bytes, Some(20_214_514_254));
        assert_eq!(d.file_count, Some(1));
        assert!(!d.freeleech);
        assert!(!d.exclusive);
        assert!(d.uploaded_at.is_some());
    }

    /// Native uploads: classic fields null, free-text `description` set.
    /// No format marker → Plain (never guess HTML on unmarked text).
    #[test]
    fn native_description_falls_back_plain() {
        let sample = r#"{
            "name": "Some.Native.Upload.1080p",
            "description": "Ripped from WEB-DL, French audio only.",
            "classic_description": null,
            "classic_description_format": "html"
        }"#;
        let raw: TorrentDetailRaw = serde_json::from_str(sample).expect("parse");
        let d = raw.into_torrent_details("tr4ker", "some-native-upload");
        assert_eq!(
            d.description.as_deref(),
            Some("Ripped from WEB-DL, French audio only."),
        );
        assert_eq!(d.description_format, DescriptionFormat::Plain);
    }

    #[test]
    fn extra_info_markdown_degrades_to_plain() {
        let sample = r##"{
            "name": "X",
            "extra_info": "# Notes\nSome markdown notes",
            "extra_info_format": "markdown"
        }"##;
        let raw: TorrentDetailRaw = serde_json::from_str(sample).expect("parse");
        let d = raw.into_torrent_details("tr4ker", "x");
        assert_eq!(
            d.description.as_deref(),
            Some("# Notes\nSome markdown notes")
        );
        assert_eq!(d.description_format, DescriptionFormat::Plain);
    }

    #[test]
    fn handles_all_optionals_missing() {
        let raw: TorrentDetailRaw = serde_json::from_str(r#"{"name": "X"}"#).expect("parse");
        let d = raw.into_torrent_details("tr4ker", "x");
        assert_eq!(d.title, "X");
        assert!(d.description.is_none());
        assert_eq!(d.description_format, DescriptionFormat::Plain);
        assert!(d.category.is_none());
        assert!(d.tags.is_empty());
        assert!(d.file_count.is_none());
    }
}
