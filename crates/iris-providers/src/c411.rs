//! c411 (Communauté 411) — French private tracker.
//!
//! Search & download go through the standard Torznab endpoint, so we
//! delegate to [`TorznabProvider`] for `search()` / `resolve()`. What
//! the spec doesn't cover is a *featured* shelf: c411 publishes a JSON
//! homepage at `/api/homepage` (the same one their Nuxt frontend
//! consumes) which gives us an editorially-curated list of recent
//! exclusives with TMDB ids and posters — perfect for the discovery
//! home.
//!
//! Config:
//! ```toml
//! [[providers]]
//! id = "c411"
//! kind = "c411"
//! enabled = true
//! base_url = "https://c411.org"
//! api_key_env = "C411_API_KEY"
//! # optional Torznab tunings (passed through to the generic provider):
//! # api_path = "/api"
//! # movie_categories = "2000,2030,2040,2045,2050"
//! # tv_categories    = "5000,5030,5040,5045,5070"
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use iris_config::ProviderEntry;
use iris_core::Error;
use iris_core::Result;
use iris_core::search::{
    DescriptionFormat, MediaKind, ProviderCapabilities, ProviderPage, SearchQuery, SearchResult,
    TorrentDetails, TorrentSource,
};
use reqwest::Client;
use reqwest::StatusCode;
use reqwest::header::{
    ACCEPT, ACCEPT_LANGUAGE, AUTHORIZATION, HeaderMap, HeaderValue, REFERER, USER_AGENT,
};
use serde::Deserialize;
use tokio::sync::Mutex;
use url::Url;

use crate::SearchProvider;
use crate::nfo;
use crate::torznab::TorznabProvider;
use crate::util::{extract_year, field_or_env, field_str};

const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:150.0) Gecko/20100101 Firefox/150.0";
/// Featured shelves are editorial — refreshes are slow. 30 min keeps
/// the home page cheap without going stale on c411's daily cadence.
const FEATURED_TTL: Duration = Duration::from_mins(30);
/// `details()` reads the same payload while the user shops around the
/// preview dialog. 60 s avoids hammering c411 when the user bounces
/// between 5 torrents in 30 seconds, but stays fresh enough for
/// seeders/leechers to be representative.
const DETAILS_TTL: Duration = Duration::from_mins(1);

pub struct C411 {
    id: String,
    base_url: Url,
    http: Client,
    torznab: Arc<TorznabProvider>,
    featured_cache: Mutex<Option<CachedFeatured>>,
    /// `infohash` -> `TorrentDetails` from c411's JSON API. Survives
    /// short windows of UI navigation without re-hitting the indexer.
    details_cache: Mutex<HashMap<String, CachedDetails>>,
}

struct CachedFeatured {
    movies: Vec<SearchResult>,
    series: Vec<SearchResult>,
    fetched_at: Instant,
}

struct CachedDetails {
    details: TorrentDetails,
    fetched_at: Instant,
}

