//! Disk-space garbage collector.
//!
//! Periodically sums the torrent download dir and any registered derived
//! caches (currently the remux/HLS cache). If total usage crosses
//! `cleanup_threshold_pct * max_storage`, we first trim the derived
//! caches — those are regenerable from the source torrent on the next
//! play, so dropping them is free compared to losing seed contribution.
//! Only if that's not enough do we evict torrents (oldest activity first:
//! the later of `last_played_at` and `added_at`) until usage drops
//! below `cleanup_target_pct * max_storage`. Recently-played torrents
//! (within `active_window`) are protected — we never yank a file out
//! from under a viewer.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use futures::FutureExt;
use futures::future::BoxFuture;
use sqlx::SqlitePool;
use tokio::time::MissedTickBehavior;

use crate::Engine;

/// Trim callback for a regenerable cache living alongside the torrent
/// download dir. Given a target byte budget for the cache itself, must
/// shrink the cache to ≤ target and return the freed bytes. The GC
/// invokes it before touching real torrents.
pub type DerivedTrimFn = Arc<dyn Fn(u64) -> BoxFuture<'static, u64> + Send + Sync>;

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
    /// Torrents whose last activity (see [`last_activity`]) falls within this
    /// window are protected from eviction.
    pub active_window: Duration,
    /// Free space the download dir's filesystem must keep (0: no floor).
    pub min_free_bytes: u64,
    /// An unreferenced file younger than this is never an orphan.
    pub orphan_min_age: Duration,
    /// Delete orphan files instead of only reporting them.
    pub delete_orphans: bool,
    /// Trees the orphan sweep never enters (the data dir, the engine's
    /// state); one that IS the download dir turns the sweep off.
    pub orphan_exclude: Vec<PathBuf>,
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
    run_lock: tokio::sync::Mutex<()>,
    /// The orphan sweep's last (files, bytes), to log only on change.
    last_orphans: std::sync::Mutex<(u64, u64)>,
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
                run_lock: tokio::sync::Mutex::new(()),
                last_orphans: std::sync::Mutex::new((0, 0)),
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
                // A panic in one pass must not end the GC until the next restart.
                match std::panic::AssertUnwindSafe(self.run_once())
                    .catch_unwind()
                    .await
                {
                    Ok(Ok(_)) => {}
                    Ok(Err(e)) => tracing::error!(error = %e, "gc cycle failed"),
                    Err(_) => tracing::error!("gc cycle panicked; the loop carries on"),
                }
            }
        });
    }

    pub async fn run_once(&self) -> anyhow::Result<GcReport> {
        let cfg = &self.inner.cfg;
        // One pass at a time: an admin run overlapping the loop would measure
        // the same usage and evict toward the same target twice.
        let _running = self.inner.run_lock.lock().await;
        // An unreadable tree must fail the pass, not read as "0 bytes used"
        // (which silently disables the GC).
        let torrent_used = dir_size(&self.inner.download_dir)
            .await
            .map_err(|e| anyhow::anyhow!("measuring the download dir: {e}"))?;
        let derived_used = match &self.inner.derived {
            Some(d) => dir_size(&d.dir)
                .await
                .map_err(|e| anyhow::anyhow!("measuring the derived cache: {e}"))?,
            None => 0,
        };
        let used = torrent_used.saturating_add(derived_used);
        let threshold = cfg.threshold_bytes();
        let target = cfg.target_bytes();
        let free = free_bytes(&self.inner.download_dir);
        // Over the budget: back down to its target. Short of real disk
        // space (other tenants of the disk, files the budget doesn't count):
        // back up to 1.5× the floor. Whichever asks for more.
        let over_budget = if used >= threshold {
            used.saturating_sub(target)
        } else {
            0
        };
        let short_of_disk = match free {
            Some(free) if cfg.min_free_bytes > 0 && free < cfg.min_free_bytes => {
                (cfg.min_free_bytes + cfg.min_free_bytes / 2).saturating_sub(free)
            }
            _ => 0,
        };
        let to_free = over_budget.max(short_of_disk);

        let mut report = GcReport {
            used_bytes_before: used,
            threshold_bytes: threshold,
            target_bytes: target,
            derived_freed_bytes: 0,
            evicted: Vec::new(),
            used_bytes_after: used,
            free_bytes_before: free,
            min_free_bytes: cfg.min_free_bytes,
            orphans: OrphanReport::default(),
        };

        if to_free > 0 {
            self.free_up(to_free, derived_used, &mut report).await?;
        } else {
            tracing::debug!(
                used,
                torrent_used,
                derived_used,
                threshold,
                free,
                "gc: under threshold, nothing to do"
            );
        }
        report.orphans = self.sweep_orphans().await;
        Ok(report)
    }

    /// Trim the derived cache first — those bytes regenerate from the
    /// underlying torrent on the next play, so dropping them costs nothing
    /// beyond a one-off ffmpeg run — and only then evict torrents.
    async fn free_up(
        &self,
        to_free: u64,
        derived_used: u64,
        report: &mut GcReport,
    ) -> anyhow::Result<()> {
        let mut remaining = to_free;
        if let Some(d) = &self.inner.derived {
            let derived_target = derived_used.saturating_sub(remaining);
            let freed = (d.trim_to)(derived_target).await;
            report.derived_freed_bytes = freed;
            remaining = remaining.saturating_sub(freed);
            tracing::info!(
                derived_target,
                freed,
                to_free,
                remaining,
                "gc: derived cache trim pass",
            );
        }
        if remaining > 0 {
            self.evict_torrents(remaining, 0, report).await?;
        }
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
            free_before = ?report.free_bytes_before,
            "gc: pass complete"
        );
        Ok(())
    }

    /// Files under the download dir that no torrent reads: left by a crash,
    /// an engine that lost a torrent, a delete from an older build. Never one
    /// a managed torrent references, never one under a live row's folder
    /// (its torrent may just have failed to restore), never one touched
    /// within `orphan_min_age`; deleted only with `delete_orphans`, else
    /// logged when the set changes.
    async fn sweep_orphans(&self) -> OrphanReport {
        let cfg = &self.inner.cfg;
        let engine = &self.inner.engine;
        let root = &self.inner.download_dir;
        let mut report = OrphanReport {
            deleted: cfg.delete_orphans,
            ..OrphanReport::default()
        };
        // Only a tree inside the download dir is in the walk's way; one that
        // is the download dir itself (the data dir as download dir) ends it.
        let excluded: Vec<&Path> = cfg
            .orphan_exclude
            .iter()
            .map(PathBuf::as_path)
            .filter(|ex| ex.starts_with(root))
            .collect();
        if excluded.contains(&root.as_path()) || !engine.file_lists_known() {
            return report;
        }
        let protected: HashSet<std::ffi::OsString> =
            match iris_db::torrents::list_active(&self.inner.pool).await {
                Ok(rows) => rows
                    .into_iter()
                    .filter(|r| !engine.contains(&r.infohash))
                    .map(|r| r.name.into())
                    .collect(),
                Err(e) => {
                    tracing::warn!(error = %e, "gc: orphan sweep skipped, rows unreadable");
                    return report;
                }
            };
        let walk = OrphanWalk {
            root,
            excluded: &excluded,
            protected: &protected,
            referenced: &engine.referenced_files(),
            min_age: cfg.orphan_min_age,
        };
        let mut emptied = Vec::new();
        for (path, len) in walk.run().await {
            report.files += 1;
            report.bytes = report.bytes.saturating_add(len);
            if !cfg.delete_orphans {
                if report.examples.len() < 5 {
                    report.examples.push(path.display().to_string());
                }
                continue;
            }
            match tokio::fs::remove_file(&path).await {
                Ok(()) => {
                    tracing::info!(path = %path.display(), size = len, "gc: orphan file deleted");
                    emptied.extend(path.parent().map(Path::to_path_buf));
                }
                Err(e) => {
                    tracing::warn!(error = %e, path = %path.display(), "gc: orphan delete failed");
                }
            }
        }
        remove_emptied(root, emptied).await;
        let seen = (report.files, report.bytes);
        let changed = {
            let mut last = self
                .inner
                .last_orphans
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            std::mem::replace(&mut *last, seen) != seen
        };
        if changed && report.files > 0 && !cfg.delete_orphans {
            tracing::info!(
                files = report.files,
                bytes = report.bytes,
                examples = ?report.examples,
                "gc: files no torrent references (dry run: storage.delete_orphan_files = true deletes them)"
            );
        }
        report
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
            .filter(|r| last_activity(r) < cutoff)
            .collect();
        candidates.sort_by_key(last_activity);

        let mut current = start_used;
        for row in candidates {
            if current <= target {
                break;
            }
            // Each eviction can wait seconds on a stopped announce: a torrent
            // whose playback started since the listing is no longer cold.
            let Some(fresh) =
                iris_db::torrents::find_by_infohash(&self.inner.pool, &row.infohash).await?
            else {
                continue;
            };
            if fresh.deleted_at.is_some() || last_activity(&fresh) >= cutoff {
                continue;
            }
            tracing::info!(
                infohash = %row.infohash,
                name = %row.name,
                size = row.total_size_bytes,
                "gc: evicting torrent"
            );
            let removed = match crate::removal::remove_torrent(
                &self.inner.engine,
                &self.inner.pool,
                &fresh,
            )
            .await
            {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(error = %e, infohash = %row.infohash, "gc: eviction failed, skipping");
                    continue;
                }
            };
            (self.inner.on_evict)(&row.infohash);
            // Only what came off the disk counts: files another torrent still
            // reads stayed, and a torrent the engine no longer had freed nothing.
            let freed = if removed.row_was_live {
                removed.freed_bytes
            } else {
                0
            };
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

#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
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
    /// Free space on the download dir's filesystem before the pass (`None`:
    /// unreadable).
    pub free_bytes_before: Option<u64>,
    pub min_free_bytes: u64,
    pub orphans: OrphanReport,
}

