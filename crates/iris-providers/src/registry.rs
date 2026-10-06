use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use chrono::{DateTime, Utc};

use iris_config::ProviderEntry;
use iris_core::Error;
use iris_core::Result;
use iris_core::search::{ProviderCapabilities, SearchQuery, SearchResult};
use serde::Serialize;
use utoipa::ToSchema;

use crate::SearchProvider;

/// Hard per-provider budget for one aggregated search. Comfortably above
/// a healthy indexer's worst case (sub-second to a few seconds) while
/// keeping a hung upstream from dragging the whole response toward the
/// 15–20 s client timeouts.
const SEARCH_DEADLINE: std::time::Duration = std::time::Duration::from_secs(8);

/// Per-provider cap on concurrent searches. Search-as-you-type bursts a
/// dozen fan-outs within seconds; a slow scraping tracker (hdtorrents)
/// then sees them all in parallel and starts answering 429. Two in
/// flight is plenty for one household — the excess queues inside the
/// search deadline and expires without ever hitting the tracker.
const MAX_INFLIGHT_PER_PROVIDER: usize = 2;

/// Deadline timeouts in a row after which searches stop asking a provider
/// for [`BREAKER_COOLDOWN`]: a tracker that is down would otherwise cost
/// every search the full [`SEARCH_DEADLINE`].
const BREAKER_TIMEOUTS: u32 = 3;
/// How long a provider that keeps timing out is skipped. Then one search
/// probes it: an answer closes the breaker, a timeout opens it again.
const BREAKER_COOLDOWN: std::time::Duration = std::time::Duration::from_mins(5);

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProviderInfo {
    pub id: String,
    pub capabilities: ProviderCapabilities,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProviderResultMeta {
    pub id: String,
    pub current_page: u32,
    pub limit: u32,
    pub total_count: Option<u64>,
    pub total_pages: Option<u32>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct AggregatedResults {
    pub results: Vec<SearchResult>,
    pub providers: Vec<ProviderResultMeta>,
    /// SCENE-parsed view of the user query when the raw `q` looked
    /// like a SCENE-style request (e.g. `Classroom of the Elite S04E11`).
    /// Frontend uses this to render a "Showing results for X · S04E11"
    /// banner; absent when the parser saw nothing useful.
    #[serde(default)]
    pub parsed_query: Option<ParsedQueryInfo>,
}

/// Surface of the SCENE parser run against the user's raw query string.
/// Lives here so iris-api's ranking module can construct it without
/// pulling iris-providers into iris-core.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ParsedQueryInfo {
    pub title: String,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub year: Option<u16>,
}

/// Per-provider behaviour declared in `providers.toml`, beyond "which
/// tracker is this". Everything here defaults to the pre-existing
/// behaviour, so an entry that declares none of it behaves exactly as
/// before.
#[derive(Debug, Clone)]
pub struct ProviderPolicy {
    /// Default language string (`"english"` / `"french"` / `"multi"`) for
    /// releases that ship with no explicit marker. Seedpool ships English
    /// by convention without ever tagging the file, and treating that as
    /// `Unknown` (= "no badge") would leave anglophone users without a
    /// visual cue. Francophone trackers tag explicitly, so they need none.
    pub default_language: Option<String>,
    /// Whether the freshness scheduler may ingest this provider's
    /// `latest()` feed into the discovery catalogue. `false` makes the
    /// provider search-only: it answers user queries and nothing else.
    ///
    /// This is what keeps a high-volume, narrow-taxonomy tracker from
    /// taking the catalogue over — nyaa.si indexes anime exclusively and
    /// publishes hundreds of releases a day, so letting its rolling window
    /// into the shelves would bury everything else under fansub raws while
    /// contributing nothing the household browses by.
    pub catalog: bool,
    /// Whether torrents grabbed from this provider keep seeding once they
    /// finish downloading. `false` pauses them at completion (files stay on
    /// disk, playback is unaffected — it reads from disk).
    pub seed: bool,
    /// How many of this tracker's torrents may be downloading at once, when
    /// the tracker itself enforces a cap. `None` = no cap declared.
    ///
    /// Over the cap, UNIT3D doesn't answer the `.torrent` request with a
    /// readable error: it 302s to the torrent page, which our JSON `Accept`
    /// turns into a Laravel `401 Unauthenticated`. Declaring the cap lets the
    /// grab be refused before we ask, with a message naming what holds the
    /// slot.
    pub leech_slots: Option<u32>,
    /// Language community the tracker serves (`"fr"` / `"en"`), declared as
    /// `origin` or implied by `default_language`. Gives an untagged release
    /// its meaning in the search language filter: English on an anglophone
    /// tracker, original version (no French track) on a francophone one,
    /// where releases with French audio or subtitles always say so.
    pub origin: Option<String>,
}

impl Default for ProviderPolicy {
    fn default() -> Self {
        Self {
            default_language: None,
            catalog: true,
            seed: true,
            leech_slots: None,
            origin: None,
        }
    }
}

/// Holds all enabled providers and fans out searches in parallel.
#[derive(Clone, Default)]
pub struct ProviderRegistry {
    providers: Arc<HashMap<String, Arc<dyn SearchProvider>>>,
    /// Provider-id → its `kind`, for every entry of the config, built or not.
    kinds: Arc<HashMap<String, String>>,
    /// Provider-id → its declared [`ProviderPolicy`].
    policies: Arc<HashMap<String, ProviderPolicy>>,
    /// Provider-id → semaphore bounding concurrent `search` calls
    /// ([`MAX_INFLIGHT_PER_PROVIDER`]).
    search_permits: Arc<HashMap<String, Arc<tokio::sync::Semaphore>>>,
    /// The TMDB id each tracker shipped with a release it listed, kept
    /// from the searches and feeds that passed through so a grab can
    /// record it (trust signal T1) — the grab request only carries
    /// `(provider_id, external_id)`.
    tracker_tmdb: Arc<Mutex<TrackerTmdbIds>>,
    /// Built providers an admin turned off at runtime (`provider_overrides`).
    /// Every fan-out and lookup skips them; the policies stay readable so
    /// seeding of what they already delivered is untouched.
    switched_off: Arc<RwLock<HashSet<String>>>,
    /// Provider-id → how its last aggregated search went.
    last_search: Arc<Mutex<HashMap<String, SearchOutcome>>>,
    /// Provider-id → its run of deadline timeouts ([`BREAKER_TIMEOUTS`]).
    breakers: Arc<Mutex<HashMap<String, Breaker>>>,
}

#[derive(Default)]
struct Breaker {
    timeouts: u32,
    /// Skipped until then; `None` while closed. A probe moves it to the
    /// probe's own deadline, so the searches meanwhile still skip it and a
    /// probe dropped half-way (client gone) can't hold it open for good.
    open_until: Option<tokio::time::Instant>,
}

enum Admit {
    Ask,
    Skip(std::time::Duration),
}

/// How a provider's last aggregated search went, for the admin view.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SearchOutcome {
    pub at: DateTime<Utc>,
    pub latency_ms: u64,
    /// `None` when it answered.
    pub error: Option<String>,
}

