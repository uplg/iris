//! dlive.sx (a `DaddyLiveHD` mirror) as a live source.
//!
//! Each listed channel becomes one `dlive://<id>/<player>` source per
//! configured player, so the per-URL health and cooldown handle the fallback
//! between players like between any other feeds. Resolution:
//! - Player 1 is built from the id and an edge host learned once a day from
//!   one embed page (`…/premium<id>/index.m3u8`, no token, no page fetch).
//!   Its segments are MPEG-TS hidden in images (see [`unwrap`]).
//! - The other players start from the iframe of their dlive.sx page, scraped
//!   lazily (budgeted, cached for `embed_cache_hours`), then follow the
//!   third-party embed to a signed `_econfig` URL (Player 2: bound to the
//!   User-Agent that fetched the page, valid 6 h) or a 5-minute token
//!   (Player 6: refreshed through the embed's endpoint).
//!
//! dlive.sx bans an IP for ~10 minutes after ~100 page loads in ~17
//! minutes, while the embeds and edges keep answering. So every dlive.sx
//! load goes through one hard budget, none happens on a channel switch, and
//! a breaker skips every dlive source while dlive looks down.

mod parse;
pub mod unwrap;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

use iris_config::DliveConfig;
use url::Url;

use super::channels::SourceOrigin;
use super::m3u::M3uEntry;
use super::refresh_cell::RefreshCell;

pub const SCHEME: &str = "dlive://";

/// The one User-Agent of every dlive request. Player 2's signed playlist is
/// bound to the UA that fetched its embed page, so page and playlist must
/// share it.
pub const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36";

/// dlive.sx pages are ~650 KB of ad scripts.
const SITE_TIMEOUT: Duration = Duration::from_secs(10);
const EMBED_TIMEOUT: Duration = Duration::from_secs(8);
/// A failed channel-list load is retried this late, so an unreachable or
/// moved dlive.sx is not hammered.
const INDEX_RETRY: Duration = Duration::from_mins(30);
/// How long a country load waits for a cold channel list before going on
/// without dlive (the load keeps running for the next refresh).
const INDEX_WAIT: Duration = Duration::from_secs(4);
/// Minimum spacing between two edge-host lookups.
const EDGE_RELEARN_EVERY: Duration = Duration::from_mins(30);
/// A half-open breaker probe that never reported back stops blocking others.
const PROBE_STALE: Duration = Duration::from_mins(1);
/// Re-resolve a signed URL this long before its expiry.
const SIGNED_MARGIN_S: i64 = 600;
/// Refresh a Player 6 token this long before its expiry.
const TOKEN_MARGIN_S: i64 = 60;
/// Validity assumed for a resolution that carries no expiry.
const DEFAULT_VALIDITY_S: i64 = 1800;
const MAX_HOPS: usize = 4;
/// Probe id for the edge-host lookup when no country lists one.
const DEFAULT_PROBE_ID: u32 = 51;

/// `dlive://469/1`.
pub fn sentinel(id: u32, player: u8) -> String {
    format!("{SCHEME}{id}/{player}")
}

/// `dlive://469/1` → `(469, 1)`.
pub fn parse_sentinel(url: &str) -> Option<(u32, u8)> {
    let (id, player) = url.strip_prefix(SCHEME)?.split_once('/')?;
    Some((id.parse().ok()?, player.parse().ok()?))
}

/// The dlive.sx page of a player.
fn player_path(player: u8) -> Option<&'static str> {
    Some(match player {
        1 => "stream",
        2 => "cast",
        3 => "watch",
        4 => "plus",
        5 => "casting",
        6 => "player",
        _ => return None,
    })
}

