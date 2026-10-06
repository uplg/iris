//! Single-flight cookie-session login shared by the scraped trackers.
//!
//! A failed login is remembered for [`LOGIN_RETRY_AFTER`]: private trackers
//! lock or ban an account after a few bad posts, and search-as-you-type would
//! otherwise re-post a stale password on every keystroke.

use std::time::{Duration, Instant};

use iris_core::{Error, Result};
use tokio::sync::Mutex;

pub(crate) const LOGIN_RETRY_AFTER: Duration = Duration::from_mins(5);

/// Identifies one successful login; see [`LoginGate::invalidate`].
pub(crate) type SessionGeneration = u64;

pub(crate) struct LoginGate {
    retry_after: Duration,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    generation: SessionGeneration,
    logged_in: bool,
    failure: Option<(Instant, String)>,
}

impl LoginGate {
    pub(crate) fn new() -> Self {
        Self::with_retry_after(LOGIN_RETRY_AFTER)
    }

    pub(crate) fn with_retry_after(retry_after: Duration) -> Self {
        Self {
            retry_after,
            state: Mutex::new(State::default()),
        }
    }

    /// The live session's generation, logging in first when there is none.
    /// The lock is held across `login` so concurrent callers share one post.
    pub(crate) async fn ensure<F, Fut>(&self, login: F) -> Result<SessionGeneration>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<()>>,
    {
        let mut st = self.state.lock().await;
        if st.logged_in {
            return Ok(st.generation);
        }
        if let Some((at, reason)) = &st.failure
            && at.elapsed() < self.retry_after
        {
            return Err(Error::Provider(format!(
                "{reason} (not retried for {} min, to spare the account)",
                self.retry_after.as_secs().div_ceil(60)
            )));
        }
        match login().await {
            Ok(()) => {
                st.generation += 1;
                st.logged_in = true;
                st.failure = None;
                Ok(st.generation)
            }
            Err(e) => {
                st.failure = Some((Instant::now(), e.to_string()));
                Err(e)
            }
        }
    }

    /// Drop the session the caller saw rejected. A no-op when another caller
    /// already logged in again since, so a straggler can't kill a fresh one.
    pub(crate) async fn invalidate(&self, seen: SessionGeneration) {
        let mut st = self.state.lock().await;
        if st.generation == seen {
            st.logged_in = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    use iris_core::Error;

    use super::LoginGate;

    #[tokio::test]
    async fn a_failed_login_is_not_retried_inside_the_window() {
        let gate = LoginGate::with_retry_after(Duration::from_hours(1));
        let posts = AtomicU32::new(0);
        let failing = || async {
            posts.fetch_add(1, Ordering::SeqCst);
            Err(Error::Provider("login failed: bad password".into()))
        };
        assert!(gate.ensure(failing).await.is_err());
        let again = gate.ensure(failing).await.expect_err("still refused");
        assert!(again.to_string().contains("bad password"), "{again}");
        assert_eq!(posts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_failure_is_retried_once_the_window_lapses() {
        let gate = LoginGate::with_retry_after(Duration::ZERO);
        let posts = AtomicU32::new(0);
        let failing = || async {
            posts.fetch_add(1, Ordering::SeqCst);
            Err(Error::Provider("down".into()))
        };
        assert!(gate.ensure(failing).await.is_err());
        assert!(gate.ensure(failing).await.is_err());
        assert_eq!(posts.load(Ordering::SeqCst), 2);
        assert_eq!(gate.ensure(|| async { Ok(()) }).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn a_stale_invalidate_keeps_the_fresh_session() {
        let gate = LoginGate::new();
        let posts = AtomicU32::new(0);
        let ok = || async {
            posts.fetch_add(1, Ordering::SeqCst);
            Ok(())
        };
        let seen_by_a = gate.ensure(ok).await.unwrap();
        let seen_by_b = gate.ensure(ok).await.unwrap();
        assert_eq!(seen_by_a, seen_by_b);
        gate.invalidate(seen_by_a).await;
        let fresh = gate.ensure(ok).await.unwrap();
        assert_eq!(posts.load(Ordering::SeqCst), 2);
        gate.invalidate(seen_by_b).await;
        assert_eq!(gate.ensure(ok).await.unwrap(), fresh);
        assert_eq!(posts.load(Ordering::SeqCst), 2);
    }
}
