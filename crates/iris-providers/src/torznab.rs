//! Generic Torznab provider — works against any indexer exposing the
//! Torznab 1.3 API. Spec: <https://torznab.github.io/spec-1.3-draft/>.
//!
//! What we implement:
//! - `t=search` (+ `t=movie` / `t=tvsearch` when a [`MediaKind`] filter
//!   is set on the query)
//! - response parsing of the RSS-with-torznab-attrs envelope
//! - download resolution by remembering each item's `<link>` URL in a
//!   per-process cache (Torznab download URLs are typically signed
//!   short-lived, so the spec's "client follows the link from search"
//!   contract is the right model — we honour it)
//!
//! Config:
//! ```toml
//! [[providers]]
//! id = "myindexer"
//! kind = "torznab"
//! enabled = true
//! base_url = "https://indexer.example"   # without /api — appended via `api_path`
//! api_path = "/api"                       # optional, default "/api"
//! api_key_env = "MYINDEXER_API_KEY"
//! # optional category overrides — comma-separated Torznab category IDs.
//! # Defaults follow the spec's coarse buckets (2000=movies, 5000=tv).
//! # movie_categories = "2000,2030,2040,2045,2050,2060"
//! # tv_categories    = "5000,5030,5040,5045,5070"
//! # referer    = "https://indexer.example/"
//! # user_agent = "..."
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use iris_config::ProviderEntry;
use iris_core::Error;
use iris_core::Result;
use iris_core::search::{
    MediaKind, ProviderCapabilities, ProviderPage, SearchQuery, SearchResult, TorrentSource,
};
use quick_xml::Reader;
use quick_xml::escape::unescape as xml_unescape;
use quick_xml::events::{BytesText, Event};
use quick_xml::events::attributes::Attribute;
use reqwest::header::{ACCEPT, ACCEPT_LANGUAGE, HeaderMap, HeaderValue, REFERER, USER_AGENT};
use reqwest::{Client, StatusCode};
use tokio::sync::Mutex;
use url::Url;

use crate::SearchProvider;
use crate::util::{extract_year, field_or_env, field_str};

const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:150.0) Gecko/20100101 Firefox/150.0";
const DEFAULT_API_PATH: &str = "/api";
const DEFAULT_MOVIE_CATEGORIES: &str = "2000";
const DEFAULT_TV_CATEGORIES: &str = "5000";
/// First byte of a valid `.torrent` file (bencoded dictionary).
const BENCODE_DICT_MARKER: u8 = b'd';
/// Keep the last N search-result links around so `resolve()` can find
/// the download URL the indexer signed for us. Older entries are
/// evicted FIFO. 4096 covers a heavy browsing session without
/// unbounded growth.
const LINK_CACHE_CAP: usize = 4096;

pub struct TorznabProvider {
    id: String,
    base_url: Url,
    api_path: String,
    api_key: String,
    movie_categories: String,
    tv_categories: String,
    http: Client,
    /// `external_id` -> signed download URL captured from search responses.
    link_cache: Mutex<LinkCache>,
}

/// Tiny FIFO cache so `resolve()` can find the indexer-signed download URL
/// for a torrent we previously surfaced via search.
struct LinkCache {
    map: HashMap<String, String>,
    order: std::collections::VecDeque<String>,
}

