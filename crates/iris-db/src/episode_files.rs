//! Library-wide mapping `(collection_id, season, episode) → (infohash, file_idx)`.
//!
//! Populated at ingest time from SCENE filename parsing and by the
//! on-demand grab endpoint when a user explicitly asks "prépare E06".
//! NOT per-user — a file lives once on disk and any authorised user
//! can play it. The "did I watch it" stays in `playback_progress`.
//!
//! Keyed on `collection_id` (SCENE-parsed identity) rather than
//! `tmdb_id`. Indexers occasionally mis-tag torrents with the wrong
//! TMDB id; if the table were keyed on `tmdb_id`, those rogue rows
//! would surface as "downloaded" episodes on unrelated Watchlist
//! follows. Going through the collection — which is itself
//! SCENE-anchored — keeps each show's episode list honest.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct EpisodeFileRow {
    pub id: Uuid,
    pub collection_id: Uuid,
    pub season: i64,
    pub episode: i64,
    pub infohash: String,
    pub file_idx: i64,
    pub derived_from: String,
    pub created_at: DateTime<Utc>,
}

/// All known files for a collection. Used by the Series detail page
/// (resolved follow → collection → these rows) and by the collection
/// detail page directly.
pub async fn list_for_collection(
    pool: &SqlitePool,
    collection_id: Uuid,
) -> Result<Vec<EpisodeFileRow>, sqlx::Error> {
    sqlx::query_as::<_, EpisodeFileRow>(
        "SELECT id, collection_id, season, episode, infohash, file_idx, derived_from, created_at \
         FROM episode_files \
         WHERE collection_id = ?1 \
         ORDER BY season, episode",
    )
    .bind(collection_id)
    .fetch_all(pool)
    .await
}

/// Single-season variant — saves a sort+filter when the caller knows
/// they only need one season's worth of rows.
pub async fn list_for_collection_season(
    pool: &SqlitePool,
    collection_id: Uuid,
    season: i64,
) -> Result<Vec<EpisodeFileRow>, sqlx::Error> {
    sqlx::query_as::<_, EpisodeFileRow>(
        "SELECT id, collection_id, season, episode, infohash, file_idx, derived_from, created_at \
         FROM episode_files \
         WHERE collection_id = ?1 AND season = ?2 \
         ORDER BY episode",
    )
    .bind(collection_id)
    .bind(season)
    .fetch_all(pool)
    .await
}

/// Cross-collection lookup by TMDB id — every season. Same join
/// rationale as [`list_for_tmdb_season`]; used by the notify
/// scheduler to know what's already on disk before adding rows to
/// `available_episodes`.
pub async fn list_for_tmdb(
    pool: &SqlitePool,
    tmdb_id: i64,
) -> Result<Vec<EpisodeFileRow>, sqlx::Error> {
    sqlx::query_as::<_, EpisodeFileRow>(
        "SELECT ef.id, ef.collection_id, ef.season, ef.episode, ef.infohash, ef.file_idx, \
                ef.derived_from, ef.created_at \
         FROM episode_files ef \
         JOIN collections c ON c.id = ef.collection_id \
         WHERE c.tmdb_id = ?1 \
         ORDER BY ef.season, ef.episode",
    )
    .bind(tmdb_id)
    .fetch_all(pool)
    .await
}

/// Cross-collection lookup by TMDB id — joins through collections.
/// Returns rows from EVERY collection enriched with this `tmdb_id`
/// (multiple are now possible: a TMDB-tagged torrent and a SCENE-only
/// one may live in distinct collections that both happen to map to
/// the same show). Powers the Series page on a Watchlist follow.
pub async fn list_for_tmdb_season(
    pool: &SqlitePool,
    tmdb_id: i64,
    season: i64,
) -> Result<Vec<EpisodeFileRow>, sqlx::Error> {
    sqlx::query_as::<_, EpisodeFileRow>(
        "SELECT ef.id, ef.collection_id, ef.season, ef.episode, ef.infohash, ef.file_idx, \
                ef.derived_from, ef.created_at \
         FROM episode_files ef \
         JOIN collections c ON c.id = ef.collection_id \
         WHERE c.tmdb_id = ?1 AND ef.season = ?2 \
         ORDER BY ef.episode",
    )
    .bind(tmdb_id)
    .bind(season)
    .fetch_all(pool)
    .await
}

/// Lookup by physical file — used by the player's end-of-episode
/// "Préparer le suivant ?" flow to identify which episode just
/// finished playing.
pub async fn find_by_file(
    pool: &SqlitePool,
    infohash: &str,
    file_idx: i64,
) -> Result<Option<EpisodeFileRow>, sqlx::Error> {
    sqlx::query_as::<_, EpisodeFileRow>(
        "SELECT id, collection_id, season, episode, infohash, file_idx, derived_from, created_at \
         FROM episode_files \
         WHERE infohash = ?1 AND file_idx = ?2",
    )
    .bind(infohash)
    .bind(file_idx)
    .fetch_optional(pool)
    .await
}

#[derive(Debug, Clone)]
pub struct UpsertEpisodeFile {
    pub collection_id: Uuid,
    pub season: i64,
    pub episode: i64,
    pub infohash: String,
    pub file_idx: i64,
    pub derived_from: DerivedFrom,
}

#[derive(Debug, Clone, Copy)]
pub enum DerivedFrom {
    /// User-driven on-demand grab — we know S/E because the grab
    /// params said so, not because a filename matched.
    TmdbMatch,
    /// SCENE regex hit on the filename.
    SceneParse,
    /// Future: user assigned via admin UI.
    Manual,
}

impl DerivedFrom {
    fn as_str(self) -> &'static str {
        match self {
            DerivedFrom::TmdbMatch => "tmdb_match",
            DerivedFrom::SceneParse => "scene_parse",
            DerivedFrom::Manual => "manual",
        }
    }
}

/// Insert if `(infohash, file_idx)` isn't already claimed; do nothing
/// otherwise. Same physical file can't map to two different episodes
/// (the UNIQUE index in the migration enforces this), and re-running
/// the assignment job at ingest time should be a no-op.
pub async fn upsert(pool: &SqlitePool, ef: UpsertEpisodeFile) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO episode_files \
            (id, collection_id, season, episode, infohash, file_idx, derived_from, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
         ON CONFLICT(infohash, file_idx) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(ef.collection_id)
    .bind(ef.season)
    .bind(ef.episode)
    .bind(&ef.infohash)
    .bind(ef.file_idx)
    .bind(ef.derived_from.as_str())
    .bind(Utc::now())
    .execute(pool)
    .await?;
    Ok(())
}