/// Why a dlive source can't be used right now.
#[derive(Debug)]
pub enum ResolveError {
    /// Not the source's fault (breaker open, page budget spent, embed not
    /// scraped yet): skip it without a cooldown.
    Skip(&'static str),
    /// This channel is not on this player (404, no player in the embed).
    Missing(String),
    /// dlive or the embed host looks down: counts toward the breaker.
    Outage(String),
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Skip(why) => write!(f, "dlive skipped: {why}"),
            Self::Missing(why) | Self::Outage(why) => write!(f, "dlive: {why}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    /// Plain URL, no token (Player 1 and its clones).
    Plain,
    /// `?s=…&e=…` signature bound to [`UA`].
    Signed,
    /// `?token=…` valid five minutes, renewed through `refresh_url`.
    Token {
        slug: String,
        token: String,
        refresh_url: String,
    },
}

/// A dlive source resolved to a playable URL plus the headers it demands.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub url: String,
    pub user_agent: Option<String>,
    pub referrer: Option<String>,
    kind: Kind,
    /// Unix seconds after which the resolution is redone.
    valid_until: i64,
}

impl Resolved {
    /// A 403 on this URL can mean an expired signature or token.
    pub fn is_signed(&self) -> bool {
        self.kind != Kind::Plain
    }
}

/// How hard [`Dlive::resolve`] may try.
#[derive(Debug, Clone, Copy, Default)]
pub struct ResolveMode {
    /// Ignore an open breaker (nothing else can play this channel).
    pub bypass_breaker: bool,
    /// Scrape a missing embed inline instead of in the background.
    pub inline_embed: bool,
}

/// Consecutive-outage breaker with a single half-open probe.
#[derive(Debug)]
struct Breaker {
    threshold: u32,
    open_for: Duration,
    failures: u32,
    open_until: Option<Instant>,
    probe_started: Option<Instant>,
}

impl Breaker {
    fn new(threshold: u32, open_for: Duration) -> Self {
        Self {
            threshold: threshold.max(1),
            open_for,
            failures: 0,
            open_until: None,
            probe_started: None,
        }
    }

    fn is_open(&self, now: Instant) -> bool {
        self.open_until.is_some_and(|until| now < until)
    }

    /// Whether one attempt may go through now. Past the open window, one
    /// attempt at a time probes; its outcome closes or re-opens.
    fn admits(&mut self, now: Instant) -> bool {
        match self.open_until {
            None => true,
            Some(until) if now < until => false,
            Some(_) => {
                if self
                    .probe_started
                    .is_some_and(|t| now.duration_since(t) < PROBE_STALE)
                {
                    return false;
                }
                self.probe_started = Some(now);
                true
            }
        }
    }

    fn success(&mut self) {
        self.failures = 0;
        self.open_until = None;
        self.probe_started = None;
    }

    /// A dlive.sx page answered: closes a breaker that was probing, but a
    /// reachable site says nothing about the edges, so it doesn't reset a
    /// closed breaker's run of playback outages.
    fn site_answered(&mut self) {
        if self.open_until.is_some() {
            self.success();
        }
    }

    fn failure(&mut self, now: Instant) {
        self.failures = self.failures.saturating_add(1);
        if self.open_until.is_some() || self.failures >= self.threshold {
            self.trip(now);
        }
    }

    fn trip(&mut self, now: Instant) {
        self.open_until = Some(now + self.open_for);
        self.probe_started = None;
        self.failures = 0;
    }
}

/// Sliding-window budget of dlive.sx page loads.
#[derive(Debug)]
struct PageBudget {
    budget: usize,
    window: Duration,
    loads: VecDeque<Instant>,
}

impl PageBudget {
    fn prune(&mut self, now: Instant) {
        while self
            .loads
            .front()
            .is_some_and(|t| now.duration_since(*t) >= self.window)
        {
            self.loads.pop_front();
        }
    }

    fn used(&mut self, now: Instant) -> usize {
        self.prune(now);
        self.loads.len()
    }

    fn take(&mut self, now: Instant) -> bool {
        if self.used(now) >= self.budget {
            return false;
        }
        self.loads.push_back(now);
        true
    }
}

#[derive(Debug)]
struct Guard {
    breaker: Breaker,
    pages: PageBudget,
}

/// The channel list: id → raw dlive name.
#[derive(Debug, Default)]
struct Index {
    names: HashMap<u32, String>,
}

type Key = (u32, u8);

/// What a player's dlive.sx page embeds.
#[derive(Debug, Clone)]
enum Embed {
    Found(String),
    /// No iframe, or the page 404s: the channel is not on this player.
    Absent,
}

impl Embed {
    fn into_url(self, (id, player): Key) -> Result<String, ResolveError> {
        match self {
            Self::Found(url) => Ok(url),
            Self::Absent => Err(ResolveError::Missing(format!(
                "no player {player} for {id}"
            ))),
        }
    }
}

/// Learned Player 1 edge: the playlist template and the embed page it came
/// from (re-read first when the edge moves, before any dlive.sx load).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Edge {
    template: String,
    embed_url: String,
}

