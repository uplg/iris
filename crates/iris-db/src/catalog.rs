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
    /// Which slice produced this row (e.g. `freshness:torr9:movie`,
    /// `reco:similar`) — diagnostic only.
    pub source: Option<String>,
    /// `'available'` for a tracker-confirmed rolling-window row, `'unknown'`
    /// for a lazy recommendation candidate (resolved on click).
    pub availability: String,
    /// Best recorded release's seeder count. The dead-torrent guard never
    /// stores a 0-seeder release; re-checked at grab time.
    pub seeders: Option<i64>,
    /// Best recorded release's grab facts — enough to ingest the exact release
    /// directly without a fresh search. `None` for lazy reco candidates.
    pub provider_id: Option<String>,
    pub external_id: Option<String>,
    pub download_url: Option<String>,
    pub infohash: Option<String>,
    /// Coarse language tag of the recorded release (`"french"` / `"english"` /
    /// `"multi"` / …), so a per-language household can prefer its own.
    pub language: Option<String>,
    /// Tracker upload time of the recorded release — basis for the sliding
    /// window ordering + GC. `None` for lazy reco candidates.
    pub released_at: Option<DateTime<Utc>>,
    /// Total size of the recorded best release, in bytes. Lets the reco skip
    /// proposing an absurdly large release (a 4K REMUX) by default. `None` for
    /// lazy reco candidates (no recorded release).
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
    /// Total size of the recorded best release, in bytes (`None` for lazy rows).
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

/// One watched title that reconciles to a (non-anime) catalogue row —
/// the raw signal for genre-affinity scoring + "because you watched".
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct WatchedSignal {
    pub tmdb_id: i64,
    pub title: String,
    /// `"movie"` | `"tv"` — needed to hit the right TMDB recommendations
    /// endpoint for "Because you watched X".
    pub kind: String,
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
        "SELECT ci.tmdb_id AS tmdb_id, ci.title AS title, ci.kind AS kind, ci.genres AS genres, \
                MAX(p.last_watched_at) AS watched_at \
         FROM playback_progress p \
         JOIN torrents t ON t.infohash = p.infohash \
         LEFT JOIN collections c ON c.id = t.collection_id \
         JOIN catalog_items ci \
             ON ci.tmdb_id = c.tmdb_id AND ci.kind = c.kind AND ci.is_anime = 0 \
         WHERE p.user_id = ?1 AND ci.tmdb_id IS NOT NULL \
         GROUP BY ci.id \
         ORDER BY watched_at DESC",
    )
    .bind(user)
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

/// Fetch catalogue rows by id (arbitrary order) — hydrates id-keyed shelves
/// (e.g. "Popular in your circle") into full cards. Order is not preserved;
/// callers re-sort by their own ranking.
pub async fn by_ids(pool: &SqlitePool, ids: &[Uuid]) -> Result<Vec<CatalogItem>, sqlx::Error> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut qb = sqlx::QueryBuilder::new(concat!(
        "SELECT ",
        select_columns!(),
        " FROM catalog_items WHERE id IN ("
    ));
    let mut sep = qb.separated(", ");
    for id in ids {
        sep.push_bind(*id);
    }
    sep.push_unseparated(")");
    qb.build_query_as::<CatalogItem>().fetch_all(pool).await
}

/// Look up a single non-anime catalogue row by `tmdb_id` — used to
/// rebuild a "because you watched" shelf from its key (the seed's genres
/// + title).
pub async fn find_by_tmdb(
    pool: &SqlitePool,
    tmdb_id: i64,
) -> Result<Option<CatalogItem>, sqlx::Error> {
    sqlx::query_as::<_, CatalogItem>(concat!(
        "SELECT ",
        select_columns!(),
        " FROM catalog_items WHERE tmdb_id = ?1 AND is_anime = 0 LIMIT 1"
    ))
    .bind(tmdb_id)
    .fetch_optional(pool)
    .await
}

