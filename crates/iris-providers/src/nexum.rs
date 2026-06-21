//! Native Nexum provider — talks to Nexum's REST API (`/api/v1/*`) rather
//! than the generic Torznab bridge. Spec: `https://nexum-core.com/api/docs`.
//!
//! Why not Torznab? Nexum's Torznab bridge only maps the video buckets
//! (2000 Movies / 5000 TV / 5070 Anime / 2070 Docs / 6000 Concerts /
//! 5060 Sports) and **dumps everything else — games, software, ebooks,
//! audio — into 2000 (Movies)**. So a search for "insane" returned Windows
//! game repacks (`Forza.Horizon.6.PORTABLE-InsaneRamZes`) labelled as
//! movies, with no category signal to filter them out. The native API
//! exposes the *real* category name per torrent (`"category": "Windows"`),
//! so we classify it ourselves and drop anything Iris can't play.
//!
//! It also hands us a stable numeric torrent id (no fragile `<guid>` URL
//! parsing), rich detail (`description` `BBCode`, `tmdb_id`, seeders) and a
//! download endpoint that returns the bencoded `.torrent` with the user's
//! announce baked in.
//!
//! Config:
//! ```toml
//! [[providers]]
//! id = "nexum"
//! kind = "nexum"
//! enabled = true
//! base_url = "https://nexum-core.com"
//! api_key_env = "NEXUM_API_KEY"   # needs the `api` right (a "simple" key works)
//! ```

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use iris_config::ProviderEntry;
use iris_core::Error;
use iris_core::Result;
use iris_core::search::{
    DescriptionFormat, MediaKind, ProviderCapabilities, ProviderPage, SearchQuery, SearchResult,
    SortField, SortOrder, TorrentDetails, TorrentSource,
};
use reqwest::header::{ACCEPT, HeaderMap, HeaderValue, USER_AGENT};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use url::Url;

use crate::SearchProvider;
use crate::util::{extract_year, field_or_env, field_str};

const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:150.0) Gecko/20100101 Firefox/150.0";
/// First byte of a valid `.torrent` file (bencoded dictionary).
const BENCODE_DICT_MARKER: u8 = b'd';

pub struct NexumProvider {
    id: String,
    base_url: Url,
    api_key: String,
    http: Client,
}

