//! `AstraTorrent` — francophone general private tracker (100 % freeleech).
//!
//! Search & latest go through its Torznab endpoint, delegated to
//! [`TorznabProvider`]. The feed carries no description, so `details()`
//! reads the site's JSON API: `/api/torrents.php?id=N` returns the TMDB
//! poster, the synopsis, the full `MediaInfo` dump, the uploader and the
//! release tags. The feed's `pubDate` carries its UTC offset where the JSON
//! `created_at` is a bare Paris wall-clock time, so the date (with
//! freeleech) comes from the feed snapshot.
//!
//! The `<guid>` is the signed download link `…/api/download.php?id=N&passkey=…`,
//! read as `external_id = N`, so `resolve()` is restart-safe: a grab whose
//! link-cache entry is gone rebuilds that link from the id.
//!
//! Config:
//! ```toml
//! [[providers]]
//! id = "astratorrent"
//! kind = "astratorrent"
//! enabled = true
//! base_url = "https://astratorrent.la"
//! api_path = "/api/torznab.php"
//! api_key_env = "ASTRA_PASSKEY"
//! tvsearch_q = false
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
use crate::nfo;
use crate::torznab::TorznabProvider;
use crate::util::{DEFAULT_USER_AGENT, base_url, field_or_env};

pub struct AstraTorrent {
    id: String,
    base_url: Url,
    passkey: String,
    http: Client,
    torznab: Arc<TorznabProvider>,
    /// torrent id -> details from the JSON API.
    details_cache: DetailsCache,
}

impl AstraTorrent {
    pub fn from_config(entry: &ProviderEntry) -> Result<Arc<Self>> {
        let base_url = base_url(entry, "astratorrent")?;
        let passkey = field_or_env(entry, "api_key")?;

        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        let http = crate::tls::client_builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| Error::Provider(format!("astratorrent http client: {e}")))?;

        Ok(Arc::new(Self {
            id: entry.id.clone(),
            base_url,
            passkey,
            http,
            torznab: TorznabProvider::from_config(entry)?,
            details_cache: DetailsCache::new(),
        }))
    }

    fn download_url(&self, torrent_id: &str) -> Result<Url> {
        let mut url = self
            .base_url
            .join("/api/download.php")
            .map_err(|e| Error::Provider(format!("astratorrent download url: {e}")))?;
        url.query_pairs_mut()
            .append_pair("id", torrent_id)
            .append_pair("passkey", &self.passkey);
        Ok(url)
    }

    async fn fetch_details(&self, torrent_id: &str) -> Result<TorrentDetails> {
        if let Some(d) = self.details_cache.get(torrent_id).await {
            return Ok(d);
        }
        let mut url = self
            .base_url
            .join("/api/torrents.php")
            .map_err(|e| Error::Provider(format!("astratorrent details url: {e}")))?;
        url.query_pairs_mut().append_pair("id", torrent_id);
        let res = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| crate::util::http_error("astratorrent details request", e))?;
        let status = res.status();
        let body = res
            .bytes()
            .await
            .map_err(|e| crate::util::http_error("astratorrent details body", e))?;
        if status != StatusCode::OK {
            return Err(Error::Provider(format!(
                "astratorrent details HTTP {status} — {}",
                crate::util::body_preview(&body),
            )));
        }
        let envelope: Envelope = serde_json::from_slice(&body)
            .map_err(|e| Error::Provider(format!("astratorrent details decode: {e}")))?;
        let Some(raw) = envelope.torrent else {
            return Err(Error::Provider(format!(
                "astratorrent details: {}",
                envelope
                    .message
                    .as_deref()
                    .unwrap_or("no torrent in the payload"),
            )));
        };

        let mut details = raw.into_torrent_details(&self.id, torrent_id);
        if let Ok(Some(feed)) = self.torznab.details(torrent_id).await {
            details.freeleech |= feed.freeleech;
            details.uploaded_at = feed.uploaded_at;
            details.category = details.category.or(feed.category);
        }
        self.details_cache
            .put(torrent_id.to_string(), details.clone())
            .await;
        Ok(details)
    }
}

/// Gate on the numeric id shape so an arbitrary `external_id` never lands in
/// a signed URL.
fn is_torrent_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 12 && s.bytes().all(|b| b.is_ascii_digit())
}

