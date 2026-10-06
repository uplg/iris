//! Live TV: per-country IPTV channels (iptv-org) played through a backend
//! HLS proxy.
//!
//! Upstream streams are plain-http, CORS-less, and several demand a browser
//! `User-Agent`, so clients never talk to them directly: the service fetches
//! a country's playlist lazily on first access (then keeps it refreshed in
//! the background), exposes the channel list, and rewrites every HLS URI to
//! the authenticated `/api/livetv/proxy` endpoint. Proxy URLs are
//! HMAC-signed so only URLs the server itself minted are fetchable — no
//! open proxy. Channels aggregate every playlist entry with the same
//! identity as ordered fallback sources; the master-playlist path rotates
//! to the next source when the active one dies.

pub mod channels;
pub mod dlive;
pub mod epg;
pub mod m3u;
pub mod proxy;
mod refresh_cell;
pub mod transcode;
pub mod vavoo;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use serde::Deserialize;
use url::Url;

use channels::{Channel, SourceOrigin, SourceTier};
use refresh_cell::RefreshCell;
use transcode::Mode;

/// Browser UA presented to upstreams that don't pin one via the playlist
/// (several French networks 403 non-browser agents).
const DEFAULT_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36";

/// Base cooldown after a source failure. Doubles per consecutive failure
/// (10 min → 20 → 40 → …) up to [`SOURCE_COOLDOWN_MAX`], so a persistently
/// dead feed is only re-probed ~once a day and never elected while a
/// healthy alternative exists.
const SOURCE_COOLDOWN_BASE: Duration = Duration::from_mins(10);
const SOURCE_COOLDOWN_MAX: Duration = Duration::from_hours(24);

/// Ceiling for cooldowns triggered by a CLIENT playback report, as opposed to
/// a failed server-side probe. A probe failure is hard evidence (we fetched
/// the playlist and its variant ourselves); a playback report is a guess made
/// by a browser about a feed the server can reach perfectly well, and browser
/// media stacks produce false accusations wholesale — one hls.js release
/// turning append hiccups into fatal errors was enough to walk all four of
/// M6's feeds up the 24 h ladder and leave the channel dark. Reports still
/// escalate (a genuinely unplayable feed keeps losing elections) but can
/// never park a source for longer than this.
const PLAYBACK_COOLDOWN_MAX: Duration = Duration::from_mins(30);

/// Concurrency cap for the background health probe.
const PROBE_CONCURRENCY: usize = 12;

/// Consecutive segment/key fetch failures on the live source before it is
/// demoted and the next feed elected.
const SEGMENT_FAIL_THRESHOLD: u64 = 5;
/// The same for a dlive source: its feeds are restreams of restreams, and
/// Vavoo is right behind.
const DLIVE_SEGMENT_FAIL_THRESHOLD: u64 = 3;

/// Proxied segment fetch deadline for a dlive source (a Player 1 segment is
/// a 2–5 MB image).
const DLIVE_SEGMENT_TIMEOUT: Duration = Duration::from_secs(20);

/// Cap on an image-wrapped segment buffered for unwrapping.
const MAX_WRAPPED_SEGMENT_BYTES: usize = 32 * 1024 * 1024;

/// Per-source fetch timeout for playlist requests — bounds the worst case
/// when rotating through several dead sources in one request.
const PLAYLIST_TIMEOUT: Duration = Duration::from_secs(8);

/// Playlists (m3u / m3u8) are text and small; cap protects the rewriter.
const MAX_PLAYLIST_BYTES: usize = 4 * 1024 * 1024;

/// How long a fetched logo stays served from memory (they're effectively
/// static). Failures are cached too, so a dead host isn't re-hammered — but
/// with two very different TTLs: a genuine miss (404/403/410) sticks for
/// [`LOGO_NEG_TTL`]; a RETRYABLE failure (429 rate-limit, 5xx, network error)
/// only sticks for [`LOGO_RETRY_TTL`] so it self-heals on the next render
/// instead of blanking a tile for minutes. This is the imgur/wikimedia case:
/// hotlink hosts 429 a datacenter IP on a cold burst, then serve fine seconds
/// later.
const LOGO_CACHE_TTL: Duration = Duration::from_hours(24);
const LOGO_NEG_TTL: Duration = Duration::from_mins(5);
const LOGO_RETRY_TTL: Duration = Duration::from_secs(20);

/// Max concurrent upstream logo fetches — low enough to stay under logo-host
/// rate limits while a full grid warms the cache.
const LOGO_FETCH_CONCURRENCY: usize = 4;

/// Upstream statuses (and our own 502 for a transport error) that are worth
/// retrying soon rather than caching as a durable miss.
pub(crate) fn logo_status_retryable(status: u16) -> bool {
    matches!(status, 408 | 425 | 429 | 500 | 502 | 503 | 504)
}

/// Hard cap on cached logo entries (each a few KB). Cleared wholesale if
/// exceeded — trivial to re-warm and far simpler than LRU bookkeeping.
const LOGO_CACHE_MAX: usize = 4096;

/// Logos above this many bytes aren't cached in memory (channel logos are
/// tiny PNG/SVG; anything larger is almost certainly not a real logo).
const LOGO_MAX_BYTES: usize = 512 * 1024;

/// Cap on the iptv-org JSON databases (streams, channels, logos, countries).
const MAX_JSON_BYTES: usize = 64 * 1024 * 1024;

/// Cap on a fetched (usually gzipped) programme guide, and on its gunzipped
/// XML.
const MAX_GUIDE_BYTES: usize = 128 * 1024 * 1024;
const MAX_GUIDE_XML_BYTES: u64 = 512 * 1024 * 1024;

/// Cap on a logo download; past [`LOGO_MAX_BYTES`] it is served uncached.
const LOGO_FETCH_MAX_BYTES: usize = 4 * 1024 * 1024;

/// How long a failed best-effort load (streams DB, search index, guide…) is
/// remembered before the next caller may retry it.
const FAILED_LOAD_RETRY: Duration = Duration::from_mins(10);

#[derive(Debug, thiserror::Error)]
pub enum LiveTvError {
    #[error("unknown country")]
    UnknownCountry,
    #[error("unknown channel")]
    UnknownChannel,
    #[error("invalid proxy request")]
    BadProxyRequest,
    #[error("upstream unavailable: {0}")]
    Upstream(String),
}

/// Envelope of tunerd's `/channels` response.
#[derive(Debug, Deserialize)]
struct TunerGrid {
    channels: Vec<channels::TunerChannelInfo>,
}

/// One entry of the country picker, from iptv-org's `countries.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct Country {
    #[serde(deserialize_with = "lowercase")]
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub flag: String,
}

fn lowercase<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    String::deserialize(d).map(|s| s.to_lowercase())
}

/// Channels per country NAME (iptv-org's `index.country.m3u` groups by the
/// `countries.json` name, not the code).
type ChannelCounts = HashMap<String, usize>;

/// Channels per `group-title` of the all-countries playlist, deduped exactly
/// like a country list is ([`channels::build_channels`]), so a count matches
/// what the country then shows before extra playlists merge in.
fn channel_counts(entries: Vec<m3u::M3uEntry>) -> ChannelCounts {
    let mut groups: HashMap<String, Vec<m3u::M3uEntry>> = HashMap::new();
    for e in entries {
        if let Some(g) = e.attrs.get("group-title").filter(|g| !g.is_empty()) {
            groups.entry(g.clone()).or_default().push(e);
        }
    }
    groups
        .into_iter()
        .map(|(name, entries)| {
            let n = channels::build_channels(&[(SourceOrigin::IptvOrg, entries)], None).len();
            (name, n)
        })
        .collect()
}

/// The picker: only countries with something to watch, each with its count.
/// A loaded country's own list is the exact count; otherwise the index. A
/// country with extra playlists, Vavoo groups or dlive ids configured stays
/// even when the index misses it (its count is unknown until loaded). Without
/// any index (upstream down) every country stays, count unknown — a picker
/// that hides everything would be worse than one that offers an empty
/// country.
fn picker_entries(
    countries: &[Country],
    counts: Option<&ChannelCounts>,
    loaded: &HashMap<String, usize>,
    configured: &HashSet<&str>,
) -> Vec<(Country, Option<usize>)> {
    countries
        .iter()
        .filter_map(|c| {
            let count = loaded
                .get(&c.code)
                .copied()
                .or_else(|| counts.map(|m| m.get(&c.name).copied().unwrap_or(0)));
            match count {
                Some(0) if configured.contains(c.code.as_str()) => Some((c.clone(), None)),
                Some(0) => None,
                count => Some((c.clone(), count)),
            }
        })
        .collect()
}

/// Alternate feeds per lowercase channel id (`"m6.fr"`), from iptv-org's
/// stream database.
type StreamsDb = HashMap<String, Vec<channels::StreamSource>>;

/// Folded channel name → logo URL, from iptv-org's channels + logos DBs.
type NameLogos = HashMap<String, String>;

/// One row of the cross-country search index. `channel_id` uses the SAME
/// slug derivation as `build_channels` (`normalize(tvg_id_base(id))`), so a
/// hit is directly openable as `(country, channel_id)` by every client.
#[derive(Clone)]
pub struct SearchEntry {
    pub country: String,
    pub channel_id: String,
    pub name: String,
    /// Folded name + alt names, for diacritics/case-insensitive matching.
    keys: String,
    /// Raw upstream logo URL from the channels DB.
    pub logo_url: Option<String>,
}

/// A served master playlist plus which upstream produced it — surfaced as
/// response headers so "which feed am I actually watching?" is one glance
/// at the network tab when a household member reports bad sound/video.
pub struct MasterPlaylist {
    pub body: String,
    pub source_index: usize,
    pub upstream_host: String,
    /// How many fallback sources the channel has in total. Surfaced to
    /// clients so a player that keeps failing rotates through ALL of them
    /// instead of giving up on a fixed budget — M6 is carried by four Vavoo
    /// feeds and a two-rotation cap could never reach the fourth.
    pub source_count: usize,
}

/// A source resolved to the URL and headers its playlist is fetched with.
struct Upstream {
    url: String,
    user_agent: Option<String>,
    referrer: Option<String>,
}

/// One election attempt on an internet source.
enum Attempt {
    Elected(String, Url),
    /// Skipped for a reason that is not the source's (no cooldown).
    Deferred(String),
    Failed(String),
}

/// A failed playlist fetch; `status` is set when the host answered.
struct FetchFailure {
    status: Option<u16>,
    error: LiveTvError,
}

impl FetchFailure {
    /// The host answered 200 with something unusable.
    fn answered(why: &str) -> Self {
        Self {
            status: Some(200),
            error: LiveTvError::Upstream(why.into()),
        }
    }

    /// Unreachable, too slow, overloaded: what a host outage looks like, as
    /// opposed to "this channel isn't here".
    fn is_outage(&self) -> bool {
        self.status.is_none_or(|s| s >= 500 || s == 429)
    }
}

/// A proxied upstream response. `from_dlive`: the channel's active source is
/// a dlive one, whose segments must unwrap to TS or count as failures.
pub struct ProxiedResponse {
    pub resp: reqwest::Response,
    pub final_url: Url,
    pub from_dlive: bool,
}

#[derive(Default)]
struct ActiveUpstream {
    user_agent: Option<String>,
    referrer: Option<String>,
    dlive: Option<(u32, u8)>,
}

/// Now/next programme pair for one channel.
pub struct NowNext {
    pub channel_id: String,
    pub now: Option<epg::Programme>,
    pub next: Option<epg::Programme>,
}

/// Liveness record for one upstream URL. Held in a service-level map keyed
/// by URL so it SURVIVES playlist refreshes — a feed that never answers
/// stays in escalating cooldown across snapshot rebuilds instead of being
/// resurrected every 12 h.
#[derive(Default)]
pub struct SourceHealth {
    /// Consecutive failures (reset on success). Drives the backoff.
    failures: AtomicU64,
    /// Epoch-millis until which the source must not be elected (0 = ok).
    cooldown_until_ms: AtomicU64,
    /// Consecutive failed segment/key fetches while this source is live
    /// (reset on any success). Demotes the source past a threshold.
    segment_failures: AtomicU64,
}

/// URI of the first variant in a master playlist, `None` when the body is
/// already a media playlist. Per RFC 8216 the URI is the first non-blank
/// line after `#EXT-X-STREAM-INF`.
fn first_variant_uri(master: &str) -> Option<&str> {
    let mut lines = master.lines();
    while let Some(line) = lines.next() {
        if line.starts_with("#EXT-X-STREAM-INF") {
            return lines
                .map(str::trim)
                .find(|l| !l.is_empty() && !l.starts_with('#'));
        }
    }
    None
}

