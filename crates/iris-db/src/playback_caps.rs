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

/// Drop records older than `before`: the log is a rolling telemetry window.
pub async fn prune(
    pool: &SqlitePool,
    before: chrono::DateTime<chrono::Utc>,
) -> Result<u64, sqlx::Error> {
    // `ts` is written by SQLite's own strftime, so compare in that format.
    let res = sqlx::query("DELETE FROM playback_caps_log WHERE ts < ?1")
        .bind(before.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
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
}
