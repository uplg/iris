//! `SQLite` persistence layer for Iris.

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
