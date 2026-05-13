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
