//! Generic `UNIT3D` provider — works against any indexer running the
//! mainline [`UNIT3D`](https://github.com/HDInnovations/UNIT3D)
//! codebase or a close fork (`TheOldSchool`, `BLU`, `Aither`,
//! `AvistaZ`, …).
//!
//! `UNIT3D` ships a JSON:API-style endpoint at `/api/torrents` that
//! returns paginated search results with the `.torrent` download URL
//! pre-signed for the authenticated user (rsskey embedded in the
//! path). We never have to construct download URLs ourselves — the
//! link in the search response is the canonical one.
//!
//! NOT Torznab — UNIT3D explicitly doesn't implement it. The
//! `kind = "torznab"` provider in this crate WILL NOT work against
//! UNIT3D trackers; use `kind = "unit3d"` instead.
//!
//! Config:
//! ```toml
//! [[providers]]
//! id = "tos"
//! kind = "unit3d"
//! enabled = true
//! base_url = "https://theoldschool.cc"
//! api_key_env = "TOS_API_KEY"
//! # optional:
//! # api_path = "/api"
//! # user_agent = "…"
//! ```

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use iris_config::ProviderEntry;
use iris_core::Error;
use iris_core::Result;
use iris_core::search::{
    DescriptionFormat, MediaKind, ProviderCapabilities, ProviderPage, SearchQuery, SearchResult,
    TorrentDetails, TorrentSource,
};
use reqwest::Client;
use reqwest::header::{ACCEPT, ACCEPT_LANGUAGE, AUTHORIZATION, HeaderMap, HeaderValue, USER_AGENT};
use serde::{Deserialize, Deserializer};
use tokio::sync::Mutex;
use url::Url;

use crate::SearchProvider;
use crate::nfo;
use crate::util::{extract_year, field_or_env, field_str};

const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:150.0) Gecko/20100101 Firefox/150.0";
const DEFAULT_API_PATH: &str = "/api";
/// First byte of a valid `.torrent` file (bencoded dictionary).
const BENCODE_DICT_MARKER: u8 = b'd';
/// FIFO cap on the `(external_id -> download_link)` map — see
/// `LinkCache` for the eviction logic.
const LINK_CACHE_CAP: usize = 4096;

pub struct Unit3dProvider {
    id: String,
    base_url: Url,
    api_path: String,
    api_token: String,
    /// Numeric `category_id` filter for movies (mainline `UNIT3D`
    /// default is `1`). Override per provider in `providers.toml`
    /// when a fork reorders the categories.
    movie_category_id: u32,
    /// Same for TV. Mainline default is `2`. theoldschool exposes
    /// the same category as `"Series"` in the response `category`
    /// field but the underlying `category_id` is still `2`.
    tv_category_id: u32,
    http: Client,
    /// Torrent id (UNIT3D's numeric `id`) -> direct `.torrent` URL,
    /// captured from `attributes.download_link` in search responses.
    /// `resolve()` looks the URL up here and fetches the bytes.
    link_cache: Mutex<LinkCache>,
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

impl Unit3dProvider {
    pub fn from_config(entry: &ProviderEntry) -> Result<Arc<Self>> {
        let base_url_str = field_str(entry, "base_url")?;
        let base_url = Url::parse(base_url_str)
            .map_err(|e| Error::Provider(format!("unit3d base_url invalid: {e}")))?;
        // UNIT3D names the auth parameter `api_token` in their JSON
        // API; we keep the config field name `api_key` for symmetry
        // with the Torznab / c411 providers — the value is the same
        // shape (a long opaque string the user generates in their
        // account settings).
        let api_token = field_or_env(entry, "api_key")?;
        let api_path = entry
            .fields
            .get("api_path")
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_API_PATH)
            .to_string();
        let movie_category_id = entry
            .fields
            .get("movie_category_id")
            .and_then(toml::Value::as_integer)
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(1);
        let tv_category_id = entry
            .fields
            .get("tv_category_id")
            .and_then(toml::Value::as_integer)
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(2);
        let user_agent = entry
            .fields
            .get("user_agent")
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_USER_AGENT);

        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(user_agent)
                .map_err(|e| Error::Provider(format!("unit3d user_agent invalid: {e}")))?,
        );
        headers.insert(ACCEPT, HeaderValue::from_static("application/json, */*"));
        headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));
        // UNIT3D accepts the token as `?api_token=` or `Authorization:
        // Bearer`; newer deployments reject the query-param form (it
        // leaks tokens into server logs), so send the header on every
        // request. Older instances ignore it — both transports coexist.
        let mut bearer = HeaderValue::from_str(&format!("Bearer {api_token}"))
            .map_err(|e| Error::Provider(format!("unit3d api token invalid in header: {e}")))?;
        bearer.set_sensitive(true);
        headers.insert(AUTHORIZATION, bearer);

        let http = crate::tls::client_builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| Error::Provider(format!("unit3d http client: {e}")))?;

        Ok(Arc::new(Self {
            id: entry.id.clone(),
            base_url,
            api_path,
            api_token,
            movie_category_id,
            tv_category_id,
            http,
            link_cache: Mutex::new(LinkCache::new()),
        }))
    }

    /// `UNIT3D` exposes two list-shaped endpoints:
    ///   * `/api/torrents` — bare paginated list. Ignores filter
    ///     params silently; returns the 25 most recent torrents.
    ///   * `/api/torrents/filter` — the actual search/filter route.
    ///     Honours `name=`, `categories[]=`, `startYear` / `endYear`,
    ///     `perPage`, etc. per the official docs:
    ///     <https://hdinnovations.github.io/UNIT3D/torrent_api.html>
    ///
    /// We use the latter; hitting `/api/torrents` from a search path
    /// is what gave us "TV in Movies" and "exact-name query returns
    /// recent uploads instead of the match" before this fix.
    fn search_url(&self) -> Result<Url> {
        let path = format!("{}/torrents/filter", self.api_path.trim_end_matches('/'));
        self.base_url
            .join(&path)
            .map_err(|e| Error::Provider(format!("unit3d join search url: {e}")))
    }

    /// `/api/torrents/{id}` — one torrent, same attribute shape as a
    /// search item (returned BARE, no `data` wrapper — verified against
    /// theoldschool). `None` on 404. Shared by `details()` and
    /// `resolve()`'s stale-link recovery (the attributes carry a
    /// freshly-signed `download_link`).
    async fn fetch_envelope(&self, external_id: &str) -> Result<Option<TorrentEnvelope>> {
        let path = format!(
            "{}/torrents/{external_id}",
            self.api_path.trim_end_matches('/')
        );
        let url = self
            .base_url
            .join(&path)
            .map_err(|e| Error::Provider(format!("unit3d join details url: {e}")))?;
        let res = self
            .http
            .get(url)
            .query(&[("api_token", &self.api_token)])
            .send()
            .await
            .map_err(|e| Error::Provider(format!("unit3d details request: {e}")))?;
        if res.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(Error::Provider(format!(
                "unit3d details HTTP {status}: {}",
                body.chars().take(200).collect::<String>(),
            )));
        }
        let body = res
            .text()
            .await
            .map_err(|e| Error::Provider(format!("unit3d details body: {e}")))?;
        let envelope: TorrentEnvelope = serde_json::from_str(&body).map_err(|e| {
            tracing::warn!(
                provider = %self.id,
                external_id,
                error = %e,
                body_preview = %body.chars().take(300).collect::<String>(),
                "unit3d details decode failed",
            );
            Error::Provider(format!("unit3d details decode: {e}"))
        })?;
        Ok(Some(envelope))
    }

    /// A `.torrent` download the tracker answered but declined to serve.
    /// The account resolved (a bad rsskey redirects to `/login` instead), so
    /// this is policy, not credentials — and the message has to say so,
    /// because the raw shape reaching the user is an unexplained
    /// `401 Unauthorized` on a preview dialog that otherwise works.
    /// Seedpool's one-download-slot limit is the case that produced it.
    fn refused(provider: &str, status: reqwest::StatusCode) -> Error {
        Error::ProviderRefused(format!(
            "`{provider}` refused the .torrent download (HTTP {status}). The API key is \
             fine — search and details still work — so this is a tracker-side rule: \
             usually a download-slot limit (finish or remove the download in progress \
             first) or a restriction on the account.",
        ))
    }

    /// GET a pre-signed `download_link` and validate the body is a
    /// bencoded `.torrent`. A rejected rsskey link doesn't 4xx directly:
    /// `UNIT3D` 302s to the torrent's web page, which our JSON `Accept`
    /// turns into a Laravel 401 `Unauthenticated` — both shapes land in
    /// the HTTP-status arm below and read clearly in the logs.
    async fn download(&self, external_id: &str, url: &str) -> Result<TorrentSource> {
        let res = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| Error::Provider(format!("unit3d download: {e}")))?;
        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            tracing::warn!(
                provider = %self.id,
                external_id,
                %status,
                body_preview = %body.chars().take(200).collect::<String>(),
                "unit3d download refused by the tracker",
            );
            if matches!(
                status,
                reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
            ) {
                return Err(Self::refused(&self.id, status));
            }
            return Err(Error::Provider(format!(
                "unit3d `{}` download failed: HTTP {status} — {}",
                self.id,
                body.chars().take(200).collect::<String>(),
            )));
        }
        let bytes = res
            .bytes()
            .await
            .map_err(|e| Error::Provider(format!("unit3d download body: {e}")))?;
        if bytes.first().copied() != Some(BENCODE_DICT_MARKER) {
            let preview = String::from_utf8_lossy(&bytes[..bytes.len().min(200)]).into_owned();
            tracing::warn!(
                provider = %self.id,
                external_id,
                url = %url,
                first_byte = ?bytes.first(),
                body_preview = %preview,
                "unit3d download returned non-bencoded body",
            );
            return Err(Error::Provider(format!(
                "unit3d `{}` download for `{external_id}` returned non-bencoded body \
                 (first byte: {:?})",
                self.id,
                bytes.first()
            )));
        }
        Ok(TorrentSource::TorrentFile(bytes.to_vec()))
    }
}

