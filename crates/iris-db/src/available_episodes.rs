//! Indexer pre-cache for episodes that aren't in the library yet.
//! Populated by the notify scheduler so a user's "Préparer E06"
//! click doesn't have to wait on a fresh indexer round-trip.
//!
//! Keyed on `normalized_name` (the SCENE-style normalised title)
//! to match the way `series_follows` is identified — TMDB id is
//! never used as a join key here.
//!
//! NOT per-user: if iris knows a SCENE-named series has S03E12
//! grabbable, every authorised user sees the same offer.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct AvailableEpisodeRow {
    pub id: Uuid,
    pub normalized_name: String,
    pub season: i64,
    pub episode: i64,
    pub indexer_provider: String,
    pub indexer_torrent_id: String,
    pub magnet: String,
    pub quality: Option<String>,
    pub seeders: Option<i64>,
    pub size_bytes: Option<i64>,
    pub found_at: DateTime<Utc>,
}

/// Best-quality offer per `(normalized_name, season, episode)`.
/// When multiple torrents are cached for the same episode, prefer
/// the one with most seeders. Returns one row per episode at most.
pub async fn list_best_for_series(
    pool: &SqlitePool,
    normalized_name: &str,
) -> Result<Vec<AvailableEpisodeRow>, sqlx::Error> {
    sqlx::query_as::<_, AvailableEpisodeRow>(
        "SELECT id, normalized_name, season, episode, indexer_provider, indexer_torrent_id, \
                magnet, quality, seeders, size_bytes, found_at \
         FROM available_episodes a \
         WHERE normalized_name = ?1 \
           AND seeders IS (SELECT MAX(seeders) FROM available_episodes \
                           WHERE normalized_name = a.normalized_name \
                             AND season = a.season \
                             AND episode = a.episode) \
         GROUP BY normalized_name, season, episode \
         ORDER BY season, episode",
    )
    .bind(normalized_name)
    .fetch_all(pool)
    .await
}

/// Count of distinct episodes whose `found_at` is newer than
/// `since`. Drives the "X nouveaux" badge on Watchlist cards.
pub async fn count_new_for_series(
    pool: &SqlitePool,
    normalized_name: &str,
    since: Option<DateTime<Utc>>,
) -> Result<i64, sqlx::Error> {
    let cutoff = since.unwrap_or_else(|| {
        // No prior visit means "everything currently available is
        // new". Old enough timestamp to catch the lot.
        DateTime::<Utc>::from_timestamp(0, 0).unwrap()
    });
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(DISTINCT season || '-' || episode) FROM available_episodes \
         WHERE normalized_name = ?1 AND found_at > ?2",
    )
    .bind(normalized_name)
    .bind(cutoff)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

#[derive(Debug, Clone)]
pub struct UpsertAvailableEpisode {
    pub normalized_name: String,
    pub season: i64,
    pub episode: i64,
    pub indexer_provider: String,
    pub indexer_torrent_id: String,
    pub magnet: String,
    pub quality: Option<String>,
    pub seeders: Option<i64>,
    pub size_bytes: Option<i64>,
}

pub async fn upsert(
    pool: &SqlitePool,
    a: UpsertAvailableEpisode,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO available_episodes \
            (id, normalized_name, season, episode, indexer_provider, indexer_torrent_id, \
             magnet, quality, seeders, size_bytes, found_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11) \
         ON CONFLICT(normalized_name, season, episode, indexer_provider, indexer_torrent_id) \
         DO UPDATE SET \
            magnet     = excluded.magnet, \
            quality    = excluded.quality, \
            seeders    = excluded.seeders, \
            size_bytes = excluded.size_bytes, \
            found_at   = excluded.found_at",
    )
    .bind(Uuid::new_v4())
    .bind(&a.normalized_name)
    .bind(a.season)
    .bind(a.episode)
    .bind(&a.indexer_provider)
    .bind(&a.indexer_torrent_id)
    .bind(&a.magnet)
    .bind(a.quality)
    .bind(a.seeders)
    .bind(a.size_bytes)
    .bind(Utc::now())
    .execute(pool)
    .await?;
    Ok(())
}