impl NexumProvider {
    pub fn from_config(entry: &ProviderEntry) -> Result<Arc<Self>> {
        let base_url_str = field_str(entry, "base_url")?;
        let base_url = Url::parse(base_url_str)
            .map_err(|e| Error::Provider(format!("nexum base_url invalid: {e}")))?;
        let api_key = field_or_env(entry, "api_key")?;
        let user_agent = entry
            .fields
            .get("user_agent")
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_USER_AGENT);

        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(user_agent)
                .map_err(|e| Error::Provider(format!("nexum user_agent invalid: {e}")))?,
        );
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(
            "X-API-Key",
            HeaderValue::from_str(&api_key)
                .map_err(|e| Error::Provider(format!("nexum api key invalid: {e}")))?,
        );

        let http = Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| Error::Provider(format!("nexum http client: {e}")))?;

        Ok(Arc::new(Self {
            id: entry.id.clone(),
            base_url,
            api_key,
            http,
        }))
    }

    fn url(&self, path: &str) -> Result<Url> {
        self.base_url
            .join(path)
            .map_err(|e| Error::Provider(format!("nexum url join `{path}`: {e}")))
    }

    /// Self-contained `.torrent` download URL — auth lives in the query
    /// string (`?apikey=`) so the persisted `available_episodes.download_url`
    /// still resolves via the header-less default `fetch_bytes` after a
    /// restart. Mirrors how the Torznab path persists signed URLs.
    fn download_url(&self, id: i64) -> Result<String> {
        let mut u = self.url(&format!("/api/v1/torrents/{id}/download"))?;
        u.query_pairs_mut().append_pair("apikey", &self.api_key);
        Ok(u.into())
    }

    async fn fetch_list(
        &self,
        qs: &[(&str, String)],
        page: u32,
        limit: u32,
    ) -> Result<ProviderPage> {
        let url = self.url("/api/v1/torrents")?;
        let body = self
            .http
            .get(url)
            .query(qs)
            .send()
            .await
            .map_err(|e| Error::Provider(format!("nexum request: {e}")))?
            .error_for_status()
            .map_err(|e| Error::Provider(format!("nexum status: {e}")))?
            .text()
            .await
            .map_err(|e| Error::Provider(format!("nexum body: {e}")))?;

        let list: TorrentList = serde_json::from_str(&body).map_err(|e| {
            tracing::warn!(
                provider = %self.id,
                error = %e,
                body_preview = %body.chars().take(300).collect::<String>(),
                "nexum list parse failed",
            );
            Error::Provider(format!("nexum json: {e}"))
        })?;

        let results = list
            .torrents
            .into_iter()
            .filter_map(|t| self.build_result(t))
            .collect();

        let total_count = list.total;
        let total_pages = total_count.map(|t| {
            let l = u64::from(limit.max(1));
            u32::try_from(t.div_ceil(l)).unwrap_or(u32::MAX)
        });

        Ok(ProviderPage {
            results,
            current_page: page,
            limit,
            total_count,
            total_pages,
        })
    }

    /// Map a native torrent row to a [`SearchResult`], or `None` when its
    /// real category is something Iris can't play (games, audio, ebooks,
    /// software, training). This is the whole point of going native — the
    /// Torznab bridge hid this behind a blanket `2000`.
    fn build_result(&self, t: TorrentItem) -> Option<SearchResult> {
        let kind = classify_category(t.category.as_deref()?)?;
        let year = t
            .year
            .and_then(|y| u16::try_from(y).ok())
            .filter(|y| (1900..=2099).contains(y))
            .or_else(|| extract_year(&t.name));
        let uploaded_at = t.created_at.as_deref().and_then(parse_iso8601);
        let download_url = self.download_url(t.id).ok();

        Some(SearchResult {
            provider_id: self.id.clone(),
            external_id: t.id.to_string(),
            title: t.name,
            year,
            size_bytes: t.size,
            seeders: t.seeders,
            leechers: t.leechers,
            infohash: t.info_hash.map(|h| h.to_ascii_lowercase()),
            magnet: None,
            category: t.category,
            tags: Vec::new(),
            freeleech: t.is_freeleech,
            uploader: None,
            uploaded_at,
            // The list endpoint omits `tmdb_id` (only the detail carries it);
            // Iris's SCENE-name resolver backfills it anyway.
            tmdb_id: None,
            kind: Some(kind),
            poster_url: None,
            already_in_library: false,
            library_infohash: None,
            library_file_idx: None,
            language: None,
            download_url,
            parsed_season: None,
            parsed_episode: None,
        })
    }
}

#[async_trait]
impl SearchProvider for NexumProvider {
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
        let limit = q.limit.unwrap_or(25).clamp(1, 100);
        let page = q.page.unwrap_or(1).max(1);

        let mut qs: Vec<(&str, String)> =
            vec![("per_page", limit.to_string()), ("page", page.to_string())];
        if !q.q.trim().is_empty() {
            qs.push(("q", q.q.clone()));
        }
        if let Some(field) = q.sort_by {
            qs.push(("sort", sort_field(field).to_string()));
            qs.push((
                "dir",
                match q.order.unwrap_or(SortOrder::Desc) {
                    SortOrder::Asc => "asc",
                    SortOrder::Desc => "desc",
                }
                .to_string(),
            ));
        }

