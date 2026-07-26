//! HD-Torrents — English private HD tracker (hd-torrents.org / hdts.ru).
//!
//! The site exposes **no API at all** (no Torznab, no JSON): auth is a
//! session cookie obtained by POSTing `uid`/`pwd` to `login.php`, and
//! search results are scraped out of the `torrents.php` HTML table.
//! Selectors and quirks mirror Prowlarr's `HDTorrents.cs` indexer
//! definition, the de-facto reference for this site's markup.
//!
//! Quirks worth knowing before touching the parser:
//! * The upload date lives in **unquoted attribute names** — the cell
//!   renders as `<a 25 Jul 2026 18:32:10 href=…>`, so html5ever sees four
//!   value-less attributes whose *names* are the date parts. This is why
//!   the workspace pins scraper's `deterministic` feature (ordered
//!   attribute maps).
//! * Moderators get extra trailing action columns ("Edit", …), shifting
//!   the seeders/leechers/grabs indices — handled by the `end index`
//!   adjustment, same as Prowlarr.
//! * Search treats `.` as a hard separator and flags parentheses as
//!   "hacking" attempts; dots are rewritten to spaces and the query is
//!   fully form-encoded (parens become `%28`/`%29`).
//!
//! Config:
//! ```toml
//! [[providers]]
//! id = "hdt"
//! kind = "hdtorrents"
//! enabled = true
//! base_url = "https://hd-torrents.org"
//! username_env = "HDT_USERNAME"
//! password_env = "HDT_PASSWORD"
//! default_language = "english"
//! ```

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use iris_config::ProviderEntry;
use iris_core::Error;
use iris_core::Result;
use iris_core::search::{
    MediaKind, ProviderCapabilities, ProviderPage, SearchQuery, SearchResult, TorrentSource,
};
use reqwest::Client;
use reqwest::header::{HeaderMap, HeaderValue, REFERER, USER_AGENT};
use scraper::{ElementRef, Html, Selector};
use tokio::sync::Mutex;
use url::Url;

use crate::SearchProvider;
use crate::util::{extract_year, field_or_env, field_str};

const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:150.0) Gecko/20100101 Firefox/150.0";

/// First byte of a valid `.torrent` file (bencoded dictionary).
const BENCODE_DICT_MARKER: u8 = b'd';

/// Body marker of an expired/absent session — the site answers 200 with
/// this string instead of a 401 (Prowlarr's `CheckIfLoginNeeded`).
const NOT_AUTHORIZED_MARKER: &str = "Error:You're not authorized";

/// Body marker of a *successful* login response (the post-login meta
/// refresh page). Its absence means we're still on the login form.
const LOGIN_OK_MARKER: &str = "if your browser doesn't have javascript enabled";

/// Same cap as the Torznab / UNIT3D link caches.
const LINK_CACHE_CAP: usize = 4096;

/// Movie category ids (UHD Blu-ray, Blu-ray, UHD Remux, Remux, 1080p/i,
/// 720p, 2160p). Deliberately excludes 63 "Movie/Audio Track".
const MOVIE_CATS: [u32; 7] = [70, 1, 71, 2, 5, 3, 64];
/// TV category ids (same quality ladder as movies).
const TV_CATS: [u32; 7] = [72, 59, 73, 60, 30, 38, 65];
/// `MOVIE_CATS` ++ `TV_CATS`, for un-filtered searches.
const ALL_CATS: [u32; 14] = [70, 1, 71, 2, 5, 3, 64, 72, 59, 73, 60, 30, 38, 65];