/// One `providers.toml` entry, as the admin sees it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ProviderStatus {
    pub id: String,
    pub kind: String,
    /// Asked by searches and feeds right now.
    pub enabled: bool,
    /// Built at boot: enabled in the config and constructed. Only these can
    /// be switched at runtime.
    pub configured: bool,
    pub last_search: Option<SearchOutcome>,
}

/// Bounded FIFO of `(provider_id, external_id) → tmdb_id`.
#[derive(Default)]
struct TrackerTmdbIds {
    ids: HashMap<(String, String), u64>,
    order: std::collections::VecDeque<(String, String)>,
}

const TRACKER_TMDB_CAP: usize = 8192;

impl TrackerTmdbIds {
    fn insert(&mut self, key: (String, String), id: u64) {
        if self.ids.insert(key.clone(), id).is_none() {
            self.order.push_back(key);
            while self.order.len() > TRACKER_TMDB_CAP {
                if let Some(old) = self.order.pop_front() {
                    self.ids.remove(&old);
                }
            }
        }
    }
}

impl ProviderRegistry {
    pub fn from_entries(entries: &[ProviderEntry]) -> Result<Self> {
        let mut map: HashMap<String, Arc<dyn SearchProvider>> = HashMap::new();
        let mut policies: HashMap<String, ProviderPolicy> = HashMap::new();
        let mut kinds: HashMap<String, String> = HashMap::new();
        for entry in entries {
            kinds.insert(entry.id.clone(), entry.kind.clone());
        }
        for entry in entries.iter().filter(|e| e.enabled) {
            policies.insert(entry.id.clone(), policy_of(entry));
            match build_provider(entry) {
                Ok(p) => {
                    tracing::info!(provider = %entry.id, kind = %entry.kind, "loaded provider");
                    map.insert(entry.id.clone(), p);
                }
                Err(e) => {
                    // Skip providers that fail to construct (e.g. missing env vars in
                    // dev) instead of bringing the whole API down.
                    tracing::error!(provider = %entry.id, kind = %entry.kind, error = %e, "failed to load provider, skipping");
                }
            }
        }
        let permits = map
            .keys()
            .map(|id| {
                (
                    id.clone(),
                    Arc::new(tokio::sync::Semaphore::new(MAX_INFLIGHT_PER_PROVIDER)),
                )
            })
            .collect();
        Ok(Self {
            providers: Arc::new(map),
            kinds: Arc::new(kinds),
            policies: Arc::new(policies),
            search_permits: Arc::new(permits),
            tracker_tmdb: Arc::default(),
            switched_off: Arc::default(),
            last_search: Arc::default(),
            breakers: Arc::default(),
        })
    }

