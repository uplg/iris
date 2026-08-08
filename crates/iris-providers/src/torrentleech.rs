//! `TorrentLeech` — English private tracker (0DAY / GENERAL).
//!
//! No Torznab: search rides the SPA's JSON browse endpoint
//! (`/torrents/browse/list/...` — categories / query / sort travel as
//! path segments, not query params) behind a cookie session obtained by
//! a form POST to `/user/account/login/`. Wire format mirrors Jackett's
//! `torrentleech.yml` definition, the de-facto reference for this site.
//!
//! Downloads do NOT need the session: `TorrentLeech` signs per-user RSS
//! download URLs (`/rss/download/{fid}/{rss_key}/{filename}`, the "RSS
//! key" from Profile → RSS), so every search result carries a
//! restart-safe `download_url` that survives both process restarts and
//! session expiry.
//!
//! Quirks worth knowing:
//! * A query-less browse (the `latest()` feed) uses the site's
//!   `newfilter/2` segment — the "recent torrents" view — instead of a
//!   `query/` segment, exactly like Jackett.
//! * `addedTimestamp` is rendered in the account profile's timezone;
//!   keep the profile on UTC (the default) for accurate dates. Parsed
//!   best-effort as UTC either way.
//! * Page size follows the profile's "Torrents per page" setting —
//!   set it to 100 for useful `latest()` coverage.
//! * `fid` arrives as a number or a string depending on endpoint
//!   vintage; `tags` as an array or a comma-joined string. Both shapes
//!   are accepted.
//! * There is no JSON detail endpoint: `details()` scrapes the
//!   server-rendered `/torrent/{fid}` page (info table, IMDb/TVMaze
//!   synopsis block, inline NFO, file list).
//!
//! Config:
//! ```toml
//! [[providers]]
//! id = "tl"
//! kind = "torrentleech"
//! enabled = true
//! # Reload mirror, NOT the primary www.torrentleech.org — the primary
//! # zone sits behind a Cloudflare Managed Challenge a headless client
//! # can't solve (see providers.toml for the full note). Mirrors front
//! # the same backend, same account + RSS key, no challenge.
//! base_url = "https://www.torrentleech.me"
//! username_env = "TL_USERNAME"
//! password_env = "TL_PASSWORD"
//! rss_key_env = "TL_RSS_KEY"
//! # alt_2fa_token_env = "TL_2FA_TOKEN"  # only when the account has 2FA enabled
//! default_language = "english"
//! ```

use std::collections::{HashMap, VecDeque};
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
use reqwest::header::{ACCEPT, HeaderMap, HeaderValue, REFERER, USER_AGENT};
use scraper::{Html, Selector};
use serde::Deserialize;
use tokio::sync::Mutex;
use url::Url;

use crate::SearchProvider;
use crate::nfo;
use crate::util::{extract_year, field_or_env, field_str, optional_field_or_env, parse_size};

const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:150.0) Gecko/20100101 Firefox/150.0";

/// First byte of a valid `.torrent` file (bencoded dictionary).
const BENCODE_DICT_MARKER: u8 = b'd';

/// Body marker of a live session — the post-login page (and every
/// logged-in page) carries the logout link.
const LOGIN_OK_MARKER: &str = "/user/account/logout";

/// Same cap as the Torznab / UNIT3D / hdtorrents link caches.
const LINK_CACHE_CAP: usize = 4096;

/// Same TTL as hdtorrents: long enough to absorb the preview-dialog +
/// pre-grab double fetch, short enough that seeder counts stay honest.
const DETAILS_TTL: Duration = Duration::from_mins(1);

/// Movie category ids: Cam, TS/TC, `DVDRip`/`DVDScreener`, `WEBRip`,
/// `HDRip`, `BlurayRip`, DVD-R, Bluray, 4K, Boxsets, Documentaries,
/// Foreign.
const MOVIE_CATS: [u32; 12] = [8, 9, 11, 37, 43, 14, 12, 13, 47, 15, 29, 36];
/// TV category ids: Episodes, Episodes HD, Boxsets, Anime, Cartoons,
/// Foreign. Deliberately excludes 16 "Music videos" — video format but
/// not a movie/series the catalogue can classify.
const TV_CATS: [u32; 6] = [26, 32, 27, 34, 35, 44];
/// `MOVIE_CATS` ++ `TV_CATS`, for un-filtered searches.
const ALL_CATS: [u32; 18] = [
    8, 9, 11, 37, 43, 14, 12, 13, 47, 15, 29, 36, 26, 32, 27, 34, 35, 44,
];

fn category_label(id: u32) -> Option<&'static str> {
    Some(match id {
        8 => "Movies/Cam",
        9 => "Movies/TS-TC",
        11 => "Movies/DVDRip",
        37 => "Movies/WEBRip",
        43 => "Movies/HDRip",
        14 => "Movies/BlurayRip",
        12 => "Movies/DVD-R",
        13 => "Movies/Bluray",
        47 => "Movies/4K",
        15 => "Movies/Boxsets",
        29 => "Movies/Documentaries",
        36 => "Movies/Foreign",
        26 => "TV/Episodes",
        32 => "TV/Episodes HD",
        27 => "TV/Boxsets",
        34 => "TV/Anime",
        35 => "TV/Cartoons",
        44 => "TV/Foreign",
        _ => return None,
    })
}

fn category_kind(id: u32) -> Option<MediaKind> {
    if MOVIE_CATS.contains(&id) {
        Some(MediaKind::Movie)
    } else if TV_CATS.contains(&id) {
        Some(MediaKind::Tv)
    } else {
        None
    }
}

struct LinkCache {
    map: HashMap<String, String>,
    order: VecDeque<String>,
}

impl LinkCache {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }
    fn put(&mut self, key: String, value: String) {
        if self.map.insert(key.clone(), value).is_none() {
            self.order.push_back(key);
            while self.order.len() > LINK_CACHE_CAP {
                if let Some(old) = self.order.pop_front() {
                    self.map.remove(&old);
                }
            }
        }
    }
    fn get(&self, key: &str) -> Option<String> {
        self.map.get(key).cloned()
    }
}

pub struct TorrentLeech {
    id: String,
    base_url: Url,
    username: String,
    password: String,
    /// Profile → "Alt 2FA Token". Only needed when the account has 2FA
    /// enabled; the login form always posts the field (empty when unused),
    /// same as the browser.
    alt_2fa_token: Option<String>,
    /// Profile → RSS key. Signs the cookie-less `/rss/download/...` URLs.
    rss_key: String,
    /// Cookie-jar client — the session IS the jar. One login primes it;
    /// every subsequent request rides the stored cookies.
    http: Client,
    /// `true` once a login round-trip succeeded. Held in a Mutex so
    /// concurrent searches single-flight the (re)login instead of
    /// hammering the login form in parallel.
    logged_in: Mutex<bool>,
    /// Torrent fid -> signed RSS download URL captured from search rows
    /// (carries the real `{filename}` tail).
    link_cache: Mutex<LinkCache>,
    /// Torrent fid -> scraped detail view, TTL-bounded. The pre-grab
    /// dead-torrent check refetches details right after the preview
    /// dialog did; this absorbs the double fetch.
    details_cache: Mutex<HashMap<String, CachedDetails>>,
}

