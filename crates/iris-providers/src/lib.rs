//! Tracker search provider abstraction + registry.
//!
//! Adding a new tracker:
//! 1. create a module under `crates/iris-providers/src/<name>.rs`
//! 2. implement [`SearchProvider`]
//! 3. register the kind in [`registry::build_provider`]
//! 4. add an entry in `providers.toml`
//!
//! When the new tracker exposes a Torznab API, prefer composing
//! [`torznab::TorznabProvider`] from the new module instead of
//! reimplementing the wire format (see [`c411`] for an example).

pub mod c411;
pub mod registry;
pub mod torr9;
pub mod torznab;
pub mod unit3d;

mod util;

use async_trait::async_trait;
use iris_core::Result;
use iris_core::search::{
    ProviderCapabilities, ProviderPage, SearchQuery, SearchResult, TorrentDetails, TorrentSource,
};

pub mod nfo;

#[async_trait]
pub trait SearchProvider: Send + Sync {
    fn id(&self) -> &str;
    fn capabilities(&self) -> ProviderCapabilities;
    async fn search(&self, q: &SearchQuery) -> Result<ProviderPage>;
    async fn resolve(&self, external_id: &str) -> Result<TorrentSource>;

    /// Curated/featured carousel for the discovery shelves. Default is
    /// empty — providers without a "featured" concept simply contribute
    /// nothing. Implementations are encouraged to cache (the discovery
    /// home will hit this on every page load).
    async fn featured_movies(&self) -> Result<Vec<SearchResult>> {
        Ok(Vec::new())
    }
    async fn featured_series(&self) -> Result<Vec<SearchResult>> {
        Ok(Vec::new())
    }

    /// Rich detail view for one torrent (description / NFO / parsed
    /// `MediaInfo` / uploader / etc.) — what powers the search-result
    /// preview dialog. Default `None` means "this provider doesn't expose
    /// a details endpoint"; the UI falls back to the basic `SearchResult`.
    async fn details(&self, _external_id: &str) -> Result<Option<TorrentDetails>> {
        Ok(None)
    }
}

pub use registry::ProviderRegistry;
