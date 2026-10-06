//! A bounded, expiring, single-flight async cache.
//!
//! Every in-memory cache of upstream data (TMDB today) goes through
//! [`TtlCache`]: entries expire after a TTL, the map never holds more than
//! its capacity, and concurrent misses on one key share a single fetch.
//! A fetch that fails (returns `None`) is never cached, so one upstream blip
//! doesn't hide data until the next restart.

use std::collections::HashMap;
use std::future::Future;
use std::hash::Hash;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use tokio::sync::OnceCell;

pub struct TtlCache<K, V> {
    ttl: Duration,
    capacity: usize,
    slots: Mutex<HashMap<K, Slot<V>>>,
}

struct Slot<V> {
    created: Instant,
    cell: Arc<OnceCell<V>>,
}

impl<K, V> TtlCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    pub fn new(ttl: Duration, capacity: usize) -> Self {
        Self {
            ttl,
            capacity: capacity.max(1),
            slots: Mutex::new(HashMap::new()),
        }
    }

    /// The cached value for `key`, fetching it on a miss or once expired.
    /// `None` from `fetch` means "failed, don't remember".
    pub async fn get_or_fetch<F, Fut>(&self, key: K, fetch: F) -> Option<V>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Option<V>>,
    {
        let cell = self.cell_for(key);
        cell.get_or_try_init(|| async { fetch().await.ok_or(()) })
            .await
            .ok()
            .cloned()
    }

    fn cell_for(&self, key: K) -> Arc<OnceCell<V>> {
        let now = Instant::now();
        let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(slot) = slots.get(&key) {
            // An empty cell is a fetch in flight (or one that failed): reuse
            // it so concurrent callers wait on the same request.
            let fresh = now.duration_since(slot.created) < self.ttl;
            if fresh || !slot.cell.initialized() {
                return Arc::clone(&slot.cell);
            }
        }
        if slots.len() >= self.capacity {
            slots.retain(|_, s| now.duration_since(s.created) < self.ttl);
        }
        if slots.len() >= self.capacity
            && let Some(oldest) = slots
                .iter()
                .min_by_key(|(_, s)| s.created)
                .map(|(k, _)| k.clone())
        {
            slots.remove(&oldest);
        }
        let cell = Arc::new(OnceCell::new());
        slots.insert(
            key,
            Slot {
                created: now,
                cell: Arc::clone(&cell),
            },
        );
        cell
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.slots
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::TtlCache;

    #[tokio::test]
    async fn caches_a_successful_fetch() {
        let cache = TtlCache::new(Duration::from_secs(60), 8);
        let calls = AtomicUsize::new(0);
        for _ in 0..3 {
            let v = cache
                .get_or_fetch("k", || async {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Some(7)
                })
                .await;
            assert_eq!(v, Some(7));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn never_caches_a_failure() {
        let cache: TtlCache<&str, u32> = TtlCache::new(Duration::from_secs(60), 8);
        assert_eq!(cache.get_or_fetch("k", || async { None }).await, None);
        assert_eq!(cache.get_or_fetch("k", || async { Some(3) }).await, Some(3));
    }

    #[tokio::test]
    async fn refetches_once_expired() {
        let cache = TtlCache::new(Duration::from_millis(20), 8);
        assert_eq!(cache.get_or_fetch("k", || async { Some(1) }).await, Some(1));
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert_eq!(cache.get_or_fetch("k", || async { Some(2) }).await, Some(2));
    }

    #[tokio::test]
    async fn concurrent_misses_share_one_fetch() {
        let cache = TtlCache::new(Duration::from_secs(60), 8);
        let calls = AtomicUsize::new(0);
        let fetch = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(20)).await;
            Some(5)
        };
        let (a, b) = tokio::join!(
            cache.get_or_fetch("k", fetch),
            cache.get_or_fetch("k", fetch)
        );
        assert_eq!((a, b), (Some(5), Some(5)));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn stays_within_capacity() {
        let cache = TtlCache::new(Duration::from_secs(60), 3);
        for i in 0..10_u32 {
            cache.get_or_fetch(i, || async move { Some(i) }).await;
        }
        assert_eq!(cache.len(), 3);
        // The newest entries survive.
        assert_eq!(cache.get_or_fetch(9, || async { None }).await, Some(9));
    }
}