        let mut page_res = self.fetch_list(&qs, page, limit).await?;
        // Honour the MediaKind filter client-side: the native `cat` param
        // takes a single id, but a "movie" filter spans Films + Film
        // documentaire (and TV spans series/anime/émissions), so filtering
        // on our derived kind is both simpler and exact.
        if let Some(want) = q.kind {
            page_res.results.retain(|r| r.kind == Some(want));
        }
        Ok(page_res)
    }

    async fn resolve(&self, external_id: &str) -> Result<TorrentSource> {
        let id: i64 = external_id
            .parse()
            .map_err(|_| Error::Provider(format!("nexum resolve: bad id `{external_id}`")))?;
        let url = self.download_url(id)?;
        let res = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| Error::Provider(format!("nexum download: {e}")))?;
        if !res.status().is_success() && res.status() != StatusCode::FOUND {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(Error::Provider(format!(
                "nexum `{}` download failed: HTTP {status} — {body}",
                self.id,
            )));
        }
        let bytes = res
            .bytes()
            .await
            .map_err(|e| Error::Provider(format!("nexum download body: {e}")))?;

        // Defensive: some trackers serve a magnet body when the user is out
        // of credit. Nexum returns a real .torrent, but honour either.
        if bytes.starts_with(b"magnet:") {
            let s = std::str::from_utf8(&bytes)
                .map_err(|e| Error::Provider(format!("nexum magnet body: {e}")))?
                .trim()
                .to_string();
            return Ok(TorrentSource::Magnet(s));
        }
        if bytes.first().copied() != Some(BENCODE_DICT_MARKER) {
            let preview = String::from_utf8_lossy(&bytes[..bytes.len().min(200)]).into_owned();
            tracing::warn!(
                provider = %self.id,
                external_id,
                first_byte = ?bytes.first(),
                body_preview = %preview,
                "nexum download returned non-bencoded body",
            );
            return Err(Error::Provider(format!(
                "nexum `{}` download for `{external_id}` returned non-bencoded body",
                self.id,
            )));
        }
        Ok(TorrentSource::TorrentFile(bytes.to_vec()))
    }

    async fn details(&self, external_id: &str) -> Result<Option<TorrentDetails>> {
        let id: i64 = match external_id.parse() {
            Ok(id) => id,
            Err(_) => return Ok(None),
        };
        let url = self.url(&format!("/api/v1/torrents/{id}"))?;
        let res = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| Error::Provider(format!("nexum details: {e}")))?;
        if res.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let res = res
            .error_for_status()
            .map_err(|e| Error::Provider(format!("nexum details status: {e}")))?;
        let body = res
            .text()
            .await
            .map_err(|e| Error::Provider(format!("nexum details body: {e}")))?;
        let d: TorrentDetailDto = serde_json::from_str(&body)
            .map_err(|e| Error::Provider(format!("nexum details json: {e}")))?;

        Ok(Some(TorrentDetails {
            provider_id: self.id.clone(),
            external_id: d.id.to_string(),
            title: d.name,
            description: d.description.filter(|s| !s.trim().is_empty()),
            // Nexum descriptions are BBCode (the upload form's dialect).
            description_format: DescriptionFormat::Bbcode,
            nfo: None,
            media_info: None,
            tags: Vec::new(),
            category: d.category,
            uploader: None,
            uploaded_at: d.created_at.as_deref().and_then(parse_iso8601),
            age: None,
            seeders: d.seeders,
            leechers: d.leechers,
            times_completed: d.completed,
            views: None,
            freeleech: d.is_freeleech,
            exclusive: false,
            file_count: d.num_files,
            file_size_bytes: d.size,
        }))
    }
}

/// Native sort key → Nexum `sort` param.
fn sort_field(field: SortField) -> &'static str {
    match field {
        SortField::Title => "name",
        SortField::Size => "size",
        SortField::Seeders => "seeders",
        SortField::Leechers => "leechers",
        SortField::Uploaded => "created_at",
    }
}

/// Classify a Nexum category *name* into a coarse [`MediaKind`], or `None`
/// when it's something Iris can't play. Drives the games / audio / ebooks /
/// software filter — the reason this provider exists. Matched on the leaf
/// category name the API returns (`"Films"`, `"Séries TV"`, `"Windows"`, …),
/// lowercased with ASCII fallbacks; unknown categories are dropped (better
/// than leaking a new non-video bucket as a fake movie — see module docs).
fn classify_category(name: &str) -> Option<MediaKind> {
    match name.trim().to_lowercase().as_str() {
        "films"
        | "film documentaire"
        | "concerts / spectacles"
        | "concerts"
        | "spectacles"
        | "sports" => Some(MediaKind::Movie),
        "séries tv"
        | "series tv"
        | "séries"
        | "series"
        | "série documentaire"
        | "serie documentaire"
        | "animés"
        | "animes"
        | "émissions tv"
        | "emissions tv" => Some(MediaKind::Tv),
        _ => None,
    }
}

/// Parse an ISO-8601 / RFC-3339 timestamp (`2026-06-10T14:48:44+02:00`).
fn parse_iso8601(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&chrono::Utc))
}

#[derive(Debug, Deserialize)]
struct TorrentList {
    #[serde(default)]
    total: Option<u64>,
    #[serde(default)]
    torrents: Vec<TorrentItem>,
}