/// Index of the source to elect first: the best-ranked one not cooling down
/// (sources arrive pre-ordered by tier+quality), else 0 so a fully-cooled
/// channel still gets tried by the election's second pass.
fn elect_seed(sources: &[Arc<SourceHealth>], now_ms: u64) -> usize {
    sources
        .iter()
        .position(|h| !h.in_cooldown(now_ms))
        .unwrap_or(0)
}

/// Fold a source's playlist entries into the cross-country search list: one
/// [`SearchEntry`] per entry with the id [`channels::build_channels`] would
/// assign (so a hit opens against the snapshot), deduped by (country, id). A
/// playlist's own `tvg-logo` wins; otherwise the iptv-org name→logo map fills
/// it (Vavoo carries none).
fn index_source_entries(
    country: &str,
    entries: &[m3u::M3uEntry],
    name_logo: &HashMap<String, String>,
    seen: &mut HashSet<(String, String)>,
    index: &mut Vec<SearchEntry>,
) {
    for entry in entries {
        if entry.url.is_empty() {
            continue;
        }
        let (display, channel_id) = channels::entry_display_and_id(entry);
        if display.is_empty()
            || channel_id.is_empty()
            || !seen.insert((country.to_string(), channel_id.clone()))
        {
            continue;
        }
        let logo = entry
            .attrs
            .get("tvg-logo")
            .filter(|s| !s.is_empty())
            .cloned()
            .or_else(|| name_logo.get(&channel_id).cloned());
        index.push(SearchEntry {
            country: country.to_string(),
            keys: channels::normalize(&display),
            logo_url: logo,
            channel_id,
            name: display,
        });
    }
}

impl SourceHealth {
    fn in_cooldown(&self, now_ms: u64) -> bool {
        self.cooldown_until_ms.load(Ordering::Relaxed) > now_ms
    }

    fn mark_failure(&self, now_ms: u64) {
        self.cool_down(now_ms, SOURCE_COOLDOWN_MAX);
    }

    /// A client said it couldn't PLAY this source. Same escalation, lower
    /// ceiling — see [`PLAYBACK_COOLDOWN_MAX`].
    fn mark_playback_failure(&self, now_ms: u64) {
        self.cool_down(now_ms, PLAYBACK_COOLDOWN_MAX);
    }

    fn cool_down(&self, now_ms: u64, ceiling: Duration) {
        let failures = self.failures.fetch_add(1, Ordering::Relaxed) + 1;
        let exp = u32::try_from(failures.saturating_sub(1))
            .unwrap_or(u32::MAX)
            .min(16);
        let backoff = SOURCE_COOLDOWN_BASE
            .saturating_mul(2u32.saturating_pow(exp))
            .min(ceiling);
        let millis = u64::try_from(backoff.as_millis()).unwrap_or(u64::MAX);
        self.cooldown_until_ms
            .store(now_ms.saturating_add(millis), Ordering::Relaxed);
    }

    fn mark_success(&self) {
        self.failures.store(0, Ordering::Relaxed);
        self.cooldown_until_ms.store(0, Ordering::Relaxed);
    }
}

/// A country's channel list plus the runtime fallback state that must
/// survive across requests (which source is live, which are cooling down).
pub struct CountrySnapshot {
    pub channels: Arc<Vec<Channel>>,
    fetched_at: Instant,
    /// Parallel to `channels`: elected source index per channel.
    active_source: Vec<AtomicUsize>,
    /// Parallel to `channels[i].sources`: shared per-URL health records.
    health: Vec<Vec<Arc<SourceHealth>>>,
}

impl CountrySnapshot {
    fn channel_index(&self, id: &str) -> Option<usize> {
        self.channels.iter().position(|c| c.id == id)
    }

    /// Every feed of channel `i` failed its last check (the probe, a zap) and
    /// is cooling down: it may come back, but it will likely not play now.
    pub fn unreachable(&self, i: usize) -> bool {
        let now = epoch_ms();
        self.health
            .get(i)
            .is_some_and(|h| !h.is_empty() && h.iter().all(|s| s.in_cooldown(now)))
    }
}

struct ServiceInner {
    cfg: iris_config::LiveTvConfig,
    http: reqwest::Client,
    signer: proxy::Signer,
    countries: RwLock<Option<(Arc<Vec<Country>>, Instant)>>,
    /// Channels per country, sizing the picker (see [`channel_counts`]).
    channel_counts: RefreshCell<ChannelCounts>,
    snapshots: RwLock<HashMap<String, Arc<CountrySnapshot>>>,
    /// Programme guide per country.
    epg: RwLock<HashMap<String, Arc<RefreshCell<epg::EpgIndex>>>>,
    /// iptv-org's full stream database (all alternate feeds per channel),
    /// cached like the playlists.
    streams_db: RefreshCell<StreamsDb>,
    /// Cross-country channel-search index (from iptv-org's channels DB,
    /// intersected with the streams DB so every hit is playable), cached
    /// like the playlists.
    search_index: RefreshCell<Vec<SearchEntry>>,
    /// Folded channel name → logo URL (from iptv-org's channels + logos DBs,
    /// no playability filter), for back-filling channels whose feed carries
    /// no logo — chiefly Vavoo entries. Cached like the playlists.
    name_logos: RefreshCell<NameLogos>,
    /// Per-URL liveness, shared across snapshots (see [`SourceHealth`]).
    health: RwLock<HashMap<String, Arc<SourceHealth>>>,
    /// Per-country cold-load locks, so N concurrent first-requests for a
    /// country fetch its playlist once without queueing other countries.
    load_locks: std::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// Fetched channel logos, keyed by upstream URL. Third-party logo hosts
    /// (GitHub raw, various CDNs) rate-limit a burst of ~200 concurrent GETs
    /// from one server IP — so a whole grid loading cold used to 429. We fetch
    /// each logo at most once per TTL and serve every later request (reloads,
    /// other household members) from here. Negative results are cached too
    /// (short TTL) so a dead host isn't re-hammered.
    logo_cache: RwLock<HashMap<String, Arc<CachedLogo>>>,
    /// Caps concurrent upstream logo fetches so a cold grid warms the cache
    /// without tripping the host's rate limit.
    logo_sem: tokio::sync::Semaphore,
    /// Logo-only HTTP client that does NOT verify TLS certificates. Channel
    /// logos are cosmetic third-party assets on a long tail of hosts (imgur,
    /// random broadcaster CDNs) whose certs are routinely expired, self-signed
    /// or from an issuer this box doesn't trust — and a skewed server clock
    /// makes even valid certs read as expired. A bad logo is harmless (served
    /// same-origin, rendered only in `<img>`/Coil where nothing executes), so
    /// we don't let cert pedantry blank the whole grid. Streams / EPG / TMDB
    /// keep the strict [`Self::http`] client.
    logo_http: reqwest::Client,
    /// Last-resort per-channel deinterlace/transcode sessions (see
    /// [`transcode::TranscodeManager`]).
    transcode: transcode::TranscodeManager,
    /// Vavoo aggregator: catalog fetch + on-demand `vavoo://` resolution
    /// (see [`vavoo::Vavoo`]).
    vavoo: vavoo::Vavoo,
    /// dlive.sx: daily list, budgeted embed scraping, `dlive://` resolution
    /// (see [`dlive::Dlive`]).
    dlive: Arc<dlive::Dlive>,
    /// Last dlive edge generation whose Player 1 cooldowns were cleared.
    dlive_edge_seen: AtomicU64,
}

/// A cached logo — the bytes + content-type on success, or just the forwarded
/// status on failure (so the negative result also short-circuits refetching).
struct CachedLogo {
    status: u16,
    content_type: String,
    bytes: Vec<u8>,
    fetched_at: Instant,
}

impl CachedLogo {
    fn is_fresh(&self, now: Instant) -> bool {
        let ttl = if self.status == 200 {
            LOGO_CACHE_TTL
        } else if logo_status_retryable(self.status) {
            LOGO_RETRY_TTL
        } else {
            LOGO_NEG_TTL
        };
        now.duration_since(self.fetched_at) < ttl
    }
}

/// Served logo — bytes + content-type on success, or the upstream error status
/// to forward (empty body → the client's letter-tile fallback).
pub struct LogoResponse {
    pub status: u16,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone)]
pub struct LiveTvService {
    inner: Arc<ServiceInner>,
}

