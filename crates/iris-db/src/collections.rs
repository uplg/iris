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
    /// Last time the collection-driven scheduler ran an indexer scan
    /// for this collection. `NULL` until the first scan completes —
    /// fresh TV collections get picked up on the next tick. Mirror
    /// of the retired `series_follows.last_checked_at` column.
    #[serde(default)]
    pub last_indexer_scan_at: Option<DateTime<Utc>>,
    /// Last time a user opened the collection detail page. Drives
    /// the "X new" badge by counting `available_episodes.found_at >
    /// this stamp` that aren't already in `episode_files`.
    #[serde(default)]
    pub last_visited_at: Option<DateTime<Utc>>,
    /// `true` when this collection holds an anime (fansub-style
    /// release detected at ingest, optionally confirmed via
    /// AniList/TMDB). Baked into `parsed_title_normalized` as an
    /// `anime:` prefix so an anime and a live-action show sharing a
    /// title (the anime *One Piece* vs the Netflix live-action one)
    /// never merge. See `iris_media::filename::collection_key_kind`.
    #[serde(default)]
    pub is_anime: bool,
    /// AniList media id when the async confirm step matched one —
    /// enrichment only (poster / recommendations), never identity.
    #[serde(default)]
    pub anilist_id: Option<i64>,
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
        "SELECT id, tmdb_id, parsed_title_normalized, display_title, kind, created_at, \
                last_indexer_scan_at, last_visited_at, is_anime, anilist_id \
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
        "SELECT id, tmdb_id, parsed_title_normalized, display_title, kind, created_at, \
                last_indexer_scan_at, last_visited_at, is_anime, anilist_id \
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
        "SELECT id, tmdb_id, parsed_title_normalized, display_title, kind, created_at, \
                last_indexer_scan_at, last_visited_at, is_anime, anilist_id \
         FROM collections \
         ORDER BY created_at",
    )
    .fetch_all(pool)
    .await
}

pub async fn get(pool: &SqlitePool, id: Uuid) -> Result<Option<CollectionRow>, sqlx::Error> {
    sqlx::query_as::<_, CollectionRow>(
        "SELECT id, tmdb_id, parsed_title_normalized, display_title, kind, created_at, \
                last_indexer_scan_at, last_visited_at, is_anime, anilist_id \
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
    is_anime: bool,
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
        "INSERT INTO collections (id, tmdb_id, parsed_title_normalized, display_title, kind, created_at, is_anime) \
         VALUES (?1, NULL, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT DO NOTHING",
    )
    .bind(id)
    .bind(normalized)
    .bind(display_title)
    .bind(kind.as_str())
    .bind(now)
    .bind(is_anime)
    .execute(pool)
    .await?;
    find_by_parsed_title(pool, normalized, kind)
        .await?
        .ok_or(sqlx::Error::RowNotFound)
}

/// Set / clear the anime flag and (optionally) the AniList id on a
/// collection. Used by the async confirm step after ingest and by the
/// boot self-heal. `is_anime` is normally only ever turned *on* (the
/// flag is baked into the identity key at creation); callers must not
/// flip a live anime collection back to non-anime without also
/// re-keying it, or its `episode_files` would orphan.
pub async fn set_is_anime(
    pool: &SqlitePool,
    id: Uuid,
    is_anime: bool,
    anilist_id: Option<i64>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE collections SET is_anime = ?1, anilist_id = COALESCE(?2, anilist_id) WHERE id = ?3",
    )
    .bind(is_anime)
    .bind(anilist_id)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
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
    sqlx::query("UPDATE collections SET tmdb_id = ?1 WHERE id = ?2 AND tmdb_id IS NULL")
        .bind(tmdb_id)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Detach the `tmdb_id` from a collection. Used by the identity
