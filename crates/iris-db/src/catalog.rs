//! Shared discovery catalogue (`catalog_items`): titles a tracker can serve
//! right now, with their best recorded release.
//!
//! Two writers: the freshness scheduler (tracker latest-release feeds) and
//! the pulse scheduler (external trending lists joined by tracker search).
//! Both only ever write tracker-confirmed rows (`availability =
//! 'available'`). `reco.rs` reads it per user at request time — all per-user
//! filtering happens there, never at write time.

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
    /// Which slice produced this row (e.g. `freshness:c411:movie`,
    /// `reco:similar`) — diagnostic only.
    pub source: Option<String>,
    /// Always `'available'` today (a tracker-confirmed row); the column
    /// predates the removal of lazy recommendation candidates.
    pub availability: String,
    /// Best recorded release's seeder count. The dead-torrent guard never
    /// stores a 0-seeder release; re-checked at grab time.
    pub seeders: Option<i64>,
    /// Best recorded release's grab facts — enough to ingest the exact release
    /// directly without a fresh search.
    pub provider_id: Option<String>,
    pub external_id: Option<String>,
    pub download_url: Option<String>,
    pub infohash: Option<String>,
    /// Coarse language tag of the recorded release (`"french"` / `"english"` /
    /// `"multi"` / …), so a per-language household can prefer its own.
    pub language: Option<String>,
    /// Tracker upload time of the recorded release — basis for the sliding
    /// window ordering + GC. `None` when the feed carried no date.
    pub released_at: Option<DateTime<Utc>>,
    /// Total size of the recorded best release, in bytes. Lets the reco skip
    /// proposing an absurdly large release (a 4K REMUX) by default.
    pub size_bytes: Option<i64>,
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
    pub seeders: Option<i64>,
    pub provider_id: Option<String>,
    pub external_id: Option<String>,
    pub download_url: Option<String>,
    pub infohash: Option<String>,
    pub language: Option<String>,
    pub released_at: Option<DateTime<Utc>>,
    /// Total size of the recorded best release, in bytes (`None` when unknown).
    pub size_bytes: Option<i64>,
}

