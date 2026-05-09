//! Library-wide mapping `(tmdb_id, season, episode) → (infohash, file_idx)`.
//!
//! Populated at ingest time (TMDB matching + SCENE filename parsing) and
//! by the on-demand grab endpoint when a user explicitly asks "prépare
//! E06". NOT per-user — a file lives once on disk and any authorised
//! user can play it. The "did I watch it" stays in `playback_progress`.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct EpisodeFileRow {
    pub id: Uuid,
    pub tmdb_id: i64,
    pub season: i64,
    pub episode: i64,
    pub infohash: String,
    pub file_idx: i64,
    pub derived_from: String,
    pub created_at: DateTime<Utc>,
}

/// All known files for a series. Used by the Series detail page to render
/// "vu / téléchargé / dispo" status per episode.
pub async fn list_for_series(
    pool: &SqlitePool,
    tmdb_id: i64,
) -> Result<Vec<EpisodeFileRow>, sqlx::Error> {
    sqlx::query_as::<_, EpisodeFileRow>(
        "SELECT id, tmdb_id, season, episode, infohash, file_idx, derived_from, created_at \
         FROM episode_files \
         WHERE tmdb_id = ?1 \
         ORDER BY season, episode",
    )
    .bind(tmdb_id)
    .fetch_all(pool)
    .await
}

/// Single-season variant — saves a sort+filter when the caller knows
/// they only need one season's worth of rows.
pub async fn list_for_season(
    pool: &SqlitePool,
    tmdb_id: i64,
    season: i64,
) -> Result<Vec<EpisodeFileRow>, sqlx::Error> {
    sqlx::query_as::<_, EpisodeFileRow>(
        "SELECT id, tmdb_id, season, episode, infohash, file_idx, derived_from, created_at \
         FROM episode_files \
         WHERE tmdb_id = ?1 AND season = ?2 \
         ORDER BY episode",
    )
    .bind(tmdb_id)
    .bind(season)
    .fetch_all(pool)
    .await
}

#[derive(Debug, Clone)]
pub struct UpsertEpisodeFile {
    pub tmdb_id: i64,
    pub season: i64,
    pub episode: i64,
    pub infohash: String,
    pub file_idx: i64,
    pub derived_from: DerivedFrom,
}

#[derive(Debug, Clone, Copy)]
pub enum DerivedFrom {
    TmdbMatch,
    SceneParse,
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
            (id, tmdb_id, season, episode, infohash, file_idx, derived_from, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
         ON CONFLICT(infohash, file_idx) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(ef.tmdb_id)
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
