//! `SQLite` persistence layer for Iris.

/// SQL for the `tmdb_verified` flag the API ships on watch rows, given the
/// torrent alias `$t` and its collection alias `$c`. A collection whose TMDB
/// match was evaluated (`tmdb_trust` set) only carries a trusted id, so the
/// flag is "has an id". A legacy row (`tmdb_trust` NULL, not yet through
/// `tmdb-trust --apply`) keeps the old per-torrent runtime check. A macro so
/// it stays a literal inside `concat!` (sqlx 0.9 only takes `&'static str`).
macro_rules! tmdb_verified_sql {
    ($t:literal, $c:literal) => {
        concat!(
            "(",
            $c,
            ".tmdb_id IS NOT NULL AND (",
            $c,
            ".tmdb_trust IS NOT NULL OR ",
            $t,
            ".tmdb_verified))"
        )
    };
}

/// SQL predicate keeping the `available_episodes` rows (column prefix `$p`)
/// whose tracker an admin has not switched off (`provider_overrides`). The
/// rows of a tracker turned off stay cached and come back when it is on again.
macro_rules! offer_tracker_on_sql {
    ($p:literal) => {
        concat!(
            $p,
            "indexer_provider NOT IN (SELECT provider_id FROM provider_overrides WHERE enabled = 0)"
        )
    };
}

pub mod audit;
pub mod available_episodes;
pub mod catalog;
pub mod collections;
pub mod device_codes;
pub mod episode_files;
pub mod follows;
pub mod invitations;
pub mod migrate;
pub mod passkeys;
pub mod playback;
pub mod playback_caps;
pub mod playback_preferences;
pub mod pool;
pub mod preferences;
pub mod provider_overrides;
pub mod pulse;
pub mod recent_searches;
pub mod reco_feedback;
pub mod refresh_tokens;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
pub mod tmdb_cache;
pub mod torrents;
pub mod users;

pub use pool::{Db, connect};
pub use sqlx::SqlitePool;
