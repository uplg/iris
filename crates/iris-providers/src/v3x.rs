//! V3X — francophone general private tracker (opened 2026-08).
//!
//! Custom stack (Next.js SPA on `v3x.club`, Fastify API on `api.v3x.club`)
//! with a clean Torznab endpoint, so search / latest delegate to
//! [`TorznabProvider`]. API keys are scoped (`torznab`, `exists`, `upload`,
//! `download`) and none of them reads the SPA's `/torrents/{id}` detail
//! route, which wants the `v3x_sid` session cookie from `POST /auth/login`.
//! With site credentials configured, `details()` rides that session (`BBCode`
//! or HTML description, NFO → media info, uploader, team, release attrs);
//! without them, or when the session call fails, it falls back to the
//! Torznab feed snapshot.
//!
//! The wrapper also makes `resolve()` restart-safe: the `<guid>` is a
//! `https://v3x.club/torrents/<uuid>` permalink and the download link is
//! `<api>/torznab/download?id=<uuid>&apikey=…`, so a grab whose link-cache
//! entry is gone (deploy, FIFO eviction) rebuilds the URL from the id
//! instead of asking the user to search again.
//!
//! Config:
//! ```toml
//! [[providers]]
//! id = "v3x"
//! kind = "v3x"
//! enabled = true
//! base_url = "https://api.v3x.club"
//! api_path = "/torznab/api"
//! api_key_env = "V3X_API_KEY"
//! username_env = "V3X_USER"       # optional: unlocks rich details
//! password_env = "V3X_PASSWORD"
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
use reqwest::header::{HeaderMap, HeaderValue, ORIGIN, REFERER};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use tokio::sync::Mutex;
use url::Url;

use crate::SearchProvider;
use crate::cache::DetailsCache;
use crate::nfo;
use crate::torznab::TorznabProvider;
use crate::util::{field_or_env, field_str, optional_field_or_env};

const SITE: &str = "https://v3x.club";

pub struct V3x {
    id: String,
    /// `<base_url>/torznab/download`, the id + key go in the query.
    download_endpoint: Url,
    api_key: String,
    torznab: Arc<TorznabProvider>,
    session: Option<Session>,
}

/// Cookie session against the SPA's API. Login is lazy (first details call)
/// and re-done once when the API answers 401 — the cookie lives 30 days but
/// the tracker may revoke it earlier.
struct Session {
    base_url: Url,
    login: String,
    password: String,
    http: Client,
    logged_in: Mutex<bool>,
    cache: DetailsCache,
}

impl V3x {
    pub fn from_config(entry: &ProviderEntry) -> Result<Arc<Self>> {
        let base_url = Url::parse(field_str(entry, "base_url")?)
            .map_err(|e| Error::Provider(format!("v3x base_url invalid: {e}")))?;
        let download_endpoint = base_url
            .join("/torznab/download")
            .map_err(|e| Error::Provider(format!("v3x download endpoint: {e}")))?;
        let session = match (
            optional_field_or_env(entry, "username")?,
            optional_field_or_env(entry, "password")?,
        ) {
            (Some(login), Some(password)) => Some(Session::new(base_url.clone(), login, password)?),
            _ => None,
        };
        Ok(Arc::new(Self {
            id: entry.id.clone(),
            download_endpoint,
            api_key: field_or_env(entry, "api_key")?,
            torznab: TorznabProvider::from_config(entry)?,
            session,
        }))
    }

    fn download_url(&self, external_id: &str) -> Option<Url> {
        if !is_uuid(external_id) {
            return None;
        }
        let mut url = self.download_endpoint.clone();
        url.query_pairs_mut()
            .append_pair("id", external_id)
            .append_pair("apikey", &self.api_key);
        Some(url)
    }
}