#[async_trait]
impl SearchProvider for Unit3dProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            returns_magnet: false,
            returns_torrent_file: true,
            // Mainline `UNIT3D` does include `info_hash` on every
            // search item — confirmed against theoldschool. We pass
            // it through to `SearchResult.infohash` so ingestion can
            // dedupe + the player's follow flow has the canonical
            // identity without parsing the `.torrent` first.
            returns_infohash: true,
        }
    }

    async fn search(&self, q: &SearchQuery) -> Result<ProviderPage> {
        let url = self.search_url()?;
        let limit = q.limit.unwrap_or(25).clamp(1, 100);
        let page = q.page.unwrap_or(1).max(1);

        // UNIT3D's `name=` filter is a permissive substring match.
        // When the SCENE parser pulled a clean title + season +
        // episode out of the raw query we rebuild a canonical SCENE
        // form (`<title> SxxExx`) so the indexer's substring match
        // narrows to the exact release line we want. Without this
        // rebuild a query like "Classroom of the Elite S04E11"
        // still works (UNIT3D matches the whole string) but typing
        // just "Classroom of the Elite" used to drown S04E11 in
        // season packs because the only filter was raw `q`.
        let name_filter = build_unit3d_name_filter(q);

        // `/api/torrents/filter` parameter names per the official docs
        // (camelCase across the board). Anything UNIT3D doesn't
        // recognise is silently dropped, so getting these exactly
        // right is the difference between "search works" and "search
        // returns the 25 latest uploads regardless of filters".
        let mut qs: Vec<(&'static str, String)> = vec![
            ("api_token", self.api_token.clone()),
            ("name", name_filter),
            ("page", page.to_string()),
            ("perPage", limit.to_string()),
            ("sortField", "created_at".into()),
            ("sortDirection", "desc".into()),
        ];
        // Tag the request with the TV/movie category when:
        //   * the caller explicitly asked for it, OR
        //   * the parser saw an SxxExx marker (an unambiguous TV signal).
        let inferred_kind = q.kind.or_else(|| q.season.map(|_| MediaKind::Tv));
        if let Some(cat_id) = match inferred_kind {
            Some(MediaKind::Movie) => Some(self.movie_category_id),
            Some(MediaKind::Tv) => Some(self.tv_category_id),
            None => None,
        } {
            qs.push(("categories[]", cat_id.to_string()));
        }

        let resp = self
            .http
            .get(url)
            .query(&qs)
            .send()
            .await
            .map_err(|e| Error::Provider(format!("unit3d request: {e}")))?
            .error_for_status()
            .map_err(|e| Error::Provider(format!("unit3d status: {e}")))?
            .text()
            .await
            .map_err(|e| Error::Provider(format!("unit3d body: {e}")))?;

        let parsed: SearchResponse = serde_json::from_str(&resp).map_err(|e| {
            tracing::warn!(
                provider = %self.id,
                error = %e,
                body_preview = %resp.chars().take(300).collect::<String>(),
                "unit3d search decode failed",
            );
            Error::Provider(format!("unit3d search decode: {e}"))
        })?;

        let mut results = Vec::with_capacity(parsed.data.len());
        {
            let mut cache = self.link_cache.lock().await;
            for item in &parsed.data {
                cache.put(item.id.clone(), item.attributes.download_link.clone());
            }
            for item in parsed.data {
                results.push(item.into_search_result(&self.id));
            }
        }

        Ok(ProviderPage {
            results,
            current_page: parsed.meta.current_page.unwrap_or(page),
            limit: parsed.meta.per_page.unwrap_or(limit),
            total_count: parsed.meta.total,
            total_pages: parsed.meta.last_page,
        })
    }

    async fn details(&self, external_id: &str) -> Result<Option<TorrentDetails>> {
        Ok(self
            .fetch_envelope(external_id)
            .await?
            .map(|envelope| envelope.into_torrent_details(&self.id, external_id)))
    }

    async fn resolve(&self, external_id: &str) -> Result<TorrentSource> {
        // Try the cached pre-signed link first (primed by search / the
        // persisted-URL grab path). A cached link can go stale — the
        // classic case is the indexer rotating the user's rsskey, which
        // turns every previously-issued link into a 302 → login →
        // 401 — so a failure here falls through to a fresh fetch
        // instead of surfacing the stale link's error.
        let cached = self.link_cache.lock().await.get(external_id);
        if let Some(url) = cached {
            match self.download(external_id, &url).await {
                Ok(source) => return Ok(source),
                Err(e) => tracing::warn!(
                    provider = %self.id,
                    external_id,
                    error = %e,
                    "unit3d cached download_link failed — re-fetching a fresh link",
                ),
            }
        }
        // Cache miss or stale link: `/api/torrents/{id}` ships the same
        // attributes as a search item, including a freshly-signed
        // `download_link`. Re-prime the cache and retry once.
        let envelope = self.fetch_envelope(external_id).await?.ok_or_else(|| {
            Error::Provider(format!(
                "unit3d `{}`: torrent `{external_id}` no longer exists on the indexer",
                self.id
            ))
        })?;
        let url = envelope.attributes.download_link.clone();
        self.link_cache
            .lock()
            .await
            .put(external_id.to_string(), url.clone());
        self.download(external_id, &url).await
    }
}

