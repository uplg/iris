//! Persistent cache for `tmdb_search` lookups by SCENE-cleaned title.
//!
//! Powers the ingestion-time `tmdb_id` override and the search-page
//! poster resolution. Hot path is `get(cleaned, kind)` — one indexed
//! lookup, no JSON parsing. Misses fall through to the live TMDB
//! client; the result (or a `NotFound` sentinel) gets `put` back here
//! so subsequent calls within the TTL window short-circuit.

use chrono::{DateTime, Duration, Utc};
use sqlx::SqlitePool;

/// `(tmdb_id, title, year, poster_path, backdrop_path, overview, fetched_at)`
/// — kept as a tuple alias because sqlx's `query_as` needs the full
/// column shape at the call site, and breaking the row apart would
/// split a tightly-coupled contract.
type Row = (
    Option<i64>,
    Option<String>,
    Option<i64>,
    Option<String>,
    Option<String>,
    Option<String>,
    DateTime<Utc>,
);

#[derive(Debug, Clone)]
pub struct ResolveEntry {
    pub tmdb_id: Option<i64>,
    pub title: Option<String>,
    pub year: Option<i64>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub overview: Option<String>,
    pub fetched_at: DateTime<Utc>,
}

impl ResolveEntry {
    /// Negative-cache sentinel — TMDB returned nothing for this name.
    pub fn not_found_at(now: DateTime<Utc>) -> Self {
        Self {
            tmdb_id: None,
            title: None,
            year: None,
            poster_path: None,
            backdrop_path: None,
            overview: None,
            fetched_at: now,
        }
    }
}

/// Read a cached resolution. Returns `None` either when there's no row
/// at all, or when the row is older than `max_age`. Negative cache
/// entries (resolved as not-found upstream) come back as `Some(entry)`
/// with `tmdb_id == None` — callers should treat that as "we already
/// know there's nothing, don't re-issue".
pub async fn get(
    pool: &SqlitePool,
    cleaned_name: &str,
    kind_hint: Option<&str>,
    max_age: Duration,
) -> Result<Option<ResolveEntry>, sqlx::Error> {
    let cutoff = Utc::now() - max_age;
    let row: Option<Row> = sqlx::query_as(
        "SELECT tmdb_id, title, year, poster_path, backdrop_path, overview, fetched_at \
         FROM tmdb_resolve_cache \
         WHERE cleaned_name = ?1 AND (kind_hint IS ?2 OR kind_hint = ?2) \
           AND fetched_at >= ?3",
    )
    .bind(cleaned_name)
    .bind(kind_hint)
    .bind(cutoff)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| ResolveEntry {
        tmdb_id: r.0,
        title: r.1,
        year: r.2,
        poster_path: r.3,
        backdrop_path: r.4,
        overview: r.5,
        fetched_at: r.6,
    }))
}

/// Insert or replace the resolution row. Idempotent — overwrites any
/// existing entry for the same `(cleaned_name, kind_hint)` pair, so
/// calling this is also the way to refresh a stale entry.
pub async fn put(
    pool: &SqlitePool,
    cleaned_name: &str,
    kind_hint: Option<&str>,
    entry: &ResolveEntry,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO tmdb_resolve_cache \
           (cleaned_name, kind_hint, tmdb_id, title, year, \
            poster_path, backdrop_path, overview, fetched_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
         ON CONFLICT(cleaned_name, kind_hint) DO UPDATE SET \
           tmdb_id = excluded.tmdb_id, \
           title = excluded.title, \
           year = excluded.year, \
           poster_path = excluded.poster_path, \
           backdrop_path = excluded.backdrop_path, \
           overview = excluded.overview, \
           fetched_at = excluded.fetched_at",
    )
    .bind(cleaned_name)
    .bind(kind_hint)
    .bind(entry.tmdb_id)
    .bind(entry.title.as_deref())
    .bind(entry.year)
    .bind(entry.poster_path.as_deref())
    .bind(entry.backdrop_path.as_deref())
    .bind(entry.overview.as_deref())
    .bind(entry.fetched_at)
    .execute(pool)
    .await?;
    Ok(())
}