impl LinkCache {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            order: std::collections::VecDeque::new(),
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

impl TorznabProvider {
    pub fn from_config(entry: &ProviderEntry) -> Result<Arc<Self>> {
        let base_url_str = field_str(entry, "base_url")?;
        let base_url = Url::parse(base_url_str)
            .map_err(|e| Error::Provider(format!("torznab base_url invalid: {e}")))?;
        let api_key = field_or_env(entry, "api_key")?;
        let api_path = entry
            .fields
            .get("api_path")
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_API_PATH)
            .to_string();
        let movie_categories = entry
            .fields
            .get("movie_categories")
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_MOVIE_CATEGORIES)
            .to_string();
        let tv_categories = entry
            .fields
            .get("tv_categories")
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_TV_CATEGORIES)
            .to_string();
        let referer = entry.fields.get("referer").and_then(|v| v.as_str());
        let user_agent = entry
            .fields
            .get("user_agent")
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_USER_AGENT);

        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(user_agent)
                .map_err(|e| Error::Provider(format!("torznab user_agent invalid: {e}")))?,
        );
        headers.insert(ACCEPT, HeaderValue::from_static("application/xml, */*"));
        headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));
        if let Some(r) = referer {
            headers.insert(
                REFERER,
                HeaderValue::from_str(r)
                    .map_err(|e| Error::Provider(format!("torznab referer invalid: {e}")))?,
            );
        }

        let http = Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| Error::Provider(format!("torznab http client: {e}")))?;

        Ok(Arc::new(Self {
            id: entry.id.clone(),
            base_url,
            api_path,
            api_key,
            movie_categories,
            tv_categories,
            http,
            link_cache: Mutex::new(LinkCache::new()),
        }))
    }

    /// Allow composing providers (e.g. c411) to push known download URLs
    /// into the cache from non-search code paths (homepage featured,
    /// curated lists, …) so `resolve()` keeps working without forcing
    /// the user to search first.
    pub async fn cache_download_url(&self, external_id: String, url: String) {
        self.link_cache.lock().await.put(external_id, url);
    }

    fn api_url(&self) -> Result<Url> {
        self.base_url
            .join(&self.api_path)
            .map_err(|e| Error::Provider(format!("torznab join api path: {e}")))
    }
}

#[async_trait]
impl SearchProvider for TorznabProvider {
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
        let url = self.api_url()?;
        let limit = q.limit.unwrap_or(25).clamp(1, 100);
        let page = q.page.unwrap_or(1).max(1);
        let offset = (page - 1) * limit;

        // Map Iris's coarse `MediaKind` filter to Torznab's two
        // dedicated search operations + the corresponding category
        // bucket. With no filter we fall back to the generic search,
        // which any compliant indexer supports.
        //
        // Override: when the SCENE parser found a season (or season+
        // episode) in the user query, force `t=tvsearch` even without
        // an explicit MediaKind. Torznab's tvsearch op accepts
        // `season=` / `ep=` parameters that filter at the indexer
        // (e.g. c411 / theoldschool) instead of relying on substring
        // matching the full `q` — a massive precision win on
        // "Classroom of the Elite S04E11" style queries.
        let has_se_hint = q.season.is_some();
        let (t_op, cats) = if has_se_hint {
            ("tvsearch", self.tv_categories.as_str())
        } else {
            match q.kind {
                Some(MediaKind::Movie) => ("movie", self.movie_categories.as_str()),
                Some(MediaKind::Tv) => ("tvsearch", self.tv_categories.as_str()),
                None => ("search", ""),
            }
        };

        // When we have a parsed title, send that as `q=` (cleaner
        // match than the raw user string with `S04E11` baked in) and
        // hand the structured season/episode separately. Falls back
        // to the raw query when the parser had nothing useful.
        let q_param = q
            .parsed_title
            .as_deref()
            .filter(|t| !t.is_empty() && has_se_hint)
            .map_or_else(|| q.q.clone(), str::to_string);

        let mut qs: Vec<(&'static str, String)> = vec![
            ("t", t_op.to_string()),
            ("apikey", self.api_key.clone()),
            ("q", q_param),
            ("limit", limit.to_string()),
            ("offset", offset.to_string()),
        ];
        if !cats.is_empty() {
            qs.push(("cat", cats.to_string()));
        }
        // Season / episode hints (Torznab tvsearch native). `episode==0`
        // is the in-band season-pack sentinel from the parser — pass
        // only the season in that case so the indexer returns the
        // pack alongside the episodes.
        if let Some(s) = q.season {
            qs.push(("season", s.to_string()));
            if let Some(e) = q.episode {
                if e > 0 {
                    qs.push(("ep", e.to_string()));
                }
            }
        }