    /// Whether `provider_id` was built from the config, on or off.
    pub fn is_configured(&self, provider_id: &str) -> bool {
        self.providers.contains_key(provider_id)
    }

    /// Whether `provider_id` is built and not turned off by an admin.
    pub fn is_enabled(&self, provider_id: &str) -> bool {
        self.is_configured(provider_id) && !self.is_switched_off(provider_id)
    }

    /// Whether an admin turned `provider_id` off (only a built provider can be).
    pub fn is_switched_off(&self, provider_id: &str) -> bool {
        self.off_set().contains(provider_id)
    }

    /// Turn a built provider on or off at runtime. `false` when the id was
    /// not built from the config (nothing to switch).
    pub fn set_enabled(&self, provider_id: &str, enabled: bool) -> bool {
        if !self.is_configured(provider_id) {
            return false;
        }
        let mut off = self
            .switched_off
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        if enabled {
            off.remove(provider_id);
        } else {
            off.insert(provider_id.to_owned());
        }
        true
    }

    /// Every config entry with its runtime state, sorted by id.
    pub fn statuses(&self) -> Vec<ProviderStatus> {
        let off = self.off_set();
        let last = self
            .last_search
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut out: Vec<ProviderStatus> = self
            .kinds
            .iter()
            .map(|(id, kind)| {
                let configured = self.providers.contains_key(id);
                ProviderStatus {
                    id: id.clone(),
                    kind: kind.clone(),
                    enabled: configured && !off.contains(id),
                    configured,
                    last_search: last.get(id).cloned(),
                }
            })
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    fn admit(&self, id: &str) -> Admit {
        let mut breakers = self.breakers.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(b) = breakers.get_mut(id) else {
            return Admit::Ask;
        };
        let Some(until) = b.open_until else {
            return Admit::Ask;
        };
        let now = tokio::time::Instant::now();
        if now < until {
            return Admit::Skip(until.saturating_duration_since(now));
        }
        b.open_until = Some(now + SEARCH_DEADLINE);
        Admit::Ask
    }

    fn settle(&self, id: &str, timed_out: bool) {
        let mut breakers = self.breakers.lock().unwrap_or_else(PoisonError::into_inner);
        if !timed_out {
            breakers.remove(id);
            return;
        }
        let b = breakers.entry(id.to_owned()).or_default();
        b.timeouts += 1;
        if b.timeouts >= BREAKER_TIMEOUTS {
            b.open_until = Some(tokio::time::Instant::now() + BREAKER_COOLDOWN);
            tracing::warn!(provider = %id, timeouts = b.timeouts, "provider keeps timing out; skipped for {} min", BREAKER_COOLDOWN.as_secs() / 60);
        }
    }

    fn record_outcome(&self, id: &str, took: std::time::Duration, error: Option<&Error>) {
        let outcome = SearchOutcome {
            at: Utc::now(),
            latency_ms: u64::try_from(took.as_millis()).unwrap_or(u64::MAX),
            error: error.map(ToString::to_string),
        };
        self.last_search
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id.to_owned(), outcome);
    }

