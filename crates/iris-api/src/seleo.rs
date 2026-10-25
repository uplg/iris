//! The films shared with `SéléoTV`, a separate app on the same host. It mounts
//! `downloads/` and `share/` of the data volume read-only and never talks to
//! Iris: what it may show is `share/seleo.json` (the manifest) and the
//! posters under `share/posters/`. Both follow `seleo_shares`: rewritten after
//! every change, once at boot, and hourly, when the shares that ended and
//! those of torrents gone from disk are dropped. `SéléoTV` runs as another uid,
//! so all of it is world-readable.

use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, SecondsFormat, Utc};
use iris_db::SqlitePool;
use iris_db::seleo_shares::ShareRow;
use serde::Serialize;

use crate::tmdb::TmdbClient;

pub const MANIFEST: &str = "seleo.json";
const POSTERS: &str = "posters";
const POSTER_SIZE: &str = "w500";
/// A w500 poster weighs ~100 KB; anything far past it isn't one.
const POSTER_MAX_BYTES: usize = 4 * 1024 * 1024;

/// One writer at a time: two requests rewriting the manifest would share
/// its temp file.
static WRITING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// `<data_dir>/share`, next to `downloads/` in the volume `SéléoTV` mounts.
#[must_use]
pub fn share_dir(cfg: &iris_config::AppConfig) -> PathBuf {
    cfg.storage.data_dir.join("share")
}

/// A share's id in the manifest, also its poster's file stem.
#[must_use]
pub fn film_id(infohash: &str, file_idx: i64) -> String {
    format!("{infohash}-{file_idx}")
}

#[derive(Debug, Serialize)]
struct Manifest {
    version: u32,
    generated_at: String,
    films: Vec<Film>,
}

#[derive(Debug, Serialize)]
struct Film {
    id: String,
    /// Relative to `downloads/`.
    path: String,
    size: i64,
    tmdb_id: Option<i64>,
    title: String,
    year: Option<i64>,
    overview: Option<String>,
    runtime_minutes: Option<i64>,
    /// Relative to `share/`.
    poster: Option<String>,
    shared_at: String,
    until: String,
}

fn rfc3339(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn manifest(rows: Vec<ShareRow>, now: DateTime<Utc>) -> Manifest {
    Manifest {
        version: 1,
        generated_at: rfc3339(now),
        films: rows
            .into_iter()
            .map(|s| Film {
                id: film_id(&s.infohash, s.file_idx),
                path: s.path,
                size: s.size_bytes,
                tmdb_id: s.tmdb_id,
                title: s.title,
                year: s.year,
                overview: s.overview,
                runtime_minutes: s.runtime_minutes,
                poster: s.poster.map(|p| format!("{POSTERS}/{p}")),
                shared_at: rfc3339(s.shared_at),
                until: rfc3339(s.until),
            })
            .collect(),
    }
}

/// Rewrite the manifest from the shares running now; answers how many it
/// lists. Atomic: `SéléoTV` reads the old file or the new one, never half.
///
/// # Errors
/// The shares can't be read, or the file can't be written.
pub async fn write_manifest(pool: &SqlitePool, dir: &Path) -> anyhow::Result<usize> {
    let _one = WRITING.lock().await;
    let now = Utc::now();
    let rows = iris_db::seleo_shares::list_live(pool, now).await?;
    let count = rows.len();
    let bytes = serde_json::to_vec_pretty(&manifest(rows, now))?;
    ensure_dirs(dir).await?;
    write_atomic(&dir.join(MANIFEST), &bytes).await?;
    Ok(count)
}

/// [`write_manifest`], a failure logged: the hourly pass writes it again.
pub async fn publish(pool: &SqlitePool, dir: &Path) {
    if let Err(e) = write_manifest(pool, dir).await {
        tracing::warn!(error = %e, "seleo: manifest not written");
    }
}

/// Drop the shares that ended and those of torrents gone from disk, their
/// posters with them, then rewrite the manifest.
///
/// # Errors
/// The table can't be read or written, or the manifest can't be written.
pub async fn sweep(pool: &SqlitePool, dir: &Path) -> anyhow::Result<()> {
    let stale = iris_db::seleo_shares::take_stale(pool, Utc::now()).await?;
    if !stale.is_empty() {
        tracing::info!(dropped = stale.len(), "seleo: ended shares dropped");
    }
    remove_posters(dir, stale).await;
    write_manifest(pool, dir).await?;
    Ok(())
}

/// A torrent left the disk: its shares go, and the manifest stops listing
/// them now rather than at the next sweep.
pub async fn forget_torrent(pool: &SqlitePool, dir: &Path, infohash: &str) {
    match iris_db::seleo_shares::delete_for_infohash(pool, infohash).await {
        Ok(posters) if posters.is_empty() => {}
        Ok(posters) => {
            tracing::info!(%infohash, "seleo: share of a deleted torrent dropped");
            remove_posters(dir, posters).await;
            publish(pool, dir).await;
        }
        Err(e) => tracing::warn!(error = %e, %infohash, "seleo: shares of a deleted torrent kept"),
    }
}

/// Stop one share: its row, its poster, its manifest entry. `false` when
/// there was none.
///
/// # Errors
/// The row can't be deleted.
pub async fn unshare(
    pool: &SqlitePool,
    dir: &Path,
    infohash: &str,
    file_idx: i64,
) -> Result<bool, sqlx::Error> {
    let Some(poster) = iris_db::seleo_shares::delete(pool, infohash, file_idx).await? else {
        return Ok(false);
    };
    remove_posters(dir, [poster]).await;
    publish(pool, dir).await;
    Ok(true)
}

/// The boot write, then the hourly sweep.
pub fn spawn(pool: SqlitePool, dir: PathBuf) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_hours(1));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            if let Some(Err(e)) = crate::supervise::tick("seleo sweep", sweep(&pool, &dir)).await {
                tracing::warn!(error = %e, "seleo: sweep failed");
            }
        }
    });
}

