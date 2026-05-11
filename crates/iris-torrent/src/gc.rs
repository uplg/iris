//! Disk-space garbage collector.
//!
//! Periodically sums the torrent download dir and any registered derived
//! caches (currently the remux/HLS cache). If total usage crosses
//! `cleanup_threshold_pct * max_storage`, we first trim the derived
//! caches — those are regenerable from the source torrent on the next
//! play, so dropping them is free compared to losing seed contribution.
//! Only if that's not enough do we evict torrents (oldest
//! `last_played_at` first, with `added_at` as fallback) until usage drops
//! below `cleanup_target_pct * max_storage`. Recently-played torrents
//! (within `active_window`) are protected — we never yank a file out
//! from under a viewer.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use futures::future::BoxFuture;
use iris_core::ids::TorrentId;
use sqlx::SqlitePool;
use tokio::time::MissedTickBehavior;

use crate::Engine;

/// Trim callback for a regenerable cache living alongside the torrent
/// download dir. Given a target byte budget for the cache itself, must
/// shrink the cache to ≤ target and return the freed bytes. The GC
/// invokes it before touching real torrents.
pub type DerivedTrimFn =
    Arc<dyn Fn(u64) -> BoxFuture<'static, u64> + Send + Sync>;

/// Pairing of a directory (counted toward the storage budget) with the
/// async callback that knows how to trim it.
pub struct DerivedCache {
    pub dir: PathBuf,
    pub trim_to: DerivedTrimFn,
}

#[derive(Debug, Clone)]
pub struct GcConfig {
    pub max_storage_bytes: u64,
    pub cleanup_threshold_pct: u8,
    pub cleanup_target_pct: u8,
    pub interval: Duration,
    /// Torrents whose `last_played_at` (or, if never played, `added_at`) falls
    /// within this window are protected from eviction.
    pub active_window: Duration,
}

impl GcConfig {
    pub fn threshold_bytes(&self) -> u64 {
        self.max_storage_bytes * u64::from(self.cleanup_threshold_pct) / 100
    }
    pub fn target_bytes(&self) -> u64 {
        self.max_storage_bytes * u64::from(self.cleanup_target_pct) / 100
    }
}

#[derive(Clone)]
pub struct Gc {
    inner: Arc<Inner>,
}

struct Inner {
    engine: Arc<Engine>,
    pool: SqlitePool,
    cfg: GcConfig,
    download_dir: PathBuf,
    derived: Option<DerivedCache>,
    /// Fired after a torrent is evicted from disk so derived caches keyed
    /// by infohash (e.g. the remuxer's `.fmp4` files) can clean up too.
    on_evict: Box<dyn Fn(&str) + Send + Sync>,
}