impl C411 {
    pub fn from_config(entry: &ProviderEntry) -> Result<Arc<Self>> {
        let base_url_str = field_str(entry, "base_url")?;
        let base_url = Url::parse(base_url_str)
            .map_err(|e| Error::Provider(format!("c411 base_url invalid: {e}")))?;
        // Same key c411 uses for the Torznab endpoint also authenticates
        // the JSON `/api/*` routes when sent as a Bearer token — that's
        // how the SPA wires through to authenticated users without
        // re-doing the CSRF dance.
        let api_key = field_or_env(entry, "api_key")?;

        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));
        headers.insert(ACCEPT, HeaderValue::from_static("application/json, */*"));
        headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("fr,en;q=0.7"));
        let bearer = format!("Bearer {api_key}");
        let mut auth_value = HeaderValue::from_str(&bearer)
            .map_err(|e| Error::Provider(format!("c411 api_key invalid header: {e}")))?;
        auth_value.set_sensitive(true);
        headers.insert(AUTHORIZATION, auth_value);
        let referer = format!("{}/", base_url.as_str().trim_end_matches('/'));
        headers.insert(
            REFERER,
            HeaderValue::from_str(&referer)
                .map_err(|e| Error::Provider(format!("c411 referer invalid: {e}")))?,
        );

        let http = crate::tls::client_builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| Error::Provider(format!("c411 http client: {e}")))?;

        let torznab = TorznabProvider::from_config(entry)?;

        Ok(Arc::new(Self {
            id: entry.id.clone(),
            base_url,
            http,
            torznab,
            featured_cache: Mutex::new(None),
            details_cache: Mutex::new(HashMap::new()),
        }))
    }

    async fn fetch_details(&self, infohash: &str) -> Result<Option<TorrentDetails>> {
        if !is_infohash(infohash) {
            // Featured items expose the infohash as `external_id`; if a
            // caller hands us something else (e.g. a numeric Torznab guid
            // from another indexer mistakenly routed here), the c411 API
            // would 404 — surface as "no details" instead of an error.
            return Ok(None);
        }
        {
            let cache = self.details_cache.lock().await;
            if let Some(c) = cache.get(infohash)
                && c.fetched_at.elapsed() < DETAILS_TTL
            {
                return Ok(Some(c.details.clone()));
            }
        }

        let url = self
            .base_url
            .join(&format!("/api/torrents/{infohash}"))
            .map_err(|e| Error::Provider(format!("c411 join details: {e}")))?;
        let res = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| Error::Provider(format!("c411 details request: {e}")))?;
        if res.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(Error::Provider(format!(
                "c411 details HTTP {status} — {}",
                body.chars().take(200).collect::<String>(),
            )));
        }
        let body = res
            .text()
            .await
            .map_err(|e| Error::Provider(format!("c411 details body: {e}")))?;
        let raw: TorrentDetailRaw = serde_json::from_str(&body).map_err(|e| {
            tracing::warn!(
                provider = %self.id,
                infohash,
                error = %e,
                body_preview = %body.chars().take(300).collect::<String>(),
                "c411 details decode failed",
            );
            Error::Provider(format!("c411 details decode: {e}"))
        })?;

        let details = raw.into_torrent_details(&self.id, infohash);
        self.details_cache.lock().await.insert(
            infohash.to_string(),
            CachedDetails {
                details: details.clone(),
                fetched_at: Instant::now(),
            },
        );
        Ok(Some(details))
    }

    async fn refresh_featured_if_stale(&self) -> Result<()> {
        if let Some(c) = self.featured_cache.lock().await.as_ref()
            && c.fetched_at.elapsed() < FEATURED_TTL
        {
            return Ok(());
        }

        let url = self
            .base_url
            .join("/api/homepage")
            .map_err(|e| Error::Provider(format!("c411 join homepage: {e}")))?;
        let body = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| Error::Provider(format!("c411 homepage request: {e}")))?
            .error_for_status()
            .map_err(|e| Error::Provider(format!("c411 homepage status: {e}")))?
            .text()
            .await
            .map_err(|e| Error::Provider(format!("c411 homepage body: {e}")))?;

        let resp: HomepageResponse = serde_json::from_str(&body).map_err(|e| {
            tracing::warn!(
                provider = %self.id,
                error = %e,
                body_preview = %body.chars().take(300).collect::<String>(),
                "c411 homepage decode failed",
            );
            Error::Provider(format!("c411 homepage decode: {e}"))
        })?;

        let mut movies = Vec::new();
        let mut series = Vec::new();
        for item in resp.exclusive_recent {
            let kind = classify_title(&item.title);
            // Use the infohash as `external_id` so featured items live
            // in the same namespace as Torznab search hits (whose
            // `<guid>` content is the infohash for c411). Without this
            // alignment, `resolve()` would never find the cached
            // `.torrent` download URL populated by an earlier search
            // — and the preview path would fail with a "magnet
            // sources need full ingest first" error.
            let external_id = item.info_hash.clone();

            let result = SearchResult {
                provider_id: self.id.clone(),
                external_id,
                title: item.title.clone(),
                year: extract_year(&item.title),
                size_bytes: None,
                seeders: None,
                leechers: None,
                infohash: Some(item.info_hash),
                // No magnet: c411 serves real `.torrent` files via its
                // Torznab endpoint and that's what we want to land at
                // ingest / preview time. `resolve()` primes the
                // Torznab link cache on demand below.
                magnet: None,
                category: None,
                tags: Vec::new(),
                freeleech: false,
                uploader: Some(item.uploader_username),
                uploaded_at: None,
                tmdb_id: item.tmdb_id.filter(|n| *n > 0),
                kind: Some(kind),
                poster_url: item.poster_url,
                already_in_library: false,
                library_infohash: None,
                library_file_idx: None,
                language: None,
                codec: None,
                // c411 featured items don't ship a `.torrent` URL
                // — they're identified by infohash and resolved
                // through the underlying Torznab search at grab
                // time. The Torznab `into_search_result()` path
                // does set this when it has one.
                download_url: None,
                parsed_season: None,
                parsed_episode: None,
            };
            match kind {
                MediaKind::Movie => movies.push(result),
                MediaKind::Tv => series.push(result),
            }
        }

        tracing::info!(
            provider = %self.id,
            movies = movies.len(),
            series = series.len(),
            "c411 featured refreshed",
        );

        *self.featured_cache.lock().await = Some(CachedFeatured {
            movies,
            series,
            fetched_at: Instant::now(),
        });
        Ok(())
    }

    /// Look up a featured item's release title (the `name` we got from
    /// `/api/homepage`) by its infohash. Used by `resolve()` to seed a
    /// Torznab search just-in-time when the user clicks a featured
    /// item before ever running a search.
    async fn featured_title_for(&self, infohash: &str) -> Option<String> {
        let cache = self.featured_cache.lock().await;
        let featured = cache.as_ref()?;
        featured
            .movies
            .iter()
            .chain(featured.series.iter())
            .find(|r| r.external_id == infohash)
            .map(|r| r.title.clone())
    }
}