/// Column list for `CatalogItem` reads — shared so the row queries can't
/// drift from the struct's `FromRow` field order. A macro rather than a
/// `const &str` so it expands to a string literal usable inside
/// `concat!`, keeping the simple reads compile-time `&'static str` — the
/// only type sqlx 0.9 accepts natively (`SqlSafeStr`) — and feeding the
/// dynamic `QueryBuilder` path (`query_for_user`) a static initializer
/// too, with no `format!`/`AssertSqlSafe` audit hatch anywhere.
macro_rules! select_columns {
    () => {
        "id, tmdb_id, anilist_id, kind, title, original_language, genres, \
         is_anime, poster_path, backdrop_path, overview, popularity, vote_average, release_date, \
         availability, available_provider, seeders, provider_id, external_id, download_url, infohash, \
         language, released_at, size_bytes"
    };
}

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
    /// Most recently uploaded to a tracker first — the rolling-window
    /// "what just dropped" ordering (NULLs sort last).
    Released,
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
             source, first_seen_at, last_refreshed_at, \
             availability, seeders, provider_id, external_id, download_url, infohash, \
             language, released_at, size_bytes) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?16, \
                 ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25) \
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
            last_refreshed_at = excluded.last_refreshed_at, \
            availability = excluded.availability, \
            seeders = excluded.seeders, \
            provider_id = excluded.provider_id, \
            external_id = excluded.external_id, \
            download_url = excluded.download_url, \
            infohash = excluded.infohash, \
            language = excluded.language, \
            released_at = excluded.released_at, \
            size_bytes = excluded.size_bytes",
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
    .bind(&item.availability)
    .bind(item.seeders)
    .bind(&item.provider_id)
    .bind(&item.external_id)
    .bind(&item.download_url)
    .bind(&item.infohash)
    .bind(&item.language)
    .bind(item.released_at)
    .bind(item.size_bytes)
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
             source, first_seen_at, last_refreshed_at, \
             availability, seeders, provider_id, external_id, download_url, infohash, \
             language, released_at, size_bytes) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?16, \
                 ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25) \
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
            last_refreshed_at = excluded.last_refreshed_at, \
            availability = excluded.availability, \
            seeders = excluded.seeders, \
            provider_id = excluded.provider_id, \
            external_id = excluded.external_id, \
            download_url = excluded.download_url, \
            infohash = excluded.infohash, \
            language = excluded.language, \
            released_at = excluded.released_at, \
            size_bytes = excluded.size_bytes",
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
    .bind(&item.availability)
    .bind(item.seeders)
    .bind(&item.provider_id)
    .bind(&item.external_id)
    .bind(&item.download_url)
    .bind(&item.infohash)
    .bind(&item.language)
    .bind(item.released_at)
    .bind(item.size_bytes)
    .execute(pool)
    .await?;
    // Invariant: an anime row OWNS its `(tmdb_id, kind)`. Drop any non-anime twin
    // left by a slice where the AniList match transiently failed (the genre16+ja
    // anime gate then fell back to a plain `upsert_item` row). This is what
    // prevents the anime/non-anime classification split at the source; migration
    // 0028 heals legacy ones. Scoped to the same kind so a genuine movie/tv
    // id-collision (different works sharing a numeric id) is never touched.
    if let Some(tmdb_id) = item.tmdb_id {
        sqlx::query("DELETE FROM catalog_items WHERE is_anime = 0 AND tmdb_id = ?1 AND kind = ?2")
            .bind(tmdb_id)
            .bind(&item.kind)
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// Query catalogue candidates for a recommendation shelf, ordered by
/// popularity descending (nulls sort last).
pub async fn query_for_user(
    pool: &SqlitePool,
    q: &CatalogQuery,
) -> Result<Vec<CatalogItem>, sqlx::Error> {
    let mut qb = sqlx::QueryBuilder::new(concat!(
        "SELECT ",
        select_columns!(),
        " FROM catalog_items WHERE 1 = 1"
    ));
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
        // Same NULLs-last behaviour for the tracker upload time.
        CatalogOrder::Released => " ORDER BY released_at DESC LIMIT ",
    };
    qb.push(order).push_bind(q.limit);
    qb.build_query_as::<CatalogItem>().fetch_all(pool).await
}

/// One title this user watched, for their genre affinity.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct WatchedTitle {
    pub tmdb_id: i64,
    /// `"movie"` | `"tv"` — TMDB keeps separate id spaces per kind.
    pub kind: String,
    pub watched_at: DateTime<Utc>,
}

/// The user's most recently watched titles (by collection), newest first.
/// Independent of `catalog_items`: what someone watched months ago has long
/// left the discovery window but still says what they like.
pub async fn recent_watched_titles(
    pool: &SqlitePool,
    user_id: UserId,
    limit: i64,
) -> Result<Vec<WatchedTitle>, sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query_as::<_, WatchedTitle>(
        "SELECT c.tmdb_id AS tmdb_id, c.kind AS kind, MAX(p.last_watched_at) AS watched_at \
         FROM playback_progress p \
         JOIN torrents t ON t.infohash = p.infohash \
         JOIN collections c ON c.id = t.collection_id \
         WHERE p.user_id = ?1 AND c.tmdb_id IS NOT NULL AND c.kind IN ('movie', 'tv') \
         GROUP BY c.tmdb_id, c.kind \
         ORDER BY watched_at DESC \
         LIMIT ?2",
    )
    .bind(user)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// The persisted pre-signed `.torrent` URL for a recorded rolling-window
/// release, by `(provider_id, external_id)`. Lets the preview / grab paths
/// bypass the provider's per-process link cache — cold after a restart or FIFO
/// eviction, which otherwise broke c411/torznab catalogue previews + grabs
/// ("no cached download URL for …").
pub async fn download_url_for(
    pool: &SqlitePool,
    provider_id: &str,
    external_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT download_url FROM catalog_items \
         WHERE provider_id = ?1 AND external_id = ?2 AND download_url IS NOT NULL LIMIT 1",
    )
    .bind(provider_id)
    .bind(external_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(|(u,)| u))
}

