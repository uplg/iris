use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use iris_core::search::TorrentDetails;
use tokio::sync::Mutex;

/// FIFO cap of every per-provider `external_id`-keyed cache.
pub(crate) const LINK_CACHE_CAP: usize = 4096;

/// How long a release's detail snapshot answers repeated `details()` calls:
/// the user shopping the preview dialog bounces between releases (and a grab
/// re-reads the one it picked); a minute spares the tracker without letting
/// seeder counts go meaningfully stale.
pub(crate) const DETAILS_TTL: Duration = Duration::from_mins(1);

/// FIFO cache keyed by `external_id`, capped at [`LINK_CACHE_CAP`]: lets
/// `resolve()` find the signed download link captured by a previous search
/// (and torznab answer `details()` from the feed item it came with).
pub(crate) struct FifoCache<V> {
    map: HashMap<String, V>,
    order: VecDeque<String>,
}

impl<V: Clone> FifoCache<V> {
    pub(crate) fn new() -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub(crate) fn put(&mut self, key: String, value: V) {
        if self.map.insert(key.clone(), value).is_none() {
            self.order.push_back(key);
            while self.order.len() > LINK_CACHE_CAP {
                if let Some(old) = self.order.pop_front() {
                    self.map.remove(&old);
                }
            }
        }
    }

    pub(crate) fn get(&self, key: &str) -> Option<V> {
        self.map.get(key).cloned()
    }
}

/// Detail snapshots fetched from a tracker's detail endpoint, served for
/// [`DETAILS_TTL`].
pub(crate) struct DetailsCache {
    map: Mutex<HashMap<String, (TorrentDetails, Instant)>>,
}

impl DetailsCache {
    pub(crate) fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) async fn get(&self, key: &str) -> Option<TorrentDetails> {
        self.map
            .lock()
            .await
            .get(key)
            .filter(|(_, at)| at.elapsed() < DETAILS_TTL)
            .map(|(d, _)| d.clone())
    }

    /// Stale entries are swept here: `get` only filters them, and every
    /// previewed release would otherwise stay for the life of the process.
    pub(crate) async fn put(&self, key: String, details: TorrentDetails) {
        let mut map = self.map.lock().await;
        map.retain(|_, (_, at)| at.elapsed() < DETAILS_TTL);
        map.insert(key, (details, Instant::now()));
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use iris_core::search::TorrentDetails;

    use super::{DETAILS_TTL, DetailsCache, FifoCache, LINK_CACHE_CAP};

    fn details() -> TorrentDetails {
        serde_json::from_value(serde_json::json!({
            "provider_id": "p", "external_id": "1", "title": "t",
        }))
        .expect("minimal details")
    }

    #[tokio::test]
    async fn details_cache_sweeps_stale_entries_on_put() {
        let cache = DetailsCache::new();
        let stale = Instant::now()
            .checked_sub(DETAILS_TTL * 2)
            .expect("monotonic clock");
        cache
            .map
            .lock()
            .await
            .insert("old".into(), (details(), stale));
        cache.put("new".into(), details()).await;
        let map = cache.map.lock().await;
        assert!(!map.contains_key("old"));
        assert!(map.contains_key("new"));
    }

    #[test]
    fn fifo_cache_evicts_oldest_first() {
        let mut c = FifoCache::new();
        for i in 0..(LINK_CACHE_CAP + 10) {
            c.put(format!("k{i}"), format!("v{i}"));
        }
        assert!(c.get("k0").is_none());
        assert!(c.get(&format!("k{}", LINK_CACHE_CAP + 9)).is_some());
        assert_eq!(c.map.len(), LINK_CACHE_CAP);
    }
}