impl LiveTvService {
    pub fn new(cfg: iris_config::LiveTvConfig, jwt_secret: &str) -> anyhow::Result<Self> {
        let http = iris_providers::tls::client_builder()
            .user_agent(DEFAULT_UA)
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()?;
        // Logo-only client: same knobs, but tolerant of bad TLS certs on the
        // cosmetic third-party logo hosts (see `logo_http`). Shorter timeout —
        // a logo is never worth blocking a tile for long.
        let logo_http = reqwest::Client::builder()
            .user_agent(DEFAULT_UA)
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::limited(5))
            .danger_accept_invalid_certs(true)
            .build()?;
        let dlive = Arc::new(dlive::Dlive::new(cfg.dlive.clone(), http.clone()));
        Ok(Self {
            inner: Arc::new(ServiceInner {
                signer: proxy::Signer::new(jwt_secret),
                cfg,
                http,
                countries: RwLock::new(None),
                channel_counts: RefreshCell::default(),
                snapshots: RwLock::new(HashMap::new()),
                epg: RwLock::new(HashMap::new()),
                streams_db: RefreshCell::default(),
                search_index: RefreshCell::default(),
                name_logos: RefreshCell::default(),
                health: RwLock::new(HashMap::new()),
                load_locks: std::sync::Mutex::new(HashMap::new()),
                logo_cache: RwLock::new(HashMap::new()),
                logo_sem: tokio::sync::Semaphore::new(LOGO_FETCH_CONCURRENCY),
                logo_http,
                transcode: transcode::TranscodeManager::default(),
                vavoo: vavoo::Vavoo::default(),
                dlive,
                dlive_edge_seen: AtomicU64::new(0),
            }),
        })
    }

    pub fn default_country(&self) -> &str {
        &self.inner.cfg.default_country
    }

    pub fn signer(&self) -> &proxy::Signer {
        &self.inner.signer
    }

    /// Country picker catalogue, cached for a day.
    pub async fn countries(&self) -> Result<Arc<Vec<Country>>, LiveTvError> {
        if let Some((cached, at)) = self.inner.countries.read().expect("poisoned").clone()
            && at.elapsed() < Duration::from_hours(24)
        {
            return Ok(cached);
        }
        let fetched = self
            .inner
            .http
            .get(&self.inner.cfg.countries_url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(upstream_err)?;
        let fetched: Vec<Country> = read_json(fetched).await?;
        let fetched = Arc::new(fetched);
        *self.inner.countries.write().expect("poisoned") = Some((fetched.clone(), Instant::now()));
        Ok(fetched)
    }

    /// The countries worth offering, with their channel counts (see
    /// [`picker_entries`]).
    pub async fn picker(&self) -> Result<Vec<(Country, Option<usize>)>, LiveTvError> {
        let countries = self.countries().await?;
        let counts = self.channel_counts(Duration::MAX).await;
        let loaded: HashMap<String, usize> = self
            .inner
            .snapshots
            .read()
            .expect("poisoned")
            .iter()
            .map(|(code, s)| (code.clone(), s.channels.len()))
            .collect();
        let cfg = &self.inner.cfg;
        let mut configured: HashSet<&str> =
            cfg.extra_playlists.keys().map(String::as_str).collect();
        if cfg.vavoo_enabled {
            configured.extend(cfg.vavoo_countries.keys().map(String::as_str));
        }
        if cfg.dlive.enabled {
            configured.extend(cfg.dlive.countries.keys().map(String::as_str));
        }
        Ok(picker_entries(
            &countries,
            counts.as_deref(),
            &loaded,
            &configured,
        ))
    }

    async fn channel_counts(&self, ttl: Duration) -> Option<Arc<ChannelCounts>> {
        self.inner
            .channel_counts
            .get(ttl, FAILED_LOAD_RETRY, || async {
                let resp = self
                    .inner
                    .http
                    .get(&self.inner.cfg.country_index_url)
                    .send()
                    .await
                    .and_then(reqwest::Response::error_for_status)
                    .inspect_err(
                        |e| tracing::warn!(error = %e, "live tv country index fetch failed"),
                    )
                    .ok()?;
                let body = read_capped(resp, MAX_JSON_BYTES)
                    .await
                    .inspect_err(
                        |e| tracing::warn!(error = %e, "live tv country index read failed"),
                    )
                    .ok()?;
                let counts = channel_counts(m3u::parse(&String::from_utf8_lossy(&body)));
                tracing::info!(countries = counts.len(), "live tv country index loaded");
                Some(counts)
            })
            .await
    }

    /// Channel list for a country, fetching its playlist on first access.
    pub async fn channels(&self, country: &str) -> Result<Arc<CountrySnapshot>, LiveTvError> {
        let country = validate_country(country)?;
        if let Some(snap) = self.inner.snapshots.read().expect("poisoned").get(&country) {
            return Ok(snap.clone());
        }
        // Cold load — single-flight so a burst of first requests fetches once.
        let lock = self
            .inner
            .load_locks
            .lock()
            .expect("poisoned")
            .entry(country.clone())
            .or_default()
            .clone();
        let _guard = lock.lock().await;
        if let Some(snap) = self.inner.snapshots.read().expect("poisoned").get(&country) {
            return Ok(snap.clone());
        }
        let snap = Arc::new(self.fetch_country(&country).await?);
        self.inner
            .snapshots
            .write()
            .expect("poisoned")
            .insert(country.clone(), snap.clone());
        // Elect each channel's source on real liveness in the background —
        // first zappers are covered by the in-request rotation meanwhile.
        self.clone().spawn_probe(country, snap.clone());
        Ok(snap)
    }

    /// Fetch + parse every playlist configured for a country. The iptv-org
    /// playlist comes first (it defines channel identity/ordering); extra
    /// playlists merge in as fallback sources.
    async fn fetch_country(&self, country: &str) -> Result<CountrySnapshot, LiveTvError> {
        let mut urls = vec![
            self.inner
                .cfg
                .playlist_url_template
                .replace("{code}", country),
        ];
        if let Some(extra) = self.inner.cfg.extra_playlists.get(country) {
            urls.extend(extra.iter().cloned());
        }

        let mut playlists = Vec::new();
        for (i, url) in urls.iter().enumerate() {
            if i == 0 {
                // iptv-org publishes no playlist for a country without streams:
                // that country is empty, not an error.
                if let Some(body) = self.fetch_primary_playlist(url).await? {
                    playlists.push((SourceOrigin::IptvOrg, m3u::parse(&body)));
                }
                continue;
            }
            match self.fetch_text(url, PLAYLIST_TIMEOUT).await {
                Ok((body, _)) => playlists.push((SourceOrigin::Extra, m3u::parse(&body))),
                Err(e) => {
                    tracing::warn!(url, error = %e, "live tv extra playlist fetch failed");
                }
            }
        }

        // Vavoo channels for this country (resolved lazily on zap). Best-effort
        // — a Vavoo outage just means no Vavoo channels this round, never a
        // failed country load.
        if self.inner.cfg.vavoo_enabled
            && let Some(groups) = self.inner.cfg.vavoo_countries.get(country)
        {
            let entries = self
                .inner
                .vavoo
                .entries_for_groups(&self.inner.http, groups)
                .await;
            if !entries.is_empty() {
                tracing::info!(
                    country,
                    vavoo_channels = entries.len(),
                    "live tv vavoo channels merged"
                );
                playlists.push((SourceOrigin::Vavoo, entries));
            }
        }
        // dlive channels (resolved lazily on zap), one list per player. Same
        // best effort: no channel list yet means no dlive sources this round.
        if self.inner.dlive.enabled() {
            for (origin, entries) in self.inner.dlive.entries(country).await {
                if !entries.is_empty() {
                    tracing::info!(
                        country,
                        ?origin,
                        dlive_channels = entries.len(),
                        "live tv dlive channels merged"
                    );
                    playlists.push((origin, entries));
                }
            }
        }

        let tnt = (country == "fr").then_some(&self.inner.cfg.tnt_overrides);
        let mut built = channels::build_channels(&playlists, tnt);
        if built.is_empty() {
            tracing::info!(country, "live tv country has no channels");
            return Ok(self.build_snapshot(built));
        }
        // Graft the alternate feeds from iptv-org's database — the playlist
        // carries a single feed per channel; the database has them all.
        if let Some(db) = self.streams_db().await {
            channels::merge_db_sources(&mut built, &db);
        }
        // The household DVB-T tuner: absolute-priority sources for the
        // channels it carries (fr only — it receives French TNT). The box
        // describes its own grid; a dead/absent box merges nothing and the
        // internet tiers serve alone.
        if country == "fr"
            && let Some(grid) = self.tuner_grid().await
        {
            channels::merge_tuner_sources(&mut built, &self.inner.cfg.tuner.base_url, &grid);
            tracing::info!(tuner_channels = grid.len(), "tuner grid merged");
        }
        // Back-fill logos for channels whose feed carries none (chiefly Vavoo,
        // which ships no usable logo) by matching the channel name against
        // iptv-org's logo DB. Best-effort; a miss falls back to the letter tile.
        if built.iter().any(|c| c.logo_url.is_none())
            && let Some(logos) = self.name_logo_index().await
        {
            for ch in &mut built {
                if ch.logo_url.is_none()
                    && let Some(url) = logos.get(&channels::normalize(&ch.name))
                {
                    ch.logo_url = Some(url.clone());
                }
            }
        }
        tracing::info!(country, channels = built.len(), "live tv playlist loaded");
        Ok(self.build_snapshot(built))
    }

    /// Fetch the tuner box's self-described channel grid (`/channels`).
    /// Best-effort with a short timeout: the tuner being unreachable must
    /// never delay or fail a playlist load.
    async fn tuner_grid(&self) -> Option<Vec<channels::TunerChannelInfo>> {
        let cfg = &self.inner.cfg.tuner;
        if !cfg.enabled || cfg.base_url.is_empty() {
            return None;
        }
        let url = format!("{}/channels", cfg.base_url.trim_end_matches('/'));
        let resp = self
            .inner
            .http
            .get(&url)
            .timeout(Duration::from_secs(4))
            .send()
            .await
            .and_then(reqwest::Response::error_for_status);
        let body = match resp {
            Ok(r) => read_json::<TunerGrid>(r).await,
            Err(e) => {
                tracing::debug!(error = %upstream_err(e), "tuner grid unreachable");
                return None;
            }
        };
        match body {
            Ok(grid) if !grid.channels.is_empty() => Some(grid.channels),
            Ok(_) => None,
            Err(e) => {
                tracing::debug!(error = %e, "tuner grid unparsable");
                None
            }
        }
    }

    /// Resolve the shared per-URL health records for a fresh channel list —
    /// this is what carries liveness knowledge across playlist refreshes.
    fn build_snapshot(&self, built: Vec<Channel>) -> CountrySnapshot {
        let mut health_map = self.inner.health.write().expect("poisoned");
        let health: Vec<Vec<Arc<SourceHealth>>> = built
            .iter()
            .map(|c| {
                c.sources
                    .iter()
                    .map(|s| health_map.entry(s.url.clone()).or_default().clone())
                    .collect()
            })
            .collect();
        // Drop records no snapshot references any more (tokenised URLs mint a
        // new key every refresh). The snapshot being replaced is still alive
        // here, so a URL that survives the refresh keeps its record.
        health_map.retain(|_, h| Arc::strong_count(h) > 1);
        drop(health_map);
        // Seed the elected index at the BEST source that isn't cooling down
        // rather than a blind 0 — sources are already ordered (tier, quality),
        // health survives refreshes, so a just-dead feed stays skipped instead
        // of being re-elected every refresh and forcing the first viewer to
        // pay a rotation.
        let now_ms = epoch_ms();
        let active_source = health
            .iter()
            .map(|sources| AtomicUsize::new(elect_seed(sources, now_ms)))
            .collect();
        CountrySnapshot {
            channels: Arc::new(built),
            fetched_at: Instant::now(),
            active_source,
            health,
        }
    }

    /// iptv-org stream database, cached with the playlist TTL. Best-effort:
    /// `None` disables the merge, never fails a channel load.
    async fn streams_db(&self) -> Option<Arc<StreamsDb>> {
        let ttl = Duration::from_hours(self.inner.cfg.playlist_refresh_hours.max(1));
        self.inner
            .streams_db
            .get(ttl, FAILED_LOAD_RETRY, || self.load_streams_db())
            .await
    }

    async fn load_streams_db(&self) -> Option<StreamsDb> {
        #[derive(serde::Deserialize)]
        struct ApiStream {
            #[serde(default)]
            channel: Option<String>,
            url: String,
            #[serde(default)]
            quality: Option<String>,
            #[serde(default)]
            user_agent: Option<String>,
            #[serde(default)]
            referrer: Option<String>,
        }

        let fetched: Vec<ApiStream> = match self
            .inner
            .http
            .get(&self.inner.cfg.streams_url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
        {
            Ok(resp) => match read_json(resp).await {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(error = %e, "live tv streams db parse failed");
                    return None;
                }
            },
            Err(e) => {
                tracing::warn!(error = %upstream_err(e), "live tv streams db fetch failed");
                return None;
            }
        };

        let mut map: HashMap<String, Vec<channels::StreamSource>> = HashMap::new();
        for s in fetched {
            let Some(channel) = s.channel.filter(|c| !c.is_empty()) else {
                continue;
            };
            if !(s.url.starts_with("http://") || s.url.starts_with("https://")) {
                continue;
            }
            map.entry(channel.to_lowercase())
                .or_default()
                .push(channels::StreamSource {
                    tier: channels::classify_source(&s.url),
                    url: s.url,
                    quality: s.quality.as_deref().and_then(channels::parse_quality),
                    user_agent: s.user_agent.filter(|v| !v.is_empty()),
                    referrer: s.referrer.filter(|v| !v.is_empty()),
                    origin: SourceOrigin::IptvOrg,
                });
        }
        tracing::info!(channels = map.len(), "live tv streams db loaded");
        Some(map)
    }

    /// A country's iptv-org playlist; `None` when iptv-org has none (404).
    async fn fetch_primary_playlist(&self, url: &str) -> Result<Option<String>, LiveTvError> {
        let resp = self
            .inner
            .http
            .get(url)
            .timeout(PLAYLIST_TIMEOUT)
            .send()
            .await
            .map_err(upstream_err)?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let resp = resp.error_for_status().map_err(upstream_err)?;
        Ok(Some(read_playlist(resp).await?))
    }

    async fn fetch_text(&self, url: &str, timeout: Duration) -> Result<(String, Url), LiveTvError> {
        let resp = self
            .inner
            .http
            .get(url)
            .timeout(timeout)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(upstream_err)?;
        let final_url = resp.url().clone();
        Ok((read_playlist(resp).await?, final_url))
    }

    /// Fetch a channel's master playlist, rotating through fallback sources
    /// until one answers, and rewrite every URI to the signed proxy.
    ///
    /// Election is stability-first: sources in cooldown are never picked
    /// while an alternative is healthy, failures escalate the cooldown
    /// exponentially (see [`SourceHealth`]), and the winner is remembered
    /// for every subsequent viewer.
    ///
    pub async fn master_playlist(
        &self,
        country: &str,
        id: &str,
    ) -> Result<MasterPlaylist, LiveTvError> {
        let country = validate_country(country)?;
        let snap = self.channels(&country).await?;
        let idx = snap.channel_index(id).ok_or(LiveTvError::UnknownChannel)?;
        let channel = &snap.channels[idx];
        let channel_key = format!("{country}:{id}");
        let now_ms = epoch_ms();
        self.dlive_housekeeping(channel);

        let n = channel.sources.len();
        let start = election_start(channel, snap.active_source[idx].load(Ordering::Relaxed));
        let mut last_err = String::new();
        // Pass 0: healthy sources. Pass 1: the ones cooling down — it must
        // run whenever pass 0 didn't succeed, not only when it tried nothing,
        // otherwise one transient failure on the only-live source makes the
        // channel 502 for its whole cooldown. Pass 2: dlive sources skipped
        // for a reason that is not theirs (breaker open, embed not scraped
        // yet), retried only because nothing else played.
        let mut tried = vec![false; n];
        let mut deferred: Vec<usize> = Vec::new();
        for pass in 0..3 {
            let order: Vec<usize> = if pass < 2 {
                (0..n).map(|step| (start + step) % n).collect()
            } else {
                std::mem::take(&mut deferred)
            };
            for (k, &si) in order.iter().enumerate() {
                let health = &snap.health[idx][si];
                if pass < 2 && (tried[si] || (pass == 0 && health.in_cooldown(now_ms))) {
                    continue;
                }
                tried[si] = true;
                let source = &channel.sources[si];
                if source.tier == SourceTier::Tuner {
                    match self
                        .attempt_tuner(&snap, idx, si, &country, id, now_ms)
                        .await
                    {
                        Ok(elected) => return Ok(elected),
                        Err(e) => {
                            last_err = e;
                            continue;
                        }
                    }
                }
                let last_option = if pass < 2 {
                    deferred.is_empty() && tried.iter().all(|t| *t)
                } else {
                    k + 1 == order.len()
                };
                match self.attempt_source(source, last_option, pass == 2).await {
                    Attempt::Elected(body, base) => {
                        snap.active_source[idx].store(si, Ordering::Relaxed);
                        health.mark_success();
                        tracing::info!(
                            channel = %channel_key,
                            source = si,
                            origin = ?source.origin,
                            tier = ?source.tier,
                            host = %base.host_str().unwrap_or("unknown"),
                            "live tv source elected"
                        );
                        return Ok(MasterPlaylist {
                            body: proxy::rewrite_playlist(
                                &body,
                                &base,
                                &channel_key,
                                &self.inner.signer,
                            ),
                            source_index: si,
                            upstream_host: base.host_str().unwrap_or("unknown").to_string(),
                            source_count: n,
                        });
                    }
                    Attempt::Deferred(why) => {
                        if pass < 2 {
                            deferred.push(si);
                        }
                        last_err = why;
                    }
                    Attempt::Failed(e) => {
                        health.mark_failure(now_ms);
                        tracing::debug!(
                            channel = %channel_key,
                            source = si,
                            error = %e,
                            "live tv source failed, rotating"
                        );
                        last_err = e;
                    }
                }
            }
        }
        Err(LiveTvError::Upstream(last_err))
    }

    /// The tuner branch of an election. `Err` carries why the election moves
    /// on to the internet sources.
    async fn attempt_tuner(
        &self,
        snap: &CountrySnapshot,
        idx: usize,
        si: usize,
        country: &str,
        id: &str,
        now_ms: u64,
    ) -> Result<MasterPlaylist, String> {
        let channel_key = format!("{country}:{id}");
        let source = &snap.channels[idx].sources[si];
        let Some(pinned) = self.tuner_admission(country, id, &source.url).await else {
            // At mux capacity: skip WITHOUT marking failure — capacity is
            // not a source defect, and the next election re-checks (the
            // tuner is always tried first).
            return Err("tuner at mux capacity".into());
        };
        // The household tuner serves raw MPEG-TS, not HLS: feed it through
        // the shared ffmpeg manager in remux mode (-c copy) and point the
        // master at the tuner segment route. Any failure (box off, no lock)
        // marks the source and falls through to the internet tiers.
        match self
            .inner
            .transcode
            .master_playlist(
                Mode::Remux,
                &channel_key,
                &source.url,
                DEFAULT_UA,
                None,
                pinned,
            )
            .await
        {
            Ok(body) => Ok(tuner_elected(snap, idx, si, &channel_key, &body)),
            Err(e) => {
                snap.health[idx][si].mark_failure(now_ms);
                tracing::debug!(
                    channel = %channel_key,
                    error = %e,
                    "tuner source failed, rotating to internet tiers"
                );
                Err(e.to_string())
            }
        }
    }

    /// Per-zap dlive upkeep, all off the request path: queue the embeds this
    /// channel's dlive sources lack, and clear Player 1 cooldowns earned on
    /// an edge host that has since been replaced.
    fn dlive_housekeeping(&self, channel: &Channel) {
        let dlive = &self.inner.dlive;
        if !dlive.enabled() {
            return;
        }
        dlive.prefetch(
            channel
                .sources
                .iter()
                .filter_map(|s| dlive::parse_sentinel(&s.url)),
        );
        let generation = dlive.edge_generation();
        if self
            .inner
            .dlive_edge_seen
            .swap(generation, Ordering::Relaxed)
            != generation
        {
            for (url, health) in self.inner.health.read().expect("poisoned").iter() {
                if dlive::parse_sentinel(url).is_some_and(|(_, player)| player == 1) {
                    health.mark_success();
                }
            }
        }
    }

    /// Resolve and fetch one internet source for the election. A dlive
    /// source gets a short first-byte deadline unless it is the channel's
    /// `last_option`; `last_resort` lets it past an open breaker.
    async fn attempt_source(
        &self,
        source: &channels::StreamSource,
        last_option: bool,
        last_resort: bool,
    ) -> Attempt {
        let Some((id, player)) = dlive::parse_sentinel(&source.url) else {
            let upstream = match self.resolve_upstream(source).await {
                Ok(u) => u,
                Err(e) => return Attempt::Failed(e.to_string()),
            };
            return match self
                .fetch_playlist_at(&upstream, PLAYLIST_TIMEOUT, PLAYLIST_TIMEOUT)
                .await
            {
                Ok((body, base)) => Attempt::Elected(body, base),
                Err(f) => Attempt::Failed(f.error.to_string()),
            };
        };
        let dlive = &self.inner.dlive;
        if !dlive.enabled() {
            return Attempt::Failed("dlive disabled".into());
        }
        let first_byte = if last_option && !dlive.breaker_open() {
            dlive.cold_start_timeout()
        } else {
            dlive.first_byte_timeout()
        };
        let mode = dlive::ResolveMode {
            bypass_breaker: last_resort,
            inline_embed: last_resort,
        };
        let mut re_resolved = false;
        loop {
            let resolved = match dlive.resolve(id, player, mode).await {
                Ok(r) => r,
                Err(dlive::ResolveError::Skip(why)) => return Attempt::Deferred(why.into()),
                Err(e @ dlive::ResolveError::Missing(_)) => return Attempt::Failed(e.to_string()),
                Err(e @ dlive::ResolveError::Outage(_)) => {
                    dlive.note_outage();
                    return Attempt::Failed(e.to_string());
                }
            };
            let upstream = Upstream {
                url: resolved.url.clone(),
                user_agent: resolved.user_agent.clone(),
                referrer: resolved.referrer.clone(),
            };
            match self
                .fetch_playlist_at(&upstream, first_byte, first_byte + PLAYLIST_TIMEOUT)
                .await
            {
                Ok((body, base)) => {
                    dlive.note_ok();
                    return Attempt::Elected(body, base);
                }
                // A signature or token that expired early: resolve afresh
                // once, then give up on this source.
                Err(f) if f.status == Some(403) && resolved.is_signed() && !re_resolved => {
                    dlive.invalidate(id, player);
                    re_resolved = true;
                }
                Err(f) => {
                    if resolved.is_signed() {
                        dlive.invalidate(id, player);
                    }
                    if f.is_outage() {
                        dlive.note_outage();
                        if player == 1 && f.status.is_none() {
                            dlive.spawn_learn_edge();
                        }
                    } else {
                        dlive.note_ok();
                    }
                    return Attempt::Failed(f.error.to_string());
                }
            }
        }
    }

    /// Tuner-branch election prelude: mux admission + pinned flag.
    ///
    /// Mux admission first: two RF frontends = two concurrent frequencies.
    /// A third-mux request reclaims a mux nobody is watching; when BOTH
    /// tuned muxes have live viewers, `None` is returned and THIS viewer
    /// falls through to the internet tiers instead of cutting someone
    /// else's antenna stream. `Some(pinned)` grants admission; the flag
    /// marks prewarm mux members (kept warm for hours) even when a
    /// viewer, not the prewarm loop, spawned the session.
    async fn tuner_admission(&self, country: &str, id: &str, url: &str) -> Option<bool> {
        if let Some(freq) = transcode::tuner_freq(url)
            && !self.inner.transcode.admit_mux(&freq).await
        {
            tracing::info!(
                channel = %format!("{country}:{id}"),
                freq = %freq,
                "tuner at mux capacity — using internet source"
            );
            return None;
        }
        let pinned = self
            .mux_siblings(country, &self.inner.cfg.tuner.prewarm)
            .await
            .iter()
            .any(|warm_id| warm_id == id);
        Some(pinned)
    }

    /// Cross-country channel search. Matches folded channel names (and alt
    /// names) against a folded query; prefix hits rank first. Only playable
    /// channels are indexed (present in the streams DB), so every result can
    /// be opened as `(country, channel_id)`.
    pub async fn search(&self, query: &str, limit: usize) -> Vec<SearchEntry> {
        let q = channels::normalize(query);
        if q.len() < 2 {
            return Vec::new();
        }
        // The refresh loop rebuilds past the TTL; a search only loads a
        // missing index.
        let Some(index) = self.search_index(Duration::MAX).await else {
            return Vec::new();
        };
        let mut hits: Vec<(usize, &SearchEntry)> = index
            .iter()
            .filter_map(|entry| {
                if entry.keys.starts_with(&q) {
                    Some((0, entry))
                } else if entry.keys.contains(&q) {
                    Some((1, entry))
                } else {
                    None
                }
            })
            .collect();
        hits.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.name.len().cmp(&b.1.name.len())));
        hits.into_iter()
            .take(limit)
            .map(|(_, e)| e.clone())
            .collect()
    }

    /// Build (or serve the cached) search index from iptv-org's channels DB.
    async fn search_index(&self, ttl: Duration) -> Option<Arc<Vec<SearchEntry>>> {
        self.inner
            .search_index
            .get(ttl, FAILED_LOAD_RETRY, || self.load_search_index())
            .await
    }

    async fn load_search_index(&self) -> Option<Vec<SearchEntry>> {
        #[derive(serde::Deserialize)]
        struct ApiChannel {
            id: String,
            name: String,
            #[serde(default)]
            alt_names: Vec<String>,
            country: String,
            #[serde(default)]
            is_nsfw: bool,
            #[serde(default)]
            closed: Option<String>,
        }
        // Logos moved out of channels.json into a sibling logos.json
        // (one or more per channel id) — join them in.
        #[derive(serde::Deserialize)]
        struct ApiLogo {
            channel: String,
            url: String,
        }

        // Playability filter — a name hit without any stream is a dead card.
        let streams = self.streams_db().await?;
        let logos_url = self
            .inner
            .cfg
            .channels_url
            .replace("channels.json", "logos.json");
        let mut logo_by_channel: HashMap<String, String> = HashMap::new();
        if let Ok(resp) = self.inner.http.get(&logos_url).send().await
            && let Ok(logos) = read_json::<Vec<ApiLogo>>(resp).await
        {
            for l in logos {
                logo_by_channel.entry(l.channel).or_insert(l.url);
            }
        }

        let fetched: Vec<ApiChannel> = match self
            .inner
            .http
            .get(&self.inner.cfg.channels_url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
        {
            Ok(resp) => match read_json(resp).await {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(error = %e, "live tv channels db parse failed");
                    return None;
                }
            },
            Err(e) => {
                tracing::warn!(error = %upstream_err(e), "live tv channels db fetch failed");
                return None;
            }
        };

        // Folded channel-name → logo, built from the same DB, to give Vavoo
        // search cards a logo (they carry none of their own).
        let name_logo: HashMap<String, String> = fetched
            .iter()
            .filter_map(|c| {
                let logo = logo_by_channel.get(&c.id)?;
                let key = channels::normalize(&c.name);
                (!key.is_empty()).then(|| (key, logo.clone()))
            })
            .collect();

        let mut index: Vec<SearchEntry> = fetched
            .into_iter()
            .filter(|c| c.closed.is_none() && !c.is_nsfw)
            .filter(|c| streams.contains_key(&c.id.to_lowercase()))
            .filter_map(|c| {
                let channel_id = channels::normalize(channels::tvg_id_base(&c.id));
                if channel_id.is_empty() {
                    return None;
                }
                let mut keys = channels::normalize(&c.name);
                for alt in &c.alt_names {
                    keys.push('\u{1F}');
                    keys.push_str(&channels::normalize(alt));
                }
                Some(SearchEntry {
                    country: c.country.to_lowercase(),
                    channel_id,
                    keys,
                    logo_url: logo_by_channel.get(&c.id).cloned(),
                    name: c.name,
                })
            })
            .collect();

        // The iptv-org channels∩streams DB above misses everything that only
        // lives in an extra playlist (ParaTV, schumijo, Free-TV…) or in Vavoo —
        // yet those channels ARE playable (they land in the country snapshot),
        // so a real search must surface them too.
        self.augment_search_index(&name_logo, &mut index).await;
        tracing::info!(channels = index.len(), "live tv search index built");
        Some(index)
    }

    /// Add the extra-playlist and Vavoo channels to a freshly built iptv-org
    /// search index (each with the id `build_channels` assigns), deduped by
    /// (country, id). Fetches are best-effort: a failed playlist/catalog just
    /// contributes nothing.
    async fn augment_search_index(&self, name_logo: &NameLogos, index: &mut Vec<SearchEntry>) {
        let mut seen: HashSet<(String, String)> = index
            .iter()
            .map(|e| (e.country.clone(), e.channel_id.clone()))
            .collect();
        for (country, urls) in &self.inner.cfg.extra_playlists {
            for url in urls {
                match self.fetch_text(url, PLAYLIST_TIMEOUT).await {
                    Ok((body, _)) => {
                        index_source_entries(
                            country,
                            &m3u::parse(&body),
                            name_logo,
                            &mut seen,
                            index,
                        );
                    }
                    Err(e) => {
                        tracing::warn!(url, error = %e, "live tv search: extra playlist fetch failed");
                    }
                }
            }
        }
        if self.inner.cfg.vavoo_enabled {
            for (country, groups) in &self.inner.cfg.vavoo_countries {
                let entries = self
                    .inner
                    .vavoo
                    .entries_for_groups(&self.inner.http, groups)
                    .await;
                index_source_entries(country, &entries, name_logo, &mut seen, index);
            }
        }
        if self.inner.dlive.enabled() {
            let countries: Vec<String> = self.inner.dlive.countries().cloned().collect();
            for country in countries {
                for (_, entries) in self.inner.dlive.entries(&country).await {
                    index_source_entries(&country, &entries, name_logo, &mut seen, index);
                }
            }
        }
    }

    /// Folded channel-name → logo URL, from iptv-org's channels + logos DBs
    /// (no playability filter, unlike the search index), for back-filling
    /// feeds that carry no logo — chiefly Vavoo. Cached; best-effort (`None`
    /// on any fetch/parse failure just skips the back-fill).
    async fn name_logo_index(&self) -> Option<Arc<NameLogos>> {
        let ttl = Duration::from_hours(self.inner.cfg.playlist_refresh_hours.max(1));
        self.inner
            .name_logos
            .get(ttl, FAILED_LOAD_RETRY, || self.load_name_logo_index())
            .await
    }

    async fn load_name_logo_index(&self) -> Option<NameLogos> {
        #[derive(serde::Deserialize)]
        struct ApiChannel {
            id: String,
            name: String,
            #[serde(default)]
            alt_names: Vec<String>,
        }
        #[derive(serde::Deserialize)]
        struct ApiLogo {
            channel: String,
            url: String,
        }

        let logos_url = self
            .inner
            .cfg
            .channels_url
            .replace("channels.json", "logos.json");
        let logos = self
            .inner
            .http
            .get(&logos_url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .ok()?;
        let logos: Vec<ApiLogo> = read_json(logos).await.ok()?;
        let mut logo_by_id: HashMap<String, String> = HashMap::new();
        for l in logos {
            logo_by_id.entry(l.channel).or_insert(l.url);
        }

        let channels = self
            .inner
            .http
            .get(&self.inner.cfg.channels_url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .ok()?;
        let channels: Vec<ApiChannel> = read_json(channels).await.ok()?;
        let mut map: HashMap<String, String> = HashMap::new();
        for c in channels {
            let Some(logo) = logo_by_id.get(&c.id) else {
                continue;
            };
            for name in std::iter::once(&c.name).chain(c.alt_names.iter()) {
                let key = channels::normalize(name);
                if !key.is_empty() {
                    map.entry(key).or_insert_with(|| logo.clone());
                }
            }
        }
        tracing::info!(names = map.len(), "live tv name→logo index built");
        Some(map)
    }

    /// Last-resort transcoded playlist for a channel (see [`transcode`]).
    /// Reuses the CURRENT elected source — the client already exercised the
    /// normal proxy path (that's how it learned it can't decode the feed), so
    /// the election is warm and identical to what the plain master serves.
    pub async fn transcode_master(&self, country: &str, id: &str) -> Result<String, LiveTvError> {
        let country = validate_country(country)?;
        let snap = self.channels(&country).await?;
        let idx = snap.channel_index(id).ok_or(LiveTvError::UnknownChannel)?;
        let channel = &snap.channels[idx];
        let active = snap.active_source[idx].load(Ordering::Relaxed) % channel.sources.len();
        // ffmpeg reads the upstream directly, which dlive sources defeat
        // (image-wrapped segments, 5-minute tokens baked into every URI):
        // transcode the best other source instead.
        let si = transcode_source(channel, &snap.health[idx], active, epoch_ms())
            .ok_or_else(|| LiveTvError::Upstream("no transcodable source".into()))?;
        let source = &channel.sources[si];
        let upstream = self.resolve_upstream(source).await?;
        let upstream_url = upstream.url;
        // The elected source can be the tuner: its re-encode tunes an adapter
        // like a remux does, so it goes through the same mux admission.
        if let Some(freq) = transcode::tuner_freq(&upstream_url)
            && !self.inner.transcode.admit_mux(&freq).await
        {
            return Err(LiveTvError::Upstream("tuner at mux capacity".into()));
        }
        let channel_key = format!("{country}:{id}");
        self.inner
            .transcode
            .master_playlist(
                Mode::Reencode,
                &channel_key,
                &upstream_url,
                source.user_agent.as_deref().unwrap_or(DEFAULT_UA),
                source.referrer.as_deref(),
                false,
            )
            .await
    }

    /// One transcoded segment (name validated inside the manager).
    pub async fn transcode_segment(
        &self,
        country: &str,
        id: &str,
        name: &str,
    ) -> Result<Vec<u8>, LiveTvError> {
        let country = validate_country(country)?;
        let channel_key = format!("{country}:{id}");
        self.inner
            .transcode
            .segment(Mode::from_segment_name(name), &channel_key, name)
            .await
    }

    /// Fetch a channel logo through the backend (signed URL minted by the
    /// channel-list response). Kills the CORS noise of hotlinking hundreds
    /// of third-party hosts and lets clients read pixels for the
    /// luminance-adaptive logo well.
    pub async fn fetch_logo(&self, encoded: &str, sig: &str) -> Result<LogoResponse, LiveTvError> {
        let upstream = proxy::decode_upstream(encoded).ok_or(LiveTvError::BadProxyRequest)?;
        if !self
            .inner
            .signer
            .verify(proxy::LOGO_KEY, upstream.as_str(), sig)
        {
            return Err(LiveTvError::BadProxyRequest);
        }
        let key = upstream.as_str().to_string();
        let now = Instant::now();

        // Fast path: a fresh cache entry (success OR recent failure) means no
        // upstream request at all — this is what stops the grid 429'ing.
        if let Some(hit) = self.cached_logo(&key, now) {
            return Ok(hit);
        }

        // Cap concurrent upstream fetches so a cold grid doesn't fan hundreds
        // of parallel GETs at one host. The permit is released on drop.
        let _permit = self
            .inner
            .logo_sem
            .acquire()
            .await
            .map_err(|e| LiveTvError::Upstream(e.to_string()))?;

        // Re-check under the permit: while we waited, another task may have
        // fetched the same logo (single-flight-ish for the common case where
        // the grid requests each URL once).
        if let Some(hit) = self.cached_logo(&key, now) {
            return Ok(hit);
        }

        // Forward the upstream status as-is (a dead logo host → 404 → the
        // card's letter-tile fallback), rather than a 502 that spams the error
        // log for a purely cosmetic asset. A transport error is cached as a
        // 502 so we don't retry it on every tile this minute.
        let cached = match self.inner.logo_http.get(upstream).send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if resp.status().is_success() {
                    let content_type = resp
                        .headers()
                        .get(reqwest::header::CONTENT_TYPE)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("image/png")
                        .to_string();
                    match read_capped(resp, LOGO_FETCH_MAX_BYTES).await {
                        Ok(bytes) if bytes.len() <= LOGO_MAX_BYTES => CachedLogo {
                            status,
                            content_type,
                            bytes,
                            fetched_at: now,
                        },
                        // Oversized — serve once, don't cache the blob.
                        Ok(bytes) => {
                            return Ok(LogoResponse {
                                status,
                                content_type,
                                bytes,
                            });
                        }
                        Err(_) => CachedLogo {
                            status: 502,
                            content_type: String::new(),
                            bytes: Vec::new(),
                            fetched_at: now,
                        },
                    }
                } else {
                    CachedLogo {
                        status,
                        content_type: String::new(),
                        bytes: Vec::new(),
                        fetched_at: now,
                    }
                }
            }
            Err(_) => CachedLogo {
                status: 502,
                content_type: String::new(),
                bytes: Vec::new(),
                fetched_at: now,
            },
        };

        let response = LogoResponse {
            status: cached.status,
            content_type: cached.content_type.clone(),
            bytes: cached.bytes.clone(),
        };
        self.store_logo(key, Arc::new(cached));
        Ok(response)
    }

    /// Return a fresh cached logo for `key`, if any.
    fn cached_logo(&self, key: &str, now: Instant) -> Option<LogoResponse> {
        let cache = self.inner.logo_cache.read().ok()?;
        let entry = cache.get(key)?;
        entry.is_fresh(now).then(|| LogoResponse {
            status: entry.status,
            content_type: entry.content_type.clone(),
            bytes: entry.bytes.clone(),
        })
    }

    /// Insert a cached logo, wholesale-clearing if the cache got too big.
    fn store_logo(&self, key: String, entry: Arc<CachedLogo>) {
        if let Ok(mut cache) = self.inner.logo_cache.write() {
            if cache.len() >= LOGO_CACHE_MAX && !cache.contains_key(&key) {
                cache.clear();
            }
            cache.insert(key, entry);
        }
    }

    /// Signed same-origin URL for a channel logo (`None` for unusable URLs).
    pub fn logo_proxy_url(&self, logo_url: &str) -> Option<String> {
        let url = Url::parse(logo_url).ok()?;
        matches!(url.scheme(), "http" | "https").then(|| proxy::logo_url(&url, &self.inner.signer))
    }

    /// A client failed to PLAY the stream it was served (unsupported audio
    /// codec, corrupt video, segments 404ing…). HTTP liveness said the feed
    /// was fine, so only the player can teach us it isn't: cool the active
    /// source down and re-elect, so the client's next master reload gets the
    /// next candidate. Global by design — a feed dirty enough to kill one
    /// player is not worth electing for the household.
    pub async fn report_playback_failure(
        &self,
        country: &str,
        id: &str,
    ) -> Result<(), LiveTvError> {
        let country = validate_country(country)?;
        let snap = self.channels(&country).await?;
        let idx = snap.channel_index(id).ok_or(LiveTvError::UnknownChannel)?;
        let channel = &snap.channels[idx];
        let now_ms = epoch_ms();

        let active = snap.active_source[idx].load(Ordering::Relaxed) % channel.sources.len();
        snap.health[idx][active].mark_playback_failure(now_ms);
        // Re-elect: first source not cooling down, if any.
        let next = (0..channel.sources.len()).find(|&si| !snap.health[idx][si].in_cooldown(now_ms));
        if let Some(si) = next {
            snap.active_source[idx].store(si, Ordering::Relaxed);
        }
        tracing::info!(
            channel = %format!("{country}:{id}"),
            demoted = active,
            elected = ?next,
            "live tv playback failure reported by client"
        );
        Ok(())
    }

    /// Probe every source of a snapshot (bounded concurrency) and elect the
    /// first alive source per channel — quality order breaks ties among the
    /// living. Runs in the background after a load/refresh so viewers never
    /// pay for a dead feed's timeout.
    fn spawn_probe(self, country: String, snap: Arc<CountrySnapshot>) {
        tokio::spawn(async move {
            let semaphore = Arc::new(tokio::sync::Semaphore::new(PROBE_CONCURRENCY));
            let mut join = tokio::task::JoinSet::new();
            for (ci, channel) in snap.channels.iter().enumerate() {
                for (si, source) in channel.sources.iter().enumerate() {
                    // Vavoo sources resolve to a rotating tokenised URL — never
                    // probe them (it would burn a resolve per channel per
                    // refresh); they're elected lazily and resolved on the
                    // first real zap, self-healing via playback-failure demotion.
                    if vavoo::stream_id(&source.url).is_some() {
                        continue;
                    }
                    // Same for dlive: a probe would spend the dlive.sx page
                    // budget a whole country at a time.
                    if dlive::parse_sentinel(&source.url).is_some() {
                        continue;
                    }
                    // Tuner sources are raw-TS /tune endpoints: probing one
                    // would START A TUNE on the box (evicting a real viewer)
                    // and then fail playlist parsing, cooling the tuner down
                    // before any election. Like Vavoo they are elected
                    // lazily; the election's tuner branch marks its own
                    // failures.
                    if source.tier == SourceTier::Tuner {
                        continue;
                    }
                    let svc = self.clone();
                    let source = source.clone();
                    let health = snap.health[ci][si].clone();
                    let semaphore = semaphore.clone();
                    join.spawn(async move {
                        let _permit = semaphore.acquire_owned().await;
                        match svc.fetch_source_playlist(&source).await {
                            Ok(_) => health.mark_success(),
                            Err(_) => health.mark_failure(epoch_ms()),
                        }
                    });
                }
            }
            while join.join_next().await.is_some() {}

            // Election: first non-cooldown source in quality order.
            let now_ms = epoch_ms();
            let mut alive = 0usize;
            for (ci, channel) in snap.channels.iter().enumerate() {
                let elected =
                    (0..channel.sources.len()).find(|&si| !snap.health[ci][si].in_cooldown(now_ms));
                if let Some(si) = elected {
                    snap.active_source[ci].store(si, Ordering::Relaxed);
                    alive += 1;
                }
            }
            tracing::info!(
                country,
                channels = snap.channels.len(),
                with_live_source = alive,
                "live tv health probe complete"
            );
        });
    }

    /// Concrete upstream for a non-dlive source: a `vavoo://<id>` sentinel is
    /// resolved to a fresh tokenised playlist (the token + edge host rotate,
    /// hence resolve-on-use), every other URL passes through unchanged.
    async fn resolve_upstream(
        &self,
        source: &channels::StreamSource,
    ) -> Result<Upstream, LiveTvError> {
        let url = match vavoo::stream_id(&source.url) {
            Some(id) => self
                .inner
                .vavoo
                .resolve(&self.inner.http, id)
                .await
                .ok_or_else(|| LiveTvError::Upstream("vavoo resolve failed".into()))?,
            None => source.url.clone(),
        };
        Ok(Upstream {
            url,
            user_agent: source.user_agent.clone(),
            referrer: source.referrer.clone(),
        })
    }

    async fn fetch_source_playlist(
        &self,
        source: &channels::StreamSource,
    ) -> Result<(String, Url), LiveTvError> {
        let upstream = self.resolve_upstream(source).await?;
        self.fetch_playlist_at(&upstream, PLAYLIST_TIMEOUT, PLAYLIST_TIMEOUT)
            .await
            .map_err(|f| f.error)
    }

    /// Fetch a source's playlist: response headers within `first_byte`, the
    /// whole exchange within `total`.
    async fn fetch_playlist_at(
        &self,
        upstream: &Upstream,
        first_byte: Duration,
        total: Duration,
    ) -> Result<(String, Url), FetchFailure> {
        let (body, final_url) = self
            .fetch_upstream_text(upstream, &upstream.url, first_byte, total)
            .await?;
        if !body.trim_start().starts_with("#EXTM3U") {
            return Err(FetchFailure::answered("not an HLS playlist"));
        }
        // A 200 master proves nothing when it came from an indirection host
        // (github-hosted relays always serve their checked-in master, even
        // when the stream behind it is geo-blocked or token-expired). Elect
        // the source only if one variant actually answers — otherwise a dead
        // feed wins the election and the player hangs instead of rotating.
        if let Some(variant) = first_variant_uri(&body) {
            let vurl = final_url
                .join(variant)
                .map_err(|e| FetchFailure::answered(&format!("bad variant uri: {e}")))?;
            let (vbody, _) = self
                .fetch_upstream_text(upstream, vurl.as_str(), first_byte, total)
                .await?;
            if !vbody.trim_start().starts_with("#EXTM3U") {
                return Err(FetchFailure::answered("variant is not HLS"));
            }
        }
        Ok((body, final_url))
    }

    async fn fetch_upstream_text(
        &self,
        upstream: &Upstream,
        url: &str,
        first_byte: Duration,
        total: Duration,
    ) -> Result<(String, Url), FetchFailure> {
        let mut req = self.inner.http.get(url).timeout(total);
        if let Some(ua) = &upstream.user_agent {
            req = req.header(reqwest::header::USER_AGENT, ua);
        }
        if let Some(referrer) = &upstream.referrer {
            req = req.header(reqwest::header::REFERER, referrer);
        }
        let resp = tokio::time::timeout(first_byte, req.send())
            .await
            .map_err(|_| FetchFailure {
                status: None,
                error: LiveTvError::Upstream("no response within the deadline".into()),
            })?
            .map_err(|e| FetchFailure {
                status: None,
                error: upstream_err(e),
            })?;
        let status = resp.status();
        if !status.is_success() {
            return Err(FetchFailure {
                status: Some(status.as_u16()),
                error: LiveTvError::Upstream(format!("upstream answered HTTP {}", status.as_u16())),
            });
        }
        let final_url = resp.url().clone();
        let body = read_playlist(resp).await.map_err(|error| FetchFailure {
            status: None,
            error,
        })?;
        Ok((body, final_url))
    }

    /// Verify + fetch a signed proxy URL. Returns the upstream response for
    /// the route layer to stream through (or re-rewrite when it's a nested
    /// playlist). `channel_key` is `country:id` as minted by the rewriter.
    pub async fn proxy_fetch(
        &self,
        channel_key: &str,
        encoded_url: &str,
        sig: &str,
    ) -> Result<ProxiedResponse, LiveTvError> {
        let upstream = proxy::decode_upstream(encoded_url).ok_or(LiveTvError::BadProxyRequest)?;
        if !self
            .inner
            .signer
            .verify(channel_key, upstream.as_str(), sig)
        {
            return Err(LiveTvError::BadProxyRequest);
        }
        // Recover the channel's pinned headers; a channel that vanished in a
        // playlist refresh still streams with defaults (sig proves we minted
        // the URL).
        let active = self.active_upstream(channel_key).await;
        let dlive = &self.inner.dlive;
        let mut target = upstream.clone();
        if let Some((id, player)) = active.dlive
            && let Some(current) = dlive.refresh_upstream(id, player, &upstream).await
        {
            target = current;
        }
        let send = |url: Url| {
            let timeout = active.dlive.map(|_| {
                if proxy::is_playlist(&url, None) {
                    PLAYLIST_TIMEOUT
                } else {
                    DLIVE_SEGMENT_TIMEOUT
                }
            });
            let mut req = self.inner.http.get(url);
            if let Some(ua) = &active.user_agent {
                req = req.header(reqwest::header::USER_AGENT, ua);
            }
            if let Some(r) = &active.referrer {
                req = req.header(reqwest::header::REFERER, r);
            }
            if let Some(t) = timeout {
                req = req.timeout(t);
            }
            req.send()
        };
        // NOTE: no `error_for_status` here. Live segments roll off the
        // window, so a slightly-late fetch legitimately 404s — that must be
        // forwarded to the player AS a 404 (hls.js retries / gap-skips it),
        // NOT rewritten to a 502 that reads as a dead gateway and tanks the
        // stream. Only a real connection failure (can't reach the host) maps
        // to Upstream/502 below.
        let mut resp = send(target).await.map_err(upstream_err)?;
        // A dlive signature/token the client still holds went stale: resolve
        // afresh once and retry with the live one.
        if resp.status() == reqwest::StatusCode::FORBIDDEN
            && let Some((id, player)) = active.dlive
            && player != 1
        {
            dlive.invalidate(id, player);
            if let Some(current) = dlive.refresh_upstream(id, player, &upstream).await {
                resp = send(current).await.map_err(upstream_err)?;
            }
        }
        let final_url = resp.url().clone();
        Ok(ProxiedResponse {
            resp,
            final_url,
            from_dlive: active.dlive.is_some(),
        })
    }

    /// Buffer an image-wrapped segment and return the TS inside (with its
    /// content type). When nothing unwraps, a dlive source counts a segment
    /// failure (its demotion rotates the channel to the next source); any
    /// other source's image passes through untouched.
    pub async fn wrapped_segment(
        &self,
        channel_key: &str,
        resp: reqwest::Response,
        content_type: Option<String>,
        from_dlive: bool,
    ) -> Result<(Vec<u8>, String), LiveTvError> {
        let unwrapped = match read_capped(resp, MAX_WRAPPED_SEGMENT_BYTES).await {
            Ok(bytes) => tokio::task::spawn_blocking(move || {
                let ts = dlive::unwrap::unwrap_segment(&bytes);
                (bytes, ts)
            })
            .await
            .ok(),
            Err(_) => None,
        };
        let served = match unwrapped {
            Some((_, Some(ts))) => (ts, "video/mp2t".to_string()),
            Some((bytes, None)) if !from_dlive => (
                bytes,
                content_type.unwrap_or_else(|| "application/octet-stream".into()),
            ),
            _ => {
                self.note_segment_result(channel_key, false).await;
                return Err(LiveTvError::Upstream(
                    "no TS inside the wrapped segment".into(),
                ));
            }
        };
        self.note_segment_result(channel_key, true).await;
        Ok(served)
    }

    /// Record the outcome of a segment/key fetch for the channel's active
    /// source. A run of failures (broken origin that serves a valid playlist
    /// but 404s its segments) demotes the source and re-elects, so the
    /// client's next master reload lands on a working feed — the automatic
    /// "sanity check → fallback" the household expects.
    pub async fn note_segment_result(&self, channel_key: &str, ok: bool) {
        let Some((country, id)) = channel_key.split_once(':') else {
            return;
        };
        let Ok(snap) = self.channels(country).await else {
            return;
        };
        let Some(idx) = snap.channel_index(id) else {
            return;
        };
        let active = snap.active_source[idx].load(Ordering::Relaxed)
            % snap.channels[idx].sources.len().max(1);
        let health = &snap.health[idx][active];
        if ok {
            health.segment_failures.store(0, Ordering::Relaxed);
            return;
        }
        let is_dlive = snap.channels[idx]
            .sources
            .get(active)
            .is_some_and(|s| dlive::parse_sentinel(&s.url).is_some());
        let threshold = if is_dlive {
            DLIVE_SEGMENT_FAIL_THRESHOLD
        } else {
            SEGMENT_FAIL_THRESHOLD
        };
        let fails = health.segment_failures.fetch_add(1, Ordering::Relaxed) + 1;
        if fails < threshold {
            return;
        }
        // Too many bad segments — cool this source down and elect the next
        // healthy one. Reset the counter so the replacement gets a clean run.
        health.mark_failure(epoch_ms());
        health.segment_failures.store(0, Ordering::Relaxed);
        if is_dlive {
            self.inner.dlive.note_outage();
        }
        let now_ms = epoch_ms();
        let next = (0..snap.channels[idx].sources.len())
            .find(|&si| si != active && !snap.health[idx][si].in_cooldown(now_ms));
        if let Some(si) = next {
            snap.active_source[idx].store(si, Ordering::Relaxed);
        }
        tracing::info!(
            channel = channel_key,
            demoted = active,
            elected = ?next,
            "live tv source demoted after repeated segment failures"
        );
    }

    /// Headers (and dlive identity) of the channel's active source, for its
    /// proxied requests. A dlive source's headers come from its resolution.
    async fn active_upstream(&self, channel_key: &str) -> ActiveUpstream {
        let Some((country, id)) = channel_key.split_once(':') else {
            return ActiveUpstream::default();
        };
        let Ok(snap) = self.channels(country).await else {
            return ActiveUpstream::default();
        };
        let Some(idx) = snap.channel_index(id) else {
            return ActiveUpstream::default();
        };
        let active = snap.active_source[idx].load(Ordering::Relaxed);
        let Some(source) = snap.channels[idx]
            .sources
            .get(active)
            .or_else(|| snap.channels[idx].sources.first())
        else {
            return ActiveUpstream::default();
        };
        if let Some((id, player)) = dlive::parse_sentinel(&source.url) {
            let (user_agent, referrer) = self.inner.dlive.cached_headers(id, player);
            return ActiveUpstream {
                user_agent,
                referrer,
                dlive: Some((id, player)),
            };
        }
        ActiveUpstream {
            user_agent: source.user_agent.clone(),
            referrer: source.referrer.clone(),
            dlive: None,
        }
    }

    /// Now/next for every channel of a country that has a guide match.
    pub async fn epg_now(&self, country: &str) -> Result<Vec<NowNext>, LiveTvError> {
        let country = validate_country(country)?;
        let snap = self.channels(&country).await?;
        let Some(index) = self.epg_index(&country).await else {
            return Ok(Vec::new());
        };
        let now = chrono::Utc::now();
        let mut entries = Vec::new();
        for channel in snap.channels.iter() {
            let Some(xmltv_id) = self.resolve_epg_id(channel, &index) else {
                continue;
            };
            let (current, next) = index.now_next(&xmltv_id, now);
            if current.is_none() && next.is_none() {
                continue;
            }
            entries.push(NowNext {
                channel_id: channel.id.clone(),
                now: current.cloned(),
                next: next.cloned(),
            });
        }
        Ok(entries)
    }

    /// Guide index for a country: cached, refreshed by the background loop.
    /// `None` when the country has no configured guide or the fetch failed
    /// (now/next is best-effort, never a page error).
    async fn epg_index(&self, country: &str) -> Option<Arc<epg::EpgIndex>> {
        self.epg_index_within(country, Duration::MAX).await
    }

    /// [`Self::epg_index`], reloading a guide older than `ttl`.
    async fn epg_index_within(&self, country: &str, ttl: Duration) -> Option<Arc<epg::EpgIndex>> {
        let url = self.inner.cfg.epg_urls.get(country)?;
        let cell = self
            .inner
            .epg
            .write()
            .expect("poisoned")
            .entry(country.to_string())
            .or_default()
            .clone();
        cell.get(ttl, FAILED_LOAD_RETRY, || async {
            self.fetch_epg(url)
                .await
                .inspect_err(|e| tracing::warn!(country, error = %e, "live tv EPG fetch failed"))
                .ok()
        })
        .await
    }

    async fn fetch_epg(&self, url: &str) -> Result<epg::EpgIndex, LiveTvError> {
        let resp = self
            .inner
            .http
            .get(url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(upstream_err)?;
        let bytes = read_capped(resp, MAX_GUIDE_BYTES).await?;
        // A full guide is tens of MB: gunzip + parse on the blocking pool.
        let index = tokio::task::spawn_blocking(move || {
            // Guides are served gzipped-as-body; accept plain XML too.
            let xml = epg::decode_gzip(&bytes, MAX_GUIDE_XML_BYTES)
                .or_else(|_| String::from_utf8(bytes))
                .map_err(|_| LiveTvError::Upstream("guide is neither gzip nor utf-8 xml".into()))?;
            Ok::<_, LiveTvError>(epg::parse_xmltv(&xml, chrono::Utc::now()))
        })
        .await
        .map_err(|e| LiveTvError::Upstream(format!("guide parse task: {e}")))??;
        if index.is_empty() {
            return Err(LiveTvError::Upstream(
                "guide parsed to zero programmes".into(),
            ));
        }
        Ok(index)
    }

    /// XMLTV id for a channel: config override → exact tvg-id → nothing.
    /// (`EpgIndex` lookups are already case-insensitive.)
    fn resolve_epg_id(&self, channel: &Channel, index: &epg::EpgIndex) -> Option<String> {
        if let Some(id) = self.inner.cfg.epg_id_overrides.get(&channel.id) {
            return Some(id.clone());
        }
        if let Some(tvg_id) = channel.tvg_id.as_ref() {
            // The guide may key on the full id ("TF1.fr") or the base without
            // the variant qualifier ("TF1.fr@HD" → "TF1.fr").
            let base = tvg_id.split('@').next().unwrap_or(tvg_id);
            for candidate in [tvg_id.as_str(), base] {
                if index.contains(candidate) {
                    return Some(candidate.to_string());
                }
            }
        }
        // Fallback: match the channel's display name against the guide's
        // <display-name> entries — covers feeds with no usable tvg-id (Vavoo).
        index.id_for_name(&channel.name).map(str::to_string)
    }

    /// Expand channel ids to every channel sharing their tuner MUX (same
    /// `f=` in the tuner source URL): one tuned adapter serves the whole
    /// frequency, so its siblings are warm-able for the cost of a `-c copy`
    /// ffmpeg each. Unknown ids pass through so a config typo still
    /// surfaces as a logged prewarm failure.
    async fn mux_siblings(&self, country: &str, seeds: &[String]) -> Vec<String> {
        let Ok(snap) = self.channels(country).await else {
            return seeds.to_vec();
        };
        let freqs: HashSet<String> = snap
            .channels
            .iter()
            .filter(|c| seeds.iter().any(|s| s == &c.id))
            .filter_map(channel_tuner_freq)
            .collect();
        let mut out: Vec<String> = seeds.to_vec();
        for ch in snap.channels.iter() {
            if channel_tuner_freq(ch).is_some_and(|f| freqs.contains(&f)) && !out.contains(&ch.id) {
                out.push(ch.id.clone());
            }
        }
        out
    }

    /// Background refresh: re-fetch loaded playlists / guides past their
    /// TTL. Runs forever; spawn once at boot.
    pub fn spawn_refresh_loop(self) {
        let playlist_ttl = Duration::from_hours(self.inner.cfg.playlist_refresh_hours.max(1));
        let epg_ttl = Duration::from_hours(self.inner.cfg.epg_refresh_hours.max(1));
        // Transcode idle reaper: its 60 s idle window needs a much faster
        // cadence than the 15 min refresh ticker below.
        {
            let svc = self.clone();
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(Duration::from_secs(30));
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    ticker.tick().await;
                    crate::supervise::tick("live tv reaper", svc.inner.transcode.reap_idle()).await;
                }
            });
        }
        // dlive: load the channel list at boot, then scrape the configured
        // channels' embeds one a minute, so a zap never waits on dlive.sx.
        if self.inner.dlive.enabled() {
            let dlive = self.inner.dlive.clone();
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(Duration::from_mins(1));
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    ticker.tick().await;
                    crate::supervise::tick("live tv dlive warm-up", dlive.warm()).await;
                }
            });
        }
        // Warm the cross-country search index at boot so the first user search
        // is instant — building it cold fetches iptv-org + every extra playlist
        // + all Vavoo catalogs (~10 s). The refresh loop below keeps it fresh.
        {
            let svc = self.clone();
            tokio::spawn(async move {
                svc.channel_counts(Duration::MAX).await;
                svc.search_index(Duration::MAX).await;
            });
        }
        // Hydrate the configured tuner channels at boot and keep their remux
        // sessions warm forever (a 30 min re-touch beats the reaper). Viewers
        // then always join a session with a deep playlist window — the
        // cold-join stutter happens once per boot, with nobody watching.
        if self.inner.cfg.tuner.enabled && !self.inner.cfg.tuner.prewarm.is_empty() {
            let svc = self.clone();
            tokio::spawn(async move {
                let country = svc.default_country().to_string();
                // 5 min cadence: warm sessions answer instantly (no-op), and
                // a mux temporarily borrowed by a viewer for a third
                // frequency gets re-pinned quickly once they leave.
                let mut ticker = tokio::time::interval(Duration::from_mins(5));
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    ticker.tick().await; // first tick fires immediately = boot hydration
                    // Expand each configured id to its WHOLE MUX: a tuned
                    // adapter serves every channel of its frequency anyway
                    // (tunerd unions the PID filters), so warming the
                    // siblings costs one `-c copy` ffmpeg each — and the
                    // entire mux zaps instantly.
                    for id in svc
                        .mux_siblings(&country, &svc.inner.cfg.tuner.prewarm)
                        .await
                    {
                        let key = format!("{country}:{id}");
                        // Already running → leave it alone. Deliberately no
                        // touch: session heat must mean "a viewer is here",
                        // it's what shields watched muxes from reclaim.
                        if svc.inner.transcode.is_warm(Mode::Remux, &key).await {
                            continue;
                        }
                        // Viewers outrank warmth — never steal an adapter
                        // from watched muxes to re-pin a cold channel. The
                        // next tick retries once a mux cools down.
                        let freq = {
                            let snap = svc.channels(&country).await.ok();
                            snap.as_ref().and_then(|s| {
                                s.channel_index(&id)
                                    .and_then(|i| channel_tuner_freq(&s.channels[i]))
                            })
                        };
                        if let Some(freq) = &freq
                            && !svc.inner.transcode.mux_available(freq).await
                        {
                            tracing::debug!(
                                channel = %id,
                                freq = %freq,
                                "tuner adapters busy with watched muxes — left cold"
                            );
                            continue;
                        }
                        match svc.master_playlist(&country, &id).await {
                            Ok(mp) if mp.upstream_host == "tuner" => {
                                tracing::info!(channel = %id, "tuner session warm");
                            }
                            Ok(_) => tracing::debug!(
                                channel = %id,
                                "prewarm lost the adapter race — left cold"
                            ),
                            Err(e) => {
                                tracing::warn!(channel = %id, error = %e, "tuner prewarm failed");
                            }
                        }
                    }
                }
            });
        }
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_mins(15));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            ticker.tick().await; // skip immediate boot tick
            loop {
                ticker.tick().await;
                crate::supervise::tick(
                    "live tv refresh",
                    self.refresh_stale(playlist_ttl, epg_ttl),
                )
                .await;
            }
        });
    }

    async fn refresh_stale(&self, playlist_ttl: Duration, epg_ttl: Duration) {
        let stale_countries: Vec<String> = {
            let snaps = self.inner.snapshots.read().expect("poisoned");
            snaps
                .iter()
                .filter(|(_, s)| s.fetched_at.elapsed() > playlist_ttl)
                .map(|(c, _)| c.clone())
                .collect()
        };
        for country in stale_countries {
            match self.fetch_country(&country).await {
                // A playlist gone empty for a round is an upstream hiccup more
                // often than a country losing every channel.
                Ok(snap) if snap.channels.is_empty() => {
                    tracing::warn!(country, "live tv playlist refresh came back empty");
                }
                Ok(snap) => {
                    let snap = Arc::new(snap);
                    self.inner
                        .snapshots
                        .write()
                        .expect("poisoned")
                        .insert(country.clone(), snap.clone());
                    // Re-elect on fresh liveness (health carries over by URL,
                    // but new alternates deserve a look too).
                    self.clone().spawn_probe(country, snap);
                }
                // Keep serving the previous snapshot on failure.
                Err(e) => {
                    tracing::warn!(country, error = %e, "live tv playlist refresh failed");
                }
            }
        }

        let epg_countries: Vec<String> = self
            .inner
            .epg
            .read()
            .expect("poisoned")
            .keys()
            .cloned()
            .collect();
        for country in epg_countries {
            self.epg_index_within(&country, epg_ttl).await;
        }

        // Keep the cross-country search index fresh in the background: this is
        // a no-op while it's within its TTL and rebuilds it once past it, so a
        // search never pays the cold cost after the boot warm.
        self.search_index(playlist_ttl).await;
        self.channel_counts(playlist_ttl).await;
    }
}