/// Files under the download dir no torrent references.
#[derive(Debug, Clone, Default, serde::Serialize, utoipa::ToSchema)]
pub struct OrphanReport {
    pub files: u64,
    pub bytes: u64,
    /// They were deleted (`storage.delete_orphan_files`), not only counted.
    pub deleted: bool,
    /// A few of their paths, for the dry run.
    pub examples: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct EvictedEntry {
    pub infohash: String,
    pub name: String,
    pub freed_bytes: u64,
}

/// When a torrent was last wanted: played, or (re-)grabbed, whichever is
/// later. A re-grab of an old, once-played release is fresh activity.
fn last_activity(row: &iris_db::torrents::TorrentRow) -> chrono::DateTime<Utc> {
    later_of(row.last_played_at, row.added_at)
}

fn later_of(
    played: Option<chrono::DateTime<Utc>>,
    added: chrono::DateTime<Utc>,
) -> chrono::DateTime<Utc> {
    played.map_or(added, |p| p.max(added))
}

/// The orphan sweep's walk of the download dir.
struct OrphanWalk<'a> {
    root: &'a Path,
    excluded: &'a [&'a Path],
    /// Top-level names of live rows the engine doesn't manage.
    protected: &'a HashSet<std::ffi::OsString>,
    referenced: &'a HashSet<PathBuf>,
    min_age: Duration,
}