// ----------------------------------------------------------------- embeddings

/// A catalogue row's text fields, for (re)building its content embedding.
/// Genre *names* are resolved upstream (cached TMDB taxonomy); this layer only
/// carries the raw ids it stores.
#[derive(Debug, Clone)]
pub struct EmbeddingInput {
    pub id: Uuid,
    pub title: String,
    pub overview: Option<String>,
    /// TMDB genre ids (parsed from the stored JSON array).
    pub genres: Vec<i64>,
    pub kind: String,
}

/// Catalogue rows whose content embedding is missing or stale (a different model
/// than `model_id`), most-popular first, capped at `limit`. The ingest/backfill
/// pass embeds these and writes the vectors back via [`set_embedding`].
pub async fn items_needing_embedding(
    pool: &SqlitePool,
    model_id: &str,
    limit: i64,
) -> Result<Vec<EmbeddingInput>, sqlx::Error> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT id, title, overview, genres, kind FROM catalog_items \
         WHERE content_embedding IS NULL OR embedding_model IS NULL OR embedding_model <> ?1 \
         ORDER BY popularity DESC \
         LIMIT ?2",
    )
    .bind(model_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let genres_json: String = r.get("genres");
            EmbeddingInput {
                id: r.get("id"),
                title: r.get("title"),
                overview: r.get("overview"),
                genres: serde_json::from_str(&genres_json).unwrap_or_default(),
                kind: r.get("kind"),
            }
        })
        .collect())
}

/// Persist an item's L2-normalized embedding (little-endian f32 BLOB) plus the
/// model id that produced it.
pub async fn set_embedding(
    pool: &SqlitePool,
    id: Uuid,
    vector: &[f32],
    model_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE catalog_items SET content_embedding = ?1, embedding_model = ?2 WHERE id = ?3",
    )
    .bind(vec_to_blob(vector))
    .bind(model_id)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Every catalogue item carrying a current-model embedding, as `(id, vector)`.
/// Small enough (≈9 MB for the whole catalogue at dim 512) to hold in memory and
/// rank over with dot products at request time.
pub async fn load_embeddings(
    pool: &SqlitePool,
    model_id: &str,
) -> Result<Vec<(Uuid, Vec<f32>)>, sqlx::Error> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT id, content_embedding FROM catalog_items \
         WHERE content_embedding IS NOT NULL AND embedding_model = ?1",
    )
    .bind(model_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            let blob: Vec<u8> = r.get("content_embedding");
            let vec = blob_to_vec(&blob)?;
            let id: Uuid = r.get("id");
            Some((id, vec))
        })
        .collect())
}

/// Encode a vector as a little-endian f32 byte blob.
fn vec_to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// Decode a little-endian f32 byte blob. `None` on a non-multiple-of-4 length
/// (corrupt / truncated) so a bad row is skipped rather than panicking.
fn blob_to_vec(blob: &[u8]) -> Option<Vec<f32>> {
    if blob.is_empty() || !blob.len().is_multiple_of(4) {
        return None;
    }
    Some(
        blob.as_chunks::<4>()
            .0
            .iter()
            .copied()
            .map(f32::from_le_bytes)
            .collect(),
    )
}

// --------------------------------------------------------- reco signals

/// Confidence assigned to a *grab* (an explicit download = strong intent).
const GRAB_CONFIDENCE: f32 = 0.7;
/// Confidence for a play we can't grade (no duration recorded).
const UNGRADED_PLAY_CONFIDENCE: f32 = 0.3;

