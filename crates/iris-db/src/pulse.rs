//! Discovery pulse: snapshots of external "what's hot" lists
//! (`pulse_signals`) and the tracker-join bookkeeping (`pulse_checks`).
//!
//! Lists are replaced wholesale per `(list, kind)` each cycle; the join itself
//! lands in `catalog_items` (only on a hit) through the freshness upsert path.

use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

/// One entry of an external list, as written by the pulse scheduler.
#[derive(Debug, Clone)]
pub struct NewSignal {
    pub tmdb_id: i64,
    pub watched: Option<i64>,
    pub title: String,
    pub release_date: Option<String>,
}

/// A stored list entry.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Signal {
    pub list: String,
    pub kind: String,
    pub tmdb_id: i64,
    pub rank: i64,
    pub watched: Option<i64>,
    pub title: String,
    pub release_date: Option<String>,
}

/// Replace the snapshot of one `(list, kind)`. Ranks follow `entries` order;
/// duplicate ids keep their first (hottest) position. An empty `entries` is a
/// failed fetch, not an empty list: the previous snapshot is kept.
pub async fn replace_list(
    pool: &SqlitePool,
    list: &str,
    kind: &str,
    entries: &[NewSignal],
) -> Result<(), sqlx::Error> {
    if entries.is_empty() {
        return Ok(());
    }
    let now = Utc::now();
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM pulse_signals WHERE list = ?1 AND kind = ?2")
        .bind(list)
        .bind(kind)
        .execute(&mut *tx)
        .await?;
    for (rank, e) in entries.iter().enumerate() {
        sqlx::query(
            "INSERT OR IGNORE INTO pulse_signals \
                (list, kind, tmdb_id, rank, watched, title, release_date, fetched_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .bind(list)
        .bind(kind)
        .bind(e.tmdb_id)
        .bind(i64::try_from(rank).unwrap_or(i64::MAX))
        .bind(e.watched)
        .bind(&e.title)
        .bind(&e.release_date)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await
}

/// Every stored signal whose list is `list` or, when `list` ends with `:`,
/// starts with it (`mood:` = every mood list).
pub async fn signals(pool: &SqlitePool, list: &str) -> Result<Vec<Signal>, sqlx::Error> {
    let sql = "SELECT list, kind, tmdb_id, rank, watched, title, release_date \
               FROM pulse_signals WHERE list = ?1 OR (substr(?1, -1) = ':' AND list LIKE ?1 || '%') \
               ORDER BY rank";
    sqlx::query_as::<_, Signal>(sql)
        .bind(list)
        .fetch_all(pool)
        .await
}

/// Every stored signal, all lists.
pub async fn all_signals(pool: &SqlitePool) -> Result<Vec<Signal>, sqlx::Error> {
    sqlx::query_as::<_, Signal>(
        "SELECT list, kind, tmdb_id, rank, watched, title, release_date \
         FROM pulse_signals ORDER BY rank",
    )
    .fetch_all(pool)
    .await
}

/// `(tmdb_id, kind)` pairs that need no tracker join right now: checked since
/// `hit_since` with a hit, or since `miss_since` without one.
pub async fn recently_checked(
    pool: &SqlitePool,
    hit_since: DateTime<Utc>,
    miss_since: DateTime<Utc>,
) -> Result<Vec<(i64, String)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT tmdb_id, kind FROM pulse_checks \
         WHERE (found = 1 AND checked_at >= ?1) OR (found = 0 AND checked_at >= ?2)",
    )
    .bind(hit_since)
    .bind(miss_since)
    .fetch_all(pool)
    .await
}

pub async fn record_check(
    pool: &SqlitePool,
    tmdb_id: i64,
    kind: &str,
    found: bool,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO pulse_checks (tmdb_id, kind, checked_at, found) VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT(tmdb_id, kind) DO UPDATE SET \
            checked_at = excluded.checked_at, found = excluded.found",
    )
    .bind(tmdb_id)
    .bind(kind)
    .bind(Utc::now())
    .bind(found)
    .execute(pool)
    .await?;
    Ok(())
}

/// Drop snapshots not refreshed since `before` (a list whose source stopped
/// answering) and join records older than it. Returns rows removed.
pub async fn prune(pool: &SqlitePool, before: DateTime<Utc>) -> Result<u64, sqlx::Error> {
    let a = sqlx::query("DELETE FROM pulse_signals WHERE fetched_at < ?1")
        .bind(before)
        .execute(pool)
        .await?;
    let b = sqlx::query("DELETE FROM pulse_checks WHERE checked_at < ?1")
        .bind(before)
        .execute(pool)
        .await?;
    Ok(a.rows_affected() + b.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn migrated_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("open in-memory sqlite");
        crate::migrate::run(&pool).await.expect("run migrations");
        pool
    }

    fn sig(tmdb_id: i64) -> NewSignal {
        NewSignal {
            tmdb_id,
            watched: None,
            title: format!("t{tmdb_id}"),
            release_date: None,
        }
    }

    #[tokio::test]
    async fn lists_replace_and_prefix_match() {
        let pool = migrated_pool().await;
        replace_list(&pool, "trending", "movie", &[sig(1), sig(2), sig(1)])
            .await
            .unwrap();
        replace_list(&pool, "mood:chills", "movie", &[sig(3)])
            .await
            .unwrap();
        replace_list(&pool, "mood:laughs", "tv", &[sig(4)])
            .await
            .unwrap();

        let trending = signals(&pool, "trending").await.unwrap();
        assert_eq!(
            trending
                .iter()
                .map(|s| (s.tmdb_id, s.rank))
                .collect::<Vec<_>>(),
            vec![(1, 0), (2, 1)],
            "duplicates keep their first rank"
        );
        assert_eq!(signals(&pool, "mood:").await.unwrap().len(), 2);
        assert_eq!(signals(&pool, "mood:chills").await.unwrap().len(), 1);

        replace_list(&pool, "trending", "movie", &[sig(9)])
            .await
            .unwrap();
        replace_list(&pool, "trending", "movie", &[]).await.unwrap();
        let trending = signals(&pool, "trending").await.unwrap();
        assert_eq!(trending.len(), 1, "replaced, and an empty fetch keeps it");
        assert_eq!(trending[0].tmdb_id, 9);
    }

    #[tokio::test]
    async fn checks_honour_hit_and_miss_ttls() {
        let pool = migrated_pool().await;
        record_check(&pool, 1, "movie", true).await.unwrap();
        record_check(&pool, 2, "movie", false).await.unwrap();
        let future = Utc::now() + chrono::Duration::hours(1);
        let past = Utc::now() - chrono::Duration::hours(1);
        assert_eq!(
            recently_checked(&pool, past, future).await.unwrap(),
            vec![(1, "movie".to_string())]
        );
        assert_eq!(recently_checked(&pool, past, past).await.unwrap().len(), 2);
        assert_eq!(prune(&pool, future).await.unwrap(), 2);
    }
}
