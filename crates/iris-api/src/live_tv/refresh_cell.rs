//! A cached upstream document with single-flight refresh, stale serving and a
//! failure stamp — the shape every best-effort live TV loader (streams DB,
//! search index, name→logo index, EPG guides) shares.
//!
//! Without the failure stamp, an upstream outage made every caller re-download
//! (tens of MB for a guide or `streams.json`) because only successes were
//! cached; without single-flight, N callers past the TTL rebuilt N times.

use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

pub struct RefreshCell<T> {
    slot: RwLock<Slot<T>>,
    refresh: tokio::sync::Mutex<()>,
}

struct Slot<T> {
    value: Option<Arc<T>>,
    loaded_at: Option<Instant>,
    failed_at: Option<Instant>,
}

impl<T> Default for RefreshCell<T> {
    fn default() -> Self {
        Self {
            slot: RwLock::new(Slot {
                value: None,
                loaded_at: None,
                failed_at: None,
            }),
            refresh: tokio::sync::Mutex::new(()),
        }
    }
}

impl<T> RefreshCell<T> {
    /// The last successfully loaded value, however old.
    pub fn peek(&self) -> Option<Arc<T>> {
        self.slot.read().expect("poisoned").value.clone()
    }

    /// No load is warranted while the value is within `ttl`, or while the last
    /// load failed less than `retry` ago.
    fn settled(&self, ttl: Duration, retry: Duration) -> bool {
        let slot = self.slot.read().expect("poisoned");
        let fresh = slot.value.is_some() && slot.loaded_at.is_some_and(|at| at.elapsed() < ttl);
        fresh || slot.failed_at.is_some_and(|at| at.elapsed() < retry)
    }

    /// The cached value, reloading through `load` when it is older than `ttl`
    /// (`Duration::MAX` = load only when absent). One load at a time: callers
    /// arriving mid-reload get the stale value if there is one, else wait for
    /// the reload. A failed load (`None`) keeps the stale value and is not
    /// retried for `retry`.
    pub async fn get<F, Fut>(&self, ttl: Duration, retry: Duration, load: F) -> Option<Arc<T>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Option<T>>,
    {
        if self.settled(ttl, retry) {
            return self.peek();
        }
        let _guard = if let Ok(guard) = self.refresh.try_lock() {
            guard
        } else {
            if let Some(stale) = self.peek() {
                return Some(stale);
            }
            self.refresh.lock().await
        };
        if self.settled(ttl, retry) {
            return self.peek();
        }
        let loaded = load().await.map(Arc::new);
        let mut slot = self.slot.write().expect("poisoned");
        let now = Instant::now();
        if let Some(value) = loaded {
            slot.value = Some(value.clone());
            slot.loaded_at = Some(now);
            slot.failed_at = None;
            Some(value)
        } else {
            slot.failed_at = Some(now);
            slot.value.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const HOUR: Duration = Duration::from_hours(1);

    #[tokio::test]
    async fn failure_is_negative_cached_and_keeps_the_stale_value() {
        let cell = RefreshCell::<u32>::default();
        let calls = AtomicUsize::new(0);
        let fail = || async {
            calls.fetch_add(1, Ordering::Relaxed);
            None
        };
        assert_eq!(cell.get(HOUR, HOUR, fail).await, None);
        assert_eq!(cell.get(HOUR, HOUR, fail).await, None);
        assert_eq!(calls.load(Ordering::Relaxed), 1, "failure must not refetch");

        let cell = RefreshCell::<u32>::default();
        assert_eq!(
            cell.get(HOUR, HOUR, || async { Some(7) }).await.as_deref(),
            Some(&7)
        );
        // Past the TTL (zero) the reload fails: the stale value is served.
        assert_eq!(
            cell.get(Duration::ZERO, HOUR, fail).await.as_deref(),
            Some(&7)
        );
        assert_eq!(
            cell.get(Duration::ZERO, HOUR, fail).await.as_deref(),
            Some(&7)
        );
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn fresh_value_skips_the_loader_and_expired_failure_retries() {
        let cell = RefreshCell::<u32>::default();
        assert_eq!(
            cell.get(HOUR, Duration::ZERO, || async { None }).await,
            None
        );
        // retry window of zero: the next call loads again.
        assert_eq!(
            cell.get(HOUR, Duration::ZERO, || async { Some(1) })
                .await
                .as_deref(),
            Some(&1)
        );
        assert_eq!(
            cell.get(HOUR, Duration::ZERO, || async { Some(2) })
                .await
                .as_deref(),
            Some(&1)
        );
        assert_eq!(
            cell.get(Duration::MAX, Duration::ZERO, || async { Some(3) })
                .await
                .as_deref(),
            Some(&1)
        );
    }

    #[tokio::test]
    async fn concurrent_cold_callers_load_once() {
        let cell = Arc::new(RefreshCell::<u32>::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new(tokio::sync::Notify::new());
        let first = {
            let (cell, calls, gate) = (cell.clone(), calls.clone(), gate.clone());
            tokio::spawn(async move {
                cell.get(HOUR, HOUR, || async {
                    calls.fetch_add(1, Ordering::Relaxed);
                    gate.notified().await;
                    Some(5)
                })
                .await
            })
        };
        while calls.load(Ordering::Relaxed) == 0 {
            tokio::task::yield_now().await;
        }
        let second = {
            let (cell, calls) = (cell.clone(), calls.clone());
            tokio::spawn(async move {
                cell.get(HOUR, HOUR, || async {
                    calls.fetch_add(1, Ordering::Relaxed);
                    Some(6)
                })
                .await
            })
        };
        tokio::task::yield_now().await;
        gate.notify_one();
        assert_eq!(first.await.unwrap().as_deref(), Some(&5));
        assert_eq!(second.await.unwrap().as_deref(), Some(&5));
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn stale_value_is_served_while_a_reload_runs() {
        let cell = Arc::new(RefreshCell::<u32>::default());
        cell.get(HOUR, HOUR, || async { Some(1) }).await;
        let started = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new(tokio::sync::Notify::new());
        let reload = {
            let (cell, started, gate) = (cell.clone(), started.clone(), gate.clone());
            tokio::spawn(async move {
                cell.get(Duration::ZERO, HOUR, || async {
                    started.store(1, Ordering::Relaxed);
                    gate.notified().await;
                    Some(2)
                })
                .await
            })
        };
        while started.load(Ordering::Relaxed) == 0 {
            tokio::task::yield_now().await;
        }
        let during = cell
            .get(Duration::ZERO, HOUR, || async { panic!("second reload") })
            .await;
        assert_eq!(during.as_deref(), Some(&1));
        gate.notify_one();
        assert_eq!(reload.await.unwrap().as_deref(), Some(&2));
    }
}
