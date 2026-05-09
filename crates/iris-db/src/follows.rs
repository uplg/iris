//! Per-user series follows. Each row is "user X is watching TMDB show Y".
//! Drives the Watchlist shelf and tells the notify scheduler which shows
//! to poll for new episodes.

use chrono::{DateTime, Utc};
use iris_core::ids::UserId;
use serde::Serialize;
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct FollowRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub tmdb_id: i64,
    pub name: String,
    pub total_seasons: Option<i64>,
    pub last_checked_at: Option<DateTime<Utc>>,
    pub last_visited_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

pub async fn list_for_user(
    pool: &SqlitePool,
    user_id: UserId,
) -> Result<Vec<FollowRow>, sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query_as::<_, FollowRow>(
        "SELECT id, user_id, tmdb_id, name, total_seasons, last_checked_at, \
                last_visited_at, created_at \
         FROM series_follows \
         WHERE user_id = ?1 \
         ORDER BY created_at DESC",
    )
    .bind(user)
    .fetch_all(pool)
    .await
}

pub async fn get(
    pool: &SqlitePool,
    user_id: UserId,
    tmdb_id: i64,
) -> Result<Option<FollowRow>, sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query_as::<_, FollowRow>(
        "SELECT id, user_id, tmdb_id, name, total_seasons, last_checked_at, \
                last_visited_at, created_at \
         FROM series_follows \
         WHERE user_id = ?1 AND tmdb_id = ?2",
    )
    .bind(user)
    .bind(tmdb_id)
    .fetch_optional(pool)
    .await
}

/// Idempotent insert. If the user already follows this show, returns the
/// existing row unchanged (no-op on the snapshot fields — we don't
/// retroactively rename someone's follow when TMDB renames the show).
pub async fn add(
    pool: &SqlitePool,
    user_id: UserId,
    tmdb_id: i64,
    name: &str,
    total_seasons: Option<i64>,
) -> Result<FollowRow, sqlx::Error> {
    let user: Uuid = user_id.into();
    let id = Uuid::new_v4();
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO series_follows (id, user_id, tmdb_id, name, total_seasons, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT(user_id, tmdb_id) DO NOTHING",
    )
    .bind(id)
    .bind(user)
    .bind(tmdb_id)
    .bind(name)
    .bind(total_seasons)
    .bind(now)
    .execute(pool)
    .await?;
    // Read-back regardless of whether we just inserted or hit the conflict.
    // Caller always wants the row that "is now in the table".
    get(pool, user_id, tmdb_id)
        .await?
        .ok_or_else(|| sqlx::Error::RowNotFound)
}

pub async fn delete(
    pool: &SqlitePool,
    user_id: UserId,
    tmdb_id: i64,
) -> Result<bool, sqlx::Error> {
    let user: Uuid = user_id.into();
    let res = sqlx::query("DELETE FROM series_follows WHERE user_id = ?1 AND tmdb_id = ?2")
        .bind(user)
        .bind(tmdb_id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Bumped every time the user opens the series detail page. The
/// "X nouveaux" badge on Watchlist cards counts available episodes whose
/// `found_at` is newer than this timestamp.
pub async fn mark_visited(
    pool: &SqlitePool,
    user_id: UserId,
    tmdb_id: i64,
) -> Result<(), sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query(
        "UPDATE series_follows SET last_visited_at = ?3 \
         WHERE user_id = ?1 AND tmdb_id = ?2",
    )
    .bind(user)
    .bind(tmdb_id)
    .bind(Utc::now())
    .execute(pool)
    .await?;
    Ok(())
}

/// Bumped by the notify scheduler after each TMDB+indexer pass for this
/// follow. Used by the scheduler itself to skip recently-checked follows
/// (cheap pacing without tracking interval state separately).
pub async fn mark_checked(
    pool: &SqlitePool,
    user_id: UserId,
    tmdb_id: i64,
) -> Result<(), sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query(
        "UPDATE series_follows SET last_checked_at = ?3 \
         WHERE user_id = ?1 AND tmdb_id = ?2",
    )
    .bind(user)
    .bind(tmdb_id)
    .bind(Utc::now())
    .execute(pool)
    .await?;
    Ok(())
}