impl OrphanWalk<'_> {
    /// Each orphan file with its size.
    async fn run(&self) -> Vec<(PathBuf, u64)> {
        let mut found = Vec::new();
        let mut stack = vec![self.root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(mut read) = tokio::fs::read_dir(&dir).await else {
                continue;
            };
            while let Ok(Some(entry)) = read.next_entry().await {
                let path = entry.path();
                if self.excluded.iter().any(|ex| path.starts_with(ex))
                    || (dir == self.root && self.protected.contains(&entry.file_name()))
                {
                    continue;
                }
                let Ok(meta) = entry.metadata().await else {
                    continue;
                };
                if meta.is_dir() {
                    stack.push(path);
                    continue;
                }
                let old = meta
                    .modified()
                    .ok()
                    .and_then(|m| m.elapsed().ok())
                    .is_some_and(|age| age >= self.min_age);
                if old && !self.referenced.contains(&path) {
                    found.push((path, meta.len()));
                }
            }
        }
        found
    }
}

/// The directories the deleted orphans sat in, and their parents, once
/// empty; never `root` nor above it.
async fn remove_emptied(root: &Path, mut dirs: Vec<PathBuf>) {
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    dirs.dedup();
    for dir in dirs {
        for d in dir
            .ancestors()
            .take_while(|d| *d != root && d.starts_with(root))
        {
            if tokio::fs::remove_dir(d).await.is_err() {
                break;
            }
        }
    }
}