impl Session {
    fn new(base_url: Url, login: String, password: String) -> Result<Self> {
        // The API checks the SPA's origin on credentialed routes.
        let mut headers = HeaderMap::new();
        headers.insert(ORIGIN, HeaderValue::from_static(SITE));
        headers.insert(REFERER, HeaderValue::from_static("https://v3x.club/"));
        let http = crate::tls::client_builder()
            .default_headers(headers)
            .cookie_store(true)
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| Error::Provider(format!("v3x http client: {e}")))?;
        Ok(Self {
            base_url,
            login,
            password,
            http,
            logged_in: Mutex::new(false),
            cache: DetailsCache::new(),
        })
    }

    async fn ensure_login(&self) -> Result<()> {
        let mut logged = self.logged_in.lock().await;
        if *logged {
            return Ok(());
        }
        let url = self
            .base_url
            .join("/auth/login")
            .map_err(|e| Error::Provider(format!("v3x login url: {e}")))?;
        let res = self
            .http
            .post(url)
            .json(&serde_json::json!({
                "login": self.login,
                "password": self.password,
                "remember": true,
            }))
            .send()
            .await
            .map_err(|e| Error::Provider(format!("v3x login: {e}")))?;
        let status = res.status();
        let body: serde_json::Value = res.json().await.unwrap_or_default();
        if !status.is_success() {
            return Err(Error::Provider(format!("v3x login failed: HTTP {status}")));
        }
        if body
            .get("twoFactorRequired")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        {
            return Err(Error::Provider(
                "v3x login needs 2FA, which Iris can't answer — disable it for this account".into(),
            ));
        }
        *logged = true;
        Ok(())
    }

    async fn details(&self, provider_id: &str, id: &str) -> Result<TorrentDetails> {
        if let Some(d) = self.cache.get(id).await {
            return Ok(d);
        }
        let url = self
            .base_url
            .join(&format!("/torrents/{id}"))
            .map_err(|e| Error::Provider(format!("v3x details url: {e}")))?;
        let mut retried = false;
        let raw: DetailRaw = loop {
            self.ensure_login().await?;
            let res = self
                .http
                .get(url.clone())
                .send()
                .await
                .map_err(|e| Error::Provider(format!("v3x details: {e}")))?;
            if res.status() == StatusCode::UNAUTHORIZED && !retried {
                retried = true;
                *self.logged_in.lock().await = false;
                continue;
            }
            if !res.status().is_success() {
                return Err(Error::Provider(format!(
                    "v3x details: HTTP {}",
                    res.status()
                )));
            }
            break res
                .json()
                .await
                .map_err(|e| Error::Provider(format!("v3x details body: {e}")))?;
        };
        let d = raw.into_details(provider_id, id);
        self.cache.put(id.to_string(), d.clone()).await;
        Ok(d)
    }
}

/// `GET /torrents/{id}` — the SPA's own release page payload.
#[expect(clippy::struct_excessive_bools)] // the API payload's own flags
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DetailRaw {
    name: String,
    #[serde(default)]
    description: Option<String>,
    /// `"standard"` (`BBCode`) or `"html"`.
    #[serde(default)]
    description_format: Option<String>,
    #[serde(default)]
    nfo: Option<String>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    file_count: Option<u32>,
    #[serde(default)]
    seeders: Option<u32>,
    #[serde(default)]
    leechers: Option<u32>,
    #[serde(default)]
    completed: Option<u64>,
    #[serde(default)]
    created_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    is_exclusive: bool,
    #[serde(default)]
    is_freeleech: bool,
    #[serde(default)]
    freeleech_active: bool,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    category_name: Option<String>,
    #[serde(default)]
    uploader: Option<String>,
    #[serde(default)]
    anonymous: bool,
    #[serde(default)]
    team_name: Option<String>,
    #[serde(default)]
    attrs: Option<Attrs>,
}

#[derive(Debug, Default, Deserialize)]
struct Attrs {
    resolution: Option<String>,
    source: Option<String>,
    codec: Option<String>,
    hdr: Option<String>,
    audio: Option<String>,
}