/// Prefix every URI of an HLS media playlist — ffmpeg's on-disk playlist
/// references bare `mux000001.m4s` names, but the master is served from
/// `.../channels/{id}/master.m3u8` while tuner segments live under the
/// sibling `.../channels/{id}/transcode/{segment}` route. Besides the plain
/// URI lines, the fMP4 init segment is referenced from an `#EXT-X-MAP` tag
/// (a comment-shaped line) and must be rewritten too.
/// Finalize a successful tuner election: record stickiness + health and
/// point the returned playlist at the tuner segment route.
fn tuner_elected(
    snap: &CountrySnapshot,
    idx: usize,
    si: usize,
    channel_key: &str,
    body: &str,
) -> MasterPlaylist {
    snap.active_source[idx].store(si, Ordering::Relaxed);
    snap.health[idx][si].mark_success();
    tracing::info!(
        channel = %channel_key,
        source = si,
        tier = "tuner",
        "live tv source elected"
    );
    MasterPlaylist {
        body: prefix_segment_uris(body, "transcode/"),
        source_index: si,
        upstream_host: "tuner".to_string(),
        source_count: snap.channels[idx].sources.len(),
    }
}

/// Election starting index. The tuner tier has absolute priority:
/// always reconsider it first, even after a capacity- or failure-driven
/// fallback elected an internet source (cooldown still shields a
/// genuinely failing box in pass 0). Election stays sticky (the stored
/// active source) for internet-only channels.
fn election_start(channel: &Channel, stored: usize) -> usize {
    channel
        .sources
        .iter()
        .position(|s| s.tier == SourceTier::Tuner)
        .unwrap_or(stored % channel.sources.len())
}

