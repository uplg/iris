//! Draupnirr — francophone general private tracker.
//!
//! Search & latest go through its clean Torznab 1.3 endpoint, delegated to
//! [`TorznabProvider`]. The feed carries no description, so `details()`
//! reads the scriptable REST API instead: `/api/torrents/{id|infohash}`
//! returns the release page as `BBCode` (TMDB poster, synopsis, technical
//! sheet), the uploader and the release tags. What the REST payload lacks
//! (freeleech, grabs) is taken from the feed snapshot.
//!
//! The `<guid>` is `ygrasil-<infohash>`, so `external_id` routes straight
//! into the REST API, and `resolve()` is restart-safe: a grab whose
//! link-cache entry is gone rebuilds `/api/torrents/{infohash}/download`.
//!
//! One key everywhere: the account passkey, sent as the Torznab `apikey=`
//! and as `X-Api-Key` on the REST routes.
//!
//! Config:
//! ```toml
//! [[providers]]
//! id = "draupnirr"
//! kind = "draupnirr"
//! enabled = true
//! base_url = "https://draupnirr.xyz"
//! api_path = "/torznab/api"
//! api_key_env = "DRAUPNIRR_PASSKEY"
//! ```

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use iris_config::ProviderEntry;
use iris_core::Error;
use iris_core::Result;
use iris_core::search::{
    DescriptionFormat, MediaKind, ProviderCapabilities, ProviderPage, SearchQuery, TorrentDetails,
    TorrentSource,
};
use reqwest::header::{ACCEPT, HeaderMap, HeaderValue, USER_AGENT};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use url::Url;

use crate::SearchProvider;
use crate::cache::DetailsCache;
use crate::torznab::TorznabProvider;
use crate::util::{DEFAULT_USER_AGENT, base_url, field_or_env};

pub struct Draupnirr {
    id: String,
    base_url: Url,
    api_key: String,
    /// REST client, the passkey in `X-Api-Key`.
    http: Client,
    torznab: Arc<TorznabProvider>,
    /// `infohash` -> details from the REST API.
    details_cache: DetailsCache,
}

impl Draupnirr {
    pub fn from_config(entry: &ProviderEntry) -> Result<Arc<Self>> {
        let base_url = base_url(entry, "draupnirr")?;
        let api_key = field_or_env(entry, "api_key")?;

        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        let mut key_value = HeaderValue::from_str(&api_key)
            .map_err(|e| Error::Provider(format!("draupnirr passkey not header-safe: {e}")))?;
        key_value.set_sensitive(true);
        headers.insert("X-Api-Key", key_value);

        let http = crate::tls::client_builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| Error::Provider(format!("draupnirr http client: {e}")))?;

        Ok(Arc::new(Self {
            id: entry.id.clone(),
            base_url,
            api_key,
            http,
            torznab: TorznabProvider::from_config(entry)?,
            details_cache: DetailsCache::new(),
        }))
    }

    /// The download route reached through the Torznab client, which carries
    /// no `X-Api-Key`, so the key rides the query like the feed's own links.
    fn download_url(&self, infohash: &str) -> Result<Url> {
        let mut url = self
            .base_url
            .join(&format!("/api/torrents/{infohash}/download"))
            .map_err(|e| Error::Provider(format!("draupnirr download url: {e}")))?;
        url.query_pairs_mut().append_pair("apikey", &self.api_key);
        Ok(url)
    }

    async fn fetch_details(&self, external_id: &str, infohash: &str) -> Result<TorrentDetails> {
        if let Some(d) = self.details_cache.get(infohash).await {
            return Ok(d);
        }
        let url = self
            .base_url
            .join(&format!("/api/torrents/{infohash}"))
            .map_err(|e| Error::Provider(format!("draupnirr details url: {e}")))?;
        let res = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| crate::util::http_error("draupnirr details request", e))?;
        let status = res.status();
        let body = res
            .bytes()
            .await
            .map_err(|e| crate::util::http_error("draupnirr details body", e))?;
        if status != StatusCode::OK {
            return Err(Error::Provider(format!(
                "draupnirr details HTTP {status} — {}",
                crate::util::body_preview(&body),
            )));
        }
        let raw: TorrentDetailRaw = serde_json::from_slice(&body)
            .map_err(|e| Error::Provider(format!("draupnirr details decode: {e}")))?;

        let mut details = raw.into_torrent_details(&self.id, external_id);
        if let Ok(Some(feed)) = self.torznab.details(external_id).await {
            details.freeleech = feed.freeleech;
            details.times_completed = feed.times_completed;
            details.category = feed.category;
        }
        self.details_cache
            .put(infohash.to_string(), details.clone())
            .await;
        Ok(details)
    }
}

/// `ygrasil-<infohash>` → the infohash; anything else (a guid shape the
/// tracker might adopt later) is not routed into the REST paths.
fn infohash(external_id: &str) -> Option<&str> {
    let hash = external_id.rsplit('-').next()?;
    (hash.len() == 40 && hash.bytes().all(|b| b.is_ascii_hexdigit())).then_some(hash)
}

#[async_trait]
impl SearchProvider for Draupnirr {
    fn id(&self) -> &str {
        &self.id
    }

    fn http(&self) -> &reqwest::Client {
        self.torznab.http()
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
        let cached = self.torznab.resolve(external_id).await;
        let (Err(Error::Provider(_)), Some(hash)) = (&cached, infohash(external_id)) else {
            return cached;
        };
        self.torznab
            .cache_download_url(external_id.to_string(), self.download_url(hash)?.into())
            .await;
        self.torznab.resolve(external_id).await
    }