impl DetailRaw {
    fn into_details(self, provider_id: &str, external_id: &str) -> TorrentDetails {
        let attrs = self.attrs.unwrap_or_default();
        let tags: Vec<String> = [
            self.language,
            attrs.resolution,
            attrs.source,
            attrs.codec,
            attrs.hdr,
            attrs.audio,
            self.team_name.map(|t| format!("Team {t}")),
        ]
        .into_iter()
        .flatten()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
        let nfo = self.nfo.filter(|n| !n.trim().is_empty());
        TorrentDetails {
            provider_id: provider_id.to_string(),
            external_id: external_id.to_string(),
            title: self.name,
            description: self
                .description
                .map(|d| d.replace("\r\n", "\n"))
                .filter(|d| !d.trim().is_empty()),
            description_format: match self.description_format.as_deref() {
                Some("html") => DescriptionFormat::Html,
                _ => DescriptionFormat::Bbcode,
            },
            media_info: nfo.as_deref().and_then(nfo::parse),
            nfo,
            tags,
            category: self.category_name,
            uploader: self.uploader.filter(|_| !self.anonymous),
            uploaded_at: self.created_at,
            age: None,
            seeders: self.seeders,
            leechers: self.leechers,
            times_completed: self.completed,
            views: None,
            freeleech: self.is_freeleech || self.freeleech_active,
            exclusive: self.is_exclusive,
            file_count: self.file_count,
            file_size_bytes: self.size,
        }
    }
}

/// `8-4-4-4-12` hex. Gate on the shape so an arbitrary `external_id` never
/// lands in a signed URL.
fn is_uuid(s: &str) -> bool {
    let groups: Vec<&str> = s.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(g, n)| g.len() == n && g.bytes().all(|b| b.is_ascii_hexdigit()))
}

