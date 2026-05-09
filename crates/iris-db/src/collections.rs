//! Library-side collections — logical grouping of one or more torrents
//! into a single library entity (typically a TV show, sometimes a movie
//! plus its extras). Two paths to identity:
//!
//!   * `tmdb_id` — preferred when present, populated by the
//!     ingest-time matcher.
//!   * `parsed_title_normalized` — fallback for SCENE-named multi-
//!     torrent series with no TMDB hit.
//!
//! See `migrations/0008_follows_collections_episodes.sql` for the
//! table layout.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct CollectionRow {
    pub id: Uuid,
    pub tmdb_id: Option<i64>,
    pub parsed_title_normalized: Option<String>,
    pub display_title: String,
    pub kind: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Tv,
    Movie,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Tv => "tv",
            Kind::Movie => "movie",
        }
    }
}

pub async fn find_by_tmdb(
    pool: &SqlitePool,
    tmdb_id: i64,
) -> Result<Option<CollectionRow>, sqlx::Error> {
    sqlx::query_as::<_, CollectionRow>(
        "SELECT id, tmdb_id, parsed_title_normalized, display_title, kind, created_at \
         FROM collections WHERE tmdb_id = ?1",
    )
    .bind(tmdb_id)
    .fetch_optional(pool)
    .await
}

pub async fn find_by_parsed_title(
    pool: &SqlitePool,
    normalized: &str,
    kind: Kind,
) -> Result<Option<CollectionRow>, sqlx::Error> {
    sqlx::query_as::<_, CollectionRow>(
        "SELECT id, tmdb_id, parsed_title_normalized, display_title, kind, created_at \
         FROM collections \
         WHERE parsed_title_normalized = ?1 AND kind = ?2",
    )
    .bind(normalized)
    .bind(kind.as_str())
    .fetch_optional(pool)
    .await
}

pub async fn get(pool: &SqlitePool, id: Uuid) -> Result<Option<CollectionRow>, sqlx::Error> {
    sqlx::query_as::<_, CollectionRow>(
        "SELECT id, tmdb_id, parsed_title_normalized, display_title, kind, created_at \
         FROM collections WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

/// Idempotent: returns the existing row when one already matches by
/// `tmdb_id`, otherwise inserts and returns the new row. Useful at
/// ingest time when we know the TMDB id from the search hit.
pub async fn find_or_create_by_tmdb(
    pool: &SqlitePool,
    tmdb_id: i64,
    kind: Kind,
    display_title: &str,
) -> Result<CollectionRow, sqlx::Error> {
    if let Some(existing) = find_by_tmdb(pool, tmdb_id).await? {
        return Ok(existing);
    }
    let id = Uuid::new_v4();
    let now = Utc::now();
    // ON CONFLICT WITHOUT a target — SQLite's targeted form requires the
    // WHERE clause of the matching unique index, and our `collections_*`
    // indexes are partial (`WHERE tmdb_id IS NOT NULL` /
    // `WHERE parsed_title_normalized IS NOT NULL`). Targeting them with
    // `ON CONFLICT(tmdb_id) DO NOTHING` fails to plan with "ON CONFLICT
    // clause does not match any PRIMARY KEY or UNIQUE constraint".
    // The targetless form catches any unique violation, which here can
    // only be the partial index we care about anyway (find-first happens
    // above, so this only triggers on a tight race).
    sqlx::query(
        "INSERT INTO collections (id, tmdb_id, parsed_title_normalized, display_title, kind, created_at) \
         VALUES (?1, ?2, NULL, ?3, ?4, ?5) \
         ON CONFLICT DO NOTHING",
    )
    .bind(id)
    .bind(tmdb_id)
    .bind(display_title)
    .bind(kind.as_str())
    .bind(now)
    .execute(pool)
    .await?;
    find_by_tmdb(pool, tmdb_id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)
}

pub async fn find_or_create_by_parsed_title(
    pool: &SqlitePool,
    normalized: &str,
    display_title: &str,
    kind: Kind,
) -> Result<CollectionRow, sqlx::Error> {
    if let Some(existing) = find_by_parsed_title(pool, normalized, kind).await? {
        return Ok(existing);
    }
    let id = Uuid::new_v4();
    let now = Utc::now();
    // Targetless ON CONFLICT — see find_or_create_by_tmdb for the rationale
    // (partial unique index can't be targeted directly).
    sqlx::query(
        "INSERT INTO collections (id, tmdb_id, parsed_title_normalized, display_title, kind, created_at) \
         VALUES (?1, NULL, ?2, ?3, ?4, ?5) \
         ON CONFLICT DO NOTHING",
    )
    .bind(id)
    .bind(normalized)
    .bind(display_title)
    .bind(kind.as_str())
    .bind(now)
    .execute(pool)
    .await?;
    find_by_parsed_title(pool, normalized, kind)
        .await?
        .ok_or(sqlx::Error::RowNotFound)
}

/// Standalone collection — used when neither TMDB id nor a parseable
/// SCENE title is available. Each call inserts a fresh row (no dedup),
/// since "no identity" by definition can't merge.
pub async fn create_standalone(
    pool: &SqlitePool,
    display_title: &str,
    kind: Kind,
) -> Result<CollectionRow, sqlx::Error> {
    let id = Uuid::new_v4();
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO collections (id, tmdb_id, parsed_title_normalized, display_title, kind, created_at) \
         VALUES (?1, NULL, NULL, ?2, ?3, ?4)",
    )
    .bind(id)
    .bind(display_title)
    .bind(kind.as_str())
    .bind(now)
    .execute(pool)
    .await?;
    get(pool, id).await?.ok_or(sqlx::Error::RowNotFound)
}

/// Aggregate row for the library list — collection metadata + summary
/// stats joined from `torrents` and `episode_files`. Used by
/// `GET /api/library?view=collections`.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct CollectionSummary {
    pub id: Uuid,
    pub tmdb_id: Option<i64>,
    pub display_title: String,
    pub kind: String,
    pub created_at: DateTime<Utc>,
    pub torrent_count: i64,
    pub total_size_bytes: i64,
    pub episode_count: i64,
    /// Any one of the collection's torrents — used by clients as a
    /// fallback navigation target when the rich routing path (TMDB →
    /// Series page) doesn't apply. Picks the most-recently-played
    /// torrent so a `/watch/<infohash>/0` jump tends to land somewhere
    /// the user actually wants to be.
    pub representative_infohash: Option<String>,
}

pub async fn list_summaries(
    pool: &SqlitePool,
) -> Result<Vec<CollectionSummary>, sqlx::Error> {
    sqlx::query_as::<_, CollectionSummary>(
        "SELECT \
            c.id, c.tmdb_id, c.display_title, c.kind, c.created_at, \
            COUNT(DISTINCT t.id) AS torrent_count, \
            COALESCE(SUM(t.total_size_bytes), 0) AS total_size_bytes, \
            (SELECT COUNT(*) FROM episode_files ef WHERE ef.tmdb_id = c.tmdb_id) AS episode_count, \
            (SELECT t2.infohash FROM torrents t2 \
             WHERE t2.collection_id = c.id AND t2.deleted_at IS NULL \
             ORDER BY COALESCE(t2.last_played_at, t2.added_at) DESC LIMIT 1) AS representative_infohash \
         FROM collections c \
         LEFT JOIN torrents t ON t.collection_id = c.id AND t.deleted_at IS NULL \
         GROUP BY c.id \
         HAVING torrent_count > 0 \
         ORDER BY MAX(t.last_played_at) DESC NULLS LAST, c.created_at DESC",
    )
    .fetch_all(pool)
    .await
}