    fn off_set(&self) -> std::sync::RwLockReadGuard<'_, HashSet<String>> {
        self.switched_off
            .read()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// The enabled providers, sorted by id so fan-outs are deterministic.
    fn enabled(&self) -> Vec<(String, Arc<dyn SearchProvider>)> {
        let off = self.off_set();
        let mut out: Vec<_> = self
            .providers
            .iter()
            .filter(|(id, _)| !off.contains(*id))
            .map(|(id, p)| (id.clone(), p.clone()))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Remember the TMDB ids the trackers shipped with `results`. Called
    /// on every aggregated search; feed readers (`latest`, featured) call
    /// it themselves.
    pub fn remember_tracker_ids(&self, results: &[crate::SearchResult]) {
        let mut map = self
            .tracker_tmdb
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for r in results {
            if let Some(id) = r.tmdb_id.filter(|id| *id > 0) {
                map.insert((r.provider_id.clone(), r.external_id.clone()), id);
            }
        }
    }

    /// The TMDB id the tracker shipped with this release, when a search or
    /// feed since boot listed it.
    pub fn tracker_tmdb_id(&self, provider_id: &str, external_id: &str) -> Option<u64> {
        self.tracker_tmdb
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .ids
            .get(&(provider_id.to_owned(), external_id.to_owned()))
            .copied()
    }

    /// The policy a provider declared, or the defaults for an unknown id.
    pub fn policy(&self, provider_id: &str) -> ProviderPolicy {
        self.policies.get(provider_id).cloned().unwrap_or_default()
    }

    /// Look up the default language string a provider entry declared
    /// in its config. `None` when the entry has no `default_language`
    /// field — most francophone trackers tag releases explicitly so
    /// the parser's `detect_language` covers them without a default.
    pub fn default_language(&self, provider_id: &str) -> Option<&str> {
        self.policies
            .get(provider_id)
            .and_then(|p| p.default_language.as_deref())
    }

    /// See [`ProviderPolicy::origin`].
    pub fn origin(&self, provider_id: &str) -> Option<&str> {
        self.policies
            .get(provider_id)
            .and_then(|p| p.origin.as_deref())
    }

    /// Ids the freshness scheduler may pull `latest()` from — see
    /// [`ProviderPolicy::catalog`].
    pub fn catalog_ids(&self) -> Vec<String> {
        self.enabled()
            .into_iter()
            .map(|(id, _)| id)
            .filter(|id| self.policy(id).catalog)
            .collect()
    }

    /// Whether torrents grabbed from `provider_id` should keep seeding once
    /// complete — see [`ProviderPolicy::seed`]. Unknown providers (a grab
    /// whose tracker was since removed from the config) keep seeding.
    pub fn seeds(&self, provider_id: &str) -> bool {
        self.policy(provider_id).seed
    }

    /// Concurrent-download cap the tracker enforces, if it declared one —
    /// see [`ProviderPolicy::leech_slots`].
    pub fn leech_slots(&self, provider_id: &str) -> Option<u32> {
        self.policy(provider_id).leech_slots
    }

    /// The enabled providers' ids, sorted.
    pub fn ids(&self) -> Vec<String> {
        self.enabled().into_iter().map(|(id, _)| id).collect()
    }

    pub fn info(&self) -> Vec<ProviderInfo> {
        self.enabled()
            .into_iter()
            .map(|(id, p)| ProviderInfo {
                capabilities: p.capabilities(),
                id,
            })
            .collect()
    }

    /// The provider, when it is enabled.
    pub fn get(&self, id: &str) -> Option<Arc<dyn SearchProvider>> {
        if self.is_switched_off(id) {
            return None;
        }
        self.providers.get(id).cloned()
    }

    /// Run the same query against every enabled provider in parallel and
    /// concatenate results, also returning per-provider pagination info so the
    /// UI can render proper page controls. Failed providers are reported as a
    /// metadata entry with an `error` field instead of taking the whole
    /// search down.
    ///
    /// Each provider gets [`SEARCH_DEADLINE`] to answer. Without it the
    /// aggregate blocks on the slowest client timeout (15–20 s): a sick
    /// indexer whose nginx sits on the request before 502-ing held every
    /// search hostage even when the healthy providers answered in
    /// milliseconds. Stragglers degrade to the same per-provider error
    /// entry as any other failure.
    pub async fn search_all(&self, q: &SearchQuery) -> AggregatedResults {
        self.search_where(q, |_| true).await
    }

    /// [`Self::search_all`] over the catalogue providers only (see
    /// [`ProviderPolicy::catalog`]): what the discovery schedulers keep, so
    /// a search-only tracker isn't queried for results they would drop.
    pub async fn search_catalog(&self, q: &SearchQuery) -> AggregatedResults {
        self.search_where(q, |id| self.policy(id).catalog).await
    }

    async fn search_where(
        &self,
        q: &SearchQuery,
        include: impl Fn(&str) -> bool,
    ) -> AggregatedResults {
        use futures::stream::{FuturesUnordered, StreamExt};

        let mut futs = FuturesUnordered::new();
        let mut skipped = Vec::new();
        for (id, p) in self.enabled().into_iter().filter(|(id, _)| include(id)) {
            if let Admit::Skip(left) = self.admit(&id) {
                skipped.push((id, left));
                continue;
            }
            let q = q.clone();
            let sem = self.search_permits.get(&id).cloned();
            futs.push(async move {
                // The concurrency permit is taken inside the deadline: a
                // search queued behind a burst expires without ever hitting
                // the tracker, and dropping the future (client disconnect)
                // releases the permit immediately.
                let started = std::time::Instant::now();
                let _permit = if let Some(sem) = &sem {
                    let Ok(permit) = tokio::time::timeout(SEARCH_DEADLINE, sem.acquire()).await
                    else {
                        let err = Error::Provider(format!(
                            "skipped: queued behind concurrent searches for {}s",
                            SEARCH_DEADLINE.as_secs()
                        ));
                        return (id, started.elapsed(), Err(err), None);
                    };
                    permit.ok()
                } else {
                    None
                };
                let remaining = SEARCH_DEADLINE.saturating_sub(started.elapsed());
                let (res, timed_out) = match tokio::time::timeout(remaining, p.search(&q)).await {
                    Ok(res) => (res, Some(false)),
                    Err(_) => (
                        Err(Error::Provider(format!(
                            "timed out after {}s",
                            SEARCH_DEADLINE.as_secs()
                        ))),
                        Some(true),
                    ),
                };
                (id, started.elapsed(), res, timed_out)
            });
        }

        let mut agg = AggregatedResults::default();
        let limit = q.limit.unwrap_or(25);
        let page = q.page.unwrap_or(1);
        for (id, left) in skipped {
            agg.providers.push(ProviderResultMeta {
                id,
                current_page: page,
                limit,
                total_count: None,
                total_pages: None,
                error: Some(format!(
                    "skipped: timed out {BREAKER_TIMEOUTS} times in a row, asked again in {} min",
                    left.as_secs().div_ceil(60).max(1)
                )),
            });
        }
        while let Some((id, took, res, timed_out)) = futs.next().await {
            if let Some(timed_out) = timed_out {
                self.settle(&id, timed_out);
            }
            self.record_outcome(&id, took, res.as_ref().err());
            match res {
                Ok(p) => {
                    agg.providers.push(ProviderResultMeta {
                        id: id.clone(),
                        current_page: p.current_page,
                        limit: p.limit,
                        total_count: p.total_count,
                        total_pages: p.total_pages,
                        error: None,
                    });
                    self.remember_tracker_ids(&p.results);
                    agg.results.extend(p.results);
                }
                Err(e) => {
                    tracing::warn!(provider = %id, error = %e, "provider search failed");
                    agg.providers.push(ProviderResultMeta {
                        id,
                        current_page: page,
                        limit,
                        total_count: None,
                        total_pages: None,
                        error: Some(e.to_string()),
                    });
                }
            }
        }
        agg
    }
}

fn policy_of(entry: &ProviderEntry) -> ProviderPolicy {
    let default_language = entry
        .fields
        .get("default_language")
        .and_then(|v| v.as_str())
        .map(str::to_ascii_lowercase);
    let origin = entry
        .fields
        .get("origin")
        .and_then(|v| v.as_str())
        .map(str::to_ascii_lowercase)
        .or_else(|| match default_language.as_deref() {
            Some("english") => Some("en".to_string()),
            Some("french") => Some("fr".to_string()),
            _ => None,
        });
    let flag = |key: &str, default: bool| {
        entry
            .fields
            .get(key)
            .and_then(toml::Value::as_bool)
            .unwrap_or(default)
    };
    ProviderPolicy {
        default_language,
        origin,
        catalog: flag("catalog", true),
        seed: flag("seed", true),
        leech_slots: entry
            .fields
            .get("leech_slots")
            .and_then(toml::Value::as_integer)
            .and_then(|n| u32::try_from(n).ok()),
    }
}

/// Factory: dispatches on `entry.kind` to construct a concrete provider.
/// New tracker types plug in here.
pub fn build_provider(entry: &ProviderEntry) -> Result<Arc<dyn SearchProvider>> {
    match entry.kind.as_str() {
        "torznab" => Ok(crate::torznab::TorznabProvider::from_config(entry)?),
        "tr4ker" => Ok(crate::tr4ker::Tr4ker::from_config(entry)?),
        "unit3d" => Ok(crate::unit3d::Unit3dProvider::from_config(entry)?),
        "c411" => Ok(crate::c411::C411::from_config(entry)?),
        "hdtorrents" => Ok(crate::hdtorrents::HdTorrents::from_config(entry)?),
        "nyaa" => Ok(crate::nyaa::NyaaProvider::from_config(entry)?),
        "torrentleech" => Ok(crate::torrentleech::TorrentLeech::from_config(entry)?),
        "v3x" => Ok(crate::v3x::V3x::from_config(entry)?),
        other => Err(Error::Provider(format!(
            "unknown provider kind: {other} (provider id: {})",
            entry.id
        ))),
    }
}

#[cfg(test)]
mod policy_tests {
    use super::policy_of;
    use iris_config::ProviderEntry;

