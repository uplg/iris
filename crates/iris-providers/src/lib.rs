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

use iris_core::Error;

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

    /// Fetch the bytes of a pre-signed `.torrent` URL the provider
    /// previously surfaced in a `SearchResult.download_url`. Used by
    /// the grab path as a restart-safe alternative to `resolve()` —
    /// `resolve()` relies on each provider's in-memory link cache,
    /// which evaporates on restart, while a URL persisted on
    /// `available_episodes.download_url` survives forever.
    ///
    /// Default implementation: plain GET via a fresh `reqwest`
    /// client. Sufficient for Torznab + UNIT3D layouts where the
    /// URL is signed with an `api_token=` query parameter. Providers
    /// with cookie / header auth needs override this.
    async fn fetch_bytes(&self, url: &str) -> Result<bytes::Bytes> {
        let resp = reqwest::get(url)
            .await
            .map_err(|e| Error::Provider(format!("fetch_bytes get: {e}")))?
            .error_for_status()
            .map_err(|e| Error::Provider(format!("fetch_bytes status: {e}")))?;
        resp.bytes()
            .await
            .map_err(|e| Error::Provider(format!("fetch_bytes body: {e}")))
    }
}

pub use registry::ProviderRegistry;
