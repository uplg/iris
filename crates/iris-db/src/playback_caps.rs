//! Telemetry logger for `Iris-Caps` headers.
//!
//! See `migrations/0014_playback_caps_log.sql` for the schema.

use sqlx::SqlitePool;

/// Insert a single capability record. Failures are returned but the caller
/// is expected to log-and-drop them — the application MUST NOT fail
/// playback requests because telemetry hit a row lock.
pub async fn insert(
    pool: &SqlitePool,
    infohash: Option<&str>,
    file_idx: Option<i64>,
    route: Option<&str>,
    caps_json: &str,
    user_agent: Option<&str>,
    request_id: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO playback_caps_log
             (infohash, file_idx, route, caps_json, user_agent, request_id)
           VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(infohash)
    .bind(file_idx)
    .bind(route)
    .bind(caps_json)
    .bind(user_agent)
    .bind(request_id)
    .execute(pool)
    .await
    .map(|_| ())
}

/// Rows per prune statement: each one holds SQLite's write lock, and a
/// backlog of millions in one DELETE stalled every write for 12 s.
const PRUNE_BATCH: i64 = 20_000;

/// Drop records older than `before`: the log is a rolling telemetry window.
pub async fn prune(
    pool: &SqlitePool,
    before: chrono::DateTime<chrono::Utc>,
) -> Result<u64, sqlx::Error> {
    // `ts` is written by SQLite's own strftime, so compare in that format.
    let before = before.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    let mut total = 0;
    loop {
        let res = sqlx::query(
            "DELETE FROM playback_caps_log WHERE id IN
               (SELECT id FROM playback_caps_log WHERE ts < ?1 LIMIT ?2)",
        )
        .bind(&before)
        .bind(PRUNE_BATCH)
        .execute(pool)
        .await?;
        total += res.rows_affected();
        if res.rows_affected() < u64::try_from(PRUNE_BATCH).unwrap_or(u64::MAX) {
            return Ok(total);
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use crate::test_support::migrated_pool;

    #[tokio::test]
    async fn prune_compares_down_to_the_second() {
        let pool = migrated_pool().await;
        sqlx::query("INSERT INTO playback_caps_log (ts, caps_json) VALUES (?1, '{}')")
            .bind("2026-01-01T10:00:05.000Z")
            .execute(&pool)
            .await
            .unwrap();
        let before = chrono::Utc.with_ymd_and_hms(2026, 1, 1, 10, 0, 30).unwrap();
        assert_eq!(super::prune(&pool, before).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn prune_walks_a_backlog_larger_than_one_batch() {
        let pool = migrated_pool().await;
        sqlx::query(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 45000)
             INSERT INTO playback_caps_log (ts, caps_json) SELECT '2026-01-01T00:00:00.000Z', '{}' FROM n",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO playback_caps_log (ts, caps_json) VALUES ('2026-03-01T00:00:00.000Z', '{}')")
            .execute(&pool)
            .await
            .unwrap();
        let before = chrono::Utc.with_ymd_and_hms(2026, 2, 1, 0, 0, 0).unwrap();
        assert_eq!(super::prune(&pool, before).await.unwrap(), 45_000);
        let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM playback_caps_log")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(left, 1);
    }
}
