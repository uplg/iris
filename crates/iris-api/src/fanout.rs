//! Bounded, order-preserving concurrency for per-row lookups.
//!
//! Handlers that enrich a list row by row (a DB query, a TMDB lookup per
//! item) run those lookups through [`map_ordered`] instead of awaiting them
//! one after another. The bound keeps SQLite's pool and TMDB's rate limit
//! comfortable; the output keeps the input order, which the lists rely on.

use std::future::Future;

use futures::stream::{self, StreamExt};

/// How many per-row lookups run at once.
pub const FANOUT: usize = 8;

/// Map every item through `f`, at most [`FANOUT`] at a time, keeping order.
pub async fn map_ordered<T, U, F, Fut>(items: impl IntoIterator<Item = T>, f: F) -> Vec<U>
where
    F: FnMut(T) -> Fut,
    Fut: Future<Output = U>,
{
    stream::iter(items).map(f).buffered(FANOUT).collect().await
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::map_ordered;

    #[tokio::test]
    async fn keeps_input_order_when_later_items_finish_first() {
        let out = map_ordered(0_u64..20, |i| async move {
            tokio::time::sleep(Duration::from_millis(20 - i)).await;
            i * 2
        })
        .await;
        assert_eq!(out, (0..20).map(|i| i * 2).collect::<Vec<_>>());
    }
}