pub struct Dlive {
    cfg: DliveConfig,
    http: reqwest::Client,
    guard: Mutex<Guard>,
    index: RefreshCell<Index>,
    edge: RwLock<Option<Edge>>,
    edge_attempt: Mutex<Option<Instant>>,
    /// Bumped whenever the edge template changes, so the service can clear
    /// the cooldowns earned on the old host.
    edge_generation: AtomicU64,
    /// Scraped player iframes, with when (epoch seconds). Kept on disk
    /// ([`Self::persist_at`]): the codes are stable, and re-scraping them
    /// through the page budget after a restart took over an hour.
    embeds: RwLock<HashMap<Key, (Embed, i64)>>,
    state_file: OnceLock<PathBuf>,
    pending_embeds: Mutex<HashSet<Key>>,
    resolved: RwLock<HashMap<Key, Resolved>>,
    resolve_locks: Mutex<HashMap<Key, Arc<tokio::sync::Mutex<()>>>>,
    embed_locks: Mutex<HashMap<Key, Arc<tokio::sync::Mutex<()>>>>,
}

fn epoch_s() -> i64 {
    chrono::Utc::now().timestamp()
}

fn origin_of(url: &Url) -> String {
    format!("{}/", url.origin().ascii_serialization())
}

impl Dlive {
    pub fn new(cfg: DliveConfig, http: reqwest::Client) -> Self {
        let guard = Guard {
            breaker: Breaker::new(
                cfg.breaker_failures,
                Duration::from_mins(cfg.breaker_open_mins.max(1)),
            ),
            pages: PageBudget {
                budget: usize::try_from(cfg.page_budget).unwrap_or(usize::MAX),
                window: Duration::from_mins(cfg.page_budget_window_mins.max(1)),
                loads: VecDeque::new(),
            },
        };
        Self {
            cfg,
            http,
            guard: Mutex::new(guard),
            index: RefreshCell::default(),
            edge: RwLock::new(None),
            edge_attempt: Mutex::new(None),
            edge_generation: AtomicU64::new(0),
            embeds: RwLock::new(HashMap::new()),
            state_file: OnceLock::new(),
            pending_embeds: Mutex::new(HashSet::new()),
            resolved: RwLock::new(HashMap::new()),
            resolve_locks: Mutex::new(HashMap::new()),
            embed_locks: Mutex::new(HashMap::new()),
        }
    }

    pub fn enabled(&self) -> bool {
        self.cfg.enabled
    }

    pub fn first_byte_timeout(&self) -> Duration {
        Duration::from_secs(self.cfg.first_byte_timeout_secs.max(1))
    }

    pub fn cold_start_timeout(&self) -> Duration {
        Duration::from_secs(self.cfg.cold_start_timeout_secs.max(1))
    }

    pub fn countries(&self) -> impl Iterator<Item = &String> {
        self.cfg.countries.keys()
    }

    pub fn breaker_open(&self) -> bool {
        self.guard
            .lock()
            .expect("poisoned")
            .breaker
            .is_open(Instant::now())
    }

    /// A dlive host answered (a playlist, or a clean 404).
    pub fn note_ok(&self) {
        self.guard.lock().expect("poisoned").breaker.success();
    }

    /// A dlive host failed like an outage (unreachable, timeout, 5xx,
    /// undecodable page, a run of bad segments).
    pub fn note_outage(&self) {
        self.guard
            .lock()
            .expect("poisoned")
            .breaker
            .failure(Instant::now());
    }

    pub fn edge_generation(&self) -> u64 {
        self.edge_generation.load(Ordering::Relaxed)
    }

    /// Forget a resolution (its signed URL got a 403).
    pub fn invalidate(&self, id: u32, player: u8) {
        self.resolved
            .write()
            .expect("poisoned")
            .remove(&(id, player));
    }

    /// Headers of the last resolution, for proxied playlist and segment
    /// requests. Player 1 needs none.
    pub fn cached_headers(&self, id: u32, player: u8) -> (Option<String>, Option<String>) {
        self.resolved
            .read()
            .expect("poisoned")
            .get(&(id, player))
            .map_or((None, None), |r| (r.user_agent.clone(), r.referrer.clone()))
    }