/// Fetch a film's TMDB poster into `posters/<id>.jpg`; answers its file
/// name, `None` when TMDB couldn't serve it.
pub async fn fetch_poster(tmdb: &TmdbClient, path: &str, dir: &Path, id: &str) -> Option<String> {
    let bytes = tmdb
        .image_bytes(path, POSTER_SIZE, POSTER_MAX_BYTES)
        .await?;
    let name = format!("{id}.jpg");
    let _one = WRITING.lock().await;
    let saved = async {
        ensure_dirs(dir).await?;
        write_atomic(&dir.join(POSTERS).join(&name), &bytes).await
    };
    match saved.await {
        Ok(()) => Some(name),
        Err(e) => {
            tracing::warn!(error = %e, "seleo: poster not saved");
            None
        }
    }
}

async fn remove_posters(dir: &Path, names: impl IntoIterator<Item = Option<String>>) {
    for name in names.into_iter().flatten() {
        // Our own `<infohash>-<idx>.jpg`; anything else is not ours to delete.
        if name.contains('/') || name.contains("..") {
            continue;
        }
        let path = dir.join(POSTERS).join(&name);
        if let Err(e) = tokio::fs::remove_file(&path).await
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(error = %e, poster = %name, "seleo: poster not deleted");
        }
    }
}

async fn ensure_dirs(dir: &Path) -> std::io::Result<()> {
    let posters = dir.join(POSTERS);
    tokio::fs::create_dir_all(&posters).await?;
    set_mode(dir, 0o755).await?;
    set_mode(&posters, 0o755).await
}

/// Write `bytes` beside `path`, readable by all, then rename it over `path`.
async fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    tokio::fs::write(&tmp, bytes).await?;
    set_mode(&tmp, 0o644).await?;
    tokio::fs::rename(&tmp, path).await
}

/// The mode is set, not left to the umask: `SéléoTV` reads as another uid.
#[cfg(unix)]
async fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).await
}