#[async_trait]
impl SearchProvider for C411 {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> ProviderCapabilities {
        // Featured items expose infohashes → resolve() can return a
        // magnet without an HTTP round-trip. Torznab search results
        // also routinely include the infohash attr.
        ProviderCapabilities {
            returns_magnet: true,
            returns_torrent_file: true,
            returns_infohash: true,
        }
    }

    async fn search(&self, q: &SearchQuery) -> Result<ProviderPage> {
        self.torznab.search(q).await
    }

    async fn latest(&self, kind: Option<MediaKind>, page: u32) -> Result<ProviderPage> {
        // c411 is Torznab under the hood — reuse the query-less rolling-window
        // feed (the editorial /api/homepage carousel carries no upload dates,
        // so it can't drive a time-windowed catalogue).
        self.torznab.latest(kind, page).await
    }

    async fn resolve(&self, external_id: &str) -> Result<TorrentSource> {
        // Fast path: the Torznab link cache already has the signed
        // download URL for this infohash (populated either by an
        // earlier search or by a prior prime-via-search round below).
        if let Ok(source) = self.torznab.resolve(external_id).await {
            return Ok(source);
        }

        // Cache miss — typical when the user clicks a featured item
        // before searching for the show. c411 doesn't expose download
        // URLs on `/api/homepage`, but it does in the Torznab search
        // response; replay a Torznab search keyed on the release name
        // we cached at homepage-fetch time. The search side-effects
        // the link cache, then resolve() finishes through the normal
        // path.
        if is_infohash(external_id)
            && let Some(title) = self.featured_title_for(external_id).await
        {
            tracing::debug!(
                provider = %self.id,
                infohash = external_id,
                title = %title,
                "c411: priming Torznab link cache via title search",
            );
            let prime = SearchQuery {
                q: title,
                page: Some(1),
                limit: Some(25),
                sort_by: None,
                order: None,
                kind: None,
                // Priming a featured-link lookup — no need to push
                // structured hints down to the underlying Torznab.
                parsed_title: None,
                season: None,
                episode: None,
                year: None,
            };
            // Best-effort: if the search fails (network, indexer
            // 5xx), we fall through to the explicit error below
            // so the user sees a clear "couldn't resolve" rather
            // than a silent hang.
            let _ = self.torznab.search(&prime).await;
            return self.torznab.resolve(external_id).await;
        }

        Err(Error::Provider(format!(
            "c411 resolve: no cached download URL for `{external_id}` \
             and no featured entry to prime from",
        )))
    }