#[async_trait]
impl SearchProvider for AstraTorrent {
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
        if !matches!(cached, Err(Error::Provider(_))) || !is_torrent_id(external_id) {
            return cached;
        }
        self.torznab
            .cache_download_url(
                external_id.to_string(),
                self.download_url(external_id)?.into(),
            )
            .await;
        self.torznab.resolve(external_id).await
    }

    async fn details(&self, external_id: &str) -> Result<Option<TorrentDetails>> {
        if is_torrent_id(external_id) {
            match self.fetch_details(external_id).await {
                Ok(d) => return Ok(Some(d)),
                Err(e) => tracing::warn!(
                    provider = %self.id,
                    external_id,
                    error = %e,
                    "astratorrent details failed, using the feed snapshot",
                ),
            }
        }
        self.torznab.details(external_id).await
    }
}

/// `{"status":"success","torrent":{…}}` or `{"status":"error","message":…}`.
#[derive(Debug, Deserialize)]
struct Envelope {
    #[serde(default)]
    torrent: Option<TorrentRaw>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TorrentRaw {
    name: String,
    #[serde(default)]
    clean_title: Option<String>,
    #[serde(default)]
    sub_category: Option<String>,
    /// Comma-separated: `"MULTi (Français + Anglais), x265"`.
    #[serde(default)]
    tags: Option<String>,
    #[serde(default)]
    size_bytes: Option<u64>,
    #[serde(default)]
    seeders: Option<u32>,
    #[serde(default)]
    leechers: Option<u32>,
    #[serde(default)]
    completed_count: Option<u64>,
    #[serde(default)]
    uploader_name: Option<String>,
    #[serde(default)]
    team_name: Option<String>,
    #[serde(default)]
    poster_url: Option<String>,
    #[serde(default)]
    synopsis: Option<String>,
    /// The `MediaInfo` dump.
    #[serde(default)]
    nfo_text: Option<String>,
    #[serde(default)]
    is_freeleech: u8,
}

fn non_blank(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

impl TorrentRaw {
    /// The page the tracker renders around its data: poster, title, synopsis.
    fn description(&self) -> Option<String> {
        let poster = self
            .poster_url
            .as_deref()
            .map(str::trim)
            .filter(|u| u.starts_with("https://"))
            .map(|u| format!("[center][img]{u}[/img][/center]"));
        let synopsis = non_blank(self.synopsis.clone());
        if poster.is_none() && synopsis.is_none() {
            return None;
        }
        let title = non_blank(self.clean_title.clone()).map(|t| format!("[b]{t}[/b]"));
        let parts: Vec<String> = [poster, title, synopsis].into_iter().flatten().collect();
        Some(parts.join("\n\n"))
    }

    fn into_torrent_details(self, provider_id: &str, external_id: &str) -> TorrentDetails {
        let description = self.description();
        let mut tags: Vec<String> = self
            .tags
            .as_deref()
            .unwrap_or_default()
            .split(',')
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect();
        if let Some(team) = non_blank(self.team_name) {
            tags.push(format!("Team {team}"));
        }
        let nfo = non_blank(self.nfo_text);
        TorrentDetails {
            provider_id: provider_id.to_string(),
            external_id: external_id.to_string(),
            title: self.name,
            description,
            description_format: DescriptionFormat::Bbcode,
            media_info: nfo.as_deref().and_then(nfo::parse),
            nfo,
            tags,
            category: non_blank(self.sub_category),
            uploader: non_blank(self.uploader_name),
            uploaded_at: None,
            age: None,
            seeders: self.seeders,
            leechers: self.leechers,
            times_completed: self.completed_count,
            views: None,
            freeleech: self.is_freeleech != 0,
            exclusive: false,
            file_count: None,
            file_size_bytes: self.size_bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed `GET /api/torrents.php?id=21227` capture (2026-10).
    const DETAIL: &str = r#"{"status":"success","torrent":{
        "id": 21227,
        "info_hash": "bb0fc573448a34dffd89448f575a028cec0629de",
        "name": "The.Walking.Dead.2010.iNTEGRALE.MULTi.1080p.NF.WEB.DL.H265-TyHD",
        "clean_title": "The Walking Dead",
        "category": "series",
        "sub_category": "Packs Intégrales Multi-Saisons",
        "size_bytes": 229434492155,
        "seeders": 0,
        "leechers": 0,
        "completed_count": 3,
        "thanks_count": 0,
        "uploader_id": 4342,
        "uploader_name": "Bandidos",
        "team_name": "TyHD",
        "poster_url": "https://image.tmdb.org/t/p/w500/cVDzRylKifHGS1PFNlePfJ8sxUj.jpg",
        "tags": "Intégrale Série (Pack Multi-Saisons), MULTi (Français + Anglais), x265",
        "synopsis": "Après une apocalypse, un groupe mené par Rick Grimes tente de survivre.",
        "nfo_text": "General\nComplete name                            : The.Walking.Dead.S02E04.MULTi.1080p.NF.WEB.DL.H265-TyHD.mkv\nFormat                                   : Matroska\n\nVideo\nFormat                                   : HEVC\nWidth                                    : 1 920 pixels\nHeight                                   : 1 080 pixels\n\nAudio #1\nFormat                                   : E-AC-3\nLanguage                                 : French\n\nAudio #2\nFormat                                   : E-AC-3\nLanguage                                 : English\n",
        "audio_lang": "MULTi (Français + Anglais)",
        "sub_lang": "vff-forced-full",
        "is_freeleech": 0,
        "is_verified": 1,
        "created_at": "2026-10-28 22:07:18",
        "media_tracks": {"video": {"codec": "HEVC / H.265 (Main 10)"}, "audio": [], "subtitles": []}
    }}"#;

    #[test]
    fn maps_json_details() {
        let env: Envelope = serde_json::from_str(DETAIL).expect("parse");
        let d = env
            .torrent
            .expect("torrent")
            .into_torrent_details("astratorrent", "21227");
        assert_eq!(d.external_id, "21227");
        assert!(d.title.starts_with("The.Walking.Dead.2010.iNTEGRALE"));
        let desc = d.description.as_deref().unwrap();
        assert!(desc.starts_with(
            "[center][img]https://image.tmdb.org/t/p/w500/cVDzRylKifHGS1PFNlePfJ8sxUj.jpg[/img][/center]"
        ));
        assert!(desc.contains("[b]The Walking Dead[/b]"));
        assert!(desc.ends_with("tente de survivre."));
        assert_eq!(d.description_format, DescriptionFormat::Bbcode);
        assert_eq!(
            d.tags,
            [
                "Intégrale Série (Pack Multi-Saisons)",
                "MULTi (Français + Anglais)",
                "x265",
                "Team TyHD",
            ]
        );
        assert_eq!(
            d.category.as_deref(),
            Some("Packs Intégrales Multi-Saisons")
        );
        assert_eq!(d.uploader.as_deref(), Some("Bandidos"));
        assert_eq!(d.times_completed, Some(3));
        assert_eq!(d.file_size_bytes, Some(229_434_492_155));
        assert!(d.nfo.as_deref().unwrap().starts_with("General"));
        assert_eq!(d.media_info.expect("media info").audio.len(), 2);
        assert!(!d.freeleech);
    }

    #[test]
    fn missing_release_is_an_error_envelope() {
        let env: Envelope =
            serde_json::from_str(r#"{"status":"error","message":"Release introuvable"}"#)
                .expect("parse");
        assert!(env.torrent.is_none());
        assert_eq!(env.message.as_deref(), Some("Release introuvable"));
    }

    #[test]
    fn handles_all_optionals_missing() {
        let raw: TorrentRaw = serde_json::from_str(r#"{"name": "X"}"#).expect("parse");
        let d = raw.into_torrent_details("astratorrent", "1");
        assert!(d.description.is_none());
        assert!(d.nfo.is_none());
        assert!(d.media_info.is_none());
        assert!(d.uploader.is_none());
        assert_eq!(d.tags, [] as [String; 0]);
    }

    #[test]
    fn title_alone_is_no_description() {
        let raw: TorrentRaw = serde_json::from_str(
            r#"{"name": "X", "clean_title": "X", "poster_url": "http://p.jpg", "synopsis": " "}"#,
        )
        .expect("parse");
        assert!(raw.description().is_none());
    }

    #[test]
    fn only_numeric_ids_reach_the_signed_url() {
        assert!(is_torrent_id("21227"));
        assert!(!is_torrent_id(""));
        assert!(!is_torrent_id("21227&passkey=x"));
        assert!(!is_torrent_id("../download.php"));
    }
}