#[cfg(not(unix))]
async fn set_mode(_path: &Path, _mode: u32) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};
    use iris_db::seleo_shares::NewShare;
    use iris_db::test_support::{make_user, migrated_pool};

    async fn torrent(pool: &sqlx::SqlitePool, c: char) -> String {
        let by = make_user(pool).await;
        iris_db::torrents::upsert(
            pool,
            iris_db::torrents::NewTorrent {
                infohash: c.to_string().repeat(40),
                name: format!("Film.{c}.2019.1080p-GRP"),
                total_size_bytes: 1_000,
                source_provider: None,
                source_external_id: None,
                tracker_tmdb_id: None,
                added_by: by,
            },
        )
        .await
        .unwrap()
        .infohash
    }

    async fn share(pool: &sqlx::SqlitePool, infohash: &str, title: &str, poster: Option<&str>) {
        let by = make_user(pool).await;
        iris_db::seleo_shares::put(
            pool,
            &NewShare {
                infohash,
                file_idx: 0,
                shared_by: by,
                until: Utc::now() + Duration::days(7),
                path: "1917.2019.1080p-GRP/1917.mkv",
                size_bytes: 123,
                tmdb_id: Some(530_915),
                title,
                year: Some(2019),
                overview: Some("Deux soldats."),
                runtime_minutes: Some(119),
                poster,
            },
        )
        .await
        .unwrap();
    }

    fn temp_dir() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("iris-seleo-{}", uuid::Uuid::new_v4()))
    }

    fn read(dir: &std::path::Path) -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(dir.join(super::MANIFEST)).unwrap()).unwrap()
    }

    #[tokio::test]
    async fn the_manifest_lists_the_running_shares_newest_first_readable_by_all() {
        let pool = migrated_pool().await;
        let dir = temp_dir();
        let a = torrent(&pool, 'a').await;
        let b = torrent(&pool, 'b').await;
        share(&pool, &a, "1917", Some(&format!("{a}-0.jpg"))).await;
        share(&pool, &b, "Dune", None).await;

        assert_eq!(super::write_manifest(&pool, &dir).await.unwrap(), 2);
        let m = read(&dir);
        assert_eq!(m["version"], 1);
        assert!(m["generated_at"].as_str().unwrap().ends_with('Z'));
        let films = m["films"].as_array().unwrap();
        assert_eq!(films[0]["title"], "Dune");
        assert_eq!(films[0]["poster"], serde_json::Value::Null);
        let f = &films[1];
        assert_eq!(f["id"], format!("{a}-0"));
        assert_eq!(f["path"], "1917.2019.1080p-GRP/1917.mkv");
        assert_eq!(f["size"], 123);
        assert_eq!(f["tmdb_id"], 530_915);
        assert_eq!(f["year"], 2019);
        assert_eq!(f["runtime_minutes"], 119);
        assert_eq!(f["overview"], "Deux soldats.");
        assert_eq!(f["poster"], format!("posters/{a}-0.jpg"));
        let until = f["until"].as_str().unwrap();
        assert!(until.ends_with('Z') && !until.contains('.'), "{until}");
        assert!(chrono::DateTime::parse_from_rfc3339(until).is_ok());

        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [super::POSTERS, super::MANIFEST],
            "no temp file left"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode =
                |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&dir.join(super::MANIFEST)), 0o644);
            assert_eq!(mode(&dir), 0o755);
            assert_eq!(mode(&dir.join(super::POSTERS)), 0o755);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn the_sweep_drops_ended_shares_and_their_posters_from_the_manifest() {
        let pool = migrated_pool().await;
        let dir = temp_dir();
        let a = torrent(&pool, 'a').await;
        let b = torrent(&pool, 'b').await;
        let poster = format!("{a}-0.jpg");
        share(&pool, &a, "1917", Some(&poster)).await;
        share(&pool, &b, "Dune", None).await;
        super::ensure_dirs(&dir).await.unwrap();
        std::fs::write(dir.join(super::POSTERS).join(&poster), b"jpg").unwrap();
        super::write_manifest(&pool, &dir).await.unwrap();

        iris_db::seleo_shares::extend(&pool, &a, 0, Utc::now() - Duration::minutes(1))
            .await
            .unwrap();
        super::sweep(&pool, &dir).await.unwrap();
        let films = read(&dir)["films"].as_array().unwrap().clone();
        assert_eq!(films.len(), 1);
        assert_eq!(films[0]["title"], "Dune");
        assert!(!dir.join(super::POSTERS).join(&poster).exists());

        super::forget_torrent(&pool, &dir, &b).await;
        assert_eq!(read(&dir)["films"], serde_json::json!([]));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