        let body = self
            .http
            .get(url)
            .query(&qs)
            .send()
            .await
            .map_err(|e| Error::Provider(format!("torznab request: {e}")))?
            .error_for_status()
            .map_err(|e| Error::Provider(format!("torznab status: {e}")))?
            .text()
            .await
            .map_err(|e| Error::Provider(format!("torznab body: {e}")))?;

        let parsed = parse_torznab_xml(&body).map_err(|e| {
            tracing::warn!(
                provider = %self.id,
                error = %e,
                body_preview = %body.chars().take(400).collect::<String>(),
                "torznab parse failed",
            );
            e
        })?;

        let mut results = Vec::with_capacity(parsed.items.len());
        {
            let mut cache = self.link_cache.lock().await;
            for item in &parsed.items {
                if let Some(link) = &item.download_url {
                    cache.put(item.external_id.clone(), link.clone());
                }
            }
            for item in parsed.items {
                results.push(item.into_search_result(&self.id));
            }
        }

        // Torznab `<response offset total />` is optional. When the indexer
        // gives us a total we compute pages; otherwise we leave them
        // unknown and the UI shows "load more" instead of a paginator.
        let total_count = parsed.total;
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

    async fn resolve(&self, external_id: &str) -> Result<TorrentSource> {
        let url = self
            .link_cache
            .lock()
            .await
            .get(external_id)
            .ok_or_else(|| {
                Error::Provider(format!(
                    "torznab `{}`: no download URL cached for `{external_id}` — \
                     search this indexer again to refresh the link",
                    self.id
                ))
            })?;

        // The indexer's link is sometimes a magnet (rare) but almost
        // always a redirect to the bencoded .torrent file. Honour
        // either, mirroring how Sonarr / Radarr consume Torznab.
        if url.starts_with("magnet:") {
            return Ok(TorrentSource::Magnet(url));
        }

        let res = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| Error::Provider(format!("torznab download: {e}")))?;
        if !res.status().is_success() && res.status() != StatusCode::FOUND {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(Error::Provider(format!(
                "torznab `{}` download failed: HTTP {status} — {body}",
                self.id,
            )));
        }
        let bytes = res
            .bytes()
            .await
            .map_err(|e| Error::Provider(format!("torznab download body: {e}")))?;

        // Some indexers respond with a magnet body instead of a .torrent
        // file when the user lacks bandwidth credit — detect and surface
        // it as a magnet rather than a corrupted file.
        if bytes.starts_with(b"magnet:") {
            let s = std::str::from_utf8(&bytes)
                .map_err(|e| Error::Provider(format!("torznab magnet body: {e}")))?
                .trim()
                .to_string();
            return Ok(TorrentSource::Magnet(s));
        }

        if bytes.first().copied() != Some(BENCODE_DICT_MARKER) {
            // Surface the URL we hit + a short preview so misconfigured
            // indexers (HTML detail page in `<link>`, missing/expired
            // apikey returning a login HTML, etc.) are diagnosable from
            // logs without re-running the whole flow.
            let preview = String::from_utf8_lossy(&bytes[..bytes.len().min(200)]).into_owned();
            tracing::warn!(
                provider = %self.id,
                external_id,
                url = %url,
                first_byte = ?bytes.first(),
                body_preview = %preview,
                "torznab download returned non-bencoded body",
            );
            return Err(Error::Provider(format!(
                "torznab `{}` download for `{external_id}` returned non-bencoded body \
                 (first byte: {:?})",
                self.id,
                bytes.first()
            )));
        }
        Ok(TorrentSource::TorrentFile(bytes.to_vec()))
    }
}

// ===========================================================================
// XML parsing (event-driven — quick-xml's serde mode struggles with the
// `<torznab:attr name=... value=.../>` repeated pattern, and the format
// is small enough that hand-rolling stays compact).
// ===========================================================================

#[derive(Debug, Default)]
struct ParsedTorznab {
    items: Vec<RawItem>,
    /// From `<torznab:response offset="..." total="..."/>` when present.
    total: Option<u64>,
}

