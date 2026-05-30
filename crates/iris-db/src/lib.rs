//! `SQLite` persistence layer for Iris.

pub mod available_episodes;
pub mod catalog;
pub mod collections;
pub mod device_codes;
pub mod episode_files;
pub mod follows;
pub mod invitations;
pub mod migrate;
pub mod playback;
pub mod playback_caps;
pub mod pool;
pub mod preferences;
pub mod reco_feedback;
pub mod refresh_tokens;
pub mod tmdb_cache;
pub mod torrents;
pub mod users;

pub use pool::{Db, connect};
pub use sqlx::SqlitePool;