    fn entry(toml_body: &str) -> ProviderEntry {
        toml::from_str(toml_body).expect("valid provider entry")
    }

    /// A `providers.toml` still listing a removed tracker (torr9) must boot:
    /// the unknown kind is logged and skipped, the rest still load.
    #[test]
    fn unknown_kind_is_skipped_not_fatal() {
        let entries = [
            entry("id = \"torr9\"\nkind = \"torr9\"\n"),
            entry("id = \"nyaa\"\nkind = \"nyaa\"\n"),
        ];
        let registry = super::ProviderRegistry::from_entries(&entries).expect("registry");
        assert_eq!(registry.ids(), vec!["nyaa".to_string()]);
    }

    #[tokio::test]
    async fn catalog_search_skips_search_only_providers() {
        let dead = "base_url = \"http://127.0.0.1:1\"\n";
        let entries = [
            entry(&format!("id = \"shelf\"\nkind = \"nyaa\"\n{dead}")),
            entry(&format!(
                "id = \"search_only\"\nkind = \"nyaa\"\ncatalog = false\n{dead}"
            )),
        ];
        let registry = super::ProviderRegistry::from_entries(&entries).expect("registry");
        let q = iris_core::search::SearchQuery {
            q: "x".into(),
            ..Default::default()
        };
        let asked = |agg: super::AggregatedResults| {
            let mut ids: Vec<String> = agg.providers.into_iter().map(|m| m.id).collect();
            ids.sort();
            ids
        };
        assert_eq!(asked(registry.search_catalog(&q).await), ["shelf"]);
        assert_eq!(
            asked(registry.search_all(&q).await),
            ["search_only", "shelf"]
        );
    }

