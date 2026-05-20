use std::collections::HashMap;
use std::sync::Arc;

use iris_config::ProviderEntry;
use iris_core::Error;
use iris_core::Result;
use iris_core::search::{ProviderCapabilities, SearchQuery, SearchResult};
use serde::Serialize;

use crate::SearchProvider;

#[derive(Debug, Clone, Serialize)]
pub struct ProviderInfo {
    pub id: String,
    pub capabilities: ProviderCapabilities,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderResultMeta {
    pub id: String,
    pub current_page: u32,
    pub limit: u32,
    pub total_count: Option<u64>,
    pub total_pages: Option<u32>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
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
#[derive(Debug, Clone, Serialize)]
pub struct ParsedQueryInfo {
    pub title: String,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub year: Option<u16>,
}

/// Holds all enabled providers and fans out searches in parallel.
#[derive(Clone, Default)]
pub struct ProviderRegistry {
    providers: Arc<HashMap<String, Arc<dyn SearchProvider>>>,
    /// Provider-id → default language string (`"english"` / `"french"`
    /// / `"multi"`). Read from the optional `default_language` field
    /// on each `[[providers]]` entry in `providers.toml`. Used by
    /// the API layer to disambiguate releases that ship with no
    /// explicit language marker — Seedpool ships English by
    /// convention without ever tagging the file, and treating that
    /// as `Unknown` (= "no badge") would leave anglophone users
    /// without a visual cue.
    default_languages: Arc<HashMap<String, String>>,
}

impl ProviderRegistry {
    pub fn from_entries(entries: &[ProviderEntry]) -> Result<Self> {
        let mut map: HashMap<String, Arc<dyn SearchProvider>> = HashMap::new();
        let mut defaults: HashMap<String, String> = HashMap::new();
        for entry in entries.iter().filter(|e| e.enabled) {
            if let Some(toml::Value::String(s)) = entry.fields.get("default_language") {
                defaults.insert(entry.id.clone(), s.to_ascii_lowercase());
            }
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
        Ok(Self {
            providers: Arc::new(map),
            default_languages: Arc::new(defaults),
        })
    }

    /// Look up the default language string a provider entry declared
    /// in its config. `None` when the entry has no `default_language`
    /// field — most francophone trackers tag releases explicitly so
    /// the parser's `detect_language` covers them without a default.
    pub fn default_language(&self, provider_id: &str) -> Option<&str> {
        self.default_languages.get(provider_id).map(String::as_str)
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
    pub async fn search_all(&self, q: &SearchQuery) -> AggregatedResults {
        use futures::stream::{FuturesUnordered, StreamExt};

        let mut futs = FuturesUnordered::new();
        for (id, p) in self.providers.iter() {
            let p = p.clone();
            let id = id.clone();
            let q = q.clone();
            futs.push(async move {
                let res = p.search(&q).await;
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

/// Factory: dispatches on `entry.kind` to construct a concrete provider.
/// New tracker types plug in here.
pub fn build_provider(entry: &ProviderEntry) -> Result<Arc<dyn SearchProvider>> {
    match entry.kind.as_str() {
        "torr9" => Ok(crate::torr9::Torr9::from_config(entry)?),
        "torznab" => Ok(crate::torznab::TorznabProvider::from_config(entry)?),
        "unit3d" => Ok(crate::unit3d::Unit3dProvider::from_config(entry)?),
        "c411" => Ok(crate::c411::C411::from_config(entry)?),
        other => Err(Error::Provider(format!(
            "unknown provider kind: {other} (provider id: {})",
            entry.id
        ))),
    }
}