    async fn details(&self, external_id: &str) -> Result<Option<TorrentDetails>> {
        // c411 indexes the rich description, NFO, file list, etc. under
        // `/api/torrents/{infohash}`. Failures fall through to whatever
        // the Torznab cache picked up during search — better a thin
        // preview than no preview.
        match self.fetch_details(external_id).await {
            Ok(Some(d)) => Ok(Some(d)),
            Ok(None) => self.torznab.details(external_id).await,
            Err(e) => {
                tracing::warn!(
                    provider = %self.id,
                    external_id,
                    error = %e,
                    "c411 details failed, falling back to torznab cache",
                );
                self.torznab.details(external_id).await
            }
        }
    }

    async fn featured_movies(&self) -> Result<Vec<SearchResult>> {
        self.refresh_featured_if_stale().await?;
        Ok(self
            .featured_cache
            .lock()
            .await
            .as_ref()
            .map(|c| c.movies.clone())
            .unwrap_or_default())
    }

    async fn featured_series(&self) -> Result<Vec<SearchResult>> {
        self.refresh_featured_if_stale().await?;
        Ok(self
            .featured_cache
            .lock()
            .await
            .as_ref()
            .map(|c| c.series.clone())
            .unwrap_or_default())
    }
}

/// Heuristic split for the homepage feed — the JSON gives us no category,
/// only the release title. SCENE conventions are reliable enough: any
/// `S\d{2}` (with or without `E\d{2}`) is a TV release, everything else
/// is a movie.
fn classify_title(title: &str) -> MediaKind {
    let bytes = title.as_bytes();
    let mut i = 0;
    while i + 3 <= bytes.len() {
        if (bytes[i] == b'S' || bytes[i] == b's')
            && bytes[i + 1].is_ascii_digit()
            && bytes[i + 2].is_ascii_digit()
        {
            // Guard against false positives like "S04E" inside a real word.
            // We require a non-alphanumeric boundary before the `S`.
            let before_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
            if before_ok {
                return MediaKind::Tv;
            }
        }
        i += 1;
    }
    MediaKind::Movie
}

#[derive(Debug, Deserialize)]
struct HomepageResponse {
    #[serde(default, rename = "exclusiveRecent")]
    exclusive_recent: Vec<HomepageItem>,
}

#[derive(Debug, Deserialize)]
struct HomepageItem {
    #[serde(rename = "infoHash")]
    info_hash: String,
    title: String,
    #[serde(default, rename = "posterUrl")]
    poster_url: Option<String>,
    #[serde(default, rename = "tmdbId")]
    tmdb_id: Option<u64>,
    #[serde(default, rename = "uploaderUsername")]
    uploader_username: String,
}

/// Cheap check: c411 indexes torrents by their 40-char hex SHA-1
/// infohash. Anything else is wrong-API or a stale id from another
/// indexer that got mistakenly routed to us — skip the HTTP call.
fn is_infohash(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

// ===========================================================================
// /api/torrents/{infohash} response
// ===========================================================================

#[derive(Debug, Deserialize)]
struct TorrentDetailRaw {
    name: String,
    /// HTML body rendered to the user — sanitised on the client.
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    seeders: Option<u32>,
    #[serde(default)]
    leechers: Option<u32>,
    #[serde(default)]
    completions: Option<u64>,
    #[serde(default)]
    uploader: Option<String>,
    #[serde(default, rename = "createdAt")]
    created_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, rename = "isFreeleech")]
    is_freeleech: bool,
    #[serde(default)]
    files: Vec<TorrentFile>,
    #[serde(default)]
    category: Option<RawNamed>,
    #[serde(default)]
    metadata: Option<TorrentMetadata>,
}