/// Total and free bytes (as an unprivileged writer sees them) of the
/// filesystem holding `path`.
#[must_use]
pub fn disk_space(path: &Path) -> Option<(u64, u64)> {
    let st = rustix::fs::statvfs(path).ok()?;
    let block = to_u64(st.f_frsize);
    Some((
        to_u64(st.f_blocks).saturating_mul(block),
        to_u64(st.f_bavail).saturating_mul(block),
    ))
}

/// `statvfs` field widths differ across platforms (`u32` on macOS, `u64`
/// or `c_long` on Linux).
fn to_u64<T: TryInto<u64>>(v: T) -> u64 {
    v.try_into().unwrap_or(0)
}

fn free_bytes(path: &Path) -> Option<u64> {
    disk_space(path).map(|(_, free)| free)
}

/// Bytes under `path`, recursively. A missing directory counts as empty.
pub async fn dir_size(path: &Path) -> std::io::Result<u64> {
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

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};

    use super::{Gc, GcConfig, later_of};
    use crate::Engine;
    use crate::removal::tests::{grab, temp_dir, torrent_bytes, write};

    fn config(max_storage_bytes: u64, min_free_bytes: u64, delete_orphans: bool) -> GcConfig {
        GcConfig {
            max_storage_bytes,
            cleanup_threshold_pct: 90,
            cleanup_target_pct: 75,
            interval: std::time::Duration::from_mins(15),
            active_window: std::time::Duration::ZERO,
            min_free_bytes,
            orphan_min_age: std::time::Duration::from_hours(24),
            delete_orphans,
            orphan_exclude: Vec::new(),
        }
    }

    /// Two copies of one release (shared files) plus nothing else.
    async fn shared_pair(dir: &std::path::Path) -> (std::sync::Arc<Engine>, sqlx::SqlitePool) {
        let engine = Engine::offline(dir.to_path_buf()).await.unwrap();
        let pool = iris_db::test_support::migrated_pool().await;
        let name = "Show.S01.1080p.WEB-GRP";
        write(dir, &format!("{name}/Show.S01E01.mkv"), 40_000);
        write(dir, &format!("{name}/Show.S01E02.mkv"), 30_000);
        let files = [("Show.S01E01.mkv", 40_000), ("Show.S01E02.mkv", 30_000)];
        grab(&engine, &pool, torrent_bytes(name, &files, "a")).await;
        grab(&engine, &pool, torrent_bytes(name, &files, "b")).await;
        (engine, pool)
    }

    #[tokio::test]
    async fn eviction_credits_only_the_bytes_that_left_the_disk() {
        let dir = temp_dir();
        let (engine, pool) = shared_pair(&dir).await;
        let gc = Gc::new(
            engine,
            pool,
            config(1_000, 0, false),
            dir.clone(),
            None,
            |_| {},
        );
        let report = gc.run_once().await.unwrap();
        let freed: Vec<u64> = report.evicted.iter().map(|e| e.freed_bytes).collect();
        assert_eq!(
            freed,
            [0, 70_000],
            "the first copy's files stayed for the second"
        );
        assert_eq!(report.used_bytes_after, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn too_little_free_disk_evicts_under_the_budget() {
        let dir = temp_dir();
        let (engine, pool) = shared_pair(&dir).await;
        let roomy = config(u64::MAX / 200, 0, false);
        let gc = Gc::new(
            engine.clone(),
            pool.clone(),
            roomy,
            dir.clone(),
            None,
            |_| {},
        );
        assert!(gc.run_once().await.unwrap().evicted.is_empty());
        let floor = config(u64::MAX / 200, u64::MAX / 4, false);
        let gc = Gc::new(engine, pool, floor, dir.clone(), None, |_| {});
        let report = gc.run_once().await.unwrap();
        assert!(report.free_bytes_before.is_some());
        assert_eq!(report.evicted.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn orphans_are_counted_then_deleted_only_when_asked() {
        let dir = temp_dir();
        let (engine, pool) = shared_pair(&dir).await;
        let old = std::time::SystemTime::now() - std::time::Duration::from_hours(48);
        let age = |rel: &str| {
            std::fs::File::options()
                .write(true)
                .open(dir.join(rel))
                .unwrap()
                .set_modified(old)
                .unwrap();
        };
        write(&dir, "leftover/old.mkv", 500);
        age("leftover/old.mkv");
        write(&dir, "fresh.mkv", 300);
        age("Show.S01.1080p.WEB-GRP/Show.S01E01.mkv");
        let user = iris_db::test_support::make_user(&pool).await;
        iris_db::torrents::upsert(
            &pool,
            iris_db::torrents::NewTorrent {
                infohash: "c".repeat(40),
                name: "Lost.Release-GRP".into(),
                total_size_bytes: 200,
                source_provider: None,
                source_external_id: None,
                tracker_tmdb_id: None,
                added_by: user,
            },
        )
        .await
        .unwrap();
        write(&dir, "Lost.Release-GRP/lost.mkv", 200);
        age("Lost.Release-GRP/lost.mkv");

        let roomy = |delete| config(u64::MAX / 200, 0, delete);
        let dry = Gc::new(
            engine.clone(),
            pool.clone(),
            roomy(false),
            dir.clone(),
            None,
            |_| {},
        );
        let report = dry.run_once().await.unwrap().orphans;
        assert_eq!(
            (report.files, report.bytes, report.deleted),
            (1, 500, false)
        );
        assert!(dir.join("leftover/old.mkv").exists());

        let wet = Gc::new(engine, pool, roomy(true), dir.clone(), None, |_| {});
        let report = wet.run_once().await.unwrap().orphans;
        assert_eq!((report.files, report.deleted), (1, true));
        assert!(
            !dir.join("leftover").exists(),
            "the orphan and its emptied folder"
        );
        assert!(dir.join("fresh.mkv").exists(), "too young");
        assert!(
            dir.join("Lost.Release-GRP/lost.mkv").exists(),
            "a live row's folder"
        );
        assert!(
            dir.join("Show.S01.1080p.WEB-GRP/Show.S01E01.mkv").exists(),
            "referenced"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_regrab_after_an_old_play_counts_as_fresh_activity() {
        let now = Utc::now();
        let month_ago = now - Duration::days(30);
        assert_eq!(later_of(Some(month_ago), now), now);
        assert_eq!(later_of(Some(now), month_ago), now);
        assert_eq!(later_of(None, month_ago), month_ago);
    }
}