// JSON shapes

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    data: Vec<TorrentEnvelope>,
    #[serde(default)]
    meta: Meta,
}

#[derive(Debug, Default, Deserialize)]
struct Meta {
    #[serde(default)]
    current_page: Option<u32>,
    #[serde(default)]
    per_page: Option<u32>,
    #[serde(default)]
    last_page: Option<u32>,
    #[serde(default)]
    total: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct TorrentEnvelope {
    id: String,
    attributes: TorrentAttributes,
}

#[derive(Debug, Deserialize)]
struct TorrentAttributes {
    name: String,
    #[serde(default)]
    category: Option<String>,
    /// "Encode" / "Remux" / "WEB-DL" / "Full Disc" — release type.
    /// Surfaced as a tag so the user sees it in the result row.
    #[serde(default, rename = "type")]
    release_type: Option<String>,
    #[serde(default)]
    resolution: Option<String>,
    #[serde(default)]
    seeders: Option<u32>,
    #[serde(default)]
    leechers: Option<u32>,
    /// `UNIT3D` is inconsistent about this field across forks: mainline
    /// and theoldschool emit a JSON number (`19400`, `0` for unset),
    /// but some older / customised builds emit a string (`"19400"`,
    /// `"0"`). [`flexible_u64`] accepts both — and `null` — and
    /// normalises to `Option<u64>` with zero filtered out as "no id".
    #[serde(default, deserialize_with = "flexible_u64")]
    tmdb_id: Option<u64>,
    /// File size in bytes. Always present on mainline `UNIT3D`; some
    /// older forks omit it. Optional out of caution.
    #[serde(default)]
    size: Option<u64>,
    /// SHA-1 infohash of the torrent (40 lowercase hex chars). Lets
    /// ingestion dedupe against torrents already managed by
    /// librqbit without first having to fetch the `.torrent` body.
    #[serde(default)]
    info_hash: Option<String>,
    /// Freeleech state as a percentage string emitted by `UNIT3D`:
    /// `"0%"`, `"25%"`, `"50%"`, `"75%"`, or `"100%"`. We surface
    /// `freeleech = true` only at 100 % since anything below still
    /// charges the user. Some forks emit a boolean here instead — we
    /// tolerate that variant via the lazy parse in
    /// `into_search_result`.
    #[serde(default)]
    freeleech: Option<String>,
    /// ISO 8601 (`"2026-05-15T07:07:24.000000Z"`) upload timestamp.
    /// Used to drive the "uploaded X hours ago" hint on the result
    /// row + sort-by-recent flows.
    #[serde(default)]
    created_at: Option<String>,
    /// Free-text description supplied by the uploader. `UNIT3D`
    /// renders it as `BBCode` (`[b]…[/b]`, `[center]…[/center]`,
    /// `[img]…[/img]`, …), the same dialect torr9 uses — so we keep
    /// the default `DescriptionFormat::Bbcode` rather than declaring
    /// it explicitly.
    #[serde(default)]
    description: Option<String>,
    /// Raw `MediaInfo` dump as a single string. We feed it to
    /// [`crate::nfo::parse`] to populate `media_info` on
    /// `TorrentDetails`; the original text is also surfaced via
    /// `nfo` so power users can read the full report.
    #[serde(default)]
    media_info: Option<String>,
    /// File breakdown supplied by the indexer. We only need the
    /// count + the byte total here (the actual per-file picker
    /// runs on the `.torrent` bytes inside librqbit at preview /
    /// ingest time).
    #[serde(default)]
    files: Vec<FileEntry>,
    /// Snatch count surfaced as "X downloaded" on the row.
    #[serde(default)]
    times_completed: Option<u64>,
    /// `UNIT3D` ships a separate `num_file` count; falls through to
    /// `files.len()` when absent.
    #[serde(default)]
    num_file: Option<u32>,
    /// The pre-signed `.torrent` URL with the user's rsskey embedded
    /// in the path. We cache this for `resolve()` and hit it as-is —
    /// no extra query params or auth header needed.
    download_link: String,
    /// Release year — `UNIT3D` is endpoint-inconsistent here:
    /// `/api/torrents` emits a STRING (`"2026"`), `/api/torrents/filter`
    /// emits a NUMBER (`2026`). Plus the usual null / empty / `"0"`
    /// edge cases for unset values. Run through `flexible_u64` to
    /// absorb every variant, then clamp to a plausible year range
    /// in [`TorrentEnvelope::into_search_result`].
    #[serde(default, deserialize_with = "flexible_u64")]
    release_year: Option<u64>,
    /// Optional `attributes.meta` sub-object some forks ship. Carries
    /// the TMDB poster URL pre-resolved (saves us a TMDB metadata
    /// round-trip on the discovery shelf) plus a comma-separated
    /// genre list. Both are optional — we tolerate either being
    /// absent or the whole `meta` object missing entirely.
    #[serde(default)]
    meta: Option<MetaBlock>,
}

/// Normalise an `info_hash` string into a canonical 40-char lowercase
/// hex SHA-1. Returns `None` on any unrecognised shape so a rogue
/// value can't poison downstream identity comparisons.
///
/// Two encodings observed in the wild:
/// Build the `name=` substring filter sent to UNIT3D's
/// `/api/torrents/filter`. When the SCENE parser extracted a usable
/// title + season (+ optional episode) from the raw query, rebuild a
/// canonical SCENE-form string the indexer matches verbatim
/// (`Classroom.of.the.Elite S04E11`). Without a parser hit we pass
/// the raw `q` straight through — no regression for free-text searches.
fn build_unit3d_name_filter(q: &SearchQuery) -> String {
    let parsed = match q.parsed_title.as_deref() {
        Some(t) if !t.is_empty() => t,
        _ => return q.q.clone(),
    };
    match (q.season, q.episode) {
        (Some(s), Some(e)) if e > 0 => format!("{parsed} S{s:02}E{e:02}"),
        (Some(s), _) => format!("{parsed} S{s:02}"),
        // Parser recognised a title but no S/E — keep the raw q in
        // case it contained year / qualifier info we'd lose by
        // collapsing to the parsed title alone.
        _ => q.q.clone(),
    }
}

///   * 40 hex chars — the canonical form (`/api/torrents/{id}`,
///     mainline `UNIT3D` search rows). Pass-through.
///   * 80 hex chars — `/api/torrents/filter` ships the infohash
///     hex-encoded a SECOND time: each of the 40 hex chars is
///     interpreted as a raw byte and re-hex-encoded, doubling the
///     length. Decode the outer layer, verify the inner is itself
///     a clean 40-char hex string.
fn normalize_infohash(raw: &str) -> Option<String> {
    let s = raw.trim().to_ascii_lowercase();
    if s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Some(s);
    }
    if s.len() == 80 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
        let mut inner = String::with_capacity(40);
        for chunk in s.as_bytes().as_chunks::<2>().0 {
            let hi = (chunk[0] as char).to_digit(16)?;
            let lo = (chunk[1] as char).to_digit(16)?;
            let byte = u8::try_from((hi << 4) | lo).ok()?;
            // Each decoded byte must itself be an ASCII hex digit —
            // otherwise this isn't the double-encoded form and emitting
            // it as an "infohash" would feed librqbit garbage.
            if !byte.is_ascii_hexdigit() {
                return None;
            }
            inner.push(byte as char);
        }
        return Some(inner);
    }
    None
}