/// A user's positive catalogue items as `(catalog_id, confidence)` — the input
/// to their multi-centroid taste profile. Confidence is graded (Hu-Koren-Volinsky
/// confidence weighting, denoised): a completed play scores 1.0, a partial play
/// the fraction watched, a grab `GRAB_CONFIDENCE`. Aggregated to the max per
/// catalogue row so a series' many episodes collapse to one signal. Plays/grabs
/// map to catalogue rows via the **collection's** `tmdb_id` only — `torrent.tmdb_id`
/// is ignored (it can disagree; everything is a collection at minimum, so the
/// collection id is authoritative).
#[allow(clippy::cast_precision_loss)] // position/duration are small second counts
pub async fn user_positive_catalog(
    pool: &SqlitePool,
    user_id: UserId,
) -> Result<Vec<(Uuid, f32)>, sqlx::Error> {
    use sqlx::Row;
    let user: Uuid = user_id.into();
    let mut conf: std::collections::HashMap<Uuid, f32> = std::collections::HashMap::new();

    let plays = sqlx::query(
        "SELECT ci.id AS cid, p.completed AS completed, \
                p.position_seconds AS pos, p.duration_seconds AS dur \
         FROM playback_progress p \
         JOIN torrents t ON t.infohash = p.infohash \
         LEFT JOIN collections c ON c.id = t.collection_id \
         JOIN catalog_items ci ON ci.tmdb_id = c.tmdb_id AND ci.kind = c.kind \
         WHERE p.user_id = ?1 AND ci.tmdb_id IS NOT NULL",
    )
    .bind(user)
    .fetch_all(pool)
    .await?;
    for r in plays {
        let cid: Uuid = r.get("cid");
        let completed: bool = r.get("completed");
        let pos: i64 = r.try_get("pos").unwrap_or(0);
        let dur: i64 = r.try_get("dur").unwrap_or(0);
        let c = if completed {
            1.0
        } else if dur > 0 {
            (pos as f32 / dur as f32).clamp(0.0, 1.0)
        } else {
            UNGRADED_PLAY_CONFIDENCE
        };
        let e = conf.entry(cid).or_insert(0.0);
        *e = e.max(c);
    }

    let grabs = sqlx::query(
        "SELECT DISTINCT ci.id AS cid \
         FROM torrents t \
         LEFT JOIN collections c ON c.id = t.collection_id \
         JOIN catalog_items ci ON ci.tmdb_id = c.tmdb_id AND ci.kind = c.kind \
         WHERE t.added_by = ?1 AND ci.tmdb_id IS NOT NULL",
    )
    .bind(user)
    .fetch_all(pool)
    .await?;
    for r in grabs {
        let cid: Uuid = r.get("cid");
        let e = conf.entry(cid).or_insert(0.0);
        *e = e.max(GRAB_CONFIDENCE);
    }

    Ok(conf.into_iter().collect())
}

/// Catalogue rows watched or grabbed by *other* household users, ranked by how
/// many distinct others touched them — the "Popular in your circle" signal.
/// Returns `(catalog_id, distinct_other_users)`; the caller still filters out
/// what this user already owns/saw.
pub async fn circle_popular(
    pool: &SqlitePool,
    user_id: UserId,
    limit: i64,
) -> Result<Vec<(Uuid, i64)>, sqlx::Error> {
    use sqlx::Row;
    let user: Uuid = user_id.into();
    let rows = sqlx::query(
        "SELECT ci.id AS cid, COUNT(DISTINCT u.uid) AS n FROM ( \
            SELECT p.user_id AS uid, c.tmdb_id AS tmdb, c.kind AS kind \
            FROM playback_progress p \
            JOIN torrents t ON t.infohash = p.infohash \
            LEFT JOIN collections c ON c.id = t.collection_id \
            WHERE p.user_id <> ?1 \
            UNION \
            SELECT t.added_by AS uid, c.tmdb_id AS tmdb, c.kind AS kind \
            FROM torrents t \
            LEFT JOIN collections c ON c.id = t.collection_id \
            WHERE t.added_by IS NOT NULL AND t.added_by <> ?1 \
         ) u \
         JOIN catalog_items ci ON ci.tmdb_id = u.tmdb AND ci.kind = u.kind \
         WHERE u.tmdb IS NOT NULL \
         GROUP BY ci.id \
         ORDER BY n DESC, ci.popularity DESC \
         LIMIT ?2",
    )
    .bind(user)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| (r.get::<Uuid, _>("cid"), r.get::<i64, _>("n")))
        .collect())
}

