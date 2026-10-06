//! Hourly housekeeping of tables that only ever grow: sessions nobody can
//! use any more, expired pairing codes, old capability telemetry, stale
//! TMDB resolutions and offers no indexer scan returns any more.

use std::time::Duration;

use chrono::Utc;
use iris_db::SqlitePool;

/// A rotated session stays useful for the rotation grace window (seconds);
/// a day is far past it.
const SESSION_GRACE: chrono::Duration = chrono::Duration::days(1);
const CAPS_LOG_RETENTION: chrono::Duration = chrono::Duration::days(30);
/// Must stay at or past `tmdb_resolve`'s cache max age (30 days): older rows
/// are never read again.
const TMDB_RESOLVE_RETENTION: chrono::Duration = chrono::Duration::days(31);
/// An offer every scan has missed for this long left its tracker.
const OFFER_RETENTION: chrono::Duration = chrono::Duration::days(30);

pub fn spawn(pool: SqlitePool) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_hours(1));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            crate::supervise::tick("maintenance", run_once(&pool)).await;
        }
    });
}

async fn run_once(pool: &SqlitePool) {
    let now = Utc::now();
    let sessions = iris_db::refresh_tokens::prune(pool, now - SESSION_GRACE).await;
    let codes = iris_db::device_codes::cleanup_expired(pool).await;
    let caps = iris_db::playback_caps::prune(pool, now - CAPS_LOG_RETENTION).await;
    let tmdb = iris_db::tmdb_cache::prune(pool, now - TMDB_RESOLVE_RETENTION).await;
    let offers = iris_db::available_episodes::prune_unseen(pool, now - OFFER_RETENTION).await;
    match (sessions, codes, caps, tmdb, offers) {
        (Ok(sessions), Ok(codes), Ok(caps), Ok(tmdb), Ok(offers)) => {
            tracing::debug!(sessions, codes, caps, tmdb, offers, "maintenance: pruned");
        }
        (sessions, codes, caps, tmdb, offers) => tracing::warn!(
            sessions = ?sessions.err(),
            codes = ?codes.err(),
            caps = ?caps.err(),
            tmdb = ?tmdb.err(),
            offers = ?offers.err(),
            "maintenance: prune failed"
        ),
    }
}

/// `VACUUM` the database at `db` (and fold its WAL back in), answering its
/// size before and after. For an operator after a big prune: never run by the
/// server, since it holds the write lock for as long as the rebuild takes.
///
/// # Errors
/// The database can't be opened, or SQLite refuses the vacuum (another
/// writer holds the lock past the busy timeout).
pub async fn vacuum(db: &std::path::Path) -> anyhow::Result<(u64, u64)> {
    anyhow::ensure!(db.exists(), "database {} not found", db.display());
    let size = || {
        ["", "-wal"]
            .iter()
            .filter_map(|suffix| {
                let mut p = db.as_os_str().to_owned();
                p.push(suffix);
                std::fs::metadata(p).ok()
            })
            .map(|m| m.len())
            .sum::<u64>()
    };
    let before = size();
    let pool = iris_db::connect(db).await?;
    sqlx::query("VACUUM").execute(&pool).await?;
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(&pool)
        .await?;
    pool.close().await;
    Ok((before, size()))
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn vacuum_hands_back_the_space_a_prune_freed() {
        let dir = std::env::temp_dir().join(format!("iris-vacuum-{}", uuid::Uuid::new_v4()));
        let db = dir.join("iris.db");
        let pool = iris_db::connect(&db).await.unwrap();
        sqlx::query("CREATE TABLE junk (blob BLOB)")
            .execute(&pool)
            .await
            .unwrap();
        for _ in 0..64 {
            sqlx::query("INSERT INTO junk VALUES (zeroblob(65536))")
                .execute(&pool)
                .await
                .unwrap();
        }
        sqlx::query("DELETE FROM junk")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
        let (before, after) = super::vacuum(&db).await.unwrap();
        assert!(after < before / 2, "{before} -> {after}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