    #[tokio::test]
    async fn a_switched_off_provider_is_never_asked() {
        let dead = "base_url = \"http://127.0.0.1:1\"\n";
        let entries = [
            entry(&format!("id = \"on\"\nkind = \"nyaa\"\n{dead}")),
            entry(&format!("id = \"off\"\nkind = \"nyaa\"\n{dead}")),
            entry("id = \"unbuilt\"\nkind = \"nyaa\"\nenabled = false\n"),
        ];
        let registry = super::ProviderRegistry::from_entries(&entries).expect("registry");
        assert!(registry.set_enabled("off", false));
        assert!(
            !registry.set_enabled("absent", false),
            "only built providers switch"
        );
        assert!(
            !registry.set_enabled("unbuilt", true),
            "the config has the last word"
        );

        let q = iris_core::search::SearchQuery {
            q: "x".into(),
            ..Default::default()
        };
        let asked = |agg: super::AggregatedResults| -> Vec<String> {
            agg.providers.into_iter().map(|m| m.id).collect()
        };
        assert_eq!(asked(registry.search_all(&q).await), ["on"]);
        assert_eq!(asked(registry.search_catalog(&q).await), ["on"]);
        assert_eq!(registry.ids(), ["on"]);
        assert_eq!(registry.catalog_ids(), ["on"]);
        assert_eq!(registry.info().len(), 1);
        assert!(registry.get("off").is_none());
        assert!(registry.is_configured("off") && !registry.is_enabled("off"));
        assert!(registry.seeds("off"), "seeding policy outlives the switch");

        let statuses = registry.statuses();
        let off = statuses.iter().find(|s| s.id == "off").expect("listed");
        assert!(!off.enabled && off.configured && off.last_search.is_none());
        let unbuilt = statuses.iter().find(|s| s.id == "unbuilt").expect("listed");
        assert!(!unbuilt.enabled && !unbuilt.configured);
        let on = statuses.iter().find(|s| s.id == "on").expect("listed");
        assert!(on.enabled && on.configured && on.kind == "nyaa");
        assert!(on.last_search.as_ref().is_some_and(|o| o.error.is_some()));

        assert!(registry.set_enabled("off", true));
        assert!(registry.get("off").is_some());
        assert_eq!(registry.ids(), ["off", "on"]);
    }