struct CachedDetails {
    details: TorrentDetails,
    fetched_at: Instant,
}

impl TorrentLeech {
    pub fn from_config(entry: &ProviderEntry) -> Result<Arc<Self>> {
        let base_url_str = field_str(entry, "base_url")?;
        let base_url = Url::parse(base_url_str)
            .map_err(|e| Error::Provider(format!("torrentleech base_url invalid: {e}")))?;
        let username = field_or_env(entry, "username")?;
        let password = field_or_env(entry, "password")?;
        let rss_key = field_or_env(entry, "rss_key")?;
        let alt_2fa_token = optional_field_or_env(entry, "alt_2fa_token")?;

        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/json, text/html, */*"),
        );

        let http = crate::tls::client_builder()
            .default_headers(headers)
            .cookie_store(true)
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| Error::Provider(format!("torrentleech http client: {e}")))?;

        Ok(Arc::new(Self {
            id: entry.id.clone(),
            base_url,
            username,
            password,
            alt_2fa_token,
            rss_key,
            http,
            logged_in: Mutex::new(false),
            link_cache: Mutex::new(LinkCache::new()),
            details_cache: Mutex::new(HashMap::new()),
        }))
    }

    async fn login(&self) -> Result<()> {
        let url = self
            .base_url
            .join("/user/account/login/")
            .map_err(|e| Error::Provider(format!("torrentleech join login url: {e}")))?;

        let form = [
            ("username", self.username.as_str()),
            ("password", self.password.as_str()),
            ("alt2FAToken", self.alt_2fa_token.as_deref().unwrap_or("")),
        ];
        let res = self
            .http
            .post(url.clone())
            .header(REFERER, url.as_str())
            .form(&form)
            .send()
            .await
            .map_err(|e| Error::Provider(format!("torrentleech login: {e}")))?;

        let status = res.status();
        let body = res
            .text()
            .await
            .map_err(|e| Error::Provider(format!("torrentleech login body: {e}")))?;

        if body.contains(LOGIN_OK_MARKER) {
            tracing::debug!(provider = %self.id, "torrentleech login succeeded");
            return Ok(());
        }
        let reason = extract_login_error(&body)
            .unwrap_or_else(|| format!("no logout link in response (HTTP {status})"));
        Err(Error::Provider(format!(
            "torrentleech login failed: {reason}"
        )))
    }

    async fn ensure_login(&self) -> Result<()> {
        let mut logged = self.logged_in.lock().await;
        if *logged {
            return Ok(());
        }
        self.login().await?;
        *logged = true;
        Ok(())
    }

    async fn invalidate_session(&self) {
        *self.logged_in.lock().await = false;
    }

    /// Authenticated GET expecting a JSON body. An HTML body means the
    /// session expired and we got bounced to the login page — re-login
    /// and retry once.
    async fn authed_get_json(&self, url: Url) -> Result<String> {
        let mut attempt = 0u8;
        loop {
            self.ensure_login().await?;
            let res = self
                .http
                .get(url.clone())
                .send()
                .await
                .map_err(|e| Error::Provider(format!("torrentleech request: {e}")))?;
            if !res.status().is_success() {
                let status = res.status();
                return Err(Error::Provider(format!(
                    "torrentleech request failed: HTTP {status}"
                )));
            }
            let body = res
                .text()
                .await
                .map_err(|e| Error::Provider(format!("torrentleech body: {e}")))?;
            let looks_logged_out = body.trim_start().starts_with('<');
            if looks_logged_out && attempt == 0 {
                attempt += 1;
                self.invalidate_session().await;
                continue;
            }
            if looks_logged_out {
                return Err(Error::Provider(
                    "torrentleech session rejected after re-login".into(),
                ));
            }
            return Ok(body);
        }
    }

    /// Authenticated GET expecting a logged-in HTML page (the torrent
    /// detail view). A body without the logout link means we got
    /// bounced to the login page — re-login and retry once.
    async fn authed_get_html(&self, url: Url) -> Result<String> {
        let mut attempt = 0u8;
        loop {
            self.ensure_login().await?;
            let res = self
                .http
                .get(url.clone())
                .send()
                .await
                .map_err(|e| Error::Provider(format!("torrentleech request: {e}")))?;
            if !res.status().is_success() {
                let status = res.status();
                return Err(Error::Provider(format!(
                    "torrentleech request failed: HTTP {status}"
                )));
            }
            let body = res
                .text()
                .await
                .map_err(|e| Error::Provider(format!("torrentleech body: {e}")))?;
            let looks_logged_out = !body.contains(LOGIN_OK_MARKER);
            if looks_logged_out && attempt == 0 {
                attempt += 1;
                self.invalidate_session().await;
                continue;
            }
            if looks_logged_out {
                return Err(Error::Provider(
                    "torrentleech session rejected after re-login".into(),
                ));
            }
            return Ok(body);
        }
    }

    async fn fetch_details(&self, external_id: &str) -> Result<Option<TorrentDetails>> {
        if external_id.is_empty() || !external_id.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Ok(None);
        }
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
            .join(&format!("/torrent/{external_id}"))
            .map_err(|e| Error::Provider(format!("torrentleech join details url: {e}")))?;
        let body = self.authed_get_html(url).await?;
        let Some(details) = parse_details_page(&self.id, external_id, &body) else {
            return Ok(None);
        };
        self.details_cache.lock().await.insert(
            external_id.to_string(),
            CachedDetails {
                details: details.clone(),
                fetched_at: Instant::now(),
            },
        );
        Ok(Some(details))
    }

    /// GET a signed RSS download URL. No session needed — the `rss_key` in
    /// the path IS the auth — but reusing the client keeps the pinned
    /// TLS roots and UA.
    async fn download(&self, url: Url) -> Result<bytes::Bytes> {
        let res = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| Error::Provider(format!("torrentleech download: {e}")))?;
        if !res.status().is_success() {
            let status = res.status();
            return Err(Error::Provider(format!(
                "torrentleech download failed: HTTP {status}"
            )));
        }
        let bytes = res
            .bytes()
            .await
            .map_err(|e| Error::Provider(format!("torrentleech download body: {e}")))?;
        if bytes.first().copied() != Some(BENCODE_DICT_MARKER) {
            let preview = String::from_utf8_lossy(&bytes[..bytes.len().min(200)]).into_owned();
            tracing::warn!(
                provider = %self.id,
                body_preview = %preview,
                "torrentleech download returned non-bencoded body",
            );
            return Err(Error::Provider(format!(
                "torrentleech download returned non-bencoded body (first byte: {:?})",
                bytes.first()
            )));
        }
        Ok(bytes)
    }

    fn search_url(&self, q: &SearchQuery) -> Result<Url> {
        let cats: &[u32] = match q.kind {
            Some(MediaKind::Movie) => &MOVIE_CATS,
            Some(MediaKind::Tv) => &TV_CATS,
            None => &ALL_CATS,
        };
        let term = build_search_term(q);
        let mut url = self.base_url.clone();
        {
            let mut segs = url
                .path_segments_mut()
                .map_err(|()| Error::Provider("torrentleech base_url cannot be a base".into()))?;
            segs.pop_if_empty();
            segs.extend(["torrents", "browse", "list", "categories"]);
            let cat_list = cats
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",");
            segs.push(&cat_list);
            if term.is_empty() {
                // Query-less browse = the site's "recent torrents" view.
                segs.extend(["newfilter", "2"]);
            } else {
                segs.extend(["exact", "1", "query"]);
                segs.push(&term);
            }
            segs.extend([
                "orderby",
                sort_field(q.sort_by),
                "order",
                sort_order(q.order),
            ]);
            let page = q.page.unwrap_or(1).max(1);
            if page > 1 {
                segs.push("page");
                segs.push(&page.to_string());
            }
        }
        Ok(url)
    }
}

#[async_trait]
impl SearchProvider for TorrentLeech {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            returns_magnet: false,
            returns_torrent_file: true,
            returns_infohash: false,
        }
    }

    async fn search(&self, q: &SearchQuery) -> Result<ProviderPage> {
        let url = self.search_url(q)?;
        let body = self.authed_get_json(url).await?;
        let resp: BrowseResponse = serde_json::from_str(&body).map_err(|e| {
            tracing::warn!(
                provider = %self.id,
                error = %e,
                body_preview = %body.chars().take(300).collect::<String>(),
                "torrentleech browse decode failed",
            );
            Error::Provider(format!("torrentleech browse decode: {e}"))
        })?;

        let mut results = Vec::new();
        {
            let mut cache = self.link_cache.lock().await;
            for row in resp.torrent_list {
                let Some(r) = row.into_search_result(&self.id, &self.base_url, &self.rss_key)
                else {
                    continue;
                };
                if let Some(dl) = &r.download_url {
                    cache.put(r.external_id.clone(), dl.clone());
                }
                results.push(r);
            }
        }

        let count = u32::try_from(results.len()).unwrap_or(u32::MAX);
        Ok(ProviderPage {
            results,
            current_page: q.page.unwrap_or(1).max(1),
            // Page size follows the account profile's "Torrents per
            // page" setting the API never announces; report what this
            // page actually carried and let the UI "load more".
            limit: count.max(1),
            total_count: resp.num_found,
            total_pages: None,
        })
    }

    async fn resolve(&self, external_id: &str) -> Result<TorrentSource> {
        if external_id.is_empty() || !external_id.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(Error::InvalidInput(format!(
                "torrentleech external_id must be alphanumeric, got `{external_id}`"
            )));
        }
        let url_str = match self.link_cache.lock().await.get(external_id) {
            Some(u) => u,
            // Cache wiped (restart). The `{filename}` tail of the RSS
            // download route is believed display-only (the server keys
            // on fid + rss_key); if the site does reject the synthetic
            // name, the bencode check below surfaces a clear error.
            None => rss_download_url(
                &self.base_url,
                external_id,
                &self.rss_key,
                &format!("{external_id}.torrent"),
            )?
            .to_string(),
        };
        let url = Url::parse(&url_str)
            .map_err(|e| Error::Provider(format!("torrentleech download url invalid: {e}")))?;
        let bytes = self.download(url).await?;
        Ok(TorrentSource::TorrentFile(bytes.to_vec()))
    }

    /// RSS download URLs are cookie-less, so the default plain-GET would
    /// work — but routing through our client keeps the pinned TLS roots
    /// and adds the bencode sanity check (the site answers HTML when the
    /// `rss_key` was rotated).
    async fn fetch_bytes(&self, url: &str) -> Result<bytes::Bytes> {
        let url = Url::parse(url)
            .map_err(|e| Error::Provider(format!("torrentleech fetch_bytes: {e}")))?;
        self.download(url).await
    }

    /// Scraped from the server-rendered `/torrent/{fid}` page (there is
    /// no JSON detail endpoint). Failures degrade to "no details" — the
    /// preview dialog then shows the search-result fields only.
    async fn details(&self, external_id: &str) -> Result<Option<TorrentDetails>> {
        match self.fetch_details(external_id).await {
            Ok(d) => Ok(d),
            Err(e) => {
                tracing::warn!(
                    provider = %self.id,
                    external_id,
                    error = %e,
                    "torrentleech details failed",
                );
                Ok(None)
            }
        }
    }
}

fn sort_field(f: Option<iris_core::search::SortField>) -> &'static str {
    use iris_core::search::SortField;
    match f {
        Some(SortField::Title) => "nameSort",
        Some(SortField::Size) => "size",
        // The browse endpoint sorts by added/seeders/size/name only;
        // leechers degrades to the nearest peer-activity signal.
        Some(SortField::Seeders | SortField::Leechers) => "seeders",
        Some(SortField::Uploaded) | None => "added",
    }
}

fn sort_order(o: Option<iris_core::search::SortOrder>) -> &'static str {
    match o {
        Some(iris_core::search::SortOrder::Asc) => "asc",
        _ => "desc",
    }
}

/// SCENE-parsed title + S/E rebuilt like the other providers, then
/// adapted to TL's search engine: dots and colons are separators, and a
/// leading `-` on a word negates the term (Jackett #3096) so it's
/// stripped.
fn build_search_term(q: &SearchQuery) -> String {
    let base = match q.parsed_title.as_deref() {
        Some(t) if !t.is_empty() => match (q.season, q.episode) {
            (Some(s), Some(e)) if e > 0 => format!("{t} S{s:02}E{e:02}"),
            (Some(s), _) => format!("{t} S{s:02}"),
            _ => q.q.clone(),
        },
        _ => q.q.clone(),
    };
    let cleaned: String = base
        .chars()
        .map(|c| if c == '.' || c == ':' { ' ' } else { c })
        .collect();
    cleaned
        .split_whitespace()
        .map(|w| w.trim_start_matches('-'))
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// `/rss/download/{fid}/{rss_key}/{filename}` — signed, cookie-less.
fn rss_download_url(base: &Url, fid: &str, rss_key: &str, filename: &str) -> Result<Url> {
    let mut url = base.clone();
    {
        let mut segs = url
            .path_segments_mut()
            .map_err(|()| Error::Provider("torrentleech base_url cannot be a base".into()))?;
        segs.pop_if_empty();
        segs.extend(["rss", "download", fid, rss_key, filename]);
    }
    Ok(url)
}

/// Scrape the server-rendered `/torrent/{fid}` page into a
/// [`TorrentDetails`]. Returns `None` when the page isn't a detail view
/// (unknown fid → the site answers with a generic page whose `<title>`
/// lacks the "Torrent Details for" prefix).
///
/// Everything is best-effort per field: the page mixes hand-rendered
/// blocks (info table, tags) with IMDb/TVMaze enrichment (synopsis,
/// genres, rating) that only exists when the uploader linked an id, and
/// an NFO `<pre>` that only exists for releases that shipped one.
fn parse_details_page(provider_id: &str, external_id: &str, html: &str) -> Option<TorrentDetails> {
    let doc = Html::parse_document(html);

    let title = doc
        .select(&sel("title"))
        .next()
        .map(|t| t.text().collect::<String>())?;
    let title = title
        .trim()
        .strip_suffix(":: TorrentLeech.org")
        .unwrap_or(title.trim())
        .trim()
        .strip_prefix("Torrent Details for ")?
        .trim()
        .to_string();
    if title.is_empty() {
        return None;
    }

    let info = parse_info_table(&doc);

    let mut freeleech = false;
    let mut tags = Vec::new();
    for tag in doc.select(&sel("span.tag")) {
        let t = clean_text(tag.text());
        if t.is_empty() {
            continue;
        }
        if t.eq_ignore_ascii_case("freeleech") {
            freeleech = true;
        } else {
            tags.push(t);
        }
    }

    // The NFO <pre> only exists for releases that shipped one (the
    // template variant in the page source is commented out, so the
    // parser never sees it). Raw text, NOT whitespace-collapsed —
    // NFOs are ASCII art.
    let nfo = doc
        .select(&sel("pre.nfo"))
        .next()
        .map(|p| p.text().collect::<String>())
        .map(|s| s.trim_matches('\n').trim_end().to_string())
        .filter(|s| !s.trim().is_empty());
    let media_info = nfo.as_deref().and_then(nfo::parse);

    let file_count = u32::try_from(doc.select(&sel("#fileListTable tbody tr")).count())
        .ok()
        .filter(|&n| n > 0);

    Some(TorrentDetails {
        provider_id: provider_id.to_string(),
        external_id: external_id.to_string(),
        title,
        description: build_description(&doc),
        description_format: DescriptionFormat::Plain,
        nfo,
        media_info,
        tags,
        category: info.category,
        uploader: info.uploader,
        uploaded_at: info.uploaded_at,
        age: info.age,
        seeders: info.seeders,
        leechers: info.leechers,
        times_completed: info.times_completed,
        views: None,
        freeleech,
        exclusive: false,
        file_count,
        file_size_bytes: info.file_size_bytes.filter(|&n| n > 0),
    })
}

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static selector")
}

/// The hand-rendered facts table (`div.torrent_info_details`), keyed by
/// its `td.description` labels.
#[derive(Default)]
struct InfoTable {
    category: Option<String>,
    uploaded_at: Option<chrono::DateTime<chrono::Utc>>,
    age: Option<String>,
    file_size_bytes: Option<u64>,
    seeders: Option<u32>,
    leechers: Option<u32>,
    times_completed: Option<u64>,
    uploader: Option<String>,
}

fn parse_info_table(doc: &Html) -> InfoTable {
    let mut info = InfoTable::default();
    for row in doc.select(&sel("div.torrent_info_details tr")) {
        let tds: Vec<_> = row.select(&sel("td")).collect();
        let (Some(label_td), Some(value_td)) = (tds.first(), tds.get(1)) else {
            continue;
        };
        let label = clean_text(label_td.text());
        let value = clean_text(value_td.text());
        match label.as_str() {
            "Category" => info.category = Some(value).filter(|v| !v.is_empty()),
            "Added" => {
                // "Friday 10th July 2026 07:53:49 PM <strong>(29 days
                // ago)</strong>" — the <strong> is the human age, the
                // rest is an absolute timestamp in the account
                // profile's timezone (kept on UTC, see module docs).
                let strong = value_td
                    .select(&sel("strong"))
                    .next()
                    .map(|s| clean_text(s.text()));
                if let Some(s) = &strong {
                    info.age =
                        Some(s.trim_matches(['(', ')']).to_string()).filter(|a| !a.is_empty());
                }
                let ts = match &strong {
                    Some(s) => value.replace(s.as_str(), ""),
                    None => value,
                };
                info.uploaded_at = parse_detail_datetime(&ts);
            }
            "Size" => info.file_size_bytes = parse_size(&value),
            "Peers" => {
                // "55 Peers (54 Seeders and 1 leechers)"
                info.seeders = number_before(&value, "seeder");
                info.leechers = number_before(&value, "leecher");
            }
            "Downloaded" => {
                // "265 times"
                info.times_completed = value
                    .split_whitespace()
                    .next()
                    .and_then(|n| n.replace(',', "").parse().ok());
            }
            "Uploader" => {
                info.uploader = value_td
                    .select(&sel("span.details-uploader-name"))
                    .next()
                    .map(|s| clean_text(s.text()))
                    .filter(|s| !s.is_empty());
            }
            _ => {}
        }
    }
    info
}

/// IMDb/TVMaze synopsis + the facts the enrichment block renders (year,
/// duration, release date, genre) + the rating line. Assembled as plain
/// text — both clients wrap it cleanly and nothing here carries markup
/// worth preserving.
fn build_description(doc: &Html) -> Option<String> {
    let mut lines = Vec::new();
    if let Some(summary) = doc.select(&sel("div.torrent-summary")).next() {
        let s = clean_text(summary.text());
        if !s.is_empty() {
            lines.push(s);
            lines.push(String::new());
        }
    }
    for p in doc.select(&sel("div.torrent-extra-info p")) {
        let line = clean_text(p.text());
        if !line.is_empty() {
            lines.push(line);
        }
    }
    let imdb_url = doc
        .select(&sel("div.imdb-info a[href]"))
        .filter_map(|a| a.value().attr("href"))
        .find(|h| h.contains("imdb.com/title/"));
    if let Some(url) = imdb_url {
        let rating = doc
            .select(&sel("#rating"))
            .next()
            .map(|r| clean_text(r.text()))
            .filter(|r| !r.is_empty());
        lines.push(match rating {
            Some(r) => format!("IMDb rating: {r}/10 — {url}"),
            None => format!("IMDb: {url}"),
        });
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

/// Whitespace-collapse an element's text fragments into one line.
fn clean_text<'a>(fragments: impl Iterator<Item = &'a str>) -> String {
    fragments
        .flat_map(str::split_whitespace)
        .collect::<Vec<_>>()
        .join(" ")
}

/// `"Friday 10th July 2026 07:53:49 PM"` → UTC datetime. The weekday
/// token is dropped rather than validated (chrono errors on any
/// mismatch) and the day's ordinal suffix is stripped.
fn parse_detail_datetime(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let mut toks: Vec<&str> = s.split_whitespace().collect();
    if toks.len() == 6 {
        toks.remove(0);
    }
    if toks.len() != 5 {
        return None;
    }
    let day = toks[0].trim_end_matches(|c: char| c.is_ascii_alphabetic());
    let joined = format!("{day} {} {} {} {}", toks[1], toks[2], toks[3], toks[4]);
    chrono::NaiveDateTime::parse_from_str(&joined, "%d %B %Y %I:%M:%S %p")
        .ok()
        .map(|n| n.and_utc())
}

/// First integer token immediately preceding a word that starts with
/// `marker` (case-insensitive): `"(54 Seeders and 1 leechers)"` with
/// marker `"seeder"` → `54`.
fn number_before(text: &str, marker: &str) -> Option<u32> {
    let words: Vec<&str> = text.split_whitespace().collect();
    words.windows(2).find_map(|w| {
        let starts = w[1]
            .trim_start_matches(['(', ')'])
            .to_ascii_lowercase()
            .starts_with(marker);
        if starts {
            w[0].trim_matches(['(', ')', ',']).parse().ok()
        } else {
            None
        }
    })
}

/// Login failures render as `<p class="text-danger">reason</p>`; an
/// account with 2FA enabled gets a dedicated "One Time Password" form
/// instead.
fn extract_login_error(html: &str) -> Option<String> {
    let doc = Html::parse_document(html);
    let danger = Selector::parse("p.text-danger").expect("static selector");
    if let Some(p) = doc.select(&danger).next() {
        let t = p.text().collect::<String>().trim().to_string();
        if !t.is_empty() {
            return Some(t);
        }
    }
    let h2 = Selector::parse("h2").expect("static selector");
    if doc
        .select(&h2)
        .any(|h| h.text().collect::<String>().contains("One Time Password"))
    {
        return Some(
            "account has 2FA enabled — set `alt_2fa_token` (Profile → Alt 2FA Token)".into(),
        );
    }
    None
}

/// `[REQ]`-style prefixes mark filled requests, not part of the release
/// name (Jackett strips them too).
fn strip_req_prefix(title: &str) -> &str {
    for p in ["[REQUESTED]", "[REQUEST]", "[REQ]"] {
        if let Some(pre) = title.get(..p.len())
            && pre.eq_ignore_ascii_case(p)
        {
            return title[p.len()..].trim_start();
        }
    }
    title
}

#[derive(Debug, Deserialize)]
struct BrowseResponse {
    #[serde(default, rename = "torrentList")]
    torrent_list: Vec<TorrentRow>,
    #[serde(default, rename = "numFound")]
    num_found: Option<u64>,
}

/// `fid` arrives as a number or a string depending on endpoint vintage.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum IdValue {
    Num(u64),
    Str(String),
}

impl IdValue {
    fn into_string(self) -> String {
        match self {
            Self::Num(n) => n.to_string(),
            Self::Str(s) => s.trim().to_string(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct TorrentRow {
    #[serde(default)]
    fid: Option<IdValue>,
    #[serde(default)]
    filename: Option<String>,
    /// Can be null (#13736 in Jackett) — such rows are skipped.
    #[serde(default)]
    name: Option<String>,
    #[serde(default, rename = "categoryID")]
    category_id: Option<u32>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    seeders: Option<u32>,
    #[serde(default)]
    leechers: Option<u32>,
    #[serde(default, rename = "addedTimestamp")]
    added_timestamp: Option<String>,
    /// `0` = freeleech; anything else costs ratio. Number or string.
    #[serde(default)]
    download_multiplier: serde_json::Value,
    /// Genre tags — array of strings or one comma-joined string.
    #[serde(default)]
    tags: serde_json::Value,
}

impl TorrentRow {
    fn into_search_result(
        self,
        provider_id: &str,
        base_url: &Url,
        rss_key: &str,
    ) -> Option<SearchResult> {
        let fid = self.fid?.into_string();
        if fid.is_empty() {
            return None;
        }
        let title = strip_req_prefix(self.name.as_deref().unwrap_or("").trim());
        if title.is_empty() {
            return None;
        }
        let download_url = self
            .filename
            .as_deref()
            .map(str::trim)
            .filter(|f| !f.is_empty())
            .and_then(|f| rss_download_url(base_url, &fid, rss_key, f).ok())
            .map(String::from);

        Some(SearchResult {
            provider_id: provider_id.to_string(),
            external_id: fid,
            year: extract_year(title),
            title: title.to_string(),
            size_bytes: self.size,
            seeders: self.seeders,
            leechers: self.leechers,
            infohash: None,
            magnet: None,
            category: self.category_id.and_then(category_label).map(String::from),
            tags: tag_list(&self.tags),
            freeleech: is_freeleech(&self.download_multiplier),
            uploader: None,
            uploaded_at: self.added_timestamp.as_deref().and_then(parse_added),
            tmdb_id: None,
            kind: self.category_id.and_then(category_kind),
            poster_url: None,
            already_in_library: false,
            library_infohash: None,
            library_file_idx: None,
            language: None,
            codec: None,
            download_url,
            parsed_season: None,
            parsed_episode: None,
        })
    }
}

fn tag_list(v: &serde_json::Value) -> Vec<String> {
    match v {
        serde_json::Value::Array(items) => items
            .iter()
            .filter_map(|i| i.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect(),
        serde_json::Value::String(s) => s
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect(),
        _ => Vec::new(),
    }
}

fn is_freeleech(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Number(n) => {
            n.as_i64() == Some(0) || n.as_f64().is_some_and(|f| f.abs() < f64::EPSILON)
        }
        serde_json::Value::String(s) => matches!(s.trim(), "0" | "0.0"),
        _ => false,
    }
}

/// `2021-10-25 02:18:31` — profile-timezone-relative; treated as UTC
/// (keep the account profile on UTC).
fn parse_added(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S")
        .ok()
        .map(|n| n.and_utc())
}

#[cfg(test)]
mod tests {
    use super::*;
    use iris_core::search::{SortField, SortOrder};

    fn provider() -> Arc<TorrentLeech> {
        let mut fields = HashMap::new();
        let base =
            std::env::var("TL_BASE_URL").unwrap_or_else(|_| "https://www.torrentleech.org".into());
        fields.insert("base_url".to_string(), toml::Value::String(base));
        fields.insert("username".to_string(), toml::Value::String("user".into()));
        fields.insert("password".to_string(), toml::Value::String("pass".into()));
        fields.insert("rss_key".to_string(), toml::Value::String("KEY123".into()));
        let entry = ProviderEntry {
            id: "tl".into(),
            kind: "torrentleech".into(),
            enabled: true,
            fields,
        };
        TorrentLeech::from_config(&entry).expect("construct")
    }

    fn query(q: &str) -> SearchQuery {
        SearchQuery {
            q: q.into(),
            page: None,
            limit: None,
            sort_by: None,
            order: None,
            kind: None,
            parsed_title: None,
            season: None,
            episode: None,
            year: None,
        }
    }

    #[test]
    fn search_url_builds_query_path() {
        let p = provider();
        let mut q = query("The Movie 2023");
        q.kind = Some(MediaKind::Movie);
        q.page = Some(2);
        q.sort_by = Some(SortField::Seeders);
        q.order = Some(SortOrder::Asc);
        let url = p.search_url(&q).expect("url");
        assert_eq!(
            url.as_str(),
            "https://www.torrentleech.org/torrents/browse/list/categories/8,9,11,37,43,14,12,13,47,15,29,36/exact/1/query/The%20Movie%202023/orderby/seeders/order/asc/page/2",
        );
    }

    #[test]
    fn search_url_queryless_uses_newfilter() {
        let p = provider();
        let mut q = query("");
        q.kind = Some(MediaKind::Tv);
        let url = p.search_url(&q).expect("url");
        assert_eq!(
            url.as_str(),
            "https://www.torrentleech.org/torrents/browse/list/categories/26,32,27,34,35,44/newfilter/2/orderby/added/order/desc",
        );
    }

    #[test]
    fn search_url_no_kind_spans_all_video_categories() {
        let p = provider();
        let url = p.search_url(&query("dune")).expect("url");
        assert!(
            url.path()
                .contains("/categories/8,9,11,37,43,14,12,13,47,15,29,36,26,32,27,34,35,44/"),
        );
        assert!(url.path().ends_with("/orderby/added/order/desc"));
    }

    #[test]
    fn search_term_rebuilds_scene_form_and_sanitizes() {
        let mut q = query("Classroom.of.the.Elite S04E11 1080p");
        q.parsed_title = Some("Classroom of the Elite".into());
        q.season = Some(4);
        q.episode = Some(11);
        assert_eq!(build_search_term(&q), "Classroom of the Elite S04E11");

        // Dots and colons become separators; leading dashes are negation
        // markers on TL and must go.
        assert_eq!(
            build_search_term(&query("Mission:.Impossible -Dead.Reckoning")),
            "Mission Impossible Dead Reckoning",
        );
        // In-word dashes survive.
        assert_eq!(
            build_search_term(&query("Spider-Man 2021")),
            "Spider-Man 2021"
        );
    }

    #[test]
    fn category_mapping() {
        assert_eq!(category_kind(47), Some(MediaKind::Movie));
        assert_eq!(category_kind(34), Some(MediaKind::Tv));
        assert_eq!(category_kind(44), Some(MediaKind::Tv));
        // Games / music / unknown ids are unclassified.
        assert_eq!(category_kind(17), None);
        assert_eq!(category_kind(31), None);
        assert_eq!(category_label(32), Some("TV/Episodes HD"));
        assert_eq!(category_label(999), None);
    }

    #[test]
    fn parses_browse_response() {
        let sample = r#"{
            "torrentList": [
                {
                    "fid": 241950,
                    "filename": "The.Movie.2023.1080p.BluRay.x264-GRP.torrent",
                    "name": "[REQ] The.Movie.2023.1080p.BluRay.x264-GRP",
                    "categoryID": 14,
                    "size": 9814000271,
                    "seeders": 12,
                    "leechers": 3,
                    "completed": 45,
                    "addedTimestamp": "2026-07-25 18:32:10",
                    "download_multiplier": 0,
                    "tags": ["Action", "Sci-Fi"]
                },
                {
                    "fid": "241951",
                    "filename": "Show.S01E02.720p.torrent",
                    "name": "Show.S01E02.720p.WEB.h264-GRP",
                    "categoryID": 26,
                    "size": 734003200,
                    "seeders": 5,
                    "leechers": 1,
                    "addedTimestamp": "2026-07-26 01:02:03",
                    "download_multiplier": 1,
                    "tags": "Drama, Comedy"
                },
                {
                    "fid": 241952,
                    "filename": "x.torrent",
                    "name": null,
                    "categoryID": 26
                }
            ],
            "numFound": 123
        }"#;
        let resp: BrowseResponse = serde_json::from_str(sample).expect("parse");
        assert_eq!(resp.num_found, Some(123));
        let base = Url::parse("https://www.torrentleech.org").expect("url");
        let results: Vec<SearchResult> = resp
            .torrent_list
            .into_iter()
            .filter_map(|r| r.into_search_result("tl", &base, "KEY123"))
            .collect();
        // Null-name row skipped.
        assert_eq!(results.len(), 2);

        let r = &results[0];
        assert_eq!(r.provider_id, "tl");
        assert_eq!(r.external_id, "241950");
        // [REQ] prefix stripped.
        assert_eq!(r.title, "The.Movie.2023.1080p.BluRay.x264-GRP");
        assert_eq!(r.year, Some(2023));
        assert_eq!(r.size_bytes, Some(9_814_000_271));
        assert_eq!(r.seeders, Some(12));
        assert_eq!(r.leechers, Some(3));
        assert_eq!(r.kind, Some(MediaKind::Movie));
        assert_eq!(r.category.as_deref(), Some("Movies/BlurayRip"));
        assert!(r.freeleech, "download_multiplier 0 = freeleech");
        assert_eq!(r.tags, vec!["Action".to_string(), "Sci-Fi".to_string()]);
        assert_eq!(
            r.uploaded_at.expect("timestamp").to_rfc3339(),
            "2026-07-25T18:32:10+00:00",
        );
        assert_eq!(
            r.download_url.as_deref(),
            Some(
                "https://www.torrentleech.org/rss/download/241950/KEY123/The.Movie.2023.1080p.BluRay.x264-GRP.torrent"
            ),
        );

        // String fid + CSV tags + multiplier 1.
        let r = &results[1];
        assert_eq!(r.external_id, "241951");
        assert!(!r.freeleech);
        assert_eq!(r.tags, vec!["Drama".to_string(), "Comedy".to_string()]);
        assert_eq!(r.kind, Some(MediaKind::Tv));
    }

    #[test]
    fn tolerates_minimal_rows() {
        let sample = r#"{"torrentList": [{"fid": 1, "name": "Bare.Release"}]}"#;
        let resp: BrowseResponse = serde_json::from_str(sample).expect("parse");
        let base = Url::parse("https://www.torrentleech.org").expect("url");
        let results: Vec<SearchResult> = resp
            .torrent_list
            .into_iter()
            .filter_map(|r| r.into_search_result("tl", &base, "K"))
            .collect();
        assert_eq!(results.len(), 1);
        let r = &results[0];
        assert_eq!(r.title, "Bare.Release");
        assert!(r.download_url.is_none(), "no filename → no rss url");
        assert!(r.category.is_none());
        assert!(r.kind.is_none());
        assert!(!r.freeleech);
        assert!(r.tags.is_empty());
    }

    #[test]
    fn freeleech_multiplier_shapes() {
        assert!(is_freeleech(&serde_json::json!(0)));
        assert!(is_freeleech(&serde_json::json!(0.0)));
        assert!(is_freeleech(&serde_json::json!("0")));
        assert!(!is_freeleech(&serde_json::json!(1)));
        assert!(!is_freeleech(&serde_json::json!("1")));
        assert!(!is_freeleech(&serde_json::Value::Null));
    }

    #[test]
    fn rss_url_encodes_filename() {
        let base = Url::parse("https://www.torrentleech.org").expect("url");
        let url = rss_download_url(&base, "42", "KEY", "A Movie (2023).torrent").expect("url");
        assert_eq!(
            url.as_str(),
            "https://www.torrentleech.org/rss/download/42/KEY/A%20Movie%20(2023).torrent",
        );
    }

    #[test]
    fn strips_req_prefixes() {
        assert_eq!(strip_req_prefix("[REQ] X.2023"), "X.2023");
        assert_eq!(strip_req_prefix("[request]X.2023"), "X.2023");
        assert_eq!(strip_req_prefix("[REQUESTED] X"), "X");
        assert_eq!(strip_req_prefix("Regular.Release"), "Regular.Release");
    }

    #[test]
    fn login_error_extraction() {
        let html = r#"<html><body><form name="login-form"><p class="text-danger">Invalid username or password</p></form></body></html>"#;
        assert_eq!(
            extract_login_error(html).as_deref(),
            Some("Invalid username or password"),
        );
        let twofa = r#"<html><body><div class="login-container"><h2>One Time Password</h2></div></body></html>"#;
        assert!(
            extract_login_error(twofa)
                .expect("2fa detected")
                .contains("2FA"),
        );
        assert_eq!(extract_login_error("<html><body>ok</body></html>"), None);
    }

    /// Scratch harness: the full login + search + parse path against the
    /// live site, i.e. exactly what the app does.
    /// `TL_USERNAME=… TL_PASSWORD=… TL_RSS_KEY=… cargo test -p iris-providers -- --ignored debug_live_search --nocapture`
    #[tokio::test]
    #[ignore = "hits the live site; needs TL_USERNAME/TL_PASSWORD/TL_RSS_KEY"]
    async fn debug_live_search() {
        let mut fields = HashMap::new();
        let base =
            std::env::var("TL_BASE_URL").unwrap_or_else(|_| "https://www.torrentleech.org".into());
        fields.insert("base_url".to_string(), toml::Value::String(base));
        fields.insert(
            "username_env".to_string(),
            toml::Value::String("TL_USERNAME".into()),
        );
        fields.insert(
            "password_env".to_string(),
            toml::Value::String("TL_PASSWORD".into()),
        );
        fields.insert(
            "rss_key_env".to_string(),
            toml::Value::String("TL_RSS_KEY".into()),
        );
        let entry = ProviderEntry {
            id: "tl".into(),
            kind: "torrentleech".into(),
            enabled: true,
            fields,
        };
        let p = TorrentLeech::from_config(&entry).expect("construct");
        let page = p
            .search(&query(
                &std::env::var("TL_QUERY").unwrap_or_else(|_| "dune".into()),
            ))
            .await
            .expect("live search");
        println!(
            "live: {} results (numFound: {:?})",
            page.results.len(),
            page.total_count
        );
        for r in page.results.iter().take(5) {
            println!(
                "fid={} s={:?} l={:?} size={:?} cat={:?} kind={:?} fl={} at={:?} title={}",
                r.external_id,
                r.seeders,
                r.leechers,
                r.size_bytes,
                r.category,
                r.kind,
                r.freeleech,
                r.uploaded_at,
                r.title
            );
        }
    }

    /// Scratch harness: live grab through `resolve()` (uses the `rss_key`
    /// download path — costs a snatch on the account, pick a freeleech
    /// fid). `TL_USERNAME=… TL_PASSWORD=… TL_RSS_KEY=… TL_FID=… cargo
    /// test -p iris-providers -- --ignored debug_live_resolve --nocapture`
    #[tokio::test]
    #[ignore = "hits the live site; needs TL_USERNAME/TL_PASSWORD/TL_RSS_KEY/TL_FID"]
    async fn debug_live_resolve() {
        let mut fields = HashMap::new();
        let base =
            std::env::var("TL_BASE_URL").unwrap_or_else(|_| "https://www.torrentleech.org".into());
        fields.insert("base_url".to_string(), toml::Value::String(base));
        fields.insert(
            "username_env".to_string(),
            toml::Value::String("TL_USERNAME".into()),
        );
        fields.insert(
            "password_env".to_string(),
            toml::Value::String("TL_PASSWORD".into()),
        );
        fields.insert(
            "rss_key_env".to_string(),
            toml::Value::String("TL_RSS_KEY".into()),
        );
        let entry = ProviderEntry {
            id: "tl".into(),
            kind: "torrentleech".into(),
            enabled: true,
            fields,
        };
        let p = TorrentLeech::from_config(&entry).expect("construct");
        let fid = std::env::var("TL_FID").expect("set TL_FID to a torrent id");
        match p.resolve(&fid).await.expect("resolve") {
            TorrentSource::TorrentFile(bytes) => {
                println!("got .torrent: {} bytes", bytes.len());
                assert_eq!(bytes.first().copied(), Some(BENCODE_DICT_MARKER));
            }
            TorrentSource::Magnet(m) => panic!("unexpected magnet: {m}"),
        }
    }

    /// Trimmed-down copy of a real `/torrent/{fid}` page (mirror,
    /// 2026-08): every block `parse_details_page` reads, including the
    /// commented-out NFO editor template that must NOT be picked up as
    /// the NFO.
    const DETAILS_FIXTURE: &str = r#"<!DOCTYPE html>
<html><head><title>Torrent Details for The Dunes 2021 720p WEB H264-AFO :: TorrentLeech.org</title></head>
<body>
<a href="/user/account/logout">Log out</a>
<div class="col-md-8 col-sm-6 torrent_info_details">
  <table class="table borderless"><tbody>
    <tr><td class="description">Category</td><td><a href="/torrents/browse/index/categories/37">WEBRip</a></td></tr>
    <tr><td class="description">Added</td><td>Friday 10th July 2026 07:53:49 PM <strong>(29 days ago)</strong></td></tr>
    <tr><td class="description">Size</td><td>2.61 GB</td></tr>
    <tr><td class="description">Peers</td><td>55 Peers (54 Seeders and 1 leechers)</td></tr>
    <tr><td class="description">Downloaded</td><td>265 times</td></tr>
    <tr><td class="description">Uploader</td><td><span class="details-uploader-name">Anonymous</span>
      <button class="thankYouButton">Thank You (<span class="thankYouNum">10</span>)</button></td></tr>
    <tr><td class="description">Comments</td><td>None</td></tr>
    <tr><td class="description">Tags</td><td>
      <span class="tag label label-info">Thriller<span data-role="remove"></span></span>
      <span class="tag label label-info">FREELEECH<span data-role="remove"></span></span>
    </td></tr>
  </tbody></table>
</div>
<div class="torrent-info-details">
  <h3>The Dunes</h3>
  <div class="torrent-summary mb-20"><p>A writer becomes the obsession of a stranger.</p></div>
  <div class="torrent-extra-info">
    <p><span class="description">Year: </span> 2021</p>
    <p><span class="description">Genre: </span>Thriller</p>
    <div class="imdb-info mt-20">
      <span><a target="_blank" href="http://www.imdb.com/title/tt6910678"><img src="/images/imdb.png"></a>Rating</span>
      <span id="tenBaseRating"><span id="rating">4</span>/10</span>
    </div>
  </div>
</div>
<div class="nfo-wrapper">
  <div id="editor"> <!--
     --><!--<pre class="nfo col-xs-12"><div class="editNFO" id="nfo_text" data-pk="--><!--" title="Edit NFO">--><!--</div></pre>--> <!--
     -->
    <pre class="nfo col-xs-12"><div class="editNFO" id="nfo_text">
      AFO presents The Dunes
      Video: x264 CRF 22 @ 3100 Kbps
      Audio: English DD 5.1 @ 384 Kbps</div></pre>
  </div>
</div>
<table id="fileListTable" class="table"><thead><tr><th>Filename</th><th>Size</th></tr></thead>
  <tbody>
    <tr id="130425343"><td>Sample/afo-thedunes-720-web-sample.mkv</td><td>18 MB</td></tr>
    <tr id="130425344"><td>afo-thedunes-720-web.mkv</td><td>2.58 GB</td></tr>
    <tr id="130425345"><td>afo-thedunes-720-web.srr</td><td>12 KB</td></tr>
    <tr id="130425346"><td>the.dunes.2021.720p.web.h264-afo.nfo</td><td>7 KB</td></tr>
  </tbody>
</table>
</body></html>"#;

    #[test]
    fn details_page_parsing() {
        let d = parse_details_page("tl", "241798657", DETAILS_FIXTURE).expect("parses");
        assert_eq!(d.provider_id, "tl");
        assert_eq!(d.external_id, "241798657");
        assert_eq!(d.title, "The Dunes 2021 720p WEB H264-AFO");
        assert_eq!(d.category.as_deref(), Some("WEBRip"));
        assert_eq!(
            d.uploaded_at.map(|t| t.to_rfc3339()),
            Some("2026-07-10T19:53:49+00:00".to_string()),
        );
        assert_eq!(d.age.as_deref(), Some("29 days ago"));
        assert_eq!(d.file_size_bytes, Some(2_802_466_161));
        assert_eq!(d.seeders, Some(54));
        assert_eq!(d.leechers, Some(1));
        assert_eq!(d.times_completed, Some(265));
        assert_eq!(d.uploader.as_deref(), Some("Anonymous"));
        assert_eq!(d.tags, vec!["Thriller".to_string()]);
        assert!(d.freeleech);
        assert_eq!(d.description_format, DescriptionFormat::Plain);
        let desc = d.description.expect("description");
        assert!(desc.starts_with("A writer becomes the obsession of a stranger."));
        assert!(desc.contains("Year: 2021"));
        assert!(desc.contains("IMDb rating: 4/10 — http://www.imdb.com/title/tt6910678"));
        // Real NFO picked up (raw, multi-line), commented template ignored.
        let nfo = d.nfo.expect("nfo");
        assert!(nfo.contains("AFO presents The Dunes"));
        assert!(!nfo.contains("Edit NFO"));
        assert_eq!(d.file_count, Some(4));
    }

    #[test]
    fn details_page_rejects_non_detail_pages() {
        // Login page (no "Torrent Details for" title prefix).
        assert!(
            parse_details_page(
                "tl",
                "1",
                r"<html><head><title>Login :: TorrentLeech.org</title></head><body></body></html>",
            )
            .is_none()
        );
    }

    #[test]
    fn detail_datetime_parsing() {
        assert_eq!(
            parse_detail_datetime("Friday 10th July 2026 07:53:49 PM").map(|t| t.to_rfc3339()),
            Some("2026-07-10T19:53:49+00:00".to_string()),
        );
        // 1st/2nd/3rd ordinals and AM times.
        assert_eq!(
            parse_detail_datetime("Monday 1st September 2025 09:05:00 AM").map(|t| t.to_rfc3339()),
            Some("2025-09-01T09:05:00+00:00".to_string()),
        );
        assert_eq!(parse_detail_datetime("29 days ago"), None);
        assert_eq!(parse_detail_datetime(""), None);
    }

    /// Scratch harness: live details scrape.
    /// `TL_USERNAME=… TL_PASSWORD=… TL_RSS_KEY=… TL_FID=… cargo test -p
    /// iris-providers -- --ignored debug_live_details --nocapture`
    #[tokio::test]
    #[ignore = "hits the live site; needs TL_USERNAME/TL_PASSWORD/TL_RSS_KEY/TL_FID"]
    async fn debug_live_details() {
        let mut fields = HashMap::new();
        let base =
            std::env::var("TL_BASE_URL").unwrap_or_else(|_| "https://www.torrentleech.org".into());
        fields.insert("base_url".to_string(), toml::Value::String(base));
        fields.insert(
            "username_env".to_string(),
            toml::Value::String("TL_USERNAME".into()),
        );
        fields.insert(
            "password_env".to_string(),
            toml::Value::String("TL_PASSWORD".into()),
        );
        fields.insert(
            "rss_key_env".to_string(),
            toml::Value::String("TL_RSS_KEY".into()),
        );
        let entry = ProviderEntry {
            id: "tl".into(),
            kind: "torrentleech".into(),
            enabled: true,
            fields,
        };
        let p = TorrentLeech::from_config(&entry).expect("construct");
        let fid = std::env::var("TL_FID").expect("set TL_FID to a torrent id");
        let d = p.details(&fid).await.expect("details").expect("some");
        println!(
            "title={}\ncategory={:?} size={:?} seeders={:?} leechers={:?} dl={:?}\nuploader={:?} at={:?} age={:?}\ntags={:?} freeleech={} files={:?}\nnfo? {} media_info? {}\n--- description ---\n{}",
            d.title,
            d.category,
            d.file_size_bytes,
            d.seeders,
            d.leechers,
            d.times_completed,
            d.uploader,
            d.uploaded_at,
            d.age,
            d.tags,
            d.freeleech,
            d.file_count,
            d.nfo.as_deref().map_or(0, str::len),
            d.media_info.is_some(),
            d.description.as_deref().unwrap_or("<none>"),
        );
    }
}
