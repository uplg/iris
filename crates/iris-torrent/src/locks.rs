//! One async lock per infohash, held across a grab's engine add and its row
//! upsert, and across a removal's engine delete and soft delete, so neither
//! interleaves with the other: a grab can't revive a row whose files a
//! concurrent delete is wiping, nor a delete leave an engine torrent behind a
//! row it just removed.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

type Slot = Arc<tokio::sync::Mutex<()>>;

#[derive(Default)]
pub(crate) struct InfohashLocks {
    slots: Arc<Mutex<HashMap<String, Slot>>>,
}

/// Proof that the caller holds an infohash's lock; released on drop.
pub struct InfohashLock {
    infohash: String,
    slots: Arc<Mutex<HashMap<String, Slot>>>,
    guard: Option<tokio::sync::OwnedMutexGuard<()>>,
}

impl std::fmt::Debug for InfohashLock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("InfohashLock").field(&self.infohash).finish()
    }
}

impl InfohashLock {
    #[must_use]
    pub fn infohash(&self) -> &str {
        &self.infohash
    }
}

impl InfohashLocks {
    pub(crate) async fn lock(&self, infohash: &str) -> InfohashLock {
        let infohash = infohash.to_ascii_lowercase();
        let slot = Arc::clone(
            self.slots
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .entry(infohash.clone())
                .or_default(),
        );
        let guard = slot.lock_owned().await;
        InfohashLock {
            infohash,
            slots: Arc::clone(&self.slots),
            guard: Some(guard),
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.slots
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

impl Drop for InfohashLock {
    fn drop(&mut self) {
        let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        drop(self.guard.take());
        // Nobody else waiting on (or holding a handle to) this slot: forget it.
        if slots
            .get(&self.infohash)
            .is_some_and(|slot| Arc::strong_count(slot) == 1)
        {
            slots.remove(&self.infohash);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::InfohashLocks;

    #[tokio::test]
    async fn one_holder_per_infohash_and_the_map_empties() {
        let locks = InfohashLocks::default();
        let held = locks.lock("ABC").await;
        assert!(
            tokio::time::timeout(Duration::from_millis(50), locks.lock("abc"))
                .await
                .is_err(),
            "the same infohash, any case, waits"
        );
        let other = locks.lock("def").await;
        drop(other);
        drop(held);
        let again = locks.lock("abc").await;
        drop(again);
        assert_eq!(locks.len(), 0);
    }
}