    /// One playlist entry per listed channel of `country` and per player,
    /// grouped by player (each its own origin rank). Empty while the list
    /// is unavailable.
    pub async fn entries(self: &Arc<Self>, country: &str) -> Vec<(SourceOrigin, Vec<M3uEntry>)> {
        let Some(ids) = self
            .cfg
            .countries
            .get(country)
            .filter(|ids| !ids.is_empty())
        else {
            return Vec::new();
        };
        let Some(index) = self.index_within(INDEX_WAIT).await else {
            return Vec::new();
        };
        entries_for(&index, ids, &self.cfg.players, country)
    }

    async fn index_within(self: &Arc<Self>, wait: Duration) -> Option<Arc<Index>> {
        let me = self.clone();
        let task = tokio::spawn(async move {
            let ttl = Duration::from_hours(me.cfg.index_refresh_hours.max(1));
            me.index.get(ttl, INDEX_RETRY, || me.load_index()).await
        });
        match tokio::time::timeout(wait, task).await {
            Ok(Ok(index)) => index,
            _ => self.index.peek(),
        }
    }

    async fn load_index(&self) -> Option<Index> {
        let url = format!("{}/24-7-channels.php", self.base());
        let body = match self.site_page(&url).await {
            Ok((body, _)) => body,
            Err(e) => {
                tracing::warn!(error = %e, "dlive channel list unavailable");
                return None;
            }
        };
        let list = parse::channel_list(body.as_bytes());
        if list.is_empty() {
            tracing::warn!("dlive channel list parsed to nothing (moved domain or new layout?)");
            return None;
        }
        tracing::info!(channels = list.len(), "dlive channel list loaded");
        if self.edge.read().expect("poisoned").is_none() {
            self.learn_edge().await;
        }
        Some(Index {
            names: list.into_iter().map(|c| (c.id, c.name)).collect(),
        })
    }

    fn base(&self) -> &str {
        self.cfg.base_url.trim_end_matches('/')
    }

    fn probe_id(&self) -> u32 {
        self.cfg
            .countries
            .values()
            .flatten()
            .min()
            .copied()
            .unwrap_or(DEFAULT_PROBE_ID)
    }

    /// One budgeted dlive.sx page load. An unreachable dlive.sx (its ban
    /// refuses TCP) opens the breaker at once.
    async fn site_page(&self, url: &str) -> Result<(String, Url), ResolveError> {
        {
            let mut guard = self.guard.lock().expect("poisoned");
            let now = Instant::now();
            if !guard.breaker.admits(now) {
                return Err(ResolveError::Skip("dlive breaker open"));
            }
            if !guard.pages.take(now) {
                return Err(ResolveError::Skip("dlive page budget spent"));
            }
        }
        let referer = format!("{}/", self.base());
        let result = self.get_page(url, Some(&referer), SITE_TIMEOUT).await;
        match &result {
            Err(ResolveError::Outage(_)) => {
                self.guard
                    .lock()
                    .expect("poisoned")
                    .breaker
                    .trip(Instant::now());
            }
            Ok(_) | Err(ResolveError::Missing(_)) => {
                self.guard.lock().expect("poisoned").breaker.site_answered();
            }
            Err(ResolveError::Skip(_)) => {}
        }
        result
    }

