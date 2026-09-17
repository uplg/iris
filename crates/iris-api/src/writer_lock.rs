//! Single-writer guard for the data directory.
//!
//! The 30 s seed-stats loop merges librqbit's session upload counters into
//! `torrents.uploaded_bytes_total` with delta math that assumes ONE writer:
//! a second backend sharing the same `data_dir` (and therefore the same
//! `iris.db`) would add every delta a second time and inflate all ratios
//! with no visible error. `flock(LOCK_EX)` on `<data_dir>/.iris-writer.lock`
//! is held for the whole process lifetime; a second instance fails fast at
//! boot with a clear error instead of silently corrupting counters. The
//! kernel releases the lock on crash, so restarts are unaffected.

use std::fs::File;
use std::path::Path;

use anyhow::Context;

/// Acquire the exclusive writer lock for `data_dir`. The returned [`File`]
/// must be kept alive for the whole process lifetime — dropping it releases
/// the lock. Fails when another Iris instance already holds it.
pub(crate) fn acquire(data_dir: &Path) -> anyhow::Result<File> {
    std::fs::create_dir_all(data_dir)
        .with_context(|| format!("creating data dir {}", data_dir.display()))?;
    let path = data_dir.join(".iris-writer.lock");
    let file =
        File::create(&path).with_context(|| format!("creating lockfile {}", path.display()))?;
    // `File::try_lock` is in std since Rust 1.89 (flock on Unix): no new
    // dependency needed for the guard.
    if let Err(e) = file.try_lock() {
        if matches!(e, std::fs::TryLockError::WouldBlock) {
            anyhow::bail!(
                "another Iris instance is already running on {} ({} is locked); \
                 refusing to start a second writer on the same iris.db",
                data_dir.display(),
                path.display()
            );
        }
        return Err(anyhow::anyhow!("locking {}: {e}", path.display()));
    }
    tracing::info!(lockfile = %path.display(), "single-writer lock acquired");
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_creates_lockfile_and_succeeds() {
        let dir = std::env::temp_dir().join(format!("iris-writer-lock-{}", std::process::id()));
        let _held = acquire(&dir).expect("first acquire succeeds");
        assert!(dir.join(".iris-writer.lock").is_file());
        std::fs::remove_dir_all(&dir).ok();
    }
}
