//! Tracker search provider abstraction + registry.
//!
//! Adding a new tracker:
//! 1. create a module under `crates/iris-providers/src/<name>.rs`
//! 2. implement [`SearchProvider`]
//! 3. register the kind in [`registry::build_provider`]
//! 4. add an entry in `providers.toml`

pub mod registry;
pub mod torr9;

mod util;

use async_trait::async_trait;
use iris_core::Result;
use iris_core::search::{ProviderCapabilities, ProviderPage, SearchQuery, TorrentSource};

#[async_trait]
pub trait SearchProvider: Send + Sync {
    fn id(&self) -> &str;
    fn capabilities(&self) -> ProviderCapabilities;
    async fn search(&self, q: &SearchQuery) -> Result<ProviderPage>;
    async fn resolve(&self, external_id: &str) -> Result<TorrentSource>;
}

pub use registry::ProviderRegistry;