#[async_trait]
impl SearchProvider for V3x {
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
        let cached = self.torznab.resolve(external_id).await;
        let (Err(Error::Provider(_)), Some(url)) = (&cached, self.download_url(external_id)) else {
            return cached;
        };
        self.torznab
            .cache_download_url(external_id.to_string(), url.into())
            .await;
        self.torznab.resolve(external_id).await
    }

    async fn details(&self, external_id: &str) -> Result<Option<TorrentDetails>> {
        if let Some(session) = &self.session
            && is_uuid(external_id)
        {
            match session.details(&self.id, external_id).await {
                Ok(d) => return Ok(Some(d)),
                Err(e) => tracing::warn!(
                    provider = %self.id,
                    external_id,
                    error = %e,
                    "v3x session details failed, using the feed snapshot",
                ),
            }
        }
        self.torznab.details(external_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed `GET /torrents/{id}` capture (2026-10).
    const DETAIL: &str = r#"{
        "id": "29f20a1c-fef8-4d35-8efd-a965de04813a",
        "name": "Avengers.Endgame.2019.MULTi.TRUEFRENCH.1080p.BluRay.x264-RiFiFi",
        "description": "[center][b]Avengers : Endgame (2019)[/b][/center]\r\n[table]\r\n[tr][td]Sortie[/td][td]24 avril 2019[/td][/tr]\r\n[/table]",
        "descriptionFormat": "standard",
        "nfo": "General\r\nComplete name : Avengers.mkv\r\n\r\nVideo\r\nFormat : AVC\r\nWidth : 1 920 pixels\r\nHeight : 804 pixels\r\n\r\nAudio\r\nFormat : E-AC-3\r\nLanguage : French\r\n",
        "size": 23691360252,
        "fileCount": 2,
        "seeders": 2,
        "leechers": 0,
        "completed": 0,
        "createdAt": "2026-10-01T18:12:18.120Z",
        "isExclusive": false,
        "isFreeleech": false,
        "freeleechActive": false,
        "language": "Multi (FR inclus)",
        "categoryName": "Film",
        "uploader": "RussianFighter",
        "anonymous": false,
        "teamName": "RiFiFi",
        "attrs": {"resolution": "1080p", "hdr": null, "source": "BluRay", "codec": "x264",
                  "audio": "EAC3 7.1", "lang": "MULTI"}
    }"#;

    #[test]
    fn session_detail_maps_onto_details() {
        let raw: DetailRaw = serde_json::from_str(DETAIL).unwrap();
        let d = raw.into_details("v3x", "29f20a1c-fef8-4d35-8efd-a965de04813a");
        assert_eq!(
            d.title,
            "Avengers.Endgame.2019.MULTi.TRUEFRENCH.1080p.BluRay.x264-RiFiFi"
        );
        assert!(matches!(d.description_format, DescriptionFormat::Bbcode));
        assert!(!d.description.as_deref().unwrap().contains('\r'));
        assert_eq!(d.uploader.as_deref(), Some("RussianFighter"));
        assert_eq!(d.category.as_deref(), Some("Film"));
        assert_eq!(d.file_count, Some(2));
        assert_eq!(d.file_size_bytes, Some(23_691_360_252));
        assert_eq!(
            d.tags,
            [
                "Multi (FR inclus)",
                "1080p",
                "BluRay",
                "x264",
                "EAC3 7.1",
                "Team RiFiFi"
            ]
        );
        let video = d.media_info.and_then(|m| m.video).expect("NFO parsed");
        assert_eq!(video.resolution.as_deref(), Some("1920x804"));
    }

    #[test]
    fn anonymous_upload_hides_the_uploader_and_html_is_kept() {
        let mut v: serde_json::Value = serde_json::from_str(DETAIL).unwrap();
        v["anonymous"] = true.into();
        v["descriptionFormat"] = "html".into();
        let d = serde_json::from_value::<DetailRaw>(v)
            .unwrap()
            .into_details("v3x", "x");
        assert_eq!(d.uploader, None);
        assert!(matches!(d.description_format, DescriptionFormat::Html));
    }

    #[test]
    fn uuid_shape_gate() {
        assert!(is_uuid("3839cf2e-c14a-407c-a143-e49ac2b83cf1"));
        assert!(!is_uuid("3839cf2e-c14a-407c-a143-e49ac2b83cf"));
        assert!(!is_uuid("../../torznab/api?t=caps"));
        assert!(!is_uuid(""));
    }

    #[test]
    fn download_url_is_rebuilt_from_the_id() {
        let entry: ProviderEntry = toml::from_str(
            r#"
            id = "v3x"
            kind = "v3x"
            base_url = "https://api.v3x.club"
            api_path = "/torznab/api"
            api_key = "KEY"
            "#,
        )
        .unwrap();
        let p = V3x::from_config(&entry).unwrap();
        assert_eq!(
            p.download_url("3839cf2e-c14a-407c-a143-e49ac2b83cf1")
                .unwrap()
                .as_str(),
            "https://api.v3x.club/torznab/download?id=3839cf2e-c14a-407c-a143-e49ac2b83cf1&apikey=KEY"
        );
        assert!(p.download_url("not-an-id").is_none());
    }

    /// `V3X_API_KEY=… [V3X_USER=… V3X_PASSWORD=…] cargo test -p iris-providers v3x_live -- --ignored`.
    #[tokio::test]
    #[ignore = "hits the live V3X API, needs V3X_API_KEY"]
    async fn v3x_live_search_details_and_cold_resolve() {
        let entry: ProviderEntry = toml::from_str(
            r#"
            id = "v3x"
            kind = "v3x"
            base_url = "https://api.v3x.club"
            api_path = "/torznab/api"
            api_key_env = "V3X_API_KEY"
            username_env = "V3X_USER"
            password_env = "V3X_PASSWORD"
            "#,
        )
        .unwrap();
        let p = V3x::from_config(&entry).expect("provider builds");
        let page = p
            .search(&SearchQuery {
                q: "dune".into(),
                page: Some(1),
                limit: None,
                sort_by: None,
                order: None,
                kind: Some(MediaKind::Movie),
                parsed_title: None,
                season: None,
                episode: None,
                year: None,
            })
            .await
            .expect("search succeeds");
        let first = page.results.first().expect("dune has results");
        assert!(is_uuid(&first.external_id), "{}", first.external_id);
        let d = p
            .details(&first.external_id)
            .await
            .unwrap()
            .expect("details");
        assert_eq!(d.title, first.title);
        if std::env::var("V3X_USER").is_ok() {
            assert!(
                d.description.is_some(),
                "session details carry the description"
            );
        }

        // A fresh instance has an empty link cache: resolve must rebuild
        // the download URL from the id alone.
        let cold = V3x::from_config(&entry).unwrap();
        match cold
            .resolve(&first.external_id)
            .await
            .expect("cold resolve")
        {
            TorrentSource::TorrentFile(b) => assert_eq!(b.first(), Some(&b'd')),
            TorrentSource::Magnet(m) => panic!("expected .torrent, got {m}"),
        }
    }
}