#[derive(Debug, Deserialize)]
struct TorrentFile {
    #[serde(default)]
    length: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RawNamed {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TorrentMetadata {
    #[serde(default)]
    category: Option<RawNamed>,
    #[serde(default)]
    subcategory: Option<RawNamed>,
    #[serde(default)]
    options: Vec<MetadataOption>,
    #[serde(default, rename = "nfoContent")]
    nfo_content: Option<String>,
    #[serde(default, rename = "isExclusive")]
    is_exclusive: bool,
}

#[derive(Debug, Deserialize)]
struct MetadataOption {
    // We flatten every option group (Langue, Genre, …) into a single
    // `tags` list, so the group name itself is dropped on the floor.
    #[serde(default)]
    values: Vec<MetadataOptionValue>,
}

#[derive(Debug, Deserialize)]
struct MetadataOptionValue {
    #[serde(default)]
    value: String,
}

impl TorrentDetailRaw {
    fn into_torrent_details(self, provider_id: &str, external_id: &str) -> TorrentDetails {
        let nfo = self
            .metadata
            .as_ref()
            .and_then(|m| m.nfo_content.clone())
            .filter(|s| !s.is_empty());
        let media_info = nfo.as_deref().and_then(nfo::parse);
        let tags = self
            .metadata
            .as_ref()
            .map(|m| {
                m.options
                    .iter()
                    .flat_map(|o| o.values.iter())
                    .filter_map(|v| {
                        let s = v.value.trim();
                        (!s.is_empty()).then(|| s.to_string())
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let category = build_category(self.category.as_ref(), self.metadata.as_ref());
        let exclusive = self.metadata.as_ref().is_some_and(|m| m.is_exclusive);
        let file_count = u32::try_from(self.files.len()).ok();
        let file_size_bytes = self.size.or_else(|| {
            // Fall back to summing file lengths — `size` is empty for some
            // older torrents in the index.
            let total: u64 = self.files.iter().filter_map(|f| f.length).sum();
            (total > 0).then_some(total)
        });

        TorrentDetails {
            provider_id: provider_id.to_string(),
            external_id: external_id.to_string(),
            title: self.name,
            description: self.description.filter(|s| !s.trim().is_empty()),
            description_format: DescriptionFormat::Html,
            nfo,
            media_info,
            tags,
            category,
            uploader: self.uploader.filter(|s| !s.is_empty()),
            uploaded_at: self.created_at,
            age: None,
            seeders: self.seeders,
            leechers: self.leechers,
            times_completed: self.completions,
            views: None,
            freeleech: self.is_freeleech,
            exclusive,
            file_count,
            file_size_bytes,
        }
    }
}

fn build_category(top: Option<&RawNamed>, meta: Option<&TorrentMetadata>) -> Option<String> {
    let parent = top
        .and_then(|c| c.name.clone())
        .or_else(|| meta.and_then(|m| m.category.as_ref().and_then(|c| c.name.clone())));
    let sub = meta.and_then(|m| m.subcategory.as_ref().and_then(|c| c.name.clone()));
    match (parent, sub) {
        (Some(p), Some(s)) if p != s => Some(format!("{p} / {s}")),
        (Some(p), _) => Some(p),
        (None, Some(s)) => Some(s),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_titles() {
        assert_eq!(
            classify_title("Avatar.2009.MULTI.VFI.2160p.BluRay"),
            MediaKind::Movie,
        );
        assert_eq!(
            classify_title("Science.grand.format.S09E11.FRENCH"),
            MediaKind::Tv,
        );
        assert_eq!(classify_title("The.Madison.S01.MULTi.2160p"), MediaKind::Tv,);
        // "Soul Mate" — first letter is uppercase S but followed by space, not digits.
        assert_eq!(
            classify_title("Soul.Mate.2026.S01.MULTi.1080p"),
            MediaKind::Tv,
        );
        // No SxxEyy → movie
        assert_eq!(
            classify_title("Un.simple.accident.2025.MULTi.AD"),
            MediaKind::Movie,
        );
    }

    #[test]
    fn validates_infohash() {
        assert!(is_infohash("98259ba623eec5f33167c083b51b30122c7fa068"));
        assert!(is_infohash("ABCDEF0123456789abcdef0123456789ABCDEF01"));
        // Wrong length.
        assert!(!is_infohash("98259ba6"));
        assert!(!is_infohash(""));
        // Non-hex char.
        assert!(!is_infohash("98259ba623eec5f33167c083b51b30122c7fa06z"));
        // Numeric guid (e.g. UNIT3D torrent id) — not an infohash.
        assert!(!is_infohash("12345"));
    }

    #[test]
    fn maps_c411_details() {
        let sample = r#"{
            "id": 5157,
            "infoHash": "98259ba623eec5f33167c083b51b30122c7fa068",
            "name": "AVATAR.2009.MULTI.VFI.2160P.BluRay",
            "description": "<h1>Avatar</h1><p>Synopsis</p>",
            "size": 11271821344,
            "seeders": 12,
            "leechers": 3,
            "completions": 590,
            "uploader": "ShuMax62",
            "createdAt": "2026-01-27T12:19:24Z",
            "isFreeleech": false,
            "files": [{"path": ["a.mkv"], "length": 11271821344}],
            "category": {"name": "Films & Vidéos"},
            "metadata": {
                "category": {"name": "Films & Vidéos"},
                "subcategory": {"name": "Film"},
                "options": [
                    {"name": "Langue", "values": [{"value": "Multi"}, {"value": "Français"}]},
                    {"name": "Genre", "values": [{"value": "Action"}]}
                ],
                "nfoContent": "FILE: x.mkv\nDuration: 2h",
                "isExclusive": false
            }
        }"#;
        let raw: TorrentDetailRaw = serde_json::from_str(sample).expect("parse");
        let d = raw.into_torrent_details("c411", "98259ba623eec5f33167c083b51b30122c7fa068");

        assert_eq!(d.external_id, "98259ba623eec5f33167c083b51b30122c7fa068");
        assert_eq!(d.title, "AVATAR.2009.MULTI.VFI.2160P.BluRay");
        assert_eq!(
            d.description.as_deref(),
            Some("<h1>Avatar</h1><p>Synopsis</p>")
        );
        assert_eq!(d.description_format, DescriptionFormat::Html);
        assert_eq!(d.nfo.as_deref(), Some("FILE: x.mkv\nDuration: 2h"));
        assert_eq!(d.uploader.as_deref(), Some("ShuMax62"));
        assert_eq!(d.seeders, Some(12));
        assert_eq!(d.leechers, Some(3));
        assert_eq!(d.times_completed, Some(590));
        assert_eq!(d.file_size_bytes, Some(11_271_821_344));
        assert_eq!(d.file_count, Some(1));
        assert_eq!(d.category.as_deref(), Some("Films & Vidéos / Film"));
        // Tags flattened from all option values.
        assert!(d.tags.contains(&"Multi".to_string()));
        assert!(d.tags.contains(&"Français".to_string()));
        assert!(d.tags.contains(&"Action".to_string()));
        assert!(!d.exclusive);
        assert!(!d.freeleech);
    }

    #[test]
    fn handles_missing_optional_fields() {
        let sample = r#"{
            "id": 1,
            "infoHash": "98259ba623eec5f33167c083b51b30122c7fa068",
            "name": "X",
            "files": []
        }"#;
        let raw: TorrentDetailRaw = serde_json::from_str(sample).expect("parse");
        let d = raw.into_torrent_details("c411", "98259ba623eec5f33167c083b51b30122c7fa068");
        assert_eq!(d.title, "X");
        assert!(d.description.is_none());
        assert_eq!(d.description_format, DescriptionFormat::Html);
        assert!(d.tags.is_empty());
        assert!(d.category.is_none());
        assert_eq!(d.file_count, Some(0));
    }
}
