//! Shared recommendation catalogue (`catalog_items`).
//!
//! TMDB / `AniList` define the candidate universe; trackers are the
//! availability layer. The reco scheduler upserts candidates here
//! (`availability = 'unknown'`), a later pass flips them to 'available'
//! once a provider can serve them, and `reco.rs` queries this table per
//! user at request time. The table is household-shared — all per-user
//! filtering happens in the query, never at write time.

use chrono::{DateTime, Utc};
use iris_core::ids::UserId;
use sqlx::SqlitePool;
use uuid::Uuid;

/// Insert/update payload for a catalogue candidate.
#[derive(Debug, Clone)]
pub struct NewCatalogItem {
    pub tmdb_id: Option<i64>,
    pub anilist_id: Option<i64>,
    /// `"movie"` | `"tv"`.
    pub kind: String,
    pub title: String,
    /// TMDB ISO 639-1 code ("fr"/"en"/…).
    pub original_language: Option<String>,
    /// TMDB genre ids.
    pub genres: Vec<i64>,
    pub is_anime: bool,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub overview: Option<String>,
    pub popularity: Option<f64>,
    pub vote_average: Option<f64>,
    pub release_date: Option<String>,
    /// Which slice produced this row (e.g. `tmdb:trending`,
    /// `tmdb:discover:fr`) — diagnostic only.
    pub source: Option<String>,
}

/// A catalogue row as queried for the recommendation shelves.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CatalogItem {
    pub id: Uuid,
    pub tmdb_id: Option<i64>,
    pub anilist_id: Option<i64>,
    pub kind: String,
    pub title: String,
    pub original_language: Option<String>,
    /// JSON array of TMDB genre ids (as stored).
    pub genres: String,
    pub is_anime: bool,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub overview: Option<String>,
    pub popularity: Option<f64>,
    pub vote_average: Option<f64>,
    pub release_date: Option<String>,
    pub availability: String,
    pub available_provider: Option<String>,
}

/// Column list for `CatalogItem` reads — shared so the row queries can't
/// drift from the struct's `FromRow` field order.
const SELECT_COLUMNS: &str = "id, tmdb_id, anilist_id, kind, title, original_language, genres, \
     is_anime, poster_path, backdrop_path, overview, popularity, vote_average, release_date, \
     availability, available_provider";

/// Ordering for a catalogue query.
#[derive(Debug, Clone, Copy, Default)]
pub enum CatalogOrder {
    /// Most popular first (general shelves).
    #[default]
    Popularity,
    /// Most recently added to the catalogue first.
    FirstSeen,
    /// Newest release / air date first (freshest content).
    ReleaseDate,
}

/// Per-request catalogue filter. Empty `languages` = any language.
#[derive(Debug, Clone, Default)]
pub struct CatalogQuery {
    pub kind: Option<String>,
    /// TMDB ISO 639-1 codes to allow; empty = no language filter.
    pub languages: Vec<String>,
    pub is_anime: Option<bool>,
    /// When true, only rows a provider can actually serve.
    pub only_available: bool,
    /// When set, only rows whose `genres` JSON contains this TMDB genre id.
    pub genre: Option<i64>,
    pub order: CatalogOrder,
    pub limit: i64,
}

