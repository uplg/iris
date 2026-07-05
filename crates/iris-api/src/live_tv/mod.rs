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
pub mod epg;
pub mod m3u;
pub mod proxy;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use serde::Deserialize;
use url::Url;

use channels::Channel;

/// Browser UA presented to upstreams that don't pin one via the playlist
/// (several French networks 403 non-browser agents).
const DEFAULT_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36";

/// Base cooldown after a source failure. Doubles per consecutive failure
/// (10 min → 20 → 40 → …) up to [`SOURCE_COOLDOWN_MAX`], so a persistently
/// dead feed is only re-probed ~once a day and never elected while a
/// healthy alternative exists.
const SOURCE_COOLDOWN_BASE: Duration = Duration::from_mins(10);
const SOURCE_COOLDOWN_MAX: Duration = Duration::from_hours(24);

/// Concurrency cap for the background health probe.
const PROBE_CONCURRENCY: usize = 12;

/// Consecutive segment/key fetch failures on the live source before it is
/// demoted and the next feed elected.
const SEGMENT_FAIL_THRESHOLD: u64 = 5;

/// Per-source fetch timeout for playlist requests — bounds the worst case
/// when rotating through several dead sources in one request.
const PLAYLIST_TIMEOUT: Duration = Duration::from_secs(8);

/// Playlists (m3u / m3u8) are text and small; cap protects the rewriter.
const MAX_PLAYLIST_BYTES: usize = 4 * 1024 * 1024;

/// How long a fetched logo stays served from memory (they're effectively
/// static). A dead / rate-limited host is re-tried after the shorter
/// negative TTL rather than on every request.
const LOGO_CACHE_TTL: Duration = Duration::from_hours(24);
const LOGO_NEG_TTL: Duration = Duration::from_mins(5);

/// Max concurrent upstream logo fetches — low enough to stay under logo-host
/// rate limits while a full grid warms the cache.
const LOGO_FETCH_CONCURRENCY: usize = 6;

/// Hard cap on cached logo entries (each a few KB). Cleared wholesale if
/// exceeded — trivial to re-warm and far simpler than LRU bookkeeping.
const LOGO_CACHE_MAX: usize = 4096;

/// Logos above this many bytes aren't cached in memory (channel logos are
/// tiny PNG/SVG; anything larger is almost certainly not a real logo).
const LOGO_MAX_BYTES: usize = 512 * 1024;

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

/// Alternate feeds per lowercase channel id (`"m6.fr"`), from iptv-org's
/// stream database.
type StreamsDb = HashMap<String, Vec<channels::StreamSource>>;

/// A served master playlist plus which upstream produced it — surfaced as
/// response headers so "which feed am I actually watching?" is one glance
/// at the network tab when a household member reports bad sound/video.
pub struct MasterPlaylist {
    pub body: String,
    pub source_index: usize,
    pub upstream_host: String,
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

impl SourceHealth {
    fn in_cooldown(&self, now_ms: u64) -> bool {
        self.cooldown_until_ms.load(Ordering::Relaxed) > now_ms
    }