    /// Hangs until told to answer.
    struct Flaky {
        http: reqwest::Client,
        answers: std::sync::Arc<std::sync::atomic::AtomicBool>,
        asked: std::sync::Arc<std::sync::atomic::AtomicU32>,
    }

    #[async_trait::async_trait]
    impl crate::SearchProvider for Flaky {
        fn id(&self) -> &'static str {
            "flaky"
        }
        fn capabilities(&self) -> iris_core::search::ProviderCapabilities {
            iris_core::search::ProviderCapabilities::default()
        }
        fn http(&self) -> &reqwest::Client {
            &self.http
        }
        async fn search(
            &self,
            _q: &iris_core::search::SearchQuery,
        ) -> iris_core::Result<iris_core::search::ProviderPage> {
            use std::sync::atomic::Ordering;
            self.asked.fetch_add(1, Ordering::SeqCst);
            if !self.answers.load(Ordering::SeqCst) {
                std::future::pending::<()>().await;
            }
            Ok(iris_core::search::ProviderPage {
                results: Vec::new(),
                current_page: 1,
                limit: 25,
                total_count: None,
                total_pages: None,
            })
        }
        async fn resolve(&self, _id: &str) -> iris_core::Result<iris_core::search::TorrentSource> {
            Err(iris_core::Error::Provider("unused".into()))
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_provider_that_keeps_timing_out_is_skipped_then_probed() {
        use std::sync::atomic::Ordering;
        let answers = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let asked = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let flaky: std::sync::Arc<dyn crate::SearchProvider> = std::sync::Arc::new(Flaky {
            http: reqwest::Client::new(),
            answers: answers.clone(),
            asked: asked.clone(),
        });
        let registry = super::ProviderRegistry {
            providers: std::sync::Arc::new([("flaky".to_string(), flaky)].into()),
            ..Default::default()
        };
        let q = iris_core::search::SearchQuery {
            q: "x".into(),
            ..Default::default()
        };
        let error = |agg: super::AggregatedResults| agg.providers[0].error.clone();

        for _ in 0..super::BREAKER_TIMEOUTS {
            assert!(error(registry.search_all(&q).await).is_some_and(|e| e.contains("timed out")));
        }
        let skipped = error(registry.search_all(&q).await).expect("an error entry");
        assert!(
            skipped.starts_with("skipped: timed out 3 times"),
            "{skipped}"
        );
        assert_eq!(
            asked.load(Ordering::SeqCst),
            3,
            "an open breaker never asks"
        );

        tokio::time::advance(super::BREAKER_COOLDOWN).await;
        assert!(error(registry.search_all(&q).await).is_some_and(|e| e.contains("timed out")));
        assert_eq!(
            asked.load(Ordering::SeqCst),
            4,
            "one probe after the cooldown"
        );
        assert!(error(registry.search_all(&q).await).is_some_and(|e| e.starts_with("skipped")));

        tokio::time::advance(super::BREAKER_COOLDOWN).await;
        answers.store(true, Ordering::SeqCst);
        assert_eq!(error(registry.search_all(&q).await), None);
        assert_eq!(error(registry.search_all(&q).await), None, "closed again");
        assert_eq!(asked.load(Ordering::SeqCst), 6);
    }

    #[test]
    fn leech_slots_is_opt_in() {
        let plain = entry("id = \"tos\"\nkind = \"unit3d\"\n");
        assert_eq!(policy_of(&plain).leech_slots, None);

        let capped = entry("id = \"seedpool\"\nkind = \"unit3d\"\nleech_slots = 1\n");
        assert_eq!(policy_of(&capped).leech_slots, Some(1));

        // A nonsense value must not silently become a cap of 0, which would
        // lock every grab out of the tracker.
        let negative = entry("id = \"x\"\nkind = \"unit3d\"\nleech_slots = -3\n");
        assert_eq!(policy_of(&negative).leech_slots, None);
    }
}
