//! Hourly housekeeping of tables that only ever grow: sessions nobody can
//! use any more, expired pairing codes, and old capability telemetry.

use std::time::Duration;

use chrono::Utc;
use iris_db::SqlitePool;

/// A rotated session stays useful for the rotation grace window (seconds);
/// a day is far past it.
const SESSION_GRACE: chrono::Duration = chrono::Duration::days(1);
const CAPS_LOG_RETENTION: chrono::Duration = chrono::Duration::days(30);

pub fn spawn(pool: SqlitePool) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_hours(1));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            run_once(&pool).await;
        }
    });
}

async fn run_once(pool: &SqlitePool) {
    let now = Utc::now();
    let sessions = iris_db::refresh_tokens::prune(pool, now - SESSION_GRACE).await;
    let codes = iris_db::device_codes::cleanup_expired(pool).await;
    let caps = iris_db::playback_caps::prune(pool, now - CAPS_LOG_RETENTION).await;
    match (sessions, codes, caps) {
        (Ok(sessions), Ok(codes), Ok(caps)) => {
            tracing::debug!(sessions, codes, caps, "maintenance: pruned");
        }
        (sessions, codes, caps) => tracing::warn!(
            sessions = ?sessions.err(),
            codes = ?codes.err(),
            caps = ?caps.err(),
            "maintenance: prune failed"
        ),
    }
}