/// Household watched/grabbed `tmdb_id`s that have NO catalogue row yet — the
/// out-of-window titles. Returns `(tmdb_id, is_tv_hint)`; the hint (presence in
/// `series_follows`) disambiguates TMDB's separate movie/tv id namespaces for the
/// metadata fetch. Backfilling these as metadata-only rows lets them be embedded,
/// enriching users' taste profiles (more positives carry a vector) even though
/// they're never recommended back (already seen).
pub async fn watched_tmdbs_missing(pool: &SqlitePool) -> Result<Vec<(i64, bool)>, sqlx::Error> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT x.tmdb AS tmdb, \
                MAX(CASE WHEN sf.tmdb_id IS NOT NULL THEN 1 ELSE 0 END) AS is_tv \
         FROM ( \
            SELECT c.tmdb_id AS tmdb, c.kind AS kind \
            FROM playback_progress p \
            JOIN torrents t ON t.infohash = p.infohash \
            LEFT JOIN collections c ON c.id = t.collection_id \
            UNION \
            SELECT c.tmdb_id AS tmdb, c.kind AS kind \
            FROM torrents t \
            LEFT JOIN collections c ON c.id = t.collection_id \
            WHERE t.added_by IS NOT NULL \
         ) x \
         LEFT JOIN catalog_items ci ON ci.tmdb_id = x.tmdb AND ci.kind = x.kind \
         LEFT JOIN series_follows sf ON sf.tmdb_id = x.tmdb \
         WHERE x.tmdb IS NOT NULL AND ci.id IS NULL \
         GROUP BY x.tmdb",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| (r.get::<i64, _>("tmdb"), r.get::<i64, _>("is_tv") != 0))
        .collect())
}

/// Drop rows not refreshed since `older_than` — keeps the catalogue from
/// growing without bound as TMDB trends churn. Returns rows removed.
pub async fn prune_stale(pool: &SqlitePool, older_than: DateTime<Utc>) -> Result<u64, sqlx::Error> {
    let res = sqlx::query("DELETE FROM catalog_items WHERE last_refreshed_at < ?1")
        .bind(older_than)
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

/// Slide the rolling window. Removes rolling-window rows whose tracker upload
/// time predates `released_before` (the retention edge), plus lazy
/// recommendation candidates (no `released_at`) gone cold before `lazy_before`.
/// Titles currently in the library or followed by any user are spared — their
/// catalogue row may still matter (e.g. a followed series getting new episodes).
/// AniList-only rows (no `tmdb_id`) are always eligible. Returns rows removed.
pub async fn prune_window(
    pool: &SqlitePool,
    released_before: DateTime<Utc>,
    lazy_before: DateTime<Utc>,
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
             ) \
         )",
    )
    .bind(released_before)
    .bind(lazy_before)
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
            provider_id: Some("torr9".to_string()),
            external_id: Some(format!("ext-{tmdb_id}")),
            download_url: Some(format!("https://t/{tmdb_id}.torrent")),
            infohash: Some(format!("{tmdb_id:040x}")),
            language: Some("french".to_string()),
            released_at: Some(released_at),
            ..movie(tmdb_id, title, "fr", 10.0)
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
        assert_eq!(rows[0].provider_id.as_deref(), Some("torr9"));
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
        // A lazy reco candidate: no released_at, AniList-only (always
        // GC-eligible). Backdate last_refreshed_at so it reads as cold.
        let mut lazy = movie(3, "Lazy Rec", "fr", 8.0);
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
        assert_eq!(removed, 2, "old drop + cold lazy candidate pruned");

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
}