#[derive(Debug, Default)]
struct RawItem {
    title: String,
    /// Whatever the indexer used as the stable id (`<guid>` content). For
    /// most Torznab servers this is a numeric torrent id; we keep it as
    /// a string so we don't lose precision or non-numeric variants.
    external_id: String,
    /// Indexer-signed link to download the .torrent file (or a magnet).
    /// Set from `<link>` *or* `<enclosure url=...>` — the latter wins
    /// when both are present because some indexers (c411) put the HTML
    /// torrent-detail page in `<link>` and only the .torrent URL in
    /// `<enclosure>`.
    download_url: Option<String>,
    /// True once we've taken the URL from a `<enclosure>` — any later
    /// `<link>` text is then ignored. Lets the two events arrive in any
    /// order while still preferring the enclosure source of truth.
    download_url_from_enclosure: bool,
    size: Option<u64>,
    seeders: Option<u32>,
    leechers: Option<u32>,
    infohash: Option<String>,
    tmdb_id: Option<u64>,
    /// Numeric Torznab category(ies). First one wins for kind derivation.
    categories: Vec<u32>,
    freeleech: bool,
    description: Option<String>,
    pub_date: Option<String>,
}

impl RawItem {
    fn apply_torznab_attr(&mut self, name: &str, value: &str) {
        match name {
            "size" => {
                if let Ok(n) = value.parse() {
                    self.size = Some(n);
                }
            }
            "seeders" => {
                if let Ok(n) = value.parse() {
                    self.seeders = Some(n);
                }
            }
            // Some indexers expose `peers` (= seeders+leechers) instead of
            // a dedicated leechers attr. We only consume the unambiguous
            // names so we never under- or double-count.
            "leechers" => {
                if let Ok(n) = value.parse() {
                    self.leechers = Some(n);
                }
            }
            "infohash" if !value.is_empty() => {
                self.infohash = Some(value.to_ascii_lowercase());
            }
            "tmdbid" | "tmdb" => {
                if let Ok(n) = value.parse() {
                    if n > 0 {
                        self.tmdb_id = Some(n);
                    }
                }
            }
            "category" => {
                if let Ok(n) = value.parse() {
                    self.categories.push(n);
                }
            }
            // Jackett / Prowlarr / many UNIT3D indexers expose freeleech
            // as `downloadvolumefactor` = 0 (you owe nothing on download).
            "downloadvolumefactor" => {
                if let Ok(v) = value.parse::<f32>() {
                    if v == 0.0 {
                        self.freeleech = true;
                    }
                }
            }
            _ => {}
        }
    }

    fn into_search_result(self, provider_id: &str) -> SearchResult {
        let year = extract_year(&self.title);
        let kind = derive_kind_from_categories(&self.categories);
        let uploaded_at = self
            .pub_date
            .as_deref()
            .and_then(parse_rfc2822_lenient);

        SearchResult {
            provider_id: provider_id.to_string(),
            external_id: self.external_id,
            title: self.title,
            year,
            size_bytes: self.size,
            seeders: self.seeders,
            leechers: self.leechers,
            infohash: self.infohash,
            magnet: None,
            category: self.categories.first().map(ToString::to_string),
            tags: Vec::new(),
            freeleech: self.freeleech,
            uploader: None,
            uploaded_at,
            tmdb_id: self.tmdb_id,
            kind,
            poster_url: None,
            already_in_library: false,
            library_infohash: None,
            library_file_idx: None,
            language: None,
        }
    }
}

/// Torznab categories: 2xxx = movies, 5xxx = TV. Other namespaces
/// (3xxx audio, 7xxx books, …) we treat as unknown — Iris's UI is
/// movie/TV-only anyway.
fn derive_kind_from_categories(cats: &[u32]) -> Option<MediaKind> {
    cats.iter().find_map(|c| match c / 1000 {
        2 => Some(MediaKind::Movie),
        5 => Some(MediaKind::Tv),
        _ => None,
    })
}

#[derive(Clone, Copy)]
enum TagKind {
    Title,
    Link,
    Guid,
    Size,
    Category,
    Description,
    PubDate,
}