/// `(tmdb_id, kind)` of tracker-confirmed rows refreshed since `since` — the
/// titles the pulse join can skip because a scheduler already found them.
pub async fn fresh_available_keys(
    pool: &SqlitePool,
    since: DateTime<Utc>,
) -> Result<Vec<(i64, String)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT tmdb_id, kind FROM catalog_items \
         WHERE availability = 'available' AND tmdb_id IS NOT NULL AND last_refreshed_at >= ?1",
    )
    .bind(since)
    .fetch_all(pool)
    .await
}

/// Slide the rolling window. Removes rows whose tracker upload time predates
/// `released_before` (the retention edge), plus undated rows not refreshed
/// since `undated_before`. Titles in the library, followed by any user, or on
/// a current pulse list are spared — the first two may still matter (a
/// followed series getting new episodes), the last is what the discovery
/// shelves are built from, however old its best release.
/// AniList-only rows (no `tmdb_id`) are always eligible. Returns rows removed.
pub async fn prune_window(
    pool: &SqlitePool,
    released_before: DateTime<Utc>,
    undated_before: DateTime<Utc>,
) -> Result<u64, sqlx::Error> {
    let res = sqlx::query(
        "DELETE FROM catalog_items \
         WHERE ( \
             (released_at IS NOT NULL AND released_at < ?1) \
          OR (released_at IS NULL AND last_refreshed_at < ?2) \
         ) \
         AND ( \
             tmdb_id IS NULL OR tmdb_id NOT IN ( \
                 SELECT c.tmdb_id FROM torrents t \
                   LEFT JOIN collections c ON c.id = t.collection_id \
                   WHERE t.deleted_at IS NULL AND c.tmdb_id IS NOT NULL \
                 UNION \
                 SELECT tmdb_id FROM series_follows \
                 UNION \
                 SELECT tmdb_id FROM pulse_signals \
             ) \
         )",
    )
    .bind(released_before)
    .bind(undated_before)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::migrated_pool;
    use chrono::Utc;

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
            availability: "unknown".to_string(),
            seeders: None,
            provider_id: None,
            external_id: None,
            download_url: None,
            infohash: None,
            language: None,
            released_at: None,
            size_bytes: None,
        }
    }

    /// A rolling-window movie: `availability='available'` with grab facts and
    /// a tracker upload time.
    fn movie_release(
        tmdb_id: i64,
        title: &str,
        seeders: i64,
        released_at: DateTime<Utc>,
    ) -> NewCatalogItem {
        NewCatalogItem {
            availability: "available".to_string(),
            seeders: Some(seeders),
            provider_id: Some("c411".to_string()),
            external_id: Some(format!("ext-{tmdb_id}")),
            download_url: Some(format!("https://t/{tmdb_id}.torrent")),
            infohash: Some(format!("{tmdb_id:040x}")),
            language: Some("french".to_string()),
            released_at: Some(released_at),
            ..movie(tmdb_id, title, "fr", 10.0)
        }
    }

    #[tokio::test]
    async fn upsert_dedups_and_filters() {
        let pool = migrated_pool().await;

        // Same (tmdb_id, kind) twice → the partial-index ON CONFLICT
        // updates in place rather than inserting a duplicate.
        upsert_item(&pool, &movie(1, "Amelie", "fr", 10.0))
            .await
            .unwrap();
        let mut updated = movie(1, "Amelie (updated)", "fr", 20.0);
        updated.genres = vec![35, 18];
        upsert_item(&pool, &updated).await.unwrap();
        upsert_item(&pool, &movie(2, "Heat", "en", 15.0))
            .await
            .unwrap();

        // Language filter resolves to the single, updated French row.
        let fr = query_for_user(
            &pool,
            &CatalogQuery {
                languages: vec!["fr".to_string()],
                limit: 10,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(fr.len(), 1);
        assert_eq!(fr[0].title, "Amelie (updated)");
        assert_eq!(fr[0].genres, "[35,18]");

        // No language filter → both rows, popularity desc.
        let all = query_for_user(
            &pool,
            &CatalogQuery {
                limit: 10,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].title, "Amelie (updated)");
    }

    #[tokio::test]
    async fn release_facts_persist_and_order_by_released() {
        let pool = migrated_pool().await;
        let old = Utc::now() - chrono::Duration::days(40);
        let fresh = Utc::now() - chrono::Duration::days(2);

        upsert_item(&pool, &movie_release(1, "Old Drop", 5, old))
            .await
            .unwrap();
        upsert_item(&pool, &movie_release(2, "Fresh Drop", 12, fresh))
            .await
            .unwrap();

        // Release facts round-trip + freshest-first ordering.
        let rows = query_for_user(
            &pool,
            &CatalogQuery {
                order: CatalogOrder::Released,
                limit: 10,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].title, "Fresh Drop");
        assert_eq!(rows[0].availability, "available");
        assert_eq!(rows[0].seeders, Some(12));
        assert_eq!(rows[0].provider_id.as_deref(), Some("c411"));
        assert_eq!(rows[0].external_id.as_deref(), Some("ext-2"));
        assert!(rows[0].released_at.is_some());

        // only_available passes (rolling-window rows are 'available').
        let available = query_for_user(
            &pool,
            &CatalogQuery {
                only_available: true,
                limit: 10,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(available.len(), 2);
    }

    #[tokio::test]
    async fn prune_window_slides_and_spares_nulls() {
        let pool = migrated_pool().await;
        let old = Utc::now() - chrono::Duration::days(40);
        let fresh = Utc::now() - chrono::Duration::days(2);

        upsert_item(&pool, &movie_release(1, "Old Drop", 5, old))
            .await
            .unwrap();
        upsert_item(&pool, &movie_release(2, "Fresh Drop", 12, fresh))
            .await
            .unwrap();
        // An undated row, AniList-only (always
        // GC-eligible). Backdate last_refreshed_at so it reads as cold.
        let mut lazy = movie(3, "Undated", "fr", 8.0);
        lazy.tmdb_id = None;
        lazy.anilist_id = Some(999);
        upsert_anime(&pool, &lazy).await.unwrap();
        sqlx::query("UPDATE catalog_items SET last_refreshed_at = ?1 WHERE anilist_id = 999")
            .bind(Utc::now() - chrono::Duration::days(5))
            .execute(&pool)
            .await
            .unwrap();

        let released_cutoff = Utc::now() - chrono::Duration::days(28);
        let lazy_cutoff = Utc::now() - chrono::Duration::days(1);
        let removed = prune_window(&pool, released_cutoff, lazy_cutoff)
            .await
            .unwrap();
        assert_eq!(removed, 2, "old drop + cold undated row pruned");

        let rows = query_for_user(
            &pool,
            &CatalogQuery {
                limit: 10,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "Fresh Drop");
    }

    #[tokio::test]
    async fn prune_window_spares_pulse_titles() {
        let pool = migrated_pool().await;
        let old = Utc::now() - chrono::Duration::days(200);
        upsert_item(&pool, &movie_release(7, "Trending Classic", 40, old))
            .await
            .unwrap();
        upsert_item(&pool, &movie_release(8, "Forgotten", 40, old))
            .await
            .unwrap();
        crate::pulse::replace_list(
            &pool,
            "trending",
            "movie",
            &[crate::pulse::NewSignal {
                tmdb_id: 7,
                watched: None,
                title: "Trending Classic".to_string(),
                release_date: None,
            }],
        )
        .await
        .unwrap();
        let cutoff = Utc::now() - chrono::Duration::days(28);
        assert_eq!(prune_window(&pool, cutoff, cutoff).await.unwrap(), 1);
        let rows = query_for_user(
            &pool,
            &CatalogQuery {
                limit: 10,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].tmdb_id, Some(7));
    }
}