/// Accept a JSON number, a numeric string, or null — normalise to
/// `Option<u64>` with zero / non-positive treated as "no id". Used
/// for `tmdb_id` because `UNIT3D` forks disagree on the encoding
/// (`19400` vs `"19400"` for the same payload field), and we'd rather
/// be permissive than crash a whole search response on a single
/// rogue type. Extracted as a free function so additional ID fields
/// (`imdb_id`, `tvdb_id`, …) can reuse it the day we wire them.
fn flexible_u64<'de, D>(deserializer: D) -> std::result::Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOf {
        Number(u64),
        Signed(i64),
        String(String),
        Null,
    }
    let v = Option::<OneOf>::deserialize(deserializer)?;
    Ok(match v {
        None | Some(OneOf::Null) => None,
        Some(OneOf::Number(n)) => Some(n).filter(|n| *n > 0),
        Some(OneOf::Signed(n)) => u64::try_from(n).ok().filter(|n| *n > 0),
        Some(OneOf::String(s)) => s.trim().parse::<u64>().ok().filter(|n| *n > 0),
    })
}

#[derive(Debug, Deserialize)]
struct FileEntry {
    /// Some forks include only `name` + `size`; mainline `UNIT3D`
    /// also surfaces `index`. We don't read either today — the
    /// `.torrent` bytes parsed by librqbit are the authoritative
    /// file list — but accepting them keeps the deserialiser
    /// tolerant.
    #[serde(default)]
    #[allow(dead_code)]
    size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct MetaBlock {
    #[serde(default)]
    poster: Option<String>,
    /// Human-readable genre list (`"Drame, Western"` style) — we
    /// split on commas and surface each as an individual tag.
    #[serde(default)]
    genres: Option<String>,
}

impl TorrentEnvelope {
    fn into_search_result(self, provider_id: &str) -> SearchResult {
        let attrs = self.attributes;
        // `category` strings vary by `UNIT3D` fork: mainline emits
        // "Movie" / "TV", theoldschool emits "Series" for shows.
        // Cover the common variants — additions are cheap, no
        // regression risk.
        let kind = match attrs.category.as_deref().map(str::trim) {
            Some("Movie" | "Movies" | "Films") => Some(MediaKind::Movie),
            Some("TV" | "TV Show" | "Series" | "Show" | "Anime") => Some(MediaKind::Tv),
            _ => None,
        };
        // Deserialised as `u64` to tolerate either string or number
        // upstream encoding — fall back to a filename year-scan when
        // the field is missing / out of plausible range. `UNIT3D`
        // never emits a year above ~current+1, so 1900..=2099 is a
        // safe guard against rogue values like `99999`.
        let year = attrs
            .release_year
            .and_then(|n| u16::try_from(n).ok())
            .filter(|n| (1900..=2099).contains(n))
            .or_else(|| extract_year(&attrs.name));
        let tmdb_id = attrs.tmdb_id;

        // Surface the release type + resolution + genres as tags. The
        // UI chips them on the result row without any provider-
        // specific logic.
        let mut tags: Vec<String> = Vec::new();
        if let Some(t) = attrs.release_type
            && !t.is_empty()
        {
            tags.push(t);
        }
        if let Some(r) = attrs.resolution
            && !r.is_empty()
        {
            tags.push(r);
        }
        let (poster_url, genre_tags) = match attrs.meta {
            Some(m) => {
                let genres: Vec<String> = m
                    .genres
                    .as_deref()
                    .map(|g| {
                        g.split(',')
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(String::from)
                            .collect()
                    })
                    .unwrap_or_default();
                (m.poster.filter(|s| !s.is_empty()), genres)
            }
            None => (None, Vec::new()),
        };
        tags.extend(genre_tags);

        // `info_hash` lowercase per BEP-9 / typical librqbit usage.
        // `UNIT3D` already emits it lowercase, but be defensive — and
        // some endpoints double-encode (see `normalize_infohash`).
        let infohash = attrs.info_hash.as_deref().and_then(normalize_infohash);

        // Freeleech: anything below 100 % still charges the user
        // some download credit, so we only flag true at full.
        let freeleech = attrs.freeleech.as_deref().map(str::trim).is_some_and(|s| {
            s == "100%" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
        });

        let uploaded_at = attrs
            .created_at
            .as_deref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc));