fn category_label(id: u32) -> Option<&'static str> {
    Some(match id {
        70 => "Movie/UHD/Blu-Ray",
        1 => "Movie/Blu-Ray",
        71 => "Movie/UHD/Remux",
        2 => "Movie/Remux",
        5 => "Movie/1080p/i",
        3 => "Movie/720p",
        64 => "Movie/2160p",
        63 => "Movie/Audio Track",
        72 => "TV Show/UHD/Blu-ray",
        59 => "TV Show/Blu-ray",
        73 => "TV Show/UHD/Remux",
        60 => "TV Show/Remux",
        30 => "TV Show/1080p/i",
        38 => "TV Show/720p",
        65 => "TV Show/2160p",
        44 => "Music/Album",
        61 => "Music/Blu-Ray",
        62 => "Music/Remux",
        57 => "Music/1080p/i",
        45 => "Music/720p",
        66 => "Music/2160p",
        58 => "XXX/Blu-ray",
        74 => "XXX/UHD/Blu-ray",
        48 => "XXX/1080p/i",
        47 => "XXX/720p",
        67 => "XXX/2160p",
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

pub struct HdTorrents {
    id: String,
    base_url: Url,
    username: String,
    password: String,
    /// Cookie-jar client — the session IS the jar. One login primes it;
    /// every subsequent request rides the stored cookies.
    http: Client,
    /// `true` once a login round-trip succeeded. Held in a Mutex so
    /// concurrent searches single-flight the (re)login instead of
    /// hammering `login.php` in parallel.
    logged_in: Mutex<bool>,
    /// Torrent id -> absolute `download.php` URL captured from search
    /// rows (carries the `f=<name>.torrent` filename parameter).
    link_cache: Mutex<LinkCache>,
}

impl HdTorrents {
    pub fn from_config(entry: &ProviderEntry) -> Result<Arc<Self>> {
        let base_url_str = field_str(entry, "base_url")?;
        let mut base_url = Url::parse(base_url_str)
            .map_err(|e| Error::Provider(format!("hdtorrents base_url invalid: {e}")))?;
        // Relative joins below assume a trailing slash ("login.php" vs
        // replacing the last path segment).
        if !base_url.path().ends_with('/') {
            base_url.set_path(&format!("{}/", base_url.path()));
        }
        let username = field_or_env(entry, "username")?;
        let password = field_or_env(entry, "password")?;

        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));

        let http = crate::tls::client_builder()
            .default_headers(headers)
            .cookie_store(true)
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| Error::Provider(format!("hdtorrents http client: {e}")))?;

        Ok(Arc::new(Self {
            id: entry.id.clone(),
            base_url,
            username,
            password,
            http,
            logged_in: Mutex::new(false),
            link_cache: Mutex::new(LinkCache::new()),
        }))
    }

    async fn login(&self) -> Result<()> {
        let url = self
            .base_url
            .join("login.php")
            .map_err(|e| Error::Provider(format!("hdtorrents join login url: {e}")))?;

        let res = self
            .http
            .post(url.clone())
            .header(REFERER, url.as_str())
            .form(&[("uid", self.username.as_str()), ("pwd", &self.password)])
            .send()
            .await
            .map_err(|e| Error::Provider(format!("hdtorrents login: {e}")))?;

        let status = res.status();
        let body = res
            .text()
            .await
            .map_err(|e| Error::Provider(format!("hdtorrents login body: {e}")))?;

        if body.to_ascii_lowercase().contains(LOGIN_OK_MARKER) {
            tracing::debug!(provider = %self.id, "hdtorrents login succeeded");
            return Ok(());
        }
        let reason = extract_login_error(&body)
            .unwrap_or_else(|| format!("no success marker in response (HTTP {status})"));
        Err(Error::Provider(format!(
            "hdtorrents login failed: {reason}"
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

    /// Authenticated GET returning the response body, re-logging in and
    /// retrying once when the session-expired marker shows up.
    async fn authed_get_text(&self, url: Url) -> Result<String> {
        let mut attempt = 0u8;
        loop {
            self.ensure_login().await?;
            let res = self
                .http
                .get(url.clone())
                .send()
                .await
                .map_err(|e| Error::Provider(format!("hdtorrents request: {e}")))?;
            if !res.status().is_success() {
                let status = res.status();
                return Err(Error::Provider(format!(
                    "hdtorrents request failed: HTTP {status}"
                )));
            }
            let body = res
                .text()
                .await
                .map_err(|e| Error::Provider(format!("hdtorrents body: {e}")))?;
            if body.contains(NOT_AUTHORIZED_MARKER) && attempt == 0 {
                attempt += 1;
                self.invalidate_session().await;
                continue;
            }
            if body.contains(NOT_AUTHORIZED_MARKER) {
                return Err(Error::Provider(
                    "hdtorrents session rejected after re-login".into(),
                ));
            }
            return Ok(body);
        }
    }

    /// Authenticated GET returning raw bytes (torrent downloads). An HTML
    /// body starting the not-authorized dance triggers one re-login retry.
    async fn authed_get_bytes(&self, url: Url) -> Result<bytes::Bytes> {
        let mut attempt = 0u8;
        loop {
            self.ensure_login().await?;
            let res = self
                .http
                .get(url.clone())
                .send()
                .await
                .map_err(|e| Error::Provider(format!("hdtorrents download: {e}")))?;
            if !res.status().is_success() {
                let status = res.status();
                return Err(Error::Provider(format!(
                    "hdtorrents download failed: HTTP {status}"
                )));
            }
            let bytes = res
                .bytes()
                .await
                .map_err(|e| Error::Provider(format!("hdtorrents download body: {e}")))?;
            let looks_expired = bytes.first().copied() != Some(BENCODE_DICT_MARKER)
                && String::from_utf8_lossy(&bytes).contains(NOT_AUTHORIZED_MARKER);
            if looks_expired && attempt == 0 {
                attempt += 1;
                self.invalidate_session().await;
                continue;
            }
            return Ok(bytes);
        }
    }

    fn search_url(&self, q: &SearchQuery) -> Result<Url> {
        let mut url = self
            .base_url
            .join("torrents.php")
            .map_err(|e| Error::Provider(format!("hdtorrents join search url: {e}")))?;
        let cats: &[u32] = match q.kind {
            Some(MediaKind::Movie) => &MOVIE_CATS,
            Some(MediaKind::Tv) => &TV_CATS,
            None => &ALL_CATS,
        };
        {
            let mut qp = url.query_pairs_mut();
            for c in cats {
                qp.append_pair("category[]", &c.to_string());
            }
            qp.append_pair("search", &build_search_term(q));
            qp.append_pair("active", "0");
            qp.append_pair("options", "0");
            // torrents.php pages are 0-based. Page 1 needs no parameter.
            let page = q.page.unwrap_or(1).max(1);
            if page > 1 {
                qp.append_pair("page", &(page - 1).to_string());
            }
        }
        Ok(url)
    }
}

#[async_trait]
impl SearchProvider for HdTorrents {
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
        let url = self.search_url(q)?;
        let body = self.authed_get_text(url).await?;
        let results = parse_search_page(&self.id, &self.base_url, &body);

        {
            let mut cache = self.link_cache.lock().await;
            for r in &results {
                if let Some(dl) = &r.download_url {
                    cache.put(r.external_id.clone(), dl.clone());
                }
            }
        }

        let count = u32::try_from(results.len()).unwrap_or(u32::MAX);
        Ok(ProviderPage {
            results,
            current_page: q.page.unwrap_or(1).max(1),
            // The site has a fixed server-side page size it never
            // announces; report what this page actually carried.
            limit: count.max(1),
            total_count: None,
            total_pages: None,
        })
    }

    async fn resolve(&self, external_id: &str) -> Result<TorrentSource> {
        if external_id.is_empty() || !external_id.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(Error::InvalidInput(format!(
                "hdtorrents external_id must be alphanumeric, got `{external_id}`"
            )));
        }
        // Prefer the URL captured during search (carries the `f=` filename
        // parameter); fall back to the canonical reconstruction so grabs
        // survive a process restart that wiped the cache.
        let url_str = match self.link_cache.lock().await.get(external_id) {
            Some(u) => u,
            None => self
                .base_url
                .join(&format!("download.php?id={external_id}"))
                .map_err(|e| Error::Provider(format!("hdtorrents join download url: {e}")))?
                .to_string(),
        };
        let url = Url::parse(&url_str)
            .map_err(|e| Error::Provider(format!("hdtorrents download url invalid: {e}")))?;

        let bytes = self.authed_get_bytes(url).await?;
        if bytes.first().copied() != Some(BENCODE_DICT_MARKER) {
            let preview = String::from_utf8_lossy(&bytes[..bytes.len().min(200)]).into_owned();
            tracing::warn!(
                provider = %self.id,
                external_id,
                body_preview = %preview,
                "hdtorrents download returned non-bencoded body",
            );
            return Err(Error::Provider(format!(
                "hdtorrents download for `{external_id}` returned non-bencoded body \
                 (first byte: {:?})",
                bytes.first()
            )));
        }
        Ok(TorrentSource::TorrentFile(bytes.to_vec()))
    }

    /// Downloads need the session cookie — the default plain-GET
    /// implementation would hit the login wall.
    async fn fetch_bytes(&self, url: &str) -> Result<bytes::Bytes> {
        let url =
            Url::parse(url).map_err(|e| Error::Provider(format!("hdtorrents fetch_bytes: {e}")))?;
        self.authed_get_bytes(url).await
    }
}

/// The site's search box treats `.` as a separator and rejects raw
/// parentheses ("hacking" detection — the form encoder takes care of
/// those). SCENE-parsed title + S/E is rebuilt like the UNIT3D filter.
fn build_search_term(q: &SearchQuery) -> String {
    let base = match q.parsed_title.as_deref() {
        Some(t) if !t.is_empty() => match (q.season, q.episode) {
            (Some(s), Some(e)) if e > 0 => format!("{t} S{s:02}E{e:02}"),
            (Some(s), _) => format!("{t} S{s:02}"),
            _ => q.q.clone(),
        },
        _ => q.q.clone(),
    };
    base.replace('.', " ")
}

/// Login failures render as `<div><font color="#FF0000">reason</font></div>`.
fn extract_login_error(html: &str) -> Option<String> {
    let doc = Html::parse_document(html);
    let sel = Selector::parse("div > font[color]").expect("static selector");
    doc.select(&sel)
        .find(|f| {
            f.value()
                .attr("color")
                .is_some_and(|c| c.eq_ignore_ascii_case("#ff0000"))
        })
        .map(|f| f.text().collect::<String>().trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Prowlarr's `ParseUtil.GetBytes` equivalent: `9.14 GiB` / `700 MB` →
/// bytes, binary multipliers for both `XiB` and `XB` spellings.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn parse_size(text: &str) -> Option<u64> {
    let t = text.trim();
    let split = t.find(|c: char| c.is_ascii_alphabetic())?;
    let num: f64 = t[..split].trim().replace(',', "").parse().ok()?;
    let mult: f64 = match t[split..].trim().to_ascii_lowercase().as_str() {
        "b" => 1.0,
        "kb" | "kib" => 1024.0,
        "mb" | "mib" => 1024.0 * 1024.0,
        "gb" | "gib" => 1024.0 * 1024.0 * 1024.0,
        "tb" | "tib" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    if num < 0.0 {
        return None;
    }
    Some((num * mult).round() as u64)
}

fn parse_count(text: &str) -> Option<u32> {
    text.trim().replace(',', "").parse().ok()
}

/// The upload date is spread across the first four *attribute names* of
/// the date cell's first element (unquoted-attribute artifact, see module
/// docs). html5ever lowercases names; chrono's `%b` matches month names
/// case-insensitively.
fn parse_date_cell(el: ElementRef<'_>) -> Option<chrono::DateTime<chrono::Utc>> {
    let joined = el
        .value()
        .attrs()
        .map(|(name, _)| name)
        .take(4)
        .collect::<Vec<_>>()
        .join(" ");
    chrono::NaiveDateTime::parse_from_str(&joined, "%d %b %Y %H:%M:%S")
        .ok()
        .map(|n| n.and_utc())
}

fn query_param(url: &Url, key: &str) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

/// Scrape one `torrents.php` results page. Rows that don't fit the
/// expected shape are skipped (never a hard error — a cosmetic site
/// tweak shouldn't blank the whole provider).
fn parse_search_page(provider_id: &str, base_url: &Url, html: &str) -> Vec<SearchResult> {
    let doc = Html::parse_document(html);
    let row_sel = Selector::parse("table.mainblockcontenttt tr").expect("static selector");
    let a_sel = Selector::parse("a").expect("static selector");
    let img_sel = Selector::parse("img").expect("static selector");

    let mut out = Vec::new();
    let rows = doc.select(&row_sel).filter(|row| {
        row.child_elements().any(|td| {
            td.value().name() == "td"
                && td
                    .value()
                    .classes()
                    .any(|c| c.eq_ignore_ascii_case("mainblockcontent"))
        })
    });
    // First matching row is the column-header row.
    for row in rows.skip(1) {
        // Index over ALL element children (Prowlarr does the same) — the
        // class filter above only gates which rows qualify.
        let tds: Vec<ElementRef<'_>> = row.child_elements().collect();
        if tds.len() < 9 {
            continue;
        }

        let Some(main_link) = tds[2].select(&a_sel).next() else {
            continue;
        };
        let title = main_link.text().collect::<String>().trim().to_string();
        if title.is_empty() {
            continue;
        }
        let external_id = main_link
            .value()
            .attr("href")
            .and_then(|href| base_url.join(href).ok())
            .and_then(|u| query_param(&u, "id"));
        let Some(external_id) = external_id else {
            continue;
        };
        // The site keys everything on the torrent's infohash: details.php,
        // download.php and peers.php all take the 40-hex hash as `id`.
        // Surfacing it enables the infohash-only "In library" matching.
        let infohash = (external_id.len() == 40
            && external_id.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| external_id.to_ascii_lowercase());

        let download_url = tds[4]
            .select(&a_sel)
            .next()
            .and_then(|a| a.value().attr("href"))
            .and_then(|href| base_url.join(href).ok())
            .map(String::from);

        let category_id = tds[0]
            .select(&a_sel)
            .next()
            .and_then(|a| a.value().attr("href"))
            .and_then(|href| base_url.join(href).ok())
            .and_then(|u| query_param(&u, "category"))
            .and_then(|c| c.parse::<u32>().ok());

        let uploaded_at = tds[6].child_elements().next().and_then(parse_date_cell);
        let size_bytes = parse_size(&tds[7].text().collect::<String>());

        // Moderators see trailing action links appended to every row —
        // one ("Edit") or four ("delete / recommend / like / Edit").
        let mut end = tds.len();
        let cell_text =
            |i: usize| -> String { tds[i].text().collect::<String>().trim().to_string() };
        if cell_text(end - 1) == "Edit" {
            end -= 1;
        } else if end >= 4 && cell_text(end - 4) == "Edit" {
            end -= 4;
        }
        let seeders = (end >= 3)
            .then(|| parse_count(&cell_text(end - 3)))
            .flatten();
        let leechers = (end >= 2)
            .then(|| parse_count(&cell_text(end - 2)))
            .flatten();

        // Freeleech = download counts for nothing: the golden "free"
        // torrents and the ratio-less ones. Partial discounts (25/50/75%)
        // still cost ratio, so they don't qualify.
        let freeleech = row.select(&img_sel).any(|img| {
            img.value()
                .attr("src")
                .is_some_and(|s| s.ends_with("free.png") || s.ends_with("no_ratio.png"))
        });

        out.push(SearchResult {
            provider_id: provider_id.to_string(),
            external_id,
            year: extract_year(&title),
            title,
            size_bytes,
            seeders,
            leechers,
            infohash,
            magnet: None,
            category: category_id.and_then(category_label).map(String::from),
            tags: Vec::new(),
            freeleech,
            uploader: None,
            uploaded_at,
            tmdb_id: None,
            kind: category_id.and_then(category_kind),
            poster_url: None,
            already_in_library: false,
            library_infohash: None,
            library_file_idx: None,
            language: None,
            codec: None,
            download_url,
            parsed_season: None,
            parsed_episode: None,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One well-formed member-view row + the header row, shaped after the
    /// markup Prowlarr's selectors expect (12 columns: category, icon,
    /// title, comments, download, misc, date, size, uploader, seeders,
    /// leechers, grabs). The date really is unquoted attributes.
    const FIXTURE: &str = r##"<html><body>
<table class="navus"><tr><td>hello</td><td>Rank: Member</td></tr></table>
<table class="mainblockcontenttt" width="100%">
 <tr>
  <td class="mainblockcontent">Type</td><td class="mainblockcontent">Pic</td>
  <td class="mainblockcontent">Filename</td><td class="mainblockcontent">C</td>
  <td class="mainblockcontent">DL</td><td class="mainblockcontent">M</td>
  <td class="mainblockcontent">Added</td><td class="mainblockcontent">Size</td>
  <td class="mainblockcontent">Uploader</td><td class="mainblockcontent">S</td>
  <td class="mainblockcontent">L</td><td class="mainblockcontent">D</td>
 </tr>
 <tr>
  <td class="mainblockcontent"><a href="torrents.php?category=64"><img src="cat.png"></a></td>
  <td class="mainblockcontent"><img src="pic.gif"></td>
  <td class="mainblockcontent"><a href="details.php?id=0123456789abcdef0123456789abcdef01234567&amp;hit=1">The.Movie.2023.2160p.UHD.BluRay.x265-GRP</a><br><span>Nice rip</span></td>
  <td class="mainblockcontent">0</td>
  <td class="mainblockcontent"><a href="download.php?id=0123456789abcdef0123456789abcdef01234567&amp;f=The.Movie.torrent"><img src="dl.png"></a></td>
  <td class="mainblockcontent"><img src="free.png"></td>
  <td class="mainblockcontent"><a 25 Jul 2026 18:32:10 href="#">25/07/26</a></td>
  <td class="mainblockcontent">9.14 GiB</td>
  <td class="mainblockcontent">uploader1</td>
  <td class="mainblockcontent">12</td>
  <td class="mainblockcontent">3</td>
  <td class="mainblockcontent">45</td>
 </tr>
</table></body></html>"##;

    fn base() -> Url {
        Url::parse("https://hd-torrents.org/").expect("url")
    }

    #[test]
    fn parses_member_row() {
        let results = parse_search_page("hdt", &base(), FIXTURE);
        assert_eq!(results.len(), 1);
        let r = &results[0];
        assert_eq!(r.provider_id, "hdt");
        assert_eq!(r.external_id, "0123456789abcdef0123456789abcdef01234567");
        // The site's torrent id IS the infohash — must surface it.
        assert_eq!(
            r.infohash.as_deref(),
            Some("0123456789abcdef0123456789abcdef01234567"),
        );
        assert_eq!(r.title, "The.Movie.2023.2160p.UHD.BluRay.x265-GRP");
        assert_eq!(r.year, Some(2023));
        assert_eq!(r.size_bytes, Some(9_814_000_271));
        assert_eq!(r.seeders, Some(12));
        assert_eq!(r.leechers, Some(3));
        assert_eq!(r.kind, Some(MediaKind::Movie));
        assert_eq!(r.category.as_deref(), Some("Movie/2160p"));
        assert!(r.freeleech);
        assert_eq!(
            r.download_url.as_deref(),
            Some(
                "https://hd-torrents.org/download.php?id=0123456789abcdef0123456789abcdef01234567&f=The.Movie.torrent"
            ),
        );
        let dt = r.uploaded_at.expect("date parsed from attribute names");
        assert_eq!(dt.to_rfc3339(), "2026-07-25T18:32:10+00:00");
    }

    /// Moderator rows carry a trailing "Edit" cell that would otherwise
    /// shift seeders/leechers by one.
    #[test]
    fn moderator_edit_column_shifts_counts() {
        let html = FIXTURE.replace(
            "<td class=\"mainblockcontent\">45</td>\n </tr>\n</table>",
            "<td class=\"mainblockcontent\">45</td>\n  <td class=\"mainblockcontent\"><a href=\"#\">Edit</a></td>\n </tr>\n</table>",
        );
        let results = parse_search_page("hdt", &base(), &html);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].seeders, Some(12));
        assert_eq!(results[0].leechers, Some(3));
    }

    #[test]
    fn tv_category_maps_to_tv_kind() {
        let html = FIXTURE.replace("category=64", "category=65");
        let results = parse_search_page("hdt", &base(), &html);
        assert_eq!(results[0].kind, Some(MediaKind::Tv));
        assert_eq!(results[0].category.as_deref(), Some("TV Show/2160p"));
    }

    #[test]
    fn skips_malformed_rows_without_erroring() {
        let html = FIXTURE.replace(
            "details.php?id=0123456789abcdef0123456789abcdef01234567&amp;hit=1",
            "nowhere.php",
        );
        let results = parse_search_page("hdt", &base(), &html);
        assert!(results.is_empty());
    }

    #[test]
    fn size_units() {
        assert_eq!(parse_size("700 MiB"), Some(734_003_200));
        assert_eq!(parse_size("700 MB"), Some(734_003_200));
        assert_eq!(parse_size("1.5 GiB"), Some(1_610_612_736));
        assert_eq!(parse_size("2 TiB"), Some(2_199_023_255_552));
        assert_eq!(parse_size("1,024 KiB"), Some(1_048_576));
        assert_eq!(parse_size("12"), None);
        assert_eq!(parse_size("big"), None);
    }

    #[test]
    fn search_term_rebuilds_scene_form_and_strips_dots() {
        let q = SearchQuery {
            q: "Classroom.of.the.Elite S04E11 1080p".into(),
            page: None,
            limit: None,
            sort_by: None,
            order: None,
            kind: None,
            parsed_title: Some("Classroom of the Elite".into()),
            season: Some(4),
            episode: Some(11),
            year: None,
        };
        assert_eq!(build_search_term(&q), "Classroom of the Elite S04E11");

        let raw = SearchQuery {
            q: "The.Movie.2023".into(),
            parsed_title: None,
            season: None,
            episode: None,
            page: None,
            limit: None,
            sort_by: None,
            order: None,
            kind: None,
            year: None,
        };
        assert_eq!(build_search_term(&raw), "The Movie 2023");
    }

    /// Scratch harness: run the parser against a saved real page.
    /// `HDT_FIXTURE=/path/to/search.html cargo test -- --ignored debug_real --nocapture`
    #[test]
    #[ignore = "needs a locally saved torrents.php page"]
    fn debug_real_page() {
        let path = std::env::var("HDT_FIXTURE").expect("set HDT_FIXTURE");
        let html = std::fs::read_to_string(path).expect("read fixture");
        let results = parse_search_page("hdt", &base(), &html);
        println!("parsed {} results", results.len());
        for r in results.iter().take(5) {
            println!(
                "id={} s={:?} l={:?} size={:?} date={:?} cat={:?} kind={:?} fl={} title={}",
                r.external_id,
                r.seeders,
                r.leechers,
                r.size_bytes,
                r.uploaded_at,
                r.category,
                r.kind,
                r.freeleech,
                r.title
            );
        }
    }

    /// Scratch harness: the full login + search + parse path against the
    /// live site, i.e. exactly what the app does.
    /// `HDT_USERNAME=… HDT_PASSWORD=… cargo test -- --ignored debug_live --nocapture`
    #[tokio::test]
    #[ignore = "hits the live site; needs HDT_USERNAME/HDT_PASSWORD"]
    async fn debug_live_search() {
        let mut fields = std::collections::HashMap::new();
        fields.insert(
            "base_url".to_string(),
            toml::Value::String("https://hd-torrents.org".into()),
        );
        fields.insert(
            "username_env".to_string(),
            toml::Value::String("HDT_USERNAME".into()),
        );
        fields.insert(
            "password_env".to_string(),
            toml::Value::String("HDT_PASSWORD".into()),
        );
        let entry = ProviderEntry {
            id: "hdt".into(),
            kind: "hdtorrents".into(),
            enabled: true,
            fields,
        };
        let p = HdTorrents::from_config(&entry).expect("construct");
        let page = p
            .search(&SearchQuery {
                q: std::env::var("HDT_QUERY").unwrap_or_else(|_| "dune".into()),
                page: None,
                limit: None,
                sort_by: None,
                order: None,
                kind: None,
                parsed_title: None,
                season: None,
                episode: None,
                year: None,
            })
            .await
            .expect("live search");
        println!("live: {} results", page.results.len());
        for r in page.results.iter().take(5) {
            println!("s={:?} l={:?} title={}", r.seeders, r.leechers, r.title);
        }
    }

    #[test]
    fn login_error_extraction() {
        let html = r##"<html><body><div><font color="#FF0000">Wrong password!</font></div></body></html>"##;
        assert_eq!(
            extract_login_error(html).as_deref(),
            Some("Wrong password!")
        );
        assert_eq!(extract_login_error("<html><body>ok</body></html>"), None);
    }
}