#[derive(Debug, Deserialize)]
struct TorrentItem {
    id: i64,
    name: String,
    #[serde(default)]
    info_hash: Option<String>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    seeders: Option<u32>,
    #[serde(default)]
    leechers: Option<u32>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    is_freeleech: bool,
    #[serde(default)]
    year: Option<i64>,
    #[serde(default)]
    created_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TorrentDetailDto {
    id: i64,
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    num_files: Option<u32>,
    #[serde(default)]
    seeders: Option<u32>,
    #[serde(default)]
    leechers: Option<u32>,
    #[serde(default)]
    completed: Option<u64>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    is_freeleech: bool,
    #[serde(default)]
    created_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real `/api/v1/torrents?q=insane` payload (games mis-served as movies
    // by the Torznab bridge): every row is a Windows game repack.
    const INSANE_LIST: &str = r#"{
        "total": 2, "page": 1, "per_page": 25,
        "torrents": [
            {"id":61563,"name":"MOUSE.P.I.For.Hire.Digital.Deluxe.Edition.PORTABLE-InsaneRamZes",
             "info_hash":"38C3EEE765861A7B3AD6D08304FA6398FC97046D","size":11217697315,
             "seeders":1,"leechers":0,"completed":1,"category":"Windows","is_freeleech":false,
             "year":null,"created_at":"2026-06-10T14:48:44+02:00"},
            {"id":61498,"name":"Forza.Horizon.6.PORTABLE-InsaneRamZes",
             "info_hash":"7A26034007B7D74C4F986C3CE620D0481F6C35D6","size":167125333508,
             "seeders":1,"leechers":0,"completed":1,"category":"Windows","is_freeleech":false,
             "year":null,"created_at":"2026-06-10T12:45:24+02:00"}
        ]
    }"#;

    fn provider() -> NexumProvider {
        NexumProvider {
            id: "nexum".into(),
            base_url: Url::parse("https://nexum-core.com").unwrap(),
            api_key: "KEY".into(),
            http: Client::new(),
        }
    }

    #[test]
    fn windows_game_rows_are_dropped() {
        let list: TorrentList = serde_json::from_str(INSANE_LIST).expect("parse");
        let p = provider();
        let kept: Vec<_> = list
            .torrents
            .into_iter()
            .filter_map(|t| p.build_result(t))
            .collect();
        assert!(
            kept.is_empty(),
            "Windows game repacks must never reach search results",
        );
    }

    #[test]
    fn video_rows_are_kept_with_unique_ids_and_kind() {
        let json = r#"{"total":2,"page":1,"per_page":25,"torrents":[
            {"id":42,"name":"The.Matrix.1999.MULTi.1080p.BluRay.x264-TEAM","info_hash":"ABCDEF",
             "size":8589934592,"seeders":12,"leechers":3,"category":"Films","is_freeleech":true,
             "year":1999,"created_at":"2026-03-15T14:30:00+00:00"},
            {"id":43,"name":"Some.Show.S01E01.MULTi.1080p.WEB.x264-GRP","info_hash":"BEEF",
             "size":1073741824,"seeders":5,"leechers":0,"category":"Séries TV","is_freeleech":false,
             "year":null,"created_at":"2026-03-15T14:30:00+00:00"}
        ]}"#;
        let list: TorrentList = serde_json::from_str(json).expect("parse");
        let p = provider();
        let kept: Vec<_> = list
            .torrents
            .into_iter()
            .filter_map(|t| p.build_result(t))
            .collect();
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].external_id, "42");
        assert_eq!(kept[0].kind, Some(MediaKind::Movie));
        assert_eq!(kept[0].year, Some(1999));
        assert_eq!(kept[0].infohash.as_deref(), Some("abcdef"));
        assert!(kept[0].freeleech);
        assert!(
            kept[0].download_url.as_deref().is_some_and(|u| u
                .contains("/api/v1/torrents/42/download")
                && u.contains("apikey=")),
        );
        assert_eq!(kept[1].external_id, "43");
        assert_eq!(kept[1].kind, Some(MediaKind::Tv));
        assert_ne!(kept[0].external_id, kept[1].external_id);
    }

    #[test]
    fn classify_covers_video_and_drops_the_rest() {
        for v in [
            "Films",
            "Film documentaire",
            "Sports",
            "Concerts / Spectacles",
        ] {
            assert_eq!(classify_category(v), Some(MediaKind::Movie), "{v}");
        }
        for v in ["Séries TV", "Animés", "Série documentaire", "Émissions TV"] {
            assert_eq!(classify_category(v), Some(MediaKind::Tv), "{v}");
        }
        for v in [
            "Windows",
            "Linux",
            "MacOS",
            "Nintendo",
            "Jeux Vidéo",
            "Musique",
            "eBooks",
            "Formation",
        ] {
            assert_eq!(classify_category(v), None, "{v}");
        }
    }
}
