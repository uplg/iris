//! Background reconciliation of the persistent lifetime-upload counter.
//!
//! librqbit exposes a per-torrent `uploaded_bytes` that is a session value:
//! it resets to zero on process restart and is gone forever once the torrent
//! is removed from the engine (typically by GC eviction). We want a "since
//! the beginning" total that survives both, so this loop walks the live
//! engine snapshots every 30 s and merges deltas into
//! `torrents.uploaded_bytes_total`.

use std::sync::Arc;
use std::time::Duration;

use iris_db::SqlitePool;
use iris_torrent::Engine;

const TICK: Duration = Duration::from_secs(30);

pub fn spawn(pool: SqlitePool, engine: Arc<Engine>) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(TICK);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // Skip the immediate boot tick — engine snapshots aren't fully
        // populated yet and we'd reconcile against zeros.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            reconcile_once(&pool, &engine).await;
        }
    });
}

async fn reconcile_once(pool: &SqlitePool, engine: &Engine) {
    let snapshots = engine.list();
    for snap in snapshots {
        if let Err(e) =
            iris_db::torrents::reconcile_uploaded(pool, &snap.infohash, snap.uploaded_bytes).await
        {
            tracing::warn!(error = %e, infohash = %snap.infohash, "seed_stats reconcile failed");
        }
    }
}