/// Source the transcoder reads: the active one unless it is dlive, else the
/// first non-dlive source in election order from it, healthy ones first.
fn transcode_source(
    channel: &Channel,
    health: &[Arc<SourceHealth>],
    active: usize,
    now_ms: u64,
) -> Option<usize> {
    let n = channel.sources.len();
    let candidates: Vec<usize> = (0..n)
        .map(|step| (active + step) % n)
        .filter(|&si| dlive::parse_sentinel(&channel.sources[si].url).is_none())
        .collect();
    candidates
        .iter()
        .copied()
        .find(|&si| !health[si].in_cooldown(now_ms))
        .or_else(|| candidates.first().copied())
}

/// Mux frequency of a channel's tuner source, when it has one.
fn channel_tuner_freq(ch: &Channel) -> Option<String> {
    ch.sources
        .iter()
        .find(|s| s.tier == SourceTier::Tuner)
        .and_then(|s| transcode::tuner_freq(&s.url))
}

fn prefix_segment_uris(body: &str, prefix: &str) -> String {
    body.lines()
        .map(|line| {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("#EXT-X-MAP:URI=\"") {
                format!("#EXT-X-MAP:URI=\"{prefix}{rest}")
            } else if t.is_empty() || t.starts_with('#') {
                line.to_string()
            } else {
                format!("{prefix}{t}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Country codes are interpolated into the playlist URL — keep them to
/// exactly two ASCII letters (ISO 3166-1 alpha-2), lowercased.
fn validate_country(code: &str) -> Result<String, LiveTvError> {
    if code.len() == 2 && code.chars().all(|c| c.is_ascii_alphabetic()) {
        Ok(code.to_ascii_lowercase())
    } else {
        Err(LiveTvError::UnknownCountry)
    }
}

/// A transport/status error as a [`LiveTvError`], without its URL: source
/// URLs carry tokens (Vavoo signatures, tokenised CDN paths).
pub(crate) fn upstream_err(e: reqwest::Error) -> LiveTvError {
    LiveTvError::Upstream(e.without_url().to_string())
}

/// Read a response body, refusing one larger than `cap` — up front from
/// `Content-Length`, else while streaming. Some playlist entries are endless
/// raw video streams behind a `.m3u8` name; `text()` would buffer them until
/// the timeout.
pub(crate) async fn read_capped(
    mut resp: reqwest::Response,
    cap: usize,
) -> Result<Vec<u8>, LiveTvError> {
    let too_large = || LiveTvError::Upstream(format!("upstream body exceeds {cap} bytes"));
    if resp
        .content_length()
        .is_some_and(|n| n > u64::try_from(cap).unwrap_or(u64::MAX))
    {
        return Err(too_large());
    }
    let mut out = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(upstream_err)? {
        if out.len() + chunk.len() > cap {
            return Err(too_large());
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// [`read_capped`] at the playlist cap, as text.
pub(crate) async fn read_playlist(resp: reqwest::Response) -> Result<String, LiveTvError> {
    let bytes = read_capped(resp, MAX_PLAYLIST_BYTES).await?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// [`read_capped`] at the JSON database cap, deserialized.
async fn read_json<T: serde::de::DeserializeOwned>(
    resp: reqwest::Response,
) -> Result<T, LiveTvError> {
    let bytes = read_capped(resp, MAX_JSON_BYTES).await?;
    serde_json::from_slice(&bytes).map_err(|e| LiveTvError::Upstream(format!("bad json: {e}")))
}

fn epoch_ms() -> u64 {
    u64::try_from(chrono::Utc::now().timestamp_millis()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(body: Vec<u8>, content_length: Option<usize>) -> reqwest::Response {
        let mut builder = http::Response::builder().status(200);
        if let Some(len) = content_length {
            builder = builder.header(http::header::CONTENT_LENGTH, len);
        }
        reqwest::Response::from(builder.body(body).unwrap())
    }

    fn channel_with(urls: &[&str]) -> Channel {
        Channel {
            id: "c".into(),
            name: "C".into(),
            tvg_id: None,
            logo_url: None,
            categories: Vec::new(),
            geo_blocked: false,
            not_24_7: false,
            tnt_number: None,
            sources: urls
                .iter()
                .map(|u| channels::StreamSource {
                    url: (*u).to_string(),
                    quality: None,
                    user_agent: None,
                    referrer: None,
                    tier: channels::classify_source(u),
                    origin: SourceOrigin::IptvOrg,
                })
                .collect(),
        }
    }

    #[test]
    fn health_records_follow_live_snapshots() {
        let svc = LiveTvService::new(iris_config::LiveTvConfig::default(), "test-secret").unwrap();
        let first = svc.build_snapshot(vec![channel_with(&["http://a/1", "http://a/keep"])]);
        first.health[0][1].mark_failure(epoch_ms());
        let second = svc.build_snapshot(vec![channel_with(&["http://a/2", "http://a/keep"])]);
        assert!(
            second.health[0][1].in_cooldown(epoch_ms()),
            "survives a refresh"
        );
        drop(first);
        let _third = svc.build_snapshot(vec![channel_with(&["http://a/keep"])]);
        drop(second);
        let keys: HashSet<String> = svc.inner.health.read().unwrap().keys().cloned().collect();
        assert!(keys.contains("http://a/keep"));
        assert!(!keys.contains("http://a/1"));
    }

    #[tokio::test]
    async fn read_capped_refuses_oversized_bodies() {
        let ok = read_capped(response(vec![b'a'; 10], None), 10).await;
        assert_eq!(ok.unwrap().len(), 10);
        assert!(
            read_capped(response(vec![b'a'; 11], None), 10)
                .await
                .is_err()
        );
        // A declared length past the cap is refused before reading.
        assert!(
            read_capped(response(vec![b'a'; 4], Some(4)), 3)
                .await
                .is_err()
        );
        let text = read_playlist(response(b"#EXTM3U\n".to_vec(), None)).await;
        assert_eq!(text.unwrap(), "#EXTM3U\n");
    }

    /// End-to-end: search must surface channels that only exist via Vavoo or an
    /// extra playlist (not in iptv-org's streams DB). Live — run with
    /// `cargo test -p iris-api search_surfaces -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore = "hits live iptv-org + Vavoo APIs"]
    async fn search_surfaces_vavoo_and_extra_playlist_channels() {
        let svc = LiveTvService::new(iris_config::LiveTvConfig::default(), "test-secret").unwrap();
        let hits = svc.search("disney channel", 40).await;
        let fr: Vec<_> = hits
            .iter()
            .filter(|e| e.country == "fr")
            .map(|e| (e.name.as_str(), e.channel_id.as_str()))
            .collect();
        eprintln!("'disney channel' FR hits: {fr:?}");
        assert!(
            fr.iter().any(|(_, id)| *id == "disneychannel"),
            "Disney Channel FR (Vavoo-only) must be searchable"
        );
    }

    #[test]
    fn a_channel_is_unreachable_only_when_every_feed_cools_down() {
        let svc = LiveTvService::new(iris_config::LiveTvConfig::default(), "test-secret").unwrap();
        let snap = svc.build_snapshot(vec![channel_with(&["http://u/1", "http://u/2"])]);
        snap.health[0][0].mark_failure(epoch_ms());
        assert!(!snap.unreachable(0), "one feed left");
        snap.health[0][1].mark_failure(epoch_ms());
        assert!(snap.unreachable(0));
        assert!(!snap.unreachable(1), "no such channel");
    }

    fn country(code: &str, name: &str) -> Country {
        Country {
            code: code.into(),
            name: name.into(),
            flag: String::new(),
        }
    }

    #[test]
    fn channel_counts_group_by_country_and_dedupe_variants() {
        let body = r#"#EXTM3U
#EXTINF:-1 tvg-id="TF1.fr@HD" group-title="France",TF1 (1080p)
https://a/tf1-hd.m3u8
#EXTINF:-1 tvg-id="TF1.fr@SD" group-title="France",TF1 (576p)
https://a/tf1-sd.m3u8
#EXTINF:-1 tvg-id="France2.fr@SD" group-title="France",France 2
https://a/f2.m3u8
#EXTINF:-1 tvg-id="TF1.fr@HD" group-title="Belgium",TF1 (1080p)
https://a/tf1-hd.m3u8
#EXTINF:-1 tvg-id="X.int" group-title="",Nowhere
https://a/x.m3u8
"#;
        let counts = channel_counts(m3u::parse(body));
        assert_eq!(counts.get("France"), Some(&2), "TF1 HD + SD is one channel");
        assert_eq!(
            counts.get("Belgium"),
            Some(&1),
            "a channel counts in every country it airs in"
        );
        assert_eq!(counts.len(), 2, "no group, no country");
    }

    #[test]
    fn picker_offers_only_countries_with_channels() {
        let countries = [
            country("fr", "France"),
            country("aq", "Antarctica"),
            country("be", "Belgium"),
            country("ie", "Ireland"),
            country("de", "Germany"),
        ];
        let counts: ChannelCounts =
            [("France".to_string(), 40), ("Belgium".to_string(), 12)].into();
        let loaded: HashMap<String, usize> = [("fr".to_string(), 52), ("de".to_string(), 0)].into();
        let configured: HashSet<&str> = ["ie"].into();
        let picked: Vec<(String, Option<usize>)> =
            picker_entries(&countries, Some(&counts), &loaded, &configured)
                .into_iter()
                .map(|(c, n)| (c.code, n))
                .collect();
        assert_eq!(
            picked,
            [
                ("fr".to_string(), Some(52)),
                ("be".to_string(), Some(12)),
                ("ie".to_string(), None),
            ],
            "a loaded list is the exact count, an empty country is left out, a configured one stays"
        );
    }

    #[test]
    fn picker_without_an_index_keeps_every_country() {
        let countries = [country("fr", "France"), country("aq", "Antarctica")];
        let loaded: HashMap<String, usize> = [("fr".to_string(), 52)].into();
        let picked = picker_entries(&countries, None, &loaded, &HashSet::new());
        assert_eq!(picked.len(), 2);
        assert_eq!(picked[0].1, Some(52));
        assert_eq!(picked[1].1, None);
    }

    #[test]
    fn prefix_segment_uris_touches_only_uri_lines() {
        let body = "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MAP:URI=\"muxinit.mp4\"\n#EXTINF:4.0,\nmux000001.m4s\n\n#EXT-X-ENDLIST";
        let out = prefix_segment_uris(body, "transcode/");
        assert!(out.contains("transcode/mux000001.m4s"));
        assert!(out.contains("#EXT-X-MAP:URI=\"transcode/muxinit.mp4\""));
        assert!(out.contains("#EXT-X-TARGETDURATION:4"));
        assert!(!out.contains("transcode/#"));
    }

    #[test]
    fn validate_country_accepts_alpha2_only() {
        assert_eq!(validate_country("FR").unwrap(), "fr");
        assert_eq!(validate_country("de").unwrap(), "de");
        assert!(validate_country("fra").is_err());
        assert!(validate_country("f").is_err());
        assert!(validate_country("..").is_err());
        assert!(validate_country("f/").is_err());
    }

    #[test]
    fn first_variant_uri_finds_master_variants_only() {
        let master =
            "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000,RESOLUTION=1280x720\n\nvariant/720.m3u8\n";
        assert_eq!(first_variant_uri(master), Some("variant/720.m3u8"));
        let media = "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6.0,\nseg1.ts\n";
        assert_eq!(first_variant_uri(media), None);
        let truncated = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000\n";
        assert_eq!(first_variant_uri(truncated), None);
    }

    #[test]
    fn elect_seed_skips_cooling_sources() {
        let now = 10_000_u64;
        let mk = || Arc::new(SourceHealth::default());
        let sources = vec![mk(), mk(), mk()];

        // All healthy → the best (index 0, already tier/quality-ordered) wins.
        assert_eq!(elect_seed(&sources, now), 0);

        // Best source just died → seed jumps to the next healthy one instead
        // of re-electing the corpse (the audit-flagged reset-to-0 bug).
        sources[0].mark_failure(now);
        assert_eq!(elect_seed(&sources, now), 1);

        sources[1].mark_failure(now);
        assert_eq!(elect_seed(&sources, now), 2);

        // Everything cooling down → fall back to 0 (election pass 1 retries).
        sources[2].mark_failure(now);
        assert_eq!(elect_seed(&sources, now), 0);

        // Cooldown is time-bounded: far enough in the future, index 0 is
        // electable again.
        let far = now + u64::try_from(SOURCE_COOLDOWN_MAX.as_millis()).unwrap() + 1;
        assert_eq!(elect_seed(&sources, far), 0);
    }
}
