//! Panic containment for the background loops.
//!
//! Each loop is a detached `tokio::spawn`: a panic inside one tick would
//! end that task for good (a scheduler silently stopped until the next
//! restart), reported only by the default stderr hook. Running each tick
//! through [`tick`] logs the panic and lets the loop carry on.

use std::future::Future;
use std::panic::AssertUnwindSafe;

use futures::FutureExt;

/// Run one iteration of the loop `name`; a panic is logged, not propagated.
pub async fn tick<T>(name: &'static str, body: impl Future<Output = T>) -> Option<T> {
    match AssertUnwindSafe(body).catch_unwind().await {
        Ok(out) => Some(out),
        Err(panic) => {
            let message = panic
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "non-string panic payload".to_owned());
            tracing::error!(task = name, panic = %message, "background tick panicked; the loop carries on");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tick;

    #[tokio::test]
    async fn a_panicking_tick_is_contained() {
        assert_eq!(tick("test", async { 7 }).await, Some(7));
        let panicked: Option<()> = tick("test", async { panic!("boom") }).await;
        assert!(panicked.is_none());
    }
}
