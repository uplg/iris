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