    fn mark_failure(&self, now_ms: u64) {
        let failures = self.failures.fetch_add(1, Ordering::Relaxed) + 1;
        let exp = u32::try_from(failures.saturating_sub(1))
            .unwrap_or(u32::MAX)
            .min(16);
        let backoff = SOURCE_COOLDOWN_BASE
            .saturating_mul(2u32.saturating_pow(exp))
            .min(SOURCE_COOLDOWN_MAX);
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
}

struct EpgSnapshot {
    index: Arc<epg::EpgIndex>,
    fetched_at: Instant,
}

struct ServiceInner {
    cfg: iris_config::LiveTvConfig,
    http: reqwest::Client,
    signer: proxy::Signer,
    countries: RwLock<Option<(Arc<Vec<Country>>, Instant)>>,
    snapshots: RwLock<HashMap<String, Arc<CountrySnapshot>>>,
    epg: RwLock<HashMap<String, Arc<EpgSnapshot>>>,
    /// iptv-org's full stream database (all alternate feeds per channel),
    /// cached like the playlists.
    streams_db: RwLock<Option<(Arc<StreamsDb>, Instant)>>,
    /// Per-URL liveness, shared across snapshots (see [`SourceHealth`]).
    health: RwLock<HashMap<String, Arc<SourceHealth>>>,
    /// Serializes cold loads so N concurrent first-requests for a country
    /// fetch its playlist once.
    load_lock: tokio::sync::Mutex<()>,
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
        let http = reqwest::Client::builder()
            .user_agent(DEFAULT_UA)
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()?;
        Ok(Self {
            inner: Arc::new(ServiceInner {
                signer: proxy::Signer::new(jwt_secret),
                cfg,
                http,
                countries: RwLock::new(None),
                snapshots: RwLock::new(HashMap::new()),
                epg: RwLock::new(HashMap::new()),
                streams_db: RwLock::new(None),
                health: RwLock::new(HashMap::new()),
                load_lock: tokio::sync::Mutex::new(()),
                logo_cache: RwLock::new(HashMap::new()),
                logo_sem: tokio::sync::Semaphore::new(LOGO_FETCH_CONCURRENCY),
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
        let fetched: Vec<Country> = self
            .inner
            .http
            .get(&self.inner.cfg.countries_url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| LiveTvError::Upstream(e.to_string()))?
            .json()
            .await
            .map_err(|e| LiveTvError::Upstream(e.to_string()))?;
        let fetched = Arc::new(fetched);
        *self.inner.countries.write().expect("poisoned") = Some((fetched.clone(), Instant::now()));
        Ok(fetched)
    }

    /// Channel list for a country, fetching its playlist on first access.
    pub async fn channels(&self, country: &str) -> Result<Arc<CountrySnapshot>, LiveTvError> {
        let country = validate_country(country)?;
        if let Some(snap) = self.inner.snapshots.read().expect("poisoned").get(&country) {
            return Ok(snap.clone());
        }
        // Cold load — single-flight so a burst of first requests fetches once.
        let _guard = self.inner.load_lock.lock().await;
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
            match self.fetch_text(url, PLAYLIST_TIMEOUT).await {
                Ok((body, _)) => playlists.push(m3u::parse(&body)),
                // The primary playlist failing is fatal (probably an unknown
                // country → 404); a missing extra playlist just logs.
                Err(e) if i == 0 => return Err(e),
                Err(e) => {
                    tracing::warn!(url, error = %e, "live tv extra playlist fetch failed");
                }
            }
        }

        let tnt = (country == "fr").then_some(&self.inner.cfg.tnt_overrides);
        let mut built = channels::build_channels(&playlists, tnt);
        if built.is_empty() {
            return Err(LiveTvError::Upstream("playlist has no channels".into()));
        }
        // Graft the alternate feeds from iptv-org's database — the playlist
        // carries a single feed per channel; the database has them all.
        if let Some(db) = self.streams_db().await {
            channels::merge_db_sources(&mut built, &db);
        }
        tracing::info!(country, channels = built.len(), "live tv playlist loaded");
        Ok(self.build_snapshot(built))
    }

    /// Resolve the shared per-URL health records for a fresh channel list —
    /// this is what carries liveness knowledge across playlist refreshes.
    fn build_snapshot(&self, built: Vec<Channel>) -> CountrySnapshot {
        let mut health_map = self.inner.health.write().expect("poisoned");
        let health = built
            .iter()
            .map(|c| {
                c.sources
                    .iter()
                    .map(|s| health_map.entry(s.url.clone()).or_default().clone())
                    .collect()
            })
            .collect();
        let active_source = built.iter().map(|_| AtomicUsize::new(0)).collect();
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

        let ttl = Duration::from_hours(self.inner.cfg.playlist_refresh_hours.max(1));
        if let Some((db, at)) = self.inner.streams_db.read().expect("poisoned").clone()
            && at.elapsed() < ttl
        {
            return Some(db);
        }

        let fetched: Vec<ApiStream> = match self
            .inner
            .http
            .get(&self.inner.cfg.streams_url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
        {
            Ok(resp) => match resp.json().await {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(error = %e, "live tv streams db parse failed");
                    return self
                        .inner
                        .streams_db
                        .read()
                        .expect("poisoned")
                        .clone()
                        .map(|(db, _)| db);
                }
            },
            Err(e) => {
                tracing::warn!(error = %e, "live tv streams db fetch failed");
                return self
                    .inner
                    .streams_db
                    .read()
                    .expect("poisoned")
                    .clone()
                    .map(|(db, _)| db);
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
                    url: s.url,
                    quality: s.quality.as_deref().and_then(channels::parse_quality),
                    user_agent: s.user_agent.filter(|v| !v.is_empty()),
                    referrer: s.referrer.filter(|v| !v.is_empty()),
                });
        }
        tracing::info!(channels = map.len(), "live tv streams db loaded");
        let db = Arc::new(map);
        *self.inner.streams_db.write().expect("poisoned") = Some((db.clone(), Instant::now()));
        Some(db)
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
            .map_err(|e| LiveTvError::Upstream(e.to_string()))?;
        let final_url = resp.url().clone();
        let body = resp
            .text()
            .await
            .map_err(|e| LiveTvError::Upstream(e.to_string()))?;
        if body.len() > MAX_PLAYLIST_BYTES {
            return Err(LiveTvError::Upstream("playlist too large".into()));
        }
        Ok((body, final_url))
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

        let start = snap.active_source[idx].load(Ordering::Relaxed) % channel.sources.len();
        let mut last_err = String::new();
        // Two passes: first only healthy sources, then (if every source is
        // cooling down) all of them — a fully-cooled channel must still get
        // a chance rather than 502ing for the whole cooldown window.
        for pass in 0..2 {
            for step in 0..channel.sources.len() {
                let si = (start + step) % channel.sources.len();
                let health = &snap.health[idx][si];
                if pass == 0 && health.in_cooldown(now_ms) {
                    continue;
                }
                let source = &channel.sources[si];
                match self.fetch_source_playlist(source).await {
                    Ok((body, base)) => {
                        snap.active_source[idx].store(si, Ordering::Relaxed);
                        health.mark_success();
                        return Ok(MasterPlaylist {
                            body: proxy::rewrite_playlist(
                                &body,
                                &base,
                                &channel_key,
                                &self.inner.signer,
                            ),
                            source_index: si,
                            upstream_host: base.host_str().unwrap_or("unknown").to_string(),
                        });
                    }
                    Err(e) => {
                        health.mark_failure(now_ms);
                        tracing::debug!(
                            channel = %channel_key,
                            source = si,
                            error = %e,
                            "live tv source failed, rotating"
                        );
                        last_err = e.to_string();
                    }
                }
            }
            if pass == 0 && last_err.is_empty() {
                // Pass 0 skipped everything (all cooling down) — run pass 1.
                continue;
            }
            if pass == 0 {
                break; // real failures: every healthy source already tried
            }
        }
        Err(LiveTvError::Upstream(last_err))
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
        let cached = match self.inner.http.get(upstream).send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if resp.status().is_success() {
                    let content_type = resp
                        .headers()
                        .get(reqwest::header::CONTENT_TYPE)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("image/png")
                        .to_string();
                    let bytes = resp
                        .bytes()
                        .await
                        .map_err(|e| LiveTvError::Upstream(e.to_string()))?;
                    if bytes.len() <= LOGO_MAX_BYTES {
                        CachedLogo {
                            status,
                            content_type,
                            bytes: bytes.to_vec(),
                            fetched_at: now,
                        }
                    } else {
                        // Oversized — serve once, don't cache the blob.
                        return Ok(LogoResponse {
                            status,
                            content_type,
                            bytes: bytes.to_vec(),
                        });
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
        snap.health[idx][active].mark_failure(now_ms);
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

    async fn fetch_source_playlist(
        &self,
        source: &channels::StreamSource,
    ) -> Result<(String, Url), LiveTvError> {
        let mut req = self.inner.http.get(&source.url).timeout(PLAYLIST_TIMEOUT);
        if let Some(ua) = &source.user_agent {
            req = req.header(reqwest::header::USER_AGENT, ua);
        }
        if let Some(referrer) = &source.referrer {
            req = req.header(reqwest::header::REFERER, referrer);
        }
        let resp = req
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| LiveTvError::Upstream(e.to_string()))?;
        let final_url = resp.url().clone();
        let body = resp
            .text()
            .await
            .map_err(|e| LiveTvError::Upstream(e.to_string()))?;
        if body.len() > MAX_PLAYLIST_BYTES {
            return Err(LiveTvError::Upstream("playlist too large".into()));
        }
        if !body.trim_start().starts_with("#EXTM3U") {
            return Err(LiveTvError::Upstream("not an HLS playlist".into()));
        }
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
    ) -> Result<(reqwest::Response, Url), LiveTvError> {
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
        let (user_agent, referrer) = self.channel_headers(channel_key).await;
        let mut req = self.inner.http.get(upstream.clone());
        if let Some(ua) = user_agent {
            req = req.header(reqwest::header::USER_AGENT, ua);
        }
        if let Some(r) = referrer {
            req = req.header(reqwest::header::REFERER, r);
        }
        // NOTE: no `error_for_status` here. Live segments roll off the
        // window, so a slightly-late fetch legitimately 404s — that must be
        // forwarded to the player AS a 404 (hls.js retries / gap-skips it),
        // NOT rewritten to a 502 that reads as a dead gateway and tanks the
        // stream. Only a real connection failure (can't reach the host) maps
        // to Upstream/502 below.
        let resp = req
            .send()
            .await
            .map_err(|e| LiveTvError::Upstream(e.to_string()))?;
        let final_url = resp.url().clone();
        Ok((resp, final_url))
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
        let fails = health.segment_failures.fetch_add(1, Ordering::Relaxed) + 1;
        if fails < SEGMENT_FAIL_THRESHOLD {
            return;
        }
        // Too many bad segments — cool this source down and elect the next
        // healthy one. Reset the counter so the replacement gets a clean run.
        health.mark_failure(epoch_ms());
        health.segment_failures.store(0, Ordering::Relaxed);
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

    async fn channel_headers(&self, channel_key: &str) -> (Option<String>, Option<String>) {
        let Some((country, id)) = channel_key.split_once(':') else {
            return (None, None);
        };
        let Ok(snap) = self.channels(country).await else {
            return (None, None);
        };
        let Some(idx) = snap.channel_index(id) else {
            return (None, None);
        };
        let active = snap.active_source[idx].load(Ordering::Relaxed);
        let source = snap.channels[idx]
            .sources
            .get(active)
            .or_else(|| snap.channels[idx].sources.first());
        source.map_or((None, None), |s| (s.user_agent.clone(), s.referrer.clone()))
    }

    /// Playlist body cap for nested playlists fetched through the proxy.
    pub fn max_playlist_bytes(&self) -> usize {
        MAX_PLAYLIST_BYTES
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
        let url = self.inner.cfg.epg_urls.get(country)?.clone();
        if let Some(snap) = self.inner.epg.read().expect("poisoned").get(country) {
            return Some(snap.index.clone());
        }
        let _guard = self.inner.load_lock.lock().await;
        if let Some(snap) = self.inner.epg.read().expect("poisoned").get(country) {
            return Some(snap.index.clone());
        }
        match self.fetch_epg(&url).await {
            Ok(index) => {
                let index = Arc::new(index);
                self.inner.epg.write().expect("poisoned").insert(
                    country.to_string(),
                    Arc::new(EpgSnapshot {
                        index: index.clone(),
                        fetched_at: Instant::now(),
                    }),
                );
                Some(index)
            }
            Err(e) => {
                tracing::warn!(country, error = %e, "live tv EPG fetch failed");
                None
            }
        }
    }

    async fn fetch_epg(&self, url: &str) -> Result<epg::EpgIndex, LiveTvError> {
        let resp = self
            .inner
            .http
            .get(url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| LiveTvError::Upstream(e.to_string()))?;
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| LiveTvError::Upstream(e.to_string()))?;
        // Guides are served gzipped-as-body; accept plain XML too.
        let xml = epg::decode_gzip(&bytes)
            .or_else(|_| String::from_utf8(bytes.to_vec()))
            .map_err(|_| LiveTvError::Upstream("guide is neither gzip nor utf-8 xml".into()))?;
        let index = epg::parse_xmltv(&xml, chrono::Utc::now());
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
        let tvg_id = channel.tvg_id.as_ref()?;
        // The guide may key on the full id ("TF1.fr") or the base without
        // the variant qualifier ("TF1.fr@HD" → "TF1.fr").
        let base = tvg_id.split('@').next().unwrap_or(tvg_id);
        for candidate in [tvg_id.as_str(), base] {
            if index.contains(candidate) {
                return Some(candidate.to_string());
            }
        }
        None
    }

    /// Background refresh: re-fetch loaded playlists / guides past their
    /// TTL. Runs forever; spawn once at boot.
    pub fn spawn_refresh_loop(self) {
        let playlist_ttl = Duration::from_hours(self.inner.cfg.playlist_refresh_hours.max(1));
        let epg_ttl = Duration::from_hours(self.inner.cfg.epg_refresh_hours.max(1));
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_mins(15));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            ticker.tick().await; // skip immediate boot tick
            loop {
                ticker.tick().await;
                self.refresh_stale(playlist_ttl, epg_ttl).await;
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

        let stale_epg: Vec<(String, String)> = {
            let epgs = self.inner.epg.read().expect("poisoned");
            epgs.iter()
                .filter(|(_, s)| s.fetched_at.elapsed() > epg_ttl)
                .filter_map(|(c, _)| {
                    self.inner
                        .cfg
                        .epg_urls
                        .get(c)
                        .map(|u| (c.clone(), u.clone()))
                })
                .collect()
        };
        for (country, url) in stale_epg {
            match self.fetch_epg(&url).await {
                Ok(index) => {
                    self.inner.epg.write().expect("poisoned").insert(
                        country,
                        Arc::new(EpgSnapshot {
                            index: Arc::new(index),
                            fetched_at: Instant::now(),
                        }),
                    );
                }
                Err(e) => {
                    tracing::warn!(country, error = %e, "live tv EPG refresh failed");
                }
            }
        }
    }
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

fn epoch_ms() -> u64 {
    u64::try_from(chrono::Utc::now().timestamp_millis()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_country_accepts_alpha2_only() {
        assert_eq!(validate_country("FR").unwrap(), "fr");
        assert_eq!(validate_country("de").unwrap(), "de");
        assert!(validate_country("fra").is_err());
        assert!(validate_country("f").is_err());
        assert!(validate_country("..").is_err());
        assert!(validate_country("f/").is_err());
    }
}