/// Insert a candidate, or refresh the mutable fields of an existing one.
/// `first_seen_at` and the availability columns are preserved across
/// upserts — discovery never clobbers a tracker-confirmed availability.
pub async fn upsert_item(pool: &SqlitePool, item: &NewCatalogItem) -> Result<(), sqlx::Error> {
    let id = Uuid::new_v4();
    let now = Utc::now();
    let genres = serde_json::to_string(&item.genres).unwrap_or_else(|_| "[]".to_string());
    sqlx::query(
        "INSERT INTO catalog_items \
            (id, tmdb_id, anilist_id, kind, title, original_language, genres, is_anime, \
             poster_path, backdrop_path, overview, popularity, vote_average, release_date, \
             source, first_seen_at, last_refreshed_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?16) \
         ON CONFLICT(tmdb_id, kind) WHERE tmdb_id IS NOT NULL AND is_anime = 0 DO UPDATE SET \
            title = excluded.title, \
            original_language = excluded.original_language, \
            genres = excluded.genres, \
            is_anime = excluded.is_anime, \
            poster_path = excluded.poster_path, \
            backdrop_path = excluded.backdrop_path, \
            overview = excluded.overview, \
            popularity = excluded.popularity, \
            vote_average = excluded.vote_average, \
            release_date = excluded.release_date, \
            source = excluded.source, \
            last_refreshed_at = excluded.last_refreshed_at",
    )
    .bind(id)
    .bind(item.tmdb_id)
    .bind(item.anilist_id)
    .bind(&item.kind)
    .bind(&item.title)
    .bind(&item.original_language)
    .bind(&genres)
    .bind(item.is_anime)
    .bind(&item.poster_path)
    .bind(&item.backdrop_path)
    .bind(&item.overview)
    .bind(item.popularity)
    .bind(item.vote_average)
    .bind(&item.release_date)
    .bind(&item.source)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

/// Upsert an AniList-sourced (anime) candidate, keyed on `anilist_id`.
/// Reconciliation may attach a `tmdb_id` on a later pass — it's updated
/// here, while `first_seen_at` and the availability columns are
/// preserved. Anime rows live in their own dedup space (the tmdb unique
/// index is scoped to non-anime), so carrying a `tmdb_id` never collides
/// with a TMDB row of the same title.
pub async fn upsert_anime(pool: &SqlitePool, item: &NewCatalogItem) -> Result<(), sqlx::Error> {
    let id = Uuid::new_v4();
    let now = Utc::now();
    let genres = serde_json::to_string(&item.genres).unwrap_or_else(|_| "[]".to_string());
    sqlx::query(
        "INSERT INTO catalog_items \
            (id, tmdb_id, anilist_id, kind, title, original_language, genres, is_anime, \
             poster_path, backdrop_path, overview, popularity, vote_average, release_date, \
             source, first_seen_at, last_refreshed_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?16) \
         ON CONFLICT(anilist_id) WHERE anilist_id IS NOT NULL DO UPDATE SET \
            tmdb_id = excluded.tmdb_id, \
            kind = excluded.kind, \
            title = excluded.title, \
            original_language = excluded.original_language, \
            genres = excluded.genres, \
            is_anime = excluded.is_anime, \
            poster_path = excluded.poster_path, \
            backdrop_path = excluded.backdrop_path, \
            overview = excluded.overview, \
            popularity = excluded.popularity, \
            vote_average = excluded.vote_average, \
            release_date = excluded.release_date, \
            source = excluded.source, \
            last_refreshed_at = excluded.last_refreshed_at",
    )
    .bind(id)
    .bind(item.tmdb_id)
    .bind(item.anilist_id)
    .bind(&item.kind)
    .bind(&item.title)
    .bind(&item.original_language)
    .bind(&genres)
    .bind(item.is_anime)
    .bind(&item.poster_path)
    .bind(&item.backdrop_path)
    .bind(&item.overview)
    .bind(item.popularity)
    .bind(item.vote_average)
    .bind(&item.release_date)
    .bind(&item.source)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

/// Flip a candidate's availability (the tracker-confirmation pass, Slice
/// 3). Stamps `available_checked_at`.
pub async fn set_availability(
    pool: &SqlitePool,
    catalog_id: Uuid,
    availability: &str,
    provider: Option<&str>,
) -> Result<(), sqlx::Error> {
    let now = Utc::now();
    sqlx::query(
        "UPDATE catalog_items \
         SET availability = ?1, available_provider = ?2, available_checked_at = ?3 \
         WHERE id = ?4",
    )
    .bind(availability)
    .bind(provider)
    .bind(now)
    .bind(catalog_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Query catalogue candidates for a recommendation shelf, ordered by
/// popularity descending (nulls sort last).
pub async fn query_for_user(
    pool: &SqlitePool,
    q: &CatalogQuery,
) -> Result<Vec<CatalogItem>, sqlx::Error> {
    let mut qb =
        sqlx::QueryBuilder::new(format!("SELECT {SELECT_COLUMNS} FROM catalog_items WHERE 1 = 1"));
    if let Some(kind) = &q.kind {
        qb.push(" AND kind = ").push_bind(kind.clone());
    }
    if let Some(is_anime) = q.is_anime {
        qb.push(" AND is_anime = ").push_bind(is_anime);
    }
    if q.only_available {
        qb.push(" AND availability = 'available'");
    }
    if let Some(genre) = q.genre {
        qb.push(" AND EXISTS (SELECT 1 FROM json_each(genres) WHERE value = ")
            .push_bind(genre)
            .push(")");
    }
    if !q.languages.is_empty() {
        qb.push(" AND original_language IN (");
        let mut sep = qb.separated(", ");
        for lang in &q.languages {
            sep.push_bind(lang.clone());
        }
        sep.push_unseparated(")");
    }
    let order = match q.order {
        CatalogOrder::Popularity => " ORDER BY popularity DESC LIMIT ",
        CatalogOrder::FirstSeen => " ORDER BY first_seen_at DESC LIMIT ",
        // NULL release_date sorts last under SQLite DESC (films without a
        // date drop to the bottom rather than the top).
        CatalogOrder::ReleaseDate => " ORDER BY release_date DESC LIMIT ",
    };
    qb.push(order).push_bind(q.limit);
    qb.build_query_as::<CatalogItem>().fetch_all(pool).await
}

/// Candidates needing a tracker-availability check: never-checked
/// `unknown` rows first, then anything last checked before `cutoff`
/// (stale re-checks). Bounded by `limit` so a pass does a fixed amount
/// of work regardless of catalogue size.
pub async fn pending_confirmation(
    pool: &SqlitePool,
    cutoff: DateTime<Utc>,
    limit: i64,
) -> Result<Vec<CatalogItem>, sqlx::Error> {
    let sql = format!(
        "SELECT {SELECT_COLUMNS} FROM catalog_items \
         WHERE availability = 'unknown' OR available_checked_at IS NULL OR available_checked_at < ?1 \
         ORDER BY (availability = 'unknown') DESC, popularity DESC \
         LIMIT ?2"
    );
    sqlx::query_as::<_, CatalogItem>(&sql)
        .bind(cutoff)
        .bind(limit)
        .fetch_all(pool)
        .await
}

/// One watched title that reconciles to a (non-anime) catalogue row —
/// the raw signal for genre-affinity scoring + "because you watched".
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct WatchedSignal {
    pub tmdb_id: i64,
    pub title: String,
    /// JSON array of TMDB genre ids.
    pub genres: String,
    pub watched_at: DateTime<Utc>,
}

/// The user's watched titles that map to a catalogue row, most-recent
/// first — used to weight genre affinity and seed "because you watched".
/// Anime are excluded (their affinity isn't TMDB-genre based).
pub async fn watched_genre_signals(
    pool: &SqlitePool,
    user_id: UserId,
) -> Result<Vec<WatchedSignal>, sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query_as::<_, WatchedSignal>(
        "SELECT ci.tmdb_id AS tmdb_id, ci.title AS title, ci.genres AS genres, \
                MAX(p.last_watched_at) AS watched_at \
         FROM playback_progress p \
         JOIN torrents t ON t.infohash = p.infohash \
         LEFT JOIN collections c ON c.id = t.collection_id \
         JOIN catalog_items ci \
             ON ci.tmdb_id = COALESCE(c.tmdb_id, t.tmdb_id) AND ci.is_anime = 0 \
         WHERE p.user_id = ?1 AND ci.tmdb_id IS NOT NULL \
         GROUP BY ci.id \
         ORDER BY watched_at DESC",
    )
    .bind(user)
    .fetch_all(pool)
    .await
}

/// Recently-added anime not yet confirmed available. The hourly anime
/// fast-path re-checks these so a release surfaces as soon as any
/// provider has it. Bounded by `limit`.
pub async fn pending_anime_confirmation(
    pool: &SqlitePool,
    since: DateTime<Utc>,
    limit: i64,
) -> Result<Vec<CatalogItem>, sqlx::Error> {
    let sql = format!(
        "SELECT {SELECT_COLUMNS} FROM catalog_items \
         WHERE is_anime = 1 AND availability != 'available' AND first_seen_at >= ?1 \
         ORDER BY first_seen_at DESC \
         LIMIT ?2"
    );
    sqlx::query_as::<_, CatalogItem>(&sql)
        .bind(since)
        .bind(limit)
        .fetch_all(pool)
        .await
}

/// Look up a single non-anime catalogue row by `tmdb_id` — used to
/// rebuild a "because you watched" shelf from its key (the seed's genres
/// + title).
pub async fn find_by_tmdb(
    pool: &SqlitePool,
    tmdb_id: i64,
) -> Result<Option<CatalogItem>, sqlx::Error> {
    let sql = format!(
        "SELECT {SELECT_COLUMNS} FROM catalog_items \
         WHERE tmdb_id = ?1 AND is_anime = 0 LIMIT 1"
    );
    sqlx::query_as::<_, CatalogItem>(&sql)
        .bind(tmdb_id)
        .fetch_optional(pool)
        .await
}

/// Drop rows not refreshed since `older_than` — keeps the catalogue from
/// growing without bound as TMDB trends churn. Returns rows removed.
pub async fn prune_stale(
    pool: &SqlitePool,
    older_than: DateTime<Utc>,
) -> Result<u64, sqlx::Error> {
    let res = sqlx::query("DELETE FROM catalog_items WHERE last_refreshed_at < ?1")
        .bind(older_than)
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use sqlx::sqlite::SqlitePoolOptions;

    fn movie(tmdb_id: i64, title: &str, lang: &str, popularity: f64) -> NewCatalogItem {
        NewCatalogItem {
            tmdb_id: Some(tmdb_id),
            anilist_id: None,
            kind: "movie".to_string(),
            title: title.to_string(),
            original_language: Some(lang.to_string()),
            genres: vec![28],
            is_anime: false,
            poster_path: None,
            backdrop_path: None,
            overview: None,
            popularity: Some(popularity),
            vote_average: Some(0.8),
            release_date: Some("2001-04-25".to_string()),
            source: Some("test".to_string()),
        }
    }

    /// Single-connection in-memory pool so every query hits the same DB,
    /// migrated through the latest schema.
    async fn migrated_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("open in-memory sqlite");
        crate::migrate::run(&pool).await.expect("run migrations");
        pool
    }

    #[tokio::test]
    async fn upsert_dedups_and_filters() {
        let pool = migrated_pool().await;

        // Same (tmdb_id, kind) twice → the partial-index ON CONFLICT
        // updates in place rather than inserting a duplicate.
        upsert_item(&pool, &movie(1, "Amelie", "fr", 10.0)).await.unwrap();
        let mut updated = movie(1, "Amelie (updated)", "fr", 20.0);
        updated.genres = vec![35, 18];
        upsert_item(&pool, &updated).await.unwrap();
        upsert_item(&pool, &movie(2, "Heat", "en", 15.0)).await.unwrap();

        // Both start 'unknown' → both pending, unknowns ordered by popularity.
        let pending = pending_confirmation(&pool, Utc::now(), 10).await.unwrap();
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].title, "Amelie (updated)");

        // Language filter resolves to the single, updated French row.
        let fr = query_for_user(
            &pool,
            &CatalogQuery { languages: vec!["fr".to_string()], limit: 10, ..Default::default() },
        )
        .await
        .unwrap();
        assert_eq!(fr.len(), 1);
        assert_eq!(fr[0].title, "Amelie (updated)");
        assert_eq!(fr[0].genres, "[35,18]");

        // No language filter → both rows, popularity desc.
        let all = query_for_user(&pool, &CatalogQuery { limit: 10, ..Default::default() })
            .await
            .unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].title, "Amelie (updated)");

        // Availability starts 'unknown' → only_available excludes everything…
        let none_available = query_for_user(
            &pool,
            &CatalogQuery { only_available: true, limit: 10, ..Default::default() },
        )
        .await
        .unwrap();
        assert!(none_available.is_empty());

        // …until a tracker confirmation flips one.
        set_availability(&pool, fr[0].id, "available", Some("torznab")).await.unwrap();
        let available = query_for_user(
            &pool,
            &CatalogQuery { only_available: true, limit: 10, ..Default::default() },
        )
        .await
        .unwrap();
        assert_eq!(available.len(), 1);
        assert_eq!(available[0].available_provider.as_deref(), Some("torznab"));

        // The just-confirmed row is no longer pending (checked_at is recent);
        // the still-unknown one is.
        let yesterday = Utc::now() - chrono::Duration::days(1);
        let pending_after = pending_confirmation(&pool, yesterday, 10).await.unwrap();
        assert_eq!(pending_after.len(), 1);
        assert_eq!(pending_after[0].title, "Heat");
    }
}