fn parse_torznab_xml(body: &str) -> Result<ParsedTorznab> {
    let mut reader = Reader::from_str(body);
    reader.config_mut().trim_text(true);

    let mut out = ParsedTorznab::default();
    let mut current: Option<RawItem> = None;
    let mut in_channel = false;
    let mut last_tag: Option<TagKind> = None;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                handle_start(e.name().as_ref(), &mut in_channel, &mut current, &mut last_tag);
            }
            Ok(Event::Empty(e)) => {
                handle_empty(&e, current.as_mut(), &mut out);
            }
            Ok(Event::Text(t)) => {
                if let (Some(item), Some(kind)) = (current.as_mut(), last_tag) {
                    if let Some(text) = text_value(&t) {
                        apply_text(item, kind, text);
                    }
                }
            }
            Ok(Event::CData(c)) => {
                if let (Some(item), Some(TagKind::Description)) = (current.as_mut(), last_tag) {
                    if let Ok(text) = std::str::from_utf8(c.as_ref()) {
                        item.description = Some(text.to_string());
                    }
                }
            }
            Ok(Event::End(e)) => {
                match e.name().as_ref() {
                    b"item" => {
                        if let Some(item) = current.take() {
                            out.items.push(item);
                        }
                    }
                    b"channel" => in_channel = false,
                    _ => {}
                }
                last_tag = None;
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(Error::Provider(format!("torznab xml: {e}")));
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(out)
}

fn handle_start(
    name: &[u8],
    in_channel: &mut bool,
    current: &mut Option<RawItem>,
    last_tag: &mut Option<TagKind>,
) {
    match name {
        b"channel" => *in_channel = true,
        b"item" if *in_channel => *current = Some(RawItem::default()),
        b"title" => *last_tag = Some(TagKind::Title),
        b"link" => *last_tag = Some(TagKind::Link),
        b"guid" => *last_tag = Some(TagKind::Guid),
        b"size" => *last_tag = Some(TagKind::Size),
        b"category" => *last_tag = Some(TagKind::Category),
        b"description" => *last_tag = Some(TagKind::Description),
        b"pubDate" => *last_tag = Some(TagKind::PubDate),
        _ => *last_tag = None,
    }
}

fn handle_empty(
    e: &quick_xml::events::BytesStart<'_>,
    current: Option<&mut RawItem>,
    out: &mut ParsedTorznab,
) {
    let name = e.name();
    let bytes = name.as_ref();
    if bytes.ends_with(b"torznab:attr") || bytes.ends_with(b":attr") {
        if let Some(item) = current {
            apply_torznab_attr_element(e, item);
        }
    } else if bytes == b"enclosure" {
        if let Some(item) = current {
            apply_enclosure_element(e, item);
        }
    } else if bytes.ends_with(b":response") || bytes == b"response" {
        for attr in e.attributes().flatten() {
            if attr.key.as_ref() == b"total" {
                if let Some(s) = attr_value(&attr) {
                    if let Ok(n) = s.parse() {
                        out.total = Some(n);
                    }
                }
            }
        }
    }
}

fn apply_torznab_attr_element(e: &quick_xml::events::BytesStart<'_>, item: &mut RawItem) {
    let (mut k, mut v) = (None, None);
    for attr in e.attributes().flatten() {
        match attr.key.as_ref() {
            b"name" => k = attr_value(&attr),
            b"value" => v = attr_value(&attr),
            _ => {}
        }
    }
    if let (Some(k), Some(v)) = (k, v) {
        item.apply_torznab_attr(&k, &v);
    }
}

fn apply_enclosure_element(e: &quick_xml::events::BytesStart<'_>, item: &mut RawItem) {
    for attr in e.attributes().flatten() {
        match attr.key.as_ref() {
            b"url" => {
                if let Some(s) = attr_value(&attr) {
                    // Enclosure always wins — see [`RawItem::download_url`].
                    item.download_url = Some(s);
                    item.download_url_from_enclosure = true;
                }
            }
            b"length" => {
                if let Some(s) = attr_value(&attr) {
                    if let Ok(n) = s.parse() {
                        item.size.get_or_insert(n);
                    }
                }
            }
            _ => {}
        }
    }
}

