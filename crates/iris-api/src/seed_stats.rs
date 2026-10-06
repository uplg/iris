//! Background reconciliation of the persistent lifetime-upload counter,
//! plus enforcement of the per-provider `seed = false` policy.
//!
//! librqbit exposes a per-torrent `uploaded_bytes` that is a session value:
//! it resets to zero on process restart and is gone forever once the torrent
//! is removed from the engine (typically by GC eviction). We want a "since
//! the beginning" total that survives both, so this loop walks the live
//! engine snapshots every 30 s and merges deltas into
//! `torrents.uploaded_bytes_total`.
//!
//! The same walk is the natural place to leave the swarm on torrents whose
//! provider declared `seed = false`: a torrent is only ever "finished" as
//! observed here, and the check has to be re-run after every restart anyway
//! (librqbit restores the session un-paused).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use iris_db::SqlitePool;
use iris_providers::ProviderRegistry;
use iris_torrent::{Engine, TorrentState};

const TICK: Duration = Duration::from_secs(30);

pub fn spawn(pool: SqlitePool, engine: Arc<Engine>, providers: ProviderRegistry) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(TICK);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // Skip the immediate boot tick — engine snapshots aren't fully
        // populated yet and we'd reconcile against zeros.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            crate::supervise::tick("seed stats", reconcile_once(&pool, &engine, &providers)).await;
        }
    });
}

async fn reconcile_once(pool: &SqlitePool, engine: &Engine, providers: &ProviderRegistry) {
    let no_seed = no_seed_infohashes(pool, providers).await;
    let snapshots = engine.list();
    // Upload deltas, download max (so ratios divide two lifetime
    // quantities) and the "fully downloaded" stamp, persisted while the
    // session can still answer it: after a restart the re-checking
    // (`initializing`) session reports finished = false for minutes, and the
    // DB stamp is what lets streaming serve finished torrents from disk then.
    let counters: Vec<_> = snapshots
        .iter()
        .map(|s| iris_db::torrents::SnapshotCounters {
            infohash: &s.infohash,
            uploaded_bytes: s.uploaded_bytes,
            progress_bytes: s.progress_bytes,
            finished: s.finished,
        })
        .collect();
    if let Err(e) = iris_db::torrents::reconcile_snapshots(pool, &counters).await {
        tracing::warn!(error = %e, "seed_stats reconcile failed");
    }
    for snap in snapshots {
        if !snap.finished {
            continue;
        }
        // Provider policy: this tracker's grabs don't seed. The download is
        // done, so leave the swarm. Files stay on disk and playback reads
        // from disk, so pausing costs the viewer nothing.
        if snap.state != TorrentState::Paused && no_seed.contains(&snap.infohash) {
            match engine.pause_by_infohash(&snap.infohash).await {
                Ok(()) => tracing::info!(
                    infohash = %snap.infohash,
                    name = snap.name.as_deref().unwrap_or("?"),
                    "download complete on a no-seed provider — leaving the swarm",
                ),
                Err(e) => {
                    tracing::warn!(error = %e, infohash = %snap.infohash, "no-seed pause failed");
                }
            }
        }
    }
}

/// Infohashes of active torrents whose source provider declared
/// `seed = false`. Derived from `torrents.source_provider` + the live
/// registry rather than a stored per-torrent flag, so flipping the config
/// takes effect on the next tick for torrents already on disk — including
/// un-pausing by simply removing the knob.
async fn no_seed_infohashes(
    pool: &SqlitePool,
    providers: &ProviderRegistry,
) -> std::collections::HashSet<String> {
    let rows = match iris_db::torrents::list_active(pool).await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!(error = %e, "seed_stats could not list torrents for the no-seed policy");
            return std::collections::HashSet::new();
        }
    };
    // One registry lookup per distinct provider instead of per row.
    let mut seeds: HashMap<String, bool> = HashMap::new();
    rows.into_iter()
        .filter(|row| {
            row.source_provider.as_ref().is_some_and(|id| {
                !*seeds
                    .entry(id.clone())
                    .or_insert_with(|| providers.seeds(id))
            })
        })
        .map(|row| row.infohash)
        .collect()
}