    async fn details(&self, external_id: &str) -> Result<Option<TorrentDetails>> {
        if let Some(hash) = infohash(external_id) {
            match self.fetch_details(external_id, hash).await {
                Ok(d) => return Ok(Some(d)),
                Err(e) => tracing::warn!(
                    provider = %self.id,
                    external_id,
                    error = %e,
                    "draupnirr details failed, using the feed snapshot",
                ),
            }
        }
        self.torznab.details(external_id).await
    }
}

#[derive(Debug, Deserialize)]
struct TorrentDetailRaw {
    name: String,
    /// `BBCode`, the whole release page.
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    size_bytes: Option<u64>,
    #[serde(default)]
    file_count: Option<u32>,
    #[serde(default)]
    seeders: Option<u32>,
    #[serde(default)]
    leechers: Option<u32>,
    #[serde(default)]
    uploader: Option<String>,
    #[serde(default)]
    created_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    meta: Meta,
}

#[derive(Debug, Default, Deserialize)]
struct Meta {
    #[serde(default)]
    tags: Vec<String>,
}

impl TorrentDetailRaw {
    fn into_torrent_details(self, provider_id: &str, external_id: &str) -> TorrentDetails {
        TorrentDetails {
            provider_id: provider_id.to_string(),
            external_id: external_id.to_string(),
            title: self.name,
            description: self.description.filter(|s| !s.trim().is_empty()),
            description_format: DescriptionFormat::Bbcode,
            nfo: None,
            media_info: None,
            tags: self.meta.tags,
            category: None,
            uploader: self.uploader.filter(|s| !s.trim().is_empty()),
            uploaded_at: self.created_at,
            age: None,
            seeders: self.seeders,
            leechers: self.leechers,
            times_completed: None,
            views: None,
            freeleech: false,
            exclusive: false,
            file_count: self.file_count,
            file_size_bytes: self.size_bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed `GET /api/torrents/{infohash}` capture (2026-10).
    const DETAIL: &str = r#"{
        "id": "10bTcv0Y3a",
        "infohash": "a7f426006ec2b96dcb30cf959eb142ea0b12968a",
        "name": "Dune.Deuxieme.partie.2024.MULTi.1080p.WEB.DL.EAC3.Atmos.5.1.H264-Slay3R",
        "category": "films-film",
        "status": "approved",
        "size_bytes": 10199407366,
        "seeders": 3,
        "leechers": 0,
        "uploader": "FrostReaper",
        "created_at": "2026-10-26T12:52:01+00:00",
        "download_url": "https://draupnirr.xyz/api/torrents/a7f426006ec2b96dcb30cf959eb142ea0b12968a/download",
        "file_count": 1,
        "description": "[center][size=22][b]Dune : Deuxième partie[/b][/size][/center]\n\n[h2]Synopsis[/h2]\n[quote]Le voyage mythique de Paul Atreides.[/quote]",
        "meta": {
            "tags": ["MULTi", "1080p", "WEB.DL"],
            "year": "2024",
            "tmdb_id": "693134",
            "tmdb_type": "movie"
        },
        "slot": {"code": "HCOPT-1080-WEB", "rank": 3, "work": {"tmdb_id": 693134, "episode": null}}
    }"#;

    #[test]
    fn maps_rest_details() {
        let raw: TorrentDetailRaw = serde_json::from_str(DETAIL).expect("parse");
        let id = "ygrasil-a7f426006ec2b96dcb30cf959eb142ea0b12968a";
        let d = raw.into_torrent_details("draupnirr", id);
        assert_eq!(d.external_id, id);
        assert!(d.title.starts_with("Dune.Deuxieme.partie.2024"));
        assert!(
            d.description
                .as_deref()
                .unwrap()
                .contains("[h2]Synopsis[/h2]")
        );
        assert_eq!(d.description_format, DescriptionFormat::Bbcode);
        assert_eq!(d.uploader.as_deref(), Some("FrostReaper"));
        assert_eq!(d.tags, ["MULTi", "1080p", "WEB.DL"]);
        assert_eq!(d.seeders, Some(3));
        assert_eq!(d.leechers, Some(0));
        assert_eq!(d.file_count, Some(1));
        assert_eq!(d.file_size_bytes, Some(10_199_407_366));
        assert!(d.uploaded_at.is_some());
    }

    #[test]
    fn handles_all_optionals_missing() {
        let raw: TorrentDetailRaw = serde_json::from_str(r#"{"name": "X"}"#).expect("parse");
        let d = raw.into_torrent_details("draupnirr", "x");
        assert!(d.description.is_none());
        assert!(d.uploader.is_none());
        assert_eq!(d.tags, [] as [String; 0]);
    }

    #[test]
    fn infohash_comes_from_the_guid() {
        assert_eq!(
            infohash("ygrasil-a7f426006ec2b96dcb30cf959eb142ea0b12968a"),
            Some("a7f426006ec2b96dcb30cf959eb142ea0b12968a"),
        );
        assert_eq!(
            infohash("a7f426006ec2b96dcb30cf959eb142ea0b12968a"),
            Some("a7f426006ec2b96dcb30cf959eb142ea0b12968a"),
        );
        assert_eq!(infohash("ygrasil-10bTcv0Y3a"), None);
        assert_eq!(
            infohash("ygrasil-../../me/a7f426006ec2b96dcb30cf959eb1"),
            None
        );
    }
}