fn apply_text(item: &mut RawItem, kind: TagKind, text: String) {
    match kind {
        TagKind::Title => item.title = text,
        TagKind::Link => {
            // Enclosure URL is authoritative — only fall back to <link>
            // when no enclosure was seen for this item.
            if !item.download_url_from_enclosure {
                item.download_url.get_or_insert(text);
            }
        }
        TagKind::Guid => {
            // GUIDs can be a permalink URL — try to keep a stable numeric
            // id by taking the last path segment, falling back to the raw
            // string.
            let id = text.rsplit('/').next().unwrap_or(&text).to_string();
            item.external_id = id;
        }
        TagKind::Size => {
            if let Ok(n) = text.parse() {
                item.size.get_or_insert(n);
            }
        }
        TagKind::Category => {
            if let Ok(n) = text.parse() {
                item.categories.push(n);
            }
        }
        TagKind::Description => item.description = Some(text),
        TagKind::PubDate => item.pub_date = Some(text),
    }
}

/// Decode an attribute value (`Cow<[u8]>` raw bytes from quick-xml) into
/// an owned `String` with XML entities unescaped. Returns `None` on
/// malformed UTF-8; entity-unescape failures fall through to the raw
/// decoded string (Torznab attrs almost never carry escapable chars).
fn attr_value(attr: &Attribute) -> Option<String> {
    let raw = std::str::from_utf8(&attr.value).ok()?;
    Some(xml_unescape(raw).map_or_else(|_| raw.to_string(), std::borrow::Cow::into_owned))
}

fn text_value(t: &BytesText) -> Option<String> {
    let decoded = t.decode().ok()?;
    Some(xml_unescape(&decoded).map_or_else(
        |_| decoded.clone().into_owned(),
        std::borrow::Cow::into_owned,
    ))
}