impl Gc {
    pub fn new(
        engine: Arc<Engine>,
        pool: SqlitePool,
        cfg: GcConfig,
        download_dir: PathBuf,
        derived: Option<DerivedCache>,
        on_evict: impl Fn(&str) + Send + Sync + 'static,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                engine,
                pool,
                cfg,
                download_dir,
                derived,
                on_evict: Box::new(on_evict),
            }),
        }
    }

    pub fn spawn(self) {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(self.inner.cfg.interval);
            ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
            // Skip the first immediate tick so we don't hammer disk on boot.
            ticker.tick().await;
            loop {
                ticker.tick().await;
                if let Err(e) = self.run_once().await {
                    tracing::error!(error = %e, "gc cycle failed");
                }
            }
        });
    }

    pub async fn run_once(&self) -> anyhow::Result<GcReport> {
        let cfg = &self.inner.cfg;
        let torrent_used = dir_size(&self.inner.download_dir).await.unwrap_or(0);
        let derived_used = match &self.inner.derived {
            Some(d) => dir_size(&d.dir).await.unwrap_or(0),
            None => 0,
        };
        let used = torrent_used.saturating_add(derived_used);
        let threshold = cfg.threshold_bytes();
        let target = cfg.target_bytes();

        let mut report = GcReport {
            used_bytes_before: used,
            threshold_bytes: threshold,
            target_bytes: target,
            derived_freed_bytes: 0,
            evicted: Vec::new(),
            used_bytes_after: used,
        };

        if used < threshold {
            tracing::debug!(
                used, torrent_used, derived_used, threshold,
                "gc: under threshold, nothing to do"
            );
            return Ok(report);
        }

        // Step 1: trim the derived cache first — those bytes regenerate
        // from the underlying torrent on the next play, so dropping them
        // costs nothing beyond a one-off ffmpeg run. Aim to leave the
        // derived cache at `target - torrent_used` so that after this
        // step the total sits at exactly `target`. If torrents alone
        // already exceed target, ask the derived cache to shrink to
        // zero (it'll wipe everything that's not in flight).
        let mut current_used = used;
        if let Some(d) = &self.inner.derived {
            let derived_target = target.saturating_sub(torrent_used);
            let freed = (d.trim_to)(derived_target).await;
            report.derived_freed_bytes = freed;
            current_used = current_used.saturating_sub(freed);
            tracing::info!(
                derived_target,
                freed,
                used_before = used,
                used_after = current_used,
                "gc: derived cache trim pass",
            );
            if current_used <= target {
                report.used_bytes_after = current_used;
                return Ok(report);
            }
        }

        // Step 2: only now do we touch real torrents.
        self.evict_torrents(current_used, target, &mut report).await?;

        let post_torrent_used = dir_size(&self.inner.download_dir).await.unwrap_or(0);
        let post_derived_used = match &self.inner.derived {
            Some(d) => dir_size(&d.dir).await.unwrap_or(0),
            None => 0,
        };
        report.used_bytes_after = post_torrent_used.saturating_add(post_derived_used);
        tracing::info!(
            evicted = report.evicted.len(),
            derived_freed = report.derived_freed_bytes,
            before = report.used_bytes_before,
            after = report.used_bytes_after,
            "gc: pass complete"
        );
        Ok(report)
    }

    async fn evict_torrents(
        &self,
        start_used: u64,
        target: u64,
        report: &mut GcReport,
    ) -> anyhow::Result<()> {
        let cfg = &self.inner.cfg;
        let rows = iris_db::torrents::list_active(&self.inner.pool).await?;
        let cutoff = Utc::now() - chrono::Duration::from_std(cfg.active_window).unwrap_or_default();
        let mut candidates: Vec<_> = rows
            .into_iter()
            .filter(|r| r.last_played_at.unwrap_or(r.added_at) < cutoff)
            .collect();
        // Oldest activity first.
        candidates.sort_by_key(|r| r.last_played_at.unwrap_or(r.added_at));

        let mut current = start_used;
        for row in candidates {
            if current <= target {
                break;
            }
            tracing::info!(
                infohash = %row.infohash,
                name = %row.name,
                size = row.total_size_bytes,
                "gc: evicting torrent"
            );
            // Final upload reconcile so the bytes seeded since the last
            // 30 s tick aren't lost when librqbit drops the torrent.
            if let Some(snap) = self.inner.engine.get_by_infohash(&row.infohash) {
                let _ = iris_db::torrents::reconcile_uploaded(
                    &self.inner.pool,
                    &row.infohash,
                    snap.uploaded_bytes,
                )
                .await;
            }
            if let Err(e) = self
                .inner
                .engine
                .delete_by_infohash(&row.infohash, true)
                .await
            {
                tracing::warn!(error = %e, "gc: engine delete failed, skipping");
                continue;
            }
            (self.inner.on_evict)(&row.infohash);
            iris_db::torrents::soft_delete(&self.inner.pool, TorrentId::from(row.id)).await?;
            let freed = u64::try_from(row.total_size_bytes).unwrap_or(0);
            current = current.saturating_sub(freed);
            report.evicted.push(EvictedEntry {
                infohash: row.infohash,
                name: row.name,
                freed_bytes: freed,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GcReport {
    pub used_bytes_before: u64,
    pub used_bytes_after: u64,
    pub threshold_bytes: u64,
    pub target_bytes: u64,
    /// Bytes reclaimed from the derived cache (remux) before any
    /// torrent was touched. Zero when the threshold was hit purely by
    /// torrent footprint (or no derived cache was registered).
    pub derived_freed_bytes: u64,
    pub evicted: Vec<EvictedEntry>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EvictedEntry {
    pub infohash: String,
    pub name: String,
    pub freed_bytes: u64,
}

async fn dir_size(path: &Path) -> std::io::Result<u64> {
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(p) = stack.pop() {
        let mut read = match tokio::fs::read_dir(&p).await {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        while let Some(entry) = read.next_entry().await? {
            let Ok(m) = entry.metadata().await else {
                continue;
            };
            if m.is_dir() {
                stack.push(entry.path());
            } else {
                total += m.len();
            }
        }
    }
    Ok(total)
}
