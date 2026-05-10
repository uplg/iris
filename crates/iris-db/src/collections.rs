//! Library-side collections — logical grouping of one or more torrents
//! into a single library entity (typically a TV show, sometimes a movie
//! plus its extras).
//!
//! Identity comes from the SCENE-parsed filename. The
//! `parsed_title_normalized` column is the dedup key (lowercase,
//! punctuation-stripped, year-suffixed for movies). `tmdb_id` is pure
//! enrichment metadata: stored when known so the UI can pull a poster
//! / synopsis, but never trusted as identity. Indexers occasionally
//! mis-tag torrents (wrong TMDB id attached to the wrong file), and
//! letting that drive grouping produced collections whose display
//! title disagreed with the actual content. SCENE-first sidesteps that.
//!
//! See `migrations/0008_follows_collections_episodes.sql` for the
//! base table layout and `migrations/0009_collections_scene_first.sql`
//! for the index drop that made multiple collections per `tmdb_id`
//! legal (necessary fallout of demoting TMDB to enrichment).

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

/// Multiple collections may now share a `tmdb_id` (the unique index
/// was dropped in 0009 — see module docs). Callers that want "any
/// collection enriched with this id" use this; callers that want
/// "the canonical one" should use [`find_or_create`] instead and
/// match on the SCENE-parsed key.
pub async fn list_by_tmdb(
    pool: &SqlitePool,
    tmdb_id: i64,
) -> Result<Vec<CollectionRow>, sqlx::Error> {
    sqlx::query_as::<_, CollectionRow>(
        "SELECT id, tmdb_id, parsed_title_normalized, display_title, kind, created_at \
         FROM collections WHERE tmdb_id = ?1 ORDER BY created_at",
    )
    .bind(tmdb_id)
    .fetch_all(pool)
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

/// Every collection currently in the library. Used by the TMDB
/// backfill to walk the canonical SCENE-grouped entities directly,
/// rather than re-deriving the title from individual member torrents
/// (one of which could be poorly named and resolve to garbage).
pub async fn list_all(pool: &SqlitePool) -> Result<Vec<CollectionRow>, sqlx::Error> {
    sqlx::query_as::<_, CollectionRow>(
        "SELECT id, tmdb_id, parsed_title_normalized, display_title, kind, created_at \
         FROM collections \
         ORDER BY created_at",
    )
    .fetch_all(pool)
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

/// Find or create a collection keyed on the SCENE-parsed identity.
/// `normalized` is the year-suffixed-for-movies dedup key produced by
/// [`iris_media::filename::Parsed::collection_key`]. Idempotent.
pub async fn find_or_create(
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
    // Targetless ON CONFLICT — partial unique index on
    // (parsed_title_normalized, kind) WHERE parsed_title_normalized IS
    // NOT NULL can't be targeted directly in SQLite, and the targetless
    // form catches the only unique violation that can fire here (the
    // tmdb_id partial index was dropped in 0009).
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

/// Attach a `tmdb_id` to a collection for enrichment (poster /
/// synopsis lookups). No-op if the collection already has one set —
/// first writer wins so a later torrent with a different (and
/// possibly wrong) `tmdb_id` can't overwrite a known-good one.
pub async fn set_tmdb_id_if_missing(
    pool: &SqlitePool,
    id: Uuid,
    tmdb_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE collections SET tmdb_id = ?1 WHERE id = ?2 AND tmdb_id IS NULL",
    )
    .bind(tmdb_id)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Rewrite the human-readable display title on an existing collection.
/// Used by `tmdb_backfill` to repair rows the older filename parser
/// left with leaked tokens (`Silicon Valley S01 MULTI`, etc.). Only
/// touches `display_title` — `parsed_title_normalized` is the
/// SCENE-grouping key and changing it would risk colliding with the
/// `(parsed_title_normalized, kind)` unique index, so we leave that
/// column alone (any inconsistency is internal bookkeeping; the user-
/// visible name and the TMDB poster flow through `display_title` and
/// `tmdb_id` which we do rewrite).
pub async fn set_display_title(
    pool: &SqlitePool,
    id: Uuid,
    display_title: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE collections SET display_title = ?1 WHERE id = ?2")
        .bind(display_title)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Force-overwrite the collection's `tmdb_id`, regardless of what's
/// already there. Reserved for the backfill / migration path that
/// re-resolves torrents whose `tmdb_id` was originally set from the
/// indexer's (frequently wrong) value — the collection slot was
/// stamped with that same wrong value and the standard "first writer
/// wins" rule would block the correction. Live ingestion flows must
/// keep using [`set_tmdb_id_if_missing`].
pub async fn set_tmdb_id(
    pool: &SqlitePool,
    id: Uuid,
    tmdb_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE collections SET tmdb_id = ?1 WHERE id = ?2")
        .bind(tmdb_id)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
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
            (SELECT COUNT(*) FROM episode_files ef WHERE ef.collection_id = c.id) AS episode_count, \
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
