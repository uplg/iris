use std::collections::HashMap;
use std::sync::Arc;

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
}

impl Default for ProviderPolicy {
    fn default() -> Self {
        Self {
            default_language: None,
            catalog: true,
            seed: true,
        }
    }
}

/// Holds all enabled providers and fans out searches in parallel.
#[derive(Clone, Default)]
pub struct ProviderRegistry {
    providers: Arc<HashMap<String, Arc<dyn SearchProvider>>>,
    /// Provider-id → its declared [`ProviderPolicy`].
    policies: Arc<HashMap<String, ProviderPolicy>>,
    /// Provider-id → semaphore bounding concurrent `search` calls
    /// ([`MAX_INFLIGHT_PER_PROVIDER`]).
    search_permits: Arc<HashMap<String, Arc<tokio::sync::Semaphore>>>,
}

impl ProviderRegistry {
    pub fn from_entries(entries: &[ProviderEntry]) -> Result<Self> {
        let mut map: HashMap<String, Arc<dyn SearchProvider>> = HashMap::new();
        let mut policies: HashMap<String, ProviderPolicy> = HashMap::new();
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
            policies: Arc::new(policies),
            search_permits: Arc::new(permits),
        })
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

    /// Ids the freshness scheduler may pull `latest()` from — see
    /// [`ProviderPolicy::catalog`].
    pub fn catalog_ids(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .providers
            .keys()
            .filter(|id| self.policy(id).catalog)
            .cloned()
            .collect();
        out.sort();
        out
    }

    /// Whether torrents grabbed from `provider_id` should keep seeding once
    /// complete — see [`ProviderPolicy::seed`]. Unknown providers (a grab
    /// whose tracker was since removed from the config) keep seeding.
    pub fn seeds(&self, provider_id: &str) -> bool {
        self.policy(provider_id).seed
    }

    pub fn ids(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }

    pub fn info(&self) -> Vec<ProviderInfo> {
        let mut out: Vec<_> = self
            .providers
            .iter()
            .map(|(id, p)| ProviderInfo {
                id: id.clone(),
                capabilities: p.capabilities(),
            })
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn SearchProvider>> {
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
        use futures::stream::{FuturesUnordered, StreamExt};

        let mut futs = FuturesUnordered::new();
        for (id, p) in self.providers.iter() {
            let p = p.clone();
            let id = id.clone();
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
                        return (id, Err(err));
                    };
                    permit.ok()
                } else {
                    None
                };
                let remaining = SEARCH_DEADLINE.saturating_sub(started.elapsed());
                let res = match tokio::time::timeout(remaining, p.search(&q)).await {
                    Ok(res) => res,
                    Err(_) => Err(Error::Provider(format!(
                        "timed out after {}s",
                        SEARCH_DEADLINE.as_secs()
                    ))),
                };
                (id, res)
            });
        }

        let mut agg = AggregatedResults::default();
        let limit = q.limit.unwrap_or(25);
        let page = q.page.unwrap_or(1);
        while let Some((id, res)) = futs.next().await {
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
    let flag = |key: &str, default: bool| {
        entry
            .fields
            .get(key)
            .and_then(toml::Value::as_bool)
            .unwrap_or(default)
    };
    ProviderPolicy {
        default_language: entry
            .fields
            .get("default_language")
            .and_then(|v| v.as_str())
            .map(str::to_ascii_lowercase),
        catalog: flag("catalog", true),
        seed: flag("seed", true),
    }
}

/// Factory: dispatches on `entry.kind` to construct a concrete provider.
/// New tracker types plug in here.
pub fn build_provider(entry: &ProviderEntry) -> Result<Arc<dyn SearchProvider>> {
    match entry.kind.as_str() {
        "torr9" => Ok(crate::torr9::Torr9::from_config(entry)?),
        "torznab" => Ok(crate::torznab::TorznabProvider::from_config(entry)?),
        "tr4ker" => Ok(crate::tr4ker::Tr4ker::from_config(entry)?),
        "unit3d" => Ok(crate::unit3d::Unit3dProvider::from_config(entry)?),
        "c411" => Ok(crate::c411::C411::from_config(entry)?),
        "hdtorrents" => Ok(crate::hdtorrents::HdTorrents::from_config(entry)?),
        "nyaa" => Ok(crate::nyaa::NyaaProvider::from_config(entry)?),
        "torrentleech" => Ok(crate::torrentleech::TorrentLeech::from_config(entry)?),
        other => Err(Error::Provider(format!(
            "unknown provider kind: {other} (provider id: {})",
            entry.id
        ))),
    }
}
