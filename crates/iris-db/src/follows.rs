//! Per-user series follows.
//!
//! Identity is the SCENE-normalised name — the same string that
//! anchors a `collections` row's `parsed_title_normalized`. This
//! gives us one source of truth for "what show is this": the
//! filename. TMDB id is stored when known but treated as pure
//! decoration (poster lookup conditional on a verified collection
//! match).

use chrono::{DateTime, Utc};
use iris_core::ids::UserId;
use serde::Serialize;
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct FollowRow {
    pub id: Uuid,
    pub user_id: Uuid,
    /// SCENE-normalised name (lowercased, punctuation-stripped,
    /// single-spaced). The join key against
    /// `collections.parsed_title_normalized` and
    /// `available_episodes.normalized_name`.
    pub normalized_name: String,
    /// User-facing display name (typically what the user clicked
    /// in Discovery / Search). Also used as the indexer search
    /// query inside the notify scheduler.
    pub name: String,
    /// Decoration-only TMDB id; surfaces a poster when the
    /// collection joining via normalised name is `tmdb_verified`.
    pub tmdb_id: Option<i64>,
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
        "SELECT id, user_id, normalized_name, name, tmdb_id, last_checked_at, \
                last_visited_at, created_at \
         FROM series_follows \
         WHERE user_id = ?1 \
         ORDER BY created_at DESC",
    )
    .bind(user)
    .fetch_all(pool)
    .await
}

pub async fn get_by_id(
    pool: &SqlitePool,
    user_id: UserId,
    id: Uuid,
) -> Result<Option<FollowRow>, sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query_as::<_, FollowRow>(
        "SELECT id, user_id, normalized_name, name, tmdb_id, last_checked_at, \
                last_visited_at, created_at \
         FROM series_follows \
         WHERE user_id = ?1 AND id = ?2",
    )
    .bind(user)
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn get_by_normalized(
    pool: &SqlitePool,
    user_id: UserId,
    normalized_name: &str,
) -> Result<Option<FollowRow>, sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query_as::<_, FollowRow>(
        "SELECT id, user_id, normalized_name, name, tmdb_id, last_checked_at, \
                last_visited_at, created_at \
         FROM series_follows \
         WHERE user_id = ?1 AND normalized_name = ?2",
    )
    .bind(user)
    .bind(normalized_name)
    .fetch_optional(pool)
    .await
}

/// Idempotent insert keyed on `(user_id, normalized_name)`. Returns
/// the existing row when one already matches, otherwise inserts and
/// returns the new row.
pub async fn add(
    pool: &SqlitePool,
    user_id: UserId,
    normalized_name: &str,
    name: &str,
    tmdb_id: Option<i64>,
) -> Result<FollowRow, sqlx::Error> {
    let user: Uuid = user_id.into();
    let id = Uuid::new_v4();
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO series_follows (id, user_id, normalized_name, name, tmdb_id, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT(user_id, normalized_name) DO NOTHING",
    )
    .bind(id)
    .bind(user)
    .bind(normalized_name)
    .bind(name)
    .bind(tmdb_id)
    .bind(now)
    .execute(pool)
    .await?;
    get_by_normalized(pool, user_id, normalized_name)
        .await?
        .ok_or(sqlx::Error::RowNotFound)
}

pub async fn delete(pool: &SqlitePool, user_id: UserId, id: Uuid) -> Result<bool, sqlx::Error> {
    let user: Uuid = user_id.into();
    let res = sqlx::query("DELETE FROM series_follows WHERE user_id = ?1 AND id = ?2")
        .bind(user)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Bumped every time the user opens the series detail page.
pub async fn mark_visited(pool: &SqlitePool, user_id: UserId, id: Uuid) -> Result<(), sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query(
        "UPDATE series_follows SET last_visited_at = ?3 \
         WHERE user_id = ?1 AND id = ?2",
    )
    .bind(user)
    .bind(id)
    .bind(Utc::now())
    .execute(pool)
    .await?;
    Ok(())
}

/// Bumped by the notify scheduler after each indexer pass.
pub async fn mark_checked(pool: &SqlitePool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE series_follows SET last_checked_at = ?2 WHERE id = ?1")
        .bind(id)
        .bind(Utc::now())
        .execute(pool)
        .await?;
    Ok(())
}