        SearchResult {
            provider_id: provider_id.to_string(),
            external_id: self.id,
            title: attrs.name,
            year,
            size_bytes: attrs.size,
            seeders: attrs.seeders,
            leechers: attrs.leechers,
            infohash,
            magnet: None,
            category: attrs.category,
            tags,
            freeleech,
            uploader: None,
            uploaded_at,
            tmdb_id,
            kind,
            poster_url,
            already_in_library: false,
            library_infohash: None,
            library_file_idx: None,
            language: None,
            codec: None,
            // UNIT3D ships pre-signed `.torrent` URLs in the
            // search payload. We keep them in the in-memory
            // `link_cache` for hot grabs AND surface them here so
            // the scheduler can persist them to
            // `available_episodes.download_url` — once persisted
            // a grab survives a server restart even if the search
            // page no longer lists this release.
            download_url: Some(attrs.download_link),
            parsed_season: None,
            parsed_episode: None,
        }
    }

    fn into_torrent_details(self, provider_id: &str, external_id: &str) -> TorrentDetails {
        let attrs = self.attributes;
        let nfo = attrs.media_info.clone().filter(|s| !s.trim().is_empty());
        let media_info = nfo.as_deref().and_then(nfo::parse);
        // Drop obviously-useless `description` content:
        //   * empty / whitespace
        //   * literal placeholder "Pas de description" (theoldschool
        //     default when the uploader didn't write one)
        //   * the release name copied back into the description field
        //     (also a theoldschool tic — surfacing it would render
        //     "the.release.name.dots.and.all" as if it were a
        //     synopsis, which is what the user is seeing today).
        let description = attrs.description.filter(|s| {
            let trimmed = s.trim();
            !trimmed.is_empty()
                && trimmed != "Pas de description"
                && !trimmed.eq_ignore_ascii_case(attrs.name.trim())
        });
        let file_count = attrs
            .num_file
            .or_else(|| u32::try_from(attrs.files.len()).ok());
        let freeleech = attrs.freeleech.as_deref().map(str::trim).is_some_and(|s| {
            s == "100%" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
        });
        let uploaded_at = attrs
            .created_at
            .as_deref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc));
        // Category for the detail card: `Category / Type` matches
        // what torr9 surfaces ("Films / Encode", "Series / Remux").
        let category = match (attrs.category, attrs.release_type) {
            (Some(c), Some(t)) if !t.is_empty() => Some(format!("{c} / {t}")),
            (Some(c), _) => Some(c),
            (None, Some(t)) if !t.is_empty() => Some(t),
            _ => None,
        };
        let mut tags: Vec<String> = Vec::new();
        if let Some(r) = attrs.resolution.filter(|r| !r.is_empty()) {
            tags.push(r);
        }
        if let Some(m) = attrs.meta
            && let Some(genres) = m.genres
        {
            tags.extend(
                genres
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from),
            );
        }

        TorrentDetails {
            provider_id: provider_id.to_string(),
            external_id: external_id.to_string(),
            title: attrs.name,
            // `UNIT3D` description is BBCode — same dialect torr9
            // uses. Default `DescriptionFormat::Bbcode` is what the
            // BBCode renderer in `PreviewDialog.tsx` expects, so we
            // declare it explicitly here for clarity (and to survive
            // a future change in the `Default` impl).
            description,
            description_format: DescriptionFormat::Bbcode,
            nfo,
            media_info,
            tags,
            category,
            uploader: None,
            uploaded_at,
            age: None,
            seeders: attrs.seeders,
            leechers: attrs.leechers,
            times_completed: attrs.times_completed,
            views: None,
            freeleech,
            exclusive: false,
            file_count,
            file_size_bytes: attrs.size,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The refusal text is what the user reads in the preview dialog, so it
    /// has to name the tracker, the status, and the two things worth
    /// checking. The no-double-space assertion guards the line
    /// continuations: collapse one and the sentence grows a gap.
    #[test]
    fn refusal_message_reads_as_one_sentence() {
        let err = Unit3dProvider::refused("seedpool", reqwest::StatusCode::UNAUTHORIZED);
        let Error::ProviderRefused(msg) = err else {
            panic!("a refused download must not degrade to a generic provider error");
        };
        assert!(msg.starts_with("`seedpool` refused the .torrent download (HTTP 401"));
        assert!(msg.contains("download-slot limit"));
        assert!(
            !msg.contains("  "),
            "continuation leaked into the text: {msg}"
        );
    }

    /// Matches the runtime types audited against the live
    /// `theoldschool.cc` response (`tmdb_id` is a JSON **number**, not
    /// a string; `release_year` IS a string; `freeleech` IS a string;
    /// `created_at` IS a string; `info_hash` IS a string; numeric
    /// counters are numbers). If a fork ever flips one of these
    /// back to the alternate encoding, the [`flexible_u64`] deserialiser
    /// covers `tmdb_id`; the other tests below assert the lazy
    /// string-parse paths.
    const SAMPLE: &str = r#"{
        "data": [
            {
                "type": "torrent",
                "id": "39808",
                "attributes": {
                    "name": "Gwendoline 1984 2in1 1080p Blu-ray AVC DTS-HD MA 5.1",
                    "category": "Movie",
                    "type": "Full Disc",
                    "resolution": "1080p",
                    "seeders": 1,
                    "leechers": 1,
                    "times_completed": 0,
                    "tmdb_id": 19400,
                    "imdb_id": 0,
                    "release_year": "1984",
                    "meta": {
                        "poster": "https://image.tmdb.org/t/p/w92/gw.jpg",
                        "genres": "Action, Aventure"
                    },
                    "download_link": "https://theoldschool.cc/torrent/download/39808.RSSKEY"
                }
            },
            {
                "type": "torrent",
                "id": "39805",
                "attributes": {
                    "name": "Beware The Batman S01 1080p BluRay REMUX AVC DTS-HD MA 2.0-EPSiLON",
                    "category": "Series",
                    "type": "Remux",
                    "resolution": "1080p",
                    "seeders": 3,
                    "leechers": 0,
                    "tmdb_id": 41676,
                    "info_hash": "92cd5be563396cf77ec0b8f6213a6d4d871431b2",
                    "size": 4300630576,
                    "freeleech": "100%",
                    "created_at": "2026-05-15T07:07:24.000000Z",
                    "download_link": "https://theoldschool.cc/torrent/download/39805.RSSKEY"
                }
            }
        ],
        "meta": {
            "current_page": 1,
            "from": 1,
            "last_page": 2376,
            "path": "https://theoldschool.cc/api/torrents",
            "per_page": 15,
            "to": 15,
            "total": 35628
        }
    }"#;

    #[test]
    fn parses_sample_response() {
        let resp: SearchResponse = serde_json::from_str(SAMPLE).expect("parse");
        assert_eq!(resp.data.len(), 2);
        assert_eq!(resp.meta.total, Some(35628));
        assert_eq!(resp.meta.last_page, Some(2376));
        assert_eq!(resp.meta.per_page, Some(15));
        assert_eq!(resp.meta.current_page, Some(1));
    }

    #[test]
    fn maps_search_result_movie() {
        let resp: SearchResponse = serde_json::from_str(SAMPLE).unwrap();
        let movie = resp
            .data
            .into_iter()
            .next()
            .unwrap()
            .into_search_result("tos");
        assert_eq!(
            movie.title,
            "Gwendoline 1984 2in1 1080p Blu-ray AVC DTS-HD MA 5.1"
        );
        assert_eq!(movie.external_id, "39808");
        assert_eq!(movie.kind, Some(MediaKind::Movie));
        assert_eq!(movie.year, Some(1984));
        assert_eq!(movie.seeders, Some(1));
        assert_eq!(movie.leechers, Some(1));
        assert_eq!(movie.tmdb_id, Some(19400));
        assert_eq!(movie.category.as_deref(), Some("Movie"));
        assert!(movie.tags.iter().any(|t| t == "Full Disc"));
        assert!(movie.tags.iter().any(|t| t == "1080p"));
        // `meta.poster` populates `poster_url` directly — no TMDB
        // metadata round-trip on the discovery shelf.
        assert_eq!(
            movie.poster_url.as_deref(),
            Some("https://image.tmdb.org/t/p/w92/gw.jpg"),
        );
        // `meta.genres` splits on commas and extends `tags`.
        assert!(movie.tags.iter().any(|t| t == "Action"));
        assert!(movie.tags.iter().any(|t| t == "Aventure"));
    }

    #[test]
    fn handles_release_year_edge_cases() {
        // Number form — what `/api/torrents/filter` returns.
        let json = r#"{"data":[{"type":"torrent","id":"1","attributes":{
            "name":"Undertone.2026.FRENCH.WEB.H264-SUPPLY","category":"Films",
            "release_year":2026,"download_link":"https://x/1"
        }}],"meta":{}}"#;
        let resp: SearchResponse = serde_json::from_str(json).unwrap();
        let r = resp
            .data
            .into_iter()
            .next()
            .unwrap()
            .into_search_result("tos");
        assert_eq!(r.year, Some(2026));

        // String form — what `/api/torrents` (bare list) returns.
        let json = r#"{"data":[{"type":"torrent","id":"2","attributes":{
            "name":"Movie","category":"Movie",
            "release_year":"2024","download_link":"https://x/2"
        }}],"meta":{}}"#;
        let resp: SearchResponse = serde_json::from_str(json).unwrap();
        let r = resp
            .data
            .into_iter()
            .next()
            .unwrap()
            .into_search_result("tos");
        assert_eq!(r.year, Some(2024));

        // Empty string → fall back to title-scan.
        let json = r#"{"data":[{"type":"torrent","id":"3","attributes":{
            "name":"Some Show 2018 1080p","category":"TV",
            "release_year":"","download_link":"https://x/3"
        }}],"meta":{}}"#;
        let resp: SearchResponse = serde_json::from_str(json).unwrap();
        let r = resp
            .data
            .into_iter()
            .next()
            .unwrap()
            .into_search_result("tos");
        assert_eq!(r.year, Some(2018));

        // `"0"` (string) → fall back via flexible_u64's zero-filter.
        let json = r#"{"data":[{"type":"torrent","id":"4","attributes":{
            "name":"Movie","category":"Movie",
            "release_year":"0","download_link":"https://x/4"
        }}],"meta":{}}"#;
        let resp: SearchResponse = serde_json::from_str(json).unwrap();
        let r = resp
            .data
            .into_iter()
            .next()
            .unwrap()
            .into_search_result("tos");
        assert_eq!(r.year, None);

        // `0` (number) → same.
        let json = r#"{"data":[{"type":"torrent","id":"5","attributes":{
            "name":"Movie","category":"Movie",
            "release_year":0,"download_link":"https://x/5"
        }}],"meta":{}}"#;
        let resp: SearchResponse = serde_json::from_str(json).unwrap();
        let r = resp
            .data
            .into_iter()
            .next()
            .unwrap()
            .into_search_result("tos");
        assert_eq!(r.year, None);

        // `null` → none.
        let json = r#"{"data":[{"type":"torrent","id":"6","attributes":{
            "name":"Movie","category":"Movie",
            "release_year":null,"download_link":"https://x/6"
        }}],"meta":{}}"#;
        let resp: SearchResponse = serde_json::from_str(json).unwrap();
        let r = resp
            .data
            .into_iter()
            .next()
            .unwrap()
            .into_search_result("tos");
        assert_eq!(r.year, None);

        // Out-of-range — guard against silly values like `3000`.
        let json = r#"{"data":[{"type":"torrent","id":"7","attributes":{
            "name":"Movie","category":"Movie",
            "release_year":3000,"download_link":"https://x/7"
        }}],"meta":{}}"#;
        let resp: SearchResponse = serde_json::from_str(json).unwrap();
        let r = resp
            .data
            .into_iter()
            .next()
            .unwrap()
            .into_search_result("tos");
        assert_eq!(r.year, None);
    }

    #[test]
    fn maps_search_result_tv() {
        let resp: SearchResponse = serde_json::from_str(SAMPLE).unwrap();
        let tv = resp
            .data
            .into_iter()
            .nth(1)
            .unwrap()
            .into_search_result("tos");
        // `"Series"` is theoldschool's name for TV; mainline `UNIT3D`
        // emits `"TV"`. Both should land on `MediaKind::Tv`.
        assert_eq!(tv.kind, Some(MediaKind::Tv));
        assert_eq!(tv.tmdb_id, Some(41676));
        assert_eq!(tv.seeders, Some(3));
        assert_eq!(
            tv.infohash.as_deref(),
            Some("92cd5be563396cf77ec0b8f6213a6d4d871431b2"),
        );
        assert_eq!(tv.size_bytes, Some(4_300_630_576));
        assert!(tv.freeleech, "100% freeleech should map to freeleech=true");
        assert!(tv.uploaded_at.is_some(), "created_at should parse");
    }

    #[test]
    fn freeleech_below_full_stays_false() {
        let json = r#"{"data":[{"type":"torrent","id":"1","attributes":{
            "name":"X","category":"Movie","tmdb_id":0,
            "freeleech":"50%","download_link":"https://x/1"
        }}],"meta":{}}"#;
        let resp: SearchResponse = serde_json::from_str(json).unwrap();
        let r = resp
            .data
            .into_iter()
            .next()
            .unwrap()
            .into_search_result("tos");
        assert!(!r.freeleech);
    }

    #[test]
    fn details_mapping_uses_bbcode_format() {
        // Realistic shape of `/api/torrents/{id}` for `UNIT3D`. NOTE:
        // unlike `/api/torrents/filter` (which wraps `{"data":[…]}`),
        // the single-item endpoint emits the envelope BARE — no outer
        // `data` key. Locked down here so we can't regress to expecting
        // the wrapper (which was the bug that hid all rich details for
        // theoldschool in the preview dialog).
        let json = r#"{
            "type": "torrent",
            "id": "412772",
            "attributes": {
                "name": "Dutton.Ranch.S01E01.MULTi.1080p.WEB.H264-SUPPLY",
                "category": "Series",
                "type": "WEB",
                "resolution": "1080p",
                "release_year": "2026",
                "tmdb_id": 999,
                "info_hash": "92cd5be563396cf77ec0b8f6213a6d4d871431b2",
                "size": 4300630576,
                "freeleech": "100%",
                "created_at": "2026-05-15T07:07:24.000000Z",
                "description": "[b]Synopsis :[/b] Une famille de ranchers...",
                "media_info": "General\nDuration: 56 min 23 s\nFile size: 4.01 GiB\n",
                "files": [
                    {"index":1,"name":"a.mkv","size":4300626050},
                    {"index":2,"name":"a.nfo","size":4526}
                ],
                "num_file": 2,
                "times_completed": 44,
                "meta": {
                    "poster": "https://image.tmdb.org/t/p/w92/x.jpg",
                    "genres": "Drame, Western"
                },
                "download_link": "https://x/1"
            }
        }"#;
        let envelope: TorrentEnvelope = serde_json::from_str(json).unwrap();
        let d = envelope.into_torrent_details("tos", "412772");
        assert_eq!(d.title, "Dutton.Ranch.S01E01.MULTi.1080p.WEB.H264-SUPPLY");
        // BBCode is the format `PreviewDialog` already knows how to
        // render — same path as torr9.
        assert_eq!(d.description_format, DescriptionFormat::Bbcode);
        assert!(d.description.as_deref().unwrap().contains("[b]Synopsis"));
        // `media_info` text surfaces both as raw NFO and as a parsed
        // `MediaInfoSummary` (best-effort — empty Summary is fine).
        assert!(d.nfo.as_deref().unwrap().contains("Duration"));
        // Category combines `Series / WEB` for the detail card.
        assert_eq!(d.category.as_deref(), Some("Series / WEB"));
        assert_eq!(d.times_completed, Some(44));
        assert_eq!(d.file_count, Some(2));
        assert_eq!(d.file_size_bytes, Some(4_300_630_576));
        assert!(d.freeleech);
        assert!(d.uploaded_at.is_some());
        // Resolution + genres become tags.
        assert!(d.tags.iter().any(|t| t == "1080p"));
        assert!(d.tags.iter().any(|t| t == "Drame"));
        assert!(d.tags.iter().any(|t| t == "Western"));
    }

    /// Lock down EVERY field type the real theoldschool API emits.
    /// Audited from a 25-item response; types kept as-is. If a
    /// future fork flips a type, the regression lands here first
    /// instead of crashing in production.
    #[test]
    fn parses_full_runtime_shape() {
        let json = r#"{
            "data": [{
                "type": "torrent",
                "id": "412772",
                "attributes": {
                    "meta": {"poster": "https://x/p.jpg", "genres": "Drame"},
                    "name": "Dutton.Ranch.S01E01.MULTi.1080p.WEB.H264-SUPPLY",
                    "release_year": "2026",
                    "category": "Series",
                    "category_id": 2,
                    "type": "WEB",
                    "type_id": 12,
                    "resolution": "1080p",
                    "resolution_id": 2,
                    "media_info": "General\nDuration: 56 min\n",
                    "bd_info": null,
                    "description": "[b]Synopsis[/b]",
                    "info_hash": "92cd5be563396cf77ec0b8f6213a6d4d871431b2",
                    "size": 4300630576,
                    "num_file": 2,
                    "files": [{"index":1,"name":"a.mkv","size":4300626050}],
                    "freeleech": "100%",
                    "double_upload": false,
                    "refundable": false,
                    "internal": 0,
                    "personal_release": 0,
                    "featured": false,
                    "uploader": "anonymous",
                    "seeders": 5,
                    "leechers": 1,
                    "times_completed": 44,
                    "tmdb_id": 299167,
                    "imdb_id": 0,
                    "tvdb_id": 0,
                    "mal_id": 0,
                    "igdb_id": 0,
                    "created_at": "2026-05-15T07:07:24.000000Z",
                    "folder": "Dutton.Ranch.S01E01.MULTi.1080p.WEB.H264-SUPPLY",
                    "download_link": "https://theoldschool.cc/torrent/download/412772.RSSKEY",
                    "details_link": "https://theoldschool.cc/torrents/412772"
                }
            }],
            "meta": {"current_page":1,"from":1,"last_page":2376,"per_page":15,"to":15,"total":35628}
        }"#;
        let resp: SearchResponse = serde_json::from_str(json).expect("deserialise full shape");
        assert_eq!(resp.data.len(), 1);
        let sr = resp
            .data
            .into_iter()
            .next()
            .unwrap()
            .into_search_result("tos");
        assert_eq!(sr.tmdb_id, Some(299_167));
        assert_eq!(sr.kind, Some(MediaKind::Tv));
        assert_eq!(sr.year, Some(2026));
        assert_eq!(sr.size_bytes, Some(4_300_630_576));
        assert!(sr.freeleech);
        assert_eq!(
            sr.infohash.as_deref(),
            Some("92cd5be563396cf77ec0b8f6213a6d4d871431b2"),
        );
        assert!(sr.uploaded_at.is_some());
    }

    #[test]
    fn details_skips_release_name_as_description() {
        // theoldschool habit: when the uploader is lazy, they paste
        // the release name into the description field. We don't want
        // that rendering as a fake synopsis — drop it.
        let json = r#"{
            "type": "torrent",
            "id": "1",
            "attributes": {
                "name": "Chicago.Fire.S14E21.FASTSUB.VOSTFR.1080p.WEB.x264-TOS",
                "description": "Chicago.Fire.S14E21.FASTSUB.VOSTFR.1080p.WEB.x264-TOS",
                "download_link": "https://x/1"
            }
        }"#;
        let envelope: TorrentEnvelope = serde_json::from_str(json).unwrap();
        let d = envelope.into_torrent_details("tos", "1");
        assert!(
            d.description.is_none(),
            "release-name-as-description should be dropped"
        );
    }

    #[test]
    fn details_keeps_real_bbcode_description() {
        // Sanity: a genuine BBCode description still survives the
        // filter (only placeholders and name-echoes are dropped).
        let json = r#"{
            "type": "torrent",
            "id": "1",
            "attributes": {
                "name": "Some Movie 2026",
                "description": "[b]Synopsis[/b]\n[i]A real description with formatting[/i]",
                "download_link": "https://x/1"
            }
        }"#;
        let envelope: TorrentEnvelope = serde_json::from_str(json).unwrap();
        let d = envelope.into_torrent_details("tos", "1");
        assert!(d.description.as_deref().unwrap().contains("[b]Synopsis"));
        assert_eq!(d.description_format, DescriptionFormat::Bbcode);
    }

    #[test]
    fn details_skips_placeholder_description() {
        // theoldschool fills `description` with the literal string
        // "Pas de description" when the uploader didn't write one.
        // Don't show that to the user — collapse to `None` so the
        // PreviewDialog hides the section entirely.
        let json = r#"{
            "type": "torrent",
            "id": "1",
            "attributes": {
                "name": "Movie",
                "description": "Pas de description",
                "download_link": "https://x/1"
            }
        }"#;
        let envelope: TorrentEnvelope = serde_json::from_str(json).unwrap();
        let d = envelope.into_torrent_details("tos", "1");
        assert!(d.description.is_none());
    }

    #[test]
    fn infohash_only_set_when_well_formed() {
        // Bad length / non-hex → drop silently rather than poison
        // downstream identity comparisons.
        let json = r#"{"data":[{"type":"torrent","id":"1","attributes":{
            "name":"X","category":"Movie","tmdb_id":0,
            "info_hash":"NOT-A-VALID-HASH","download_link":"https://x/1"
        }}],"meta":{}}"#;
        let resp: SearchResponse = serde_json::from_str(json).unwrap();
        let r = resp
            .data
            .into_iter()
            .next()
            .unwrap()
            .into_search_result("tos");
        assert!(r.infohash.is_none());
    }

    #[test]
    fn tmdb_id_accepts_number_string_and_zero() {
        // Number form (mainline / theoldschool).
        let n = r#"{"data":[{"type":"torrent","id":"1","attributes":{
            "name":"X","category":"Movie","tmdb_id":19400,
            "download_link":"https://x/1"
        }}],"meta":{}}"#;
        let r: SearchResponse = serde_json::from_str(n).unwrap();
        assert_eq!(
            r.data
                .into_iter()
                .next()
                .unwrap()
                .into_search_result("tos")
                .tmdb_id,
            Some(19400),
        );
        // String form (some older forks).
        let s = r#"{"data":[{"type":"torrent","id":"2","attributes":{
            "name":"X","category":"Movie","tmdb_id":"19400",
            "download_link":"https://x/2"
        }}],"meta":{}}"#;
        let r: SearchResponse = serde_json::from_str(s).unwrap();
        assert_eq!(
            r.data
                .into_iter()
                .next()
                .unwrap()
                .into_search_result("tos")
                .tmdb_id,
            Some(19400),
        );
        // Zero (numeric) — treated as "no id".
        let z = r#"{"data":[{"type":"torrent","id":"3","attributes":{
            "name":"X","category":"Movie","tmdb_id":0,
            "download_link":"https://x/3"
        }}],"meta":{}}"#;
        let r: SearchResponse = serde_json::from_str(z).unwrap();
        assert_eq!(
            r.data
                .into_iter()
                .next()
                .unwrap()
                .into_search_result("tos")
                .tmdb_id,
            None,
        );
        // Zero (string) — same.
        let zs = r#"{"data":[{"type":"torrent","id":"4","attributes":{
            "name":"X","category":"Movie","tmdb_id":"0",
            "download_link":"https://x/4"
        }}],"meta":{}}"#;
        let r: SearchResponse = serde_json::from_str(zs).unwrap();
        assert_eq!(
            r.data
                .into_iter()
                .next()
                .unwrap()
                .into_search_result("tos")
                .tmdb_id,
            None,
        );
        // Null — also None.
        let null = r#"{"data":[{"type":"torrent","id":"5","attributes":{
            "name":"X","category":"Movie","tmdb_id":null,
            "download_link":"https://x/5"
        }}],"meta":{}}"#;
        let r: SearchResponse = serde_json::from_str(null).unwrap();
        assert_eq!(
            r.data
                .into_iter()
                .next()
                .unwrap()
                .into_search_result("tos")
                .tmdb_id,
            None,
        );
    }

    #[test]
    fn infohash_decodes_double_encoded_form() {
        // theoldschool's `/api/torrents/filter` ships the infohash hex-
        // encoded a second time: 40 hex chars → 80 hex chars where each
        // pair is the ASCII byte of the original hex digit.
        //   real  : e3f4ba8cecde146e8c1579691a2bce664cc870ec
        //   shipped: 65 33 66 34 62 61 38 63 65 63 64 65 31 34 36 65 …
        //                  (ASCII codes of 'e', '3', 'f', '4', 'b', 'a', '8', 'c', …)
        let inner = "e3f4ba8cecde146e8c1579691a2bce664cc870ec";
        let mut outer = String::with_capacity(80);
        for b in inner.bytes() {
            use std::fmt::Write;
            write!(&mut outer, "{b:02x}").unwrap();
        }
        assert_eq!(outer.len(), 80);
        assert_eq!(super::normalize_infohash(&outer).as_deref(), Some(inner));
        // 40-char canonical form survives the trip unchanged.
        assert_eq!(super::normalize_infohash(inner).as_deref(), Some(inner));
        // Mixed case → lowercased.
        assert_eq!(
            super::normalize_infohash("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA").as_deref(),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        );
        // 80-char hex that doesn't decode into hex-ascii bytes → reject
        // rather than emit garbage (e.g., a random hex blob whose bytes
        // happen to land outside `[0-9a-f]`).
        let bogus = "ff".repeat(40); // decodes to 40 × 0xff — not ASCII hex digits.
        assert_eq!(super::normalize_infohash(&bogus), None);
        // Wrong length entirely.
        assert_eq!(super::normalize_infohash("deadbeef"), None);
    }

    #[test]
    fn details_decodes_bare_envelope_shape() {
        // Pinned to the exact shape captured from theoldschool's live
        // `/api/torrents/{id}` (no outer `{data:…}` wrapper). The
        // earlier bug expected the wrapper and crashed with `missing
        // field `data`` — that returned 500, blanked the preview
        // dialog. If a fork ever re-introduces the wrapper, this test
        // fails first.
        let json = r#"{
            "type": "torrent",
            "id": "411549",
            "attributes": {
                "meta": {"poster": "https://x/p.jpg", "genres": "Horreur"},
                "name": "Undertone.2026.MULTi.2160p.WEB.H265-SUPPLY",
                "release_year": "2026",
                "category": "Films",
                "type": "WEB",
                "resolution": "2160p",
                "media_info": "General\nDuration: 1 h 50 min\n\nVideo\nFormat: HEVC\n\nAudio\nFormat: AC-3\nLanguage: French\n",
                "description": "Pas de description",
                "info_hash": "f62297537ce1acc94a5cc8a3f03398b14ea33085",
                "size": 12345,
                "num_file": 1,
                "files": [{"index":1,"name":"a.mkv","size":12345}],
                "freeleech": "50%",
                "internal": 0,
                "personal_release": 0,
                "uploader": "anonymous",
                "seeders": 5,
                "leechers": 0,
                "times_completed": 3,
                "tmdb_id": 1480387,
                "created_at": "2026-05-15T07:07:24.000000Z",
                "download_link": "https://theoldschool.cc/torrent/download/411549.RSSKEY"
            }
        }"#;
        let envelope: TorrentEnvelope = serde_json::from_str(json).expect("bare envelope");
        let d = envelope.into_torrent_details("tos", "411549");
        // Even though description is the placeholder (dropped),
        // `nfo` MUST surface — that's what populates the FactsGrid
        // and the raw NFO collapsible in PreviewDialog.
        assert!(d.description.is_none());
        assert!(d.nfo.as_deref().unwrap().contains("HEVC"));
        assert!(
            d.media_info.is_some(),
            "parsed MediaInfo should populate FactsGrid"
        );
        let mi = d.media_info.unwrap();
        assert!(mi.video.is_some());
        assert!(!mi.audio.is_empty());
        assert_eq!(d.category.as_deref(), Some("Films / WEB"));
        assert_eq!(d.times_completed, Some(3));
        assert!(d.uploaded_at.is_some());
    }

    #[test]
    fn link_cache_evicts_fifo() {
        let mut c = LinkCache::new();
        for i in 0..(LINK_CACHE_CAP + 10) {
            c.put(format!("k{i}"), format!("v{i}"));
        }
        assert!(c.get("k0").is_none());
        assert!(c.get(&format!("k{}", LINK_CACHE_CAP + 9)).is_some());
    }
}