/// self-heal when the SCENE key changes: the id was resolved from the
/// OLD title and is presumed poison, so first-writer-wins semantics
/// restart from a clean slate and the next resolve stamps the id that
/// matches the corrected identity.
pub async fn clear_tmdb_id(pool: &SqlitePool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE collections SET tmdb_id = NULL WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Rewrite the `kind` ("movie" / "tv") on an existing collection. Used
/// by `tmdb_backfill` when a torrent's filename re-parses to a kind
/// that disagrees with the stored value (a Silicon Valley S01 pack
/// originally misclassified as movie because the old parser couldn't
/// see season-only markers, etc.). The kind drives both poster
/// resolution and the watch / series routing — keeping it in sync
/// with the parser's verdict avoids loading a movie poster onto a
/// TV-row in the library.
pub async fn set_kind(pool: &SqlitePool, id: Uuid, kind: Kind) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE collections SET kind = ?1 WHERE id = ?2")
        .bind(kind.as_str())
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Rewrite `parsed_title_normalized` on an existing collection.
/// Used by the boot-time self-heal when an older parser run stamped
/// a leaky title (file-leaf SCENE garbage instead of the canonical
/// torrent name). Skipped if another row already owns the target
/// key — that case demands a migration we don't auto-resolve.
pub async fn set_parsed_title_normalized(
    pool: &SqlitePool,
    id: Uuid,
    normalized: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE collections SET parsed_title_normalized = ?1 WHERE id = ?2")
        .bind(normalized)
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
pub async fn set_tmdb_id(pool: &SqlitePool, id: Uuid, tmdb_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE collections SET tmdb_id = ?1 WHERE id = ?2")
        .bind(tmdb_id)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Hard-delete a collection row. Member torrents are `ON DELETE SET NULL`
/// (re-orphaned, then re-assigned on the next backfill tick); member
/// `episode_files` are `ON DELETE CASCADE`. So callers that need to PRESERVE
/// those children — the anime noise-split merge — must re-home them onto the
/// surviving collection FIRST (see `torrents::reassign_collection` /
/// `episode_files::reassign_collection`), then delete the emptied loser here.
pub async fn delete(pool: &SqlitePool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM collections WHERE id = ?1")
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
    #[serde(default)]
    pub is_anime: bool,
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

/// TV collections that have at least one `episode_files` row backed
/// by a still-present torrent. Powers the post-0.4 Watchlist surface
/// (`GET /api/me/watchlist` + the legacy `/api/me/follows` façade):
/// "every TV show the household has actually started watching".
/// Newest activity first via `last_visited_at` then `created_at`.
pub async fn list_tv_with_episodes(pool: &SqlitePool) -> Result<Vec<CollectionRow>, sqlx::Error> {
    sqlx::query_as::<_, CollectionRow>(
        "SELECT c.id, c.tmdb_id, c.parsed_title_normalized, c.display_title, c.kind, \
                c.created_at, c.last_indexer_scan_at, c.last_visited_at, c.is_anime, c.anilist_id \
         FROM collections c \
         WHERE c.kind = 'tv' \
           AND c.parsed_title_normalized IS NOT NULL \
           AND EXISTS ( \
             SELECT 1 FROM episode_files ef \
             JOIN torrents t ON t.infohash = ef.infohash \
             WHERE ef.collection_id = c.id AND t.deleted_at IS NULL \
           ) \
         ORDER BY COALESCE(c.last_visited_at, c.created_at) DESC, c.created_at DESC",
    )
    .fetch_all(pool)
    .await
}

/// Collections eligible for an indexer scan — TV collections whose
/// `last_indexer_scan_at` is older than `cooldown_seconds`, or null.
/// Ordered NULL-first then oldest first, so freshly-ingested TV
/// series get their initial scan before older entries recycle.
pub async fn list_due_for_scan(
    pool: &SqlitePool,
    cooldown_seconds: i64,
) -> Result<Vec<CollectionRow>, sqlx::Error> {
    sqlx::query_as::<_, CollectionRow>(
        "SELECT id, tmdb_id, parsed_title_normalized, display_title, kind, created_at, \
                last_indexer_scan_at, last_visited_at, is_anime, anilist_id \
         FROM collections \
         WHERE kind = 'tv' \
           AND parsed_title_normalized IS NOT NULL \
           AND (last_indexer_scan_at IS NULL \
                OR last_indexer_scan_at < datetime('now', '-' || ?1 || ' seconds')) \
         ORDER BY last_indexer_scan_at IS NOT NULL, last_indexer_scan_at",
    )
    .bind(cooldown_seconds)
    .fetch_all(pool)
    .await
}

/// Bump `last_indexer_scan_at` to now after the scheduler runs a scan.
/// Always called in a best-effort manner — if the scan failed we still
/// stamp it so a permanently-broken indexer doesn't cause the scheduler
/// to retry the same collection on every tick.
pub async fn touch_scanned(pool: &SqlitePool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE collections SET last_indexer_scan_at = ?1 WHERE id = ?2")
        .bind(Utc::now())
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Bump `last_visited_at` to now whenever a user opens the collection
/// detail page. The home-page Watchlist shelf badge counts the number
/// of `available_episodes` whose `found_at > last_visited_at` and which
/// don't have a matching `episode_files` row yet.
pub async fn touch_visited(pool: &SqlitePool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE collections SET last_visited_at = ?1 WHERE id = ?2")
        .bind(Utc::now())
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Collections whose SCENE-normalised title contains `needle` (itself
/// produced by `iris_media::filename::series_key` on the user's search
/// query — the same normaliser that wrote `parsed_title_normalized`, so
/// matching is consistent by construction). Powers the "already in your
/// library" rows pinned above tracker results on the search page.
/// Exact key matches sort first, then most recently played. Substring
/// containment also covers the `anime:` identity prefix and the
/// year-suffixed movie keys transparently.
pub async fn search_summaries(
    pool: &SqlitePool,
    needle: &str,
    limit: i64,
) -> Result<Vec<CollectionSummary>, sqlx::Error> {
    // `series_key` output is lowercase alphanumerics + spaces, but escape
    // LIKE metacharacters defensively — the needle is user-derived.
    let escaped = needle
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    sqlx::query_as::<_, CollectionSummary>(
        "SELECT \
            c.id, \
            c.tmdb_id AS tmdb_id, \
            c.display_title, c.kind, c.is_anime, c.created_at, \
            COUNT(DISTINCT t.id) AS torrent_count, \
            COALESCE(SUM(t.total_size_bytes), 0) AS total_size_bytes, \
            (SELECT COUNT(*) FROM episode_files ef WHERE ef.collection_id = c.id) AS episode_count, \
            (SELECT t2.infohash FROM torrents t2 \
             WHERE t2.collection_id = c.id AND t2.deleted_at IS NULL \
             ORDER BY COALESCE(t2.last_played_at, t2.added_at) DESC LIMIT 1) AS representative_infohash \
         FROM collections c \
         LEFT JOIN torrents t ON t.collection_id = c.id AND t.deleted_at IS NULL \
         WHERE c.parsed_title_normalized LIKE '%' || ?1 || '%' ESCAPE '\\' \
         GROUP BY c.id \
         HAVING torrent_count > 0 \
         ORDER BY (c.parsed_title_normalized = ?2) DESC, \
                  MAX(t.last_played_at) DESC NULLS LAST, c.created_at DESC \
         LIMIT ?3",
    )
    .bind(&escaped)
    .bind(needle)
    .bind(limit)
    .fetch_all(pool)
    .await
}

pub async fn list_summaries(pool: &SqlitePool) -> Result<Vec<CollectionSummary>, sqlx::Error> {
    // `tmdb_id` is the collection's own resolved id — the single source of truth.
    // A member torrent's `tmdb_id` is never consulted (it's an unreliable hint
    // that disagreed with the collection in prod). A freshly-ingested collection
    // shows no poster until its id is stamped (prewarm / verify / backfill), which
    // is the correct trade vs. rendering a wrong poster from a stray torrent id.
    sqlx::query_as::<_, CollectionSummary>(
        "SELECT \
            c.id, \
            c.tmdb_id AS tmdb_id, \
            c.display_title, c.kind, c.is_anime, c.created_at, \
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

/// The caller's GHOST collections: rows whose every torrent has been
/// reclaimed (soft-deleted — GC, admin cleanup) but that THIS user has
/// watch history in. Surfaced greyed-out in the Library so the user
/// can navigate back to the collection page, re-grab and resume —
/// without leaking other users' history (each user only ever sees the
/// ghosts they themselves watched; the shared live listing stays
/// [`list_summaries`]). Ordered by the user's most recent watch.
pub async fn list_ghost_summaries_for_user(
    pool: &SqlitePool,
    user_id: iris_core::ids::UserId,
) -> Result<Vec<CollectionSummary>, sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query_as::<_, CollectionSummary>(
        "SELECT \
            c.id, \
            c.tmdb_id AS tmdb_id, \
            c.display_title, c.kind, c.is_anime, c.created_at, \
            0 AS torrent_count, \
            0 AS total_size_bytes, \
            (SELECT COUNT(*) FROM episode_files ef WHERE ef.collection_id = c.id) AS episode_count, \
            NULL AS representative_infohash \
         FROM collections c \
         WHERE NOT EXISTS ( \
             SELECT 1 FROM torrents t \
             WHERE t.collection_id = c.id AND t.deleted_at IS NULL) \
           AND EXISTS ( \
             SELECT 1 FROM playback_progress p \
             JOIN torrents pt ON pt.infohash = p.infohash \
             WHERE p.user_id = ?1 AND pt.collection_id = c.id) \
         ORDER BY (SELECT MAX(p2.last_watched_at) FROM playback_progress p2 \
                   JOIN torrents pt2 ON pt2.infohash = p2.infohash \
                   WHERE p2.user_id = ?1 AND pt2.collection_id = c.id) DESC",
    )
    .bind(user)
    .fetch_all(pool)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn migrated_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("open in-memory sqlite");
        crate::migrate::run(&pool).await.expect("run migrations");
        pool
    }

    async fn ef_count(pool: &SqlitePool, cid: Uuid) -> i64 {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM episode_files WHERE collection_id = ?1")
            .bind(cid)
            .fetch_one(pool)
            .await
            .expect("count episode_files")
    }

    /// The anime noise-split merge re-homes children BEFORE deleting the loser,
    /// because `episode_files.collection_id` is `ON DELETE CASCADE` — a
    /// delete-first order would silently wipe the merged-in episodes.
    #[tokio::test]
    async fn merge_primitives_rehome_children_then_delete_loser() {
        let pool = migrated_pool().await;
        // Two halves of a noise split sharing one tmdb entity.
        let anime = find_or_create(&pool, "anime:nippon", "NIPPON", Kind::Tv, true)
            .await
            .unwrap();
        let plain = find_or_create(&pool, "nippon", "NIPPON", Kind::Tv, false)
            .await
            .unwrap();
        set_tmdb_id(&pool, anime.id, 312_474).await.unwrap();
        set_tmdb_id(&pool, plain.id, 312_474).await.unwrap();

        // An episode file under the plain twin (raw count avoids needing a
        // torrents row, which `list_for_collection` would require).
        crate::episode_files::upsert(
            &pool,
            crate::episode_files::UpsertEpisodeFile {
                collection_id: plain.id,
                season: 1,
                episode: 9,
                infohash: "abc".into(),
                file_idx: 0,
                derived_from: crate::episode_files::DerivedFrom::SceneParse,
                absolute_episode: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(ef_count(&pool, plain.id).await, 1);

        crate::episode_files::reassign_collection(&pool, plain.id, anime.id)
            .await
            .unwrap();
        assert_eq!(ef_count(&pool, plain.id).await, 0);
        assert_eq!(ef_count(&pool, anime.id).await, 1);

        delete(&pool, plain.id).await.unwrap();
        assert!(get(&pool, plain.id).await.unwrap().is_none());
        // The survivor keeps the moved episode file (no cascade wipe).
        assert_eq!(ef_count(&pool, anime.id).await, 1);
    }

    async fn make_user(pool: &SqlitePool) -> iris_core::ids::UserId {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO users (id, email, password_hash, display_name, is_admin, created_at) \
             VALUES (?1, ?2, '', 'T', 0, ?3)",
        )
        .bind(id)
        .bind(format!("{id}@t.test"))
        .bind(Utc::now())
        .execute(pool)
        .await
        .expect("insert user");
        iris_core::ids::UserId::from(id)
    }

    /// Ghosts are scoped to the user who watched them: a fully-GC'd
    /// collection appears in the watcher's ghost list only, and stops
    /// being a ghost as soon as any live torrent re-attaches.
    #[tokio::test]
    async fn ghost_summaries_are_scoped_to_the_watcher() {
        let pool = migrated_pool().await;
        let watcher = make_user(&pool).await;
        let stranger = make_user(&pool).await;
        let col = find_or_create(&pool, "goblin", "Goblin", Kind::Tv, false)
            .await
            .unwrap();
        let t = crate::torrents::upsert(
            &pool,
            crate::torrents::NewTorrent {
                infohash: Uuid::new_v4().to_string(),
                name: "Goblin.S01E01".into(),
                total_size_bytes: 1_000,
                source_provider: None,
                source_external_id: None,
                added_by: watcher,
            },
        )
        .await
        .unwrap();
        crate::torrents::set_collection(&pool, &t.infohash, Some(col.id))
            .await
            .unwrap();
        crate::playback::upsert(
            &pool,
            crate::playback::UpsertProgress {
                user_id: watcher,
                infohash: t.infohash.clone(),
                file_idx: 0,
                position_seconds: 60.0,
                duration_seconds: Some(1_200.0),
                audio_track_idx: None,
                subtitle_track_idx: None,
                completed: false,
            },
        )
        .await
        .unwrap();

        // Live torrent → not a ghost for anyone.
        assert!(
            list_ghost_summaries_for_user(&pool, watcher)
                .await
                .unwrap()
                .is_empty()
        );

        crate::torrents::soft_delete(&pool, iris_core::ids::TorrentId::from(t.id))
            .await
            .unwrap();

        let ghosts = list_ghost_summaries_for_user(&pool, watcher).await.unwrap();
        assert_eq!(ghosts.len(), 1);
        assert_eq!(ghosts[0].id, col.id);
        assert_eq!(ghosts[0].torrent_count, 0);
        // The stranger never watched it — no ghost leaks across users.
        assert!(
            list_ghost_summaries_for_user(&pool, stranger)
                .await
                .unwrap()
                .is_empty()
        );
    }
}