/// Best-effort RFC 2822 parse — Torznab `<pubDate>` follows RSS, but
/// indexers sometimes drift (missing weekday, `GMT` instead of `+0000`,
/// etc.). Returning `None` is benign; the UI just falls back to
/// "unknown date".
fn parse_rfc2822_lenient(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc2822(s)
        .ok()
        .map(|dt| dt.with_timezone(&chrono::Utc))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:torznab="http://torznab.com/schemas/2015/feed">
  <channel>
    <torznab:response offset="0" total="2" />
    <item>
      <title>Avatar 2009 1080p BluRay</title>
      <guid isPermaLink="false">12345</guid>
      <link>https://indexer.example/torrent/12345.torrent?apikey=KEY</link>
      <pubDate>Mon, 14 May 2026 12:00:00 +0000</pubDate>
      <category>2040</category>
      <size>11271821344</size>
      <enclosure url="https://indexer.example/torrent/12345.torrent?apikey=KEY"
                 length="11271821344" type="application/x-bittorrent" />
      <torznab:attr name="category" value="2040" />
      <torznab:attr name="seeders" value="278" />
      <torznab:attr name="leechers" value="1" />
      <torznab:attr name="infohash" value="98259BA623EEC5F33167C083B51B30122C7FA068" />
      <torznab:attr name="tmdbid" value="19995" />
      <torznab:attr name="downloadvolumefactor" value="0" />
    </item>
    <item>
      <title>Some.Show.S01.1080p</title>
      <guid isPermaLink="false">67890</guid>
      <link>https://indexer.example/torrent/67890.torrent?apikey=KEY</link>
      <category>5040</category>
      <size>5000000000</size>
      <torznab:attr name="category" value="5040" />
      <torznab:attr name="seeders" value="42" />
      <torznab:attr name="leechers" value="3" />
    </item>
  </channel>
</rss>"#;

    #[test]
    fn parses_sample_feed() {
        let p = parse_torznab_xml(SAMPLE).expect("parse");
        assert_eq!(p.total, Some(2));
        assert_eq!(p.items.len(), 2);

        let first = &p.items[0];
        assert_eq!(first.external_id, "12345");
        assert_eq!(first.title, "Avatar 2009 1080p BluRay");
        assert_eq!(first.size, Some(11_271_821_344));
        assert_eq!(first.seeders, Some(278));
        assert_eq!(first.leechers, Some(1));
        assert_eq!(
            first.infohash.as_deref(),
            Some("98259ba623eec5f33167c083b51b30122c7fa068"),
        );
        assert_eq!(first.tmdb_id, Some(19995));
        assert!(first.freeleech);
        assert_eq!(first.categories.first().copied(), Some(2040));
        assert_eq!(
            first.download_url.as_deref(),
            Some("https://indexer.example/torrent/12345.torrent?apikey=KEY"),
        );

        let sr = p.items[0].clone_for_test().into_search_result("c411");
        assert_eq!(sr.kind, Some(MediaKind::Movie));
        assert_eq!(sr.year, Some(2009));
        let sr2 = p.items[1].clone_for_test().into_search_result("c411");
        assert_eq!(sr2.kind, Some(MediaKind::Tv));
    }

    impl RawItem {
        fn clone_for_test(&self) -> Self {
            Self {
                title: self.title.clone(),
                external_id: self.external_id.clone(),
                download_url: self.download_url.clone(),
                download_url_from_enclosure: self.download_url_from_enclosure,
                size: self.size,
                seeders: self.seeders,
                leechers: self.leechers,
                infohash: self.infohash.clone(),
                tmdb_id: self.tmdb_id,
                categories: self.categories.clone(),
                freeleech: self.freeleech,
                description: self.description.clone(),
                pub_date: self.pub_date.clone(),
            }
        }
    }

    /// Some indexers (c411) put the HTML detail page in `<link>` and
    /// only the real download URL in `<enclosure>`. The enclosure must
    /// win regardless of element order.
    #[test]
    fn enclosure_wins_over_link() {
        // <link> first, <enclosure> after (RSS-typical order)
        let link_then_enc = r#"<?xml version="1.0"?><rss><channel><item>
            <title>X</title>
            <guid>1</guid>
            <link>https://site/torrent/1/details.html</link>
            <enclosure url="https://site/torrent/1.torrent?apikey=K" length="0" type="application/x-bittorrent" />
        </item></channel></rss>"#;
        let p = parse_torznab_xml(link_then_enc).expect("parse");
        assert_eq!(
            p.items[0].download_url.as_deref(),
            Some("https://site/torrent/1.torrent?apikey=K"),
        );

        // <enclosure> first, <link> after — enclosure still wins.
        let enc_then_link = r#"<?xml version="1.0"?><rss><channel><item>
            <title>X</title>
            <guid>1</guid>
            <enclosure url="https://site/torrent/1.torrent?apikey=K" length="0" type="application/x-bittorrent" />
            <link>https://site/torrent/1/details.html</link>
        </item></channel></rss>"#;
        let p = parse_torznab_xml(enc_then_link).expect("parse");
        assert_eq!(
            p.items[0].download_url.as_deref(),
            Some("https://site/torrent/1.torrent?apikey=K"),
        );

        // Only <link>, no <enclosure> → fallback to link.
        let link_only = r#"<?xml version="1.0"?><rss><channel><item>
            <title>X</title>
            <guid>1</guid>
            <link>https://site/torrent/1.torrent?apikey=K</link>
        </item></channel></rss>"#;
        let p = parse_torznab_xml(link_only).expect("parse");
        assert_eq!(
            p.items[0].download_url.as_deref(),
            Some("https://site/torrent/1.torrent?apikey=K"),
        );
    }

    #[test]
    fn link_cache_evicts_fifo() {
        let mut c = LinkCache::new();
        for i in 0..(LINK_CACHE_CAP + 10) {
            c.put(format!("k{i}"), format!("v{i}"));
        }
        assert!(c.get("k0").is_none(), "oldest should be evicted");
        assert!(c.get(&format!("k{}", LINK_CACHE_CAP + 9)).is_some());
        assert_eq!(c.map.len(), LINK_CACHE_CAP);
    }
}