    async fn get_page(
        &self,
        url: &str,
        referer: Option<&str>,
        timeout: Duration,
    ) -> Result<(String, Url), ResolveError> {
        let mut req = self
            .http
            .get(url)
            .header(reqwest::header::USER_AGENT, UA)
            .timeout(timeout);
        if let Some(r) = referer {
            req = req.header(reqwest::header::REFERER, r);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| ResolveError::Outage(super::upstream_err(e).to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            let why = format!("HTTP {} from {}", status.as_u16(), host_of(url));
            return Err(
                if status.is_server_error() || matches!(status.as_u16(), 403 | 408 | 429) {
                    ResolveError::Outage(why)
                } else {
                    ResolveError::Missing(why)
                },
            );
        }
        let final_url = resp.url().clone();
        let body = super::read_playlist(resp)
            .await
            .map_err(|e| ResolveError::Outage(e.to_string()))?;
        Ok((body, final_url))
    }

    fn edge_template(&self) -> Option<String> {
        self.edge
            .read()
            .expect("poisoned")
            .as_ref()
            .map(|e| e.template.clone())
    }

    /// Learn Player 1's edge from one embed page: the known embed first
    /// (no dlive.sx load), dlive.sx's Player 1 page only when that fails.
    async fn learn_edge(&self) {
        {
            let mut at = self.edge_attempt.lock().expect("poisoned");
            if at.is_some_and(|t| t.elapsed() < EDGE_RELEARN_EVERY) {
                return;
            }
            *at = Some(Instant::now());
        }
        let probe = self.probe_id();
        let known = self
            .edge
            .read()
            .expect("poisoned")
            .as_ref()
            .map(|e| e.embed_url.clone());
        let mut learned = None;
        if let Some(embed_url) = known {
            learned = self.edge_from_embed(&embed_url, probe).await;
        }
        if learned.is_none() {
            let page = format!("{}/stream/stream-{probe}.php", self.base());
            if let Ok((html, final_url)) = self.site_page(&page).await
                && let Some(src) = parse::player_iframe(&html)
                && let Ok(embed) = final_url.join(&src)
            {
                learned = self.edge_from_embed(embed.as_str(), probe).await;
            }
        }
        let Some(edge) = learned else {
            tracing::warn!("dlive player 1 edge host not found");
            return;
        };
        let mut slot = self.edge.write().expect("poisoned");
        if slot.as_ref().is_none_or(|e| e.template != edge.template) {
            tracing::info!(template = %edge.template, "dlive player 1 edge learned");
            *slot = Some(edge);
            self.edge_generation.fetch_add(1, Ordering::Relaxed);
            drop(slot);
            self.save();
        }
    }

    /// Keep the scraped embeds and the learned edge in `dir/dlive.json`, and
    /// load what an earlier run left there. Unreadable state is ignored (it
    /// only costs page loads).
    pub fn persist_at(&self, dir: &Path) {
        let file = dir.join("dlive.json");
        if let Ok(bytes) = std::fs::read(&file) {
            match serde_json::from_slice::<Persisted>(&bytes) {
                Ok(p) => {
                    let mut embeds = self.embeds.write().expect("poisoned");
                    for e in p.embeds {
                        let src = e.url.map_or(Embed::Absent, Embed::Found);
                        embeds.insert((e.id, e.player), (src, e.at));
                    }
                    tracing::info!(embeds = embeds.len(), "dlive state loaded");
                    drop(embeds);
                    if let Some(edge) = p.edge {
                        *self.edge.write().expect("poisoned") = Some(edge);
                    }
                }
                Err(e) => tracing::warn!(error = %e, "dlive state unreadable, starting empty"),
            }
        }
        let _ = self.state_file.set(file);
    }

    fn save(&self) {
        let Some(file) = self.state_file.get().cloned() else {
            return;
        };
        let snapshot = Persisted {
            edge: self.edge.read().expect("poisoned").clone(),
            embeds: self
                .embeds
                .read()
                .expect("poisoned")
                .iter()
                .map(|(&(id, player), (src, at))| PersistedEmbed {
                    id,
                    player,
                    url: match src {
                        Embed::Found(url) => Some(url.clone()),
                        Embed::Absent => None,
                    },
                    at: *at,
                })
                .collect(),
        };
        tokio::task::spawn_blocking(move || {
            if let Err(e) = write_atomically(&file, &snapshot) {
                tracing::warn!(error = %e, "dlive state not saved");
            }
        });
    }

    async fn edge_from_embed(&self, embed_url: &str, probe: u32) -> Option<Edge> {
        let (html, _) = self.get_page(embed_url, None, EMBED_TIMEOUT).await.ok()?;
        let template = parse::edge_template(&parse::const_src(&html)?, probe)?;
        Some(Edge {
            template,
            embed_url: embed_url.to_string(),
        })
    }

    /// Re-learn the edge in the background (Player 1 hit an unreachable
    /// host). Rate-limited inside [`Self::learn_edge`].
    pub fn spawn_learn_edge(self: &Arc<Self>) {
        let me = self.clone();
        tokio::spawn(async move { me.learn_edge().await });
    }

    /// A known embed and whether it is past `embed_cache_hours`. A stale one
    /// still serves (the codes rarely move) while a refresh is queued.
    fn embed(&self, key: Key) -> Option<(Embed, bool)> {
        let ttl = i64::try_from(self.cfg.embed_cache_hours.max(1) * 3600).unwrap_or(i64::MAX);
        let now = epoch_s();
        self.embeds
            .read()
            .expect("poisoned")
            .get(&key)
            .map(|(src, at)| (src.clone(), now - at >= ttl))
    }

    fn fresh_embed(&self, key: Key) -> Option<Embed> {
        self.embed(key)
            .and_then(|(src, stale)| (!stale).then_some(src))
    }

    /// Scrape one player page's iframe, once per key at a time: a caller
    /// that waited on another's fetch gets its result.
    async fn fetch_embed(&self, key: Key) -> Result<String, ResolveError> {
        let (id, player) = key;
        let lock = lock_in(&self.embed_locks, key);
        let _held = lock.lock().await;
        if let Some(found) = self.fresh_embed(key) {
            return found.into_url(key);
        }
        let Some(path) = player_path(player) else {
            return Err(ResolveError::Missing(format!("unknown player {player}")));
        };
        let page = format!("{}/{path}/stream-{id}.php", self.base());
        let found = match self.site_page(&page).await {
            Ok((html, final_url)) => parse::player_iframe(&html)
                .and_then(|src| final_url.join(&src).ok())
                .map_or(Embed::Absent, |url| Embed::Found(url.into())),
            Err(ResolveError::Missing(_)) => Embed::Absent,
            Err(e) => return Err(e),
        };
        self.embeds
            .write()
            .expect("poisoned")
            .insert(key, (found.clone(), epoch_s()));
        self.save();
        found.into_url(key)
    }

    /// Scrape a missing embed off the request path. One fetch per key at a
    /// time; the page budget still applies.
    fn spawn_embed_fetch(self: &Arc<Self>, key: Key) {
        if !self.pending_embeds.lock().expect("poisoned").insert(key) {
            return;
        }
        let me = self.clone();
        tokio::spawn(async move {
            let _ = me.fetch_embed(key).await;
            me.pending_embeds.lock().expect("poisoned").remove(&key);
        });
    }

    /// Queue the embeds a channel's dlive sources still lack, so its next
    /// election finds them cached.
    pub fn prefetch(self: &Arc<Self>, keys: impl IntoIterator<Item = Key>) {
        if self.breaker_open() {
            return;
        }
        for key in keys {
            if key.1 != 1 && player_path(key.1).is_some() && self.fresh_embed(key).is_none() {
                self.spawn_embed_fetch(key);
            }
        }
    }

    /// Background warm-up, one call a minute: keep the list fresh and scrape
    /// one missing embed of the configured channels, using at most half the
    /// page budget so zaps keep the other half.
    pub async fn warm(self: &Arc<Self>) {
        if self.breaker_open() {
            return;
        }
        let Some(index) = self.index_within(SITE_TIMEOUT).await else {
            return;
        };
        {
            let mut guard = self.guard.lock().expect("poisoned");
            let half = guard.pages.budget / 2;
            if guard.pages.used(Instant::now()) >= half {
                return;
            }
        }
        let mut ids: Vec<u32> = self.cfg.countries.values().flatten().copied().collect();
        ids.sort_unstable();
        ids.dedup();
        let wanted: Vec<Key> = ids
            .into_iter()
            .filter(|id| index.names.contains_key(id))
            .flat_map(|id| self.cfg.players.iter().map(move |&p| (id, p)))
            .filter(|&key| {
                key.1 != 1
                    && player_path(key.1).is_some()
                    && self.fresh_embed(key).is_none()
                    && !self.pending_embeds.lock().expect("poisoned").contains(&key)
            })
            .collect();
        // a channel with no embed at all before one that only needs a refresh
        let next = wanted
            .iter()
            .find(|&&key| self.embed(key).is_none())
            .or_else(|| wanted.first())
            .copied();
        if let Some(key) = next {
            let _ = self.fetch_embed(key).await;
        }
    }

    /// Resolve `dlive://<id>/<player>` to a playable URL. Never loads a
    /// dlive.sx page unless `mode.inline_embed` (nothing else can play).
    pub async fn resolve(
        self: &Arc<Self>,
        id: u32,
        player: u8,
        mode: ResolveMode,
    ) -> Result<Resolved, ResolveError> {
        if !mode.bypass_breaker
            && !self
                .guard
                .lock()
                .expect("poisoned")
                .breaker
                .admits(Instant::now())
        {
            return Err(ResolveError::Skip("dlive breaker open"));
        }
        if player == 1 {
            let Some(template) = self.edge_template() else {
                self.spawn_learn_edge();
                return Err(ResolveError::Skip("dlive edge host unknown"));
            };
            return Ok(Resolved {
                url: template.replace("{id}", &id.to_string()),
                user_agent: None,
                referrer: None,
                kind: Kind::Plain,
                valid_until: i64::MAX,
            });
        }
        let key = (id, player);
        let lock = lock_in(&self.resolve_locks, key);
        let _held = lock.lock().await;
        let now = epoch_s();
        let cached = self.resolved.read().expect("poisoned").get(&key).cloned();
        if let Some(r) = &cached
            && r.valid_until > now
        {
            return Ok(r.clone());
        }
        if let Some(r) = cached
            && let Some(renewed) = self.renew_token(&r).await
        {
            self.store(key, &renewed);
            return Ok(renewed);
        }
        let embed = match self.embed(key) {
            Some((found, stale)) => {
                if stale {
                    self.spawn_embed_fetch(key);
                }
                found.into_url(key)?
            }
            None if mode.inline_embed => self.fetch_embed(key).await?,
            None => {
                self.spawn_embed_fetch(key);
                return Err(ResolveError::Skip("dlive embed not scraped yet"));
            }
        };
        let resolved = self.follow(&embed).await?;
        self.store(key, &resolved);
        Ok(resolved)
    }

    fn store(&self, key: Key, r: &Resolved) {
        self.resolved
            .write()
            .expect("poisoned")
            .insert(key, r.clone());
    }

    /// Walk the embed's iframes to a known player shape.
    async fn follow(&self, embed: &str) -> Result<Resolved, ResolveError> {
        let mut url = embed.to_string();
        let mut referer = format!("{}/", self.base());
        for _ in 0..MAX_HOPS {
            let (html, final_url) = self.get_page(&url, Some(&referer), EMBED_TIMEOUT).await?;
            let origin = origin_of(&final_url);
            let now = epoch_s();
            if let Some(src) = parse::const_src(&html) {
                return Ok(Resolved {
                    url: src,
                    user_agent: None,
                    referrer: None,
                    kind: Kind::Plain,
                    valid_until: now + DEFAULT_VALIDITY_S,
                });
            }
            if parse::has_econfig(&html) {
                let cfg = parse::econfig(&html)
                    .ok_or_else(|| ResolveError::Outage("dlive econfig undecodable".into()))?;
                let expiry = parse::query_param(&cfg.stream_url, "e")
                    .and_then(|e| e.parse::<i64>().ok())
                    .map_or(now + DEFAULT_VALIDITY_S, |e| e - SIGNED_MARGIN_S);
                return Ok(Resolved {
                    url: cfg.stream_url,
                    user_agent: Some(UA.to_string()),
                    referrer: Some(origin),
                    kind: Kind::Signed,
                    valid_until: expiry,
                });
            }
            if let Some(cfg) = parse::wide_config(&html) {
                let expiry = parse::wide_token_expiry(&cfg.token)
                    .unwrap_or(now + i64::try_from(cfg.expires_in).unwrap_or(300));
                return Ok(Resolved {
                    url: cfg.stream_url,
                    user_agent: Some(UA.to_string()),
                    referrer: None,
                    kind: Kind::Token {
                        refresh_url: cfg
                            .refresh_url
                            .unwrap_or_else(|| format!("{origin}api/refresh_token.php")),
                        slug: cfg.slug,
                        token: cfg.token,
                    },
                    valid_until: expiry - TOKEN_MARGIN_S,
                });
            }
            if html.contains("domain protected") {
                return Err(ResolveError::Outage(format!(
                    "{} refuses our Referer",
                    host_of(&url)
                )));
            }
            let Some(next) = parse::player_iframe(&html).and_then(|s| final_url.join(&s).ok())
            else {
                return Err(ResolveError::Missing(format!(
                    "no known player behind {}",
                    host_of(&url)
                )));
            };
            referer = origin;
            url = next.into();
        }
        Err(ResolveError::Missing("dlive embed chain too deep".into()))
    }

    /// A Player 6 token renewed through its endpoint; `None` when the source
    /// is not token-based or the endpoint failed (the caller re-follows).
    async fn renew_token(&self, r: &Resolved) -> Option<Resolved> {
        let Kind::Token {
            slug,
            token,
            refresh_url,
        } = &r.kind
        else {
            return None;
        };
        let reply: parse::RefreshReply = self
            .http
            .post(refresh_url)
            .header(reqwest::header::USER_AGENT, UA)
            .timeout(EMBED_TIMEOUT)
            .json(&serde_json::json!({ "channel": slug, "current_token": token }))
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?
            .json()
            .await
            .ok()?;
        let fresh = reply.token.filter(|t| reply.success && !t.is_empty())?;
        let url = parse::replace_query_param(&r.url, "token", &fresh)?;
        let expiry = parse::wide_token_expiry(&fresh).unwrap_or_else(|| {
            epoch_s() + i64::try_from(reply.expires_in.unwrap_or(300)).unwrap_or(300)
        });
        Some(Resolved {
            url,
            user_agent: r.user_agent.clone(),
            referrer: r.referrer.clone(),
            kind: Kind::Token {
                slug: slug.clone(),
                token: fresh,
                refresh_url: refresh_url.clone(),
            },
            valid_until: expiry - TOKEN_MARGIN_S,
        })
    }

    /// The current form of a proxied URL a client still holds: a Player 6
    /// URI with the live token, a Player 2 playlist with the live signature.
    /// `None` = fetch it as is.
    pub async fn refresh_upstream(
        self: &Arc<Self>,
        id: u32,
        player: u8,
        upstream: &Url,
    ) -> Option<Url> {
        if player == 1 {
            return None;
        }
        let mode = ResolveMode {
            bypass_breaker: true,
            inline_embed: false,
        };
        let r = self.resolve(id, player, mode).await.ok()?;
        current_form(&r, upstream)
    }
}

/// `upstream` rewritten to the live signature/token of `r`, when it is one
/// of `r`'s URLs carrying a stale one.
fn current_form(r: &Resolved, upstream: &Url) -> Option<Url> {
    let live = Url::parse(&r.url).ok()?;
    if live.host_str() != upstream.host_str() {
        return None;
    }
    match &r.kind {
        Kind::Plain => None,
        Kind::Signed => {
            (live.path() == upstream.path() && live.query() != upstream.query()).then_some(live)
        }
        Kind::Token { token, .. } => {
            let held = parse::query_param(upstream.as_str(), "token")?;
            if held == token {
                return None;
            }
            parse::replace_query_param(upstream.as_str(), "token", token)
                .and_then(|u| Url::parse(&u).ok())
        }
    }
}

fn lock_in(
    locks: &Mutex<HashMap<Key, Arc<tokio::sync::Mutex<()>>>>,
    key: Key,
) -> Arc<tokio::sync::Mutex<()>> {
    locks
        .lock()
        .expect("poisoned")
        .entry(key)
        .or_default()
        .clone()
}

fn host_of(url: &str) -> String {
    Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_else(|| "upstream".into())
}

fn entries_for(
    index: &Index,
    ids: &[u32],
    players: &[u8],
    country: &str,
) -> Vec<(SourceOrigin, Vec<M3uEntry>)> {
    let mut seen = HashSet::new();
    players
        .iter()
        .filter(|p| player_path(**p).is_some() && seen.insert(**p))
        .enumerate()
        .map(|(rank, &player)| {
            let entries = ids
                .iter()
                .filter_map(|&id| {
                    let raw = index.names.get(&id)?;
                    let name = parse::clean_name(raw, country);
                    let mut attrs = HashMap::new();
                    if let Some(category) = parse::category_for(&name) {
                        attrs.insert("group-title".to_string(), category.to_string());
                    }
                    Some(M3uEntry {
                        name,
                        attrs,
                        vlc_opts: HashMap::new(),
                        url: sentinel(id, player),
                    })
                })
                .collect();
            (
                SourceOrigin::Dlive {
                    rank: u8::try_from(rank).unwrap_or(u8::MAX),
                },
                entries,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests;

#[derive(serde::Serialize, serde::Deserialize)]
struct Persisted {
    edge: Option<Edge>,
    embeds: Vec<PersistedEmbed>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PersistedEmbed {
    id: u32,
    player: u8,
    /// `None`: the channel is not on this player.
    url: Option<String>,
    at: i64,
}

fn write_atomically(file: &Path, state: &Persisted) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = file.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(state)?)?;
    std::fs::rename(tmp, file)
}
