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

/// `CollectionRow` column list, shared so the reads can't drift from the
/// struct. A macro so it stays a literal inside `concat!` (sqlx 0.9 only
/// takes `&'static str`).
macro_rules! collection_columns {
    () => {
        "id, tmdb_id, parsed_title_normalized, display_title, kind, created_at, \
         last_indexer_scan_at, last_visited_at, is_anime, anilist_id, tmdb_trust"
    };
}

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
    /// Which signal backs `tmdb_id` (`admin` / `tracker_scene` / `scene` /
    /// `tracker`, see migration 0045). `None` with an id = legacy row, not
    /// yet re-evaluated by `tmdb-trust --apply`.
    #[serde(default)]
    pub tmdb_trust: Option<String>,
}

/// A collection's kind; the `kind` column holds its wire form.
pub use iris_core::search::MediaKind as Kind;

impl CollectionRow {
    /// A series. Same rule as [`Kind::from_stored`]: an unknown kind reads as one.
    pub fn is_tv(&self) -> bool {
        Kind::from_stored(&self.kind) == Kind::Tv
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
    sqlx::query_as::<_, CollectionRow>(concat!(
        "SELECT ",
        collection_columns!(),
        " FROM collections WHERE tmdb_id = ?1 ORDER BY created_at"
    ))
    .bind(tmdb_id)
    .fetch_all(pool)
    .await
}

pub async fn find_by_parsed_title(
    pool: &SqlitePool,
    normalized: &str,
    kind: Kind,
) -> Result<Option<CollectionRow>, sqlx::Error> {
    sqlx::query_as::<_, CollectionRow>(concat!(
        "SELECT ",
        collection_columns!(),
        " FROM collections \
         WHERE parsed_title_normalized = ?1 AND kind = ?2"
    ))
    .bind(normalized)
    .bind(kind.as_wire())
    .fetch_optional(pool)
    .await
}

/// The TMDB id of the oldest TV collection under a normalised SCENE title,
/// when one carries an id.
pub async fn first_tv_tmdb_id(
    pool: &SqlitePool,
    normalized: &str,
) -> Result<Option<i64>, sqlx::Error> {
    let row: Option<(i64,)> = sqlx::query_as(
        "SELECT tmdb_id FROM collections \
         WHERE parsed_title_normalized = ?1 AND kind = 'tv' AND tmdb_id IS NOT NULL \
         ORDER BY created_at LIMIT 1",
    )
    .bind(normalized)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(t,)| t))
}

/// Every collection currently in the library. Used by the TMDB
/// backfill to walk the canonical SCENE-grouped entities directly,
/// rather than re-deriving the title from individual member torrents
/// (one of which could be poorly named and resolve to garbage).
pub async fn list_all(pool: &SqlitePool) -> Result<Vec<CollectionRow>, sqlx::Error> {
    sqlx::query_as::<_, CollectionRow>(concat!(
        "SELECT ",
        collection_columns!(),
        " FROM collections \
         ORDER BY created_at"
    ))
    .fetch_all(pool)
    .await
}

pub async fn get(pool: &SqlitePool, id: Uuid) -> Result<Option<CollectionRow>, sqlx::Error> {
    sqlx::query_as::<_, CollectionRow>(concat!(
        "SELECT ",
        collection_columns!(),
        " FROM collections WHERE id = ?1"
    ))
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
    .bind(kind.as_wire())
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

/// Detach the `tmdb_id` (and its trust) from a collection. Used by the
/// identity self-heal when the SCENE key changes — the id was resolved from
/// the OLD title and is presumed poison, so the next evaluation starts from
/// a clean slate — and by `tmdb-trust --apply` for a match no trusted
/// signal backs.
pub async fn clear_tmdb_id(pool: &SqlitePool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE collections SET tmdb_id = NULL, tmdb_trust = NULL WHERE id = ?1")
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
        .bind(kind.as_wire())
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
    let mut tx = pool.begin().await?;
    let old: Option<String> =
        sqlx::query_scalar("SELECT parsed_title_normalized FROM collections WHERE id = ?1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .flatten();
    sqlx::query("UPDATE collections SET parsed_title_normalized = ?1 WHERE id = ?2")
        .bind(normalized)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    if let Some(old) = old.as_deref().filter(|old| *old != normalized) {
        carry_name_keyed_rows(&mut tx, old, normalized).await?;
    }
    tx.commit().await
}

/// A collection's key moved from `from` to `to`: its follows go with it (a
/// user already following `to` keeps that one), and the offers cached for
/// the dead name go (the next scan repopulates `to`). Without it a renamed
/// collection strands its Watchlist follows on a name nothing joins.
async fn carry_name_keyed_rows(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    from: &str,
    to: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE OR IGNORE series_follows SET normalized_name = ?2 WHERE normalized_name = ?1",
    )
    .bind(from)
    .bind(to)
    .execute(&mut **tx)
    .await?;
    sqlx::query("DELETE FROM series_follows WHERE normalized_name = ?1")
        .bind(from)
        .execute(&mut **tx)
        .await?;
    sqlx::query("DELETE FROM available_episodes WHERE normalized_name = ?1")
        .bind(from)
        .execute(&mut **tx)
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
///
/// A changed id un-verifies the member torrents in the same transaction:
/// their runtime check vouched for the old id, not this one.
pub async fn set_tmdb_id(pool: &SqlitePool, id: Uuid, tmdb_id: i64) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let changed =
        sqlx::query("UPDATE collections SET tmdb_id = ?1 WHERE id = ?2 AND tmdb_id IS NOT ?1")
            .bind(tmdb_id)
            .bind(id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    if changed > 0 {
        sqlx::query("UPDATE torrents SET tmdb_verified = FALSE WHERE collection_id = ?1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await
}

/// Store a trusted TMDB match: the id and the signal backing it
/// (`tmdb_trust`). The caller has already applied the conflict rule
/// (`tmdb_trust::should_replace`). A changed id un-verifies the member
/// torrents' legacy runtime flag in the same transaction.
pub async fn set_trusted_tmdb(
    pool: &SqlitePool,
    id: Uuid,
    tmdb_id: i64,
    trust: &str,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let (old,): (Option<i64>,) = sqlx::query_as("SELECT tmdb_id FROM collections WHERE id = ?1")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    sqlx::query("UPDATE collections SET tmdb_id = ?1, tmdb_trust = ?2 WHERE id = ?3")
        .bind(tmdb_id)
        .bind(trust)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    if old != Some(tmdb_id) {
        sqlx::query("UPDATE torrents SET tmdb_verified = FALSE WHERE collection_id = ?1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await
}

/// Distinct tracker-shipped TMDB ids across the collection's torrents,
/// soft-deleted ones included (a reclaimed release still vouched for the
/// title). Only ids recorded as tracker-provided count.
pub async fn tracker_tmdb_ids(pool: &SqlitePool, id: Uuid) -> Result<Vec<i64>, sqlx::Error> {
    let rows: Vec<(i64,)> = sqlx::query_as(
        "SELECT DISTINCT tmdb_id FROM torrents \
         WHERE collection_id = ?1 AND tmdb_id_source = 'tracker' AND tmdb_id IS NOT NULL \
         ORDER BY tmdb_id",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(t,)| t).collect())
}

/// Hard-delete a collection row. Member torrents are `ON DELETE SET NULL`
/// (re-orphaned, then re-assigned on the next backfill tick); member
/// `episode_files` are `ON DELETE CASCADE`. A merge that must PRESERVE
/// those children goes through [`merge_into`].
pub async fn delete(pool: &SqlitePool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM collections WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Fold the `loser` collection into `winner` in one transaction: torrents,
/// episode files and the per-user rows keyed on the collection (series
/// playback preferences, Continue-Watching and ghost dismissals) move over,
/// follows and offers keyed on the loser's name are re-keyed or dropped,
/// then the emptied loser goes. In one transaction so an ingest landing on
/// the loser mid-merge can't lose its episode files to the delete's
/// cascade. A user row that already exists on the winner wins.
pub async fn merge_into(
    pool: &SqlitePool,
    loser: &CollectionRow,
    winner: &CollectionRow,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    for sql in [
        "UPDATE torrents SET collection_id = ?2 WHERE collection_id = ?1",
        "UPDATE episode_files SET collection_id = ?2 WHERE collection_id = ?1",
        "UPDATE OR IGNORE collection_playback_preferences SET collection_id = ?2 \
         WHERE collection_id = ?1",
        "UPDATE OR IGNORE cw_dismissed SET collection_id = ?2 WHERE collection_id = ?1",
        "UPDATE OR IGNORE ghost_dismissed SET collection_id = ?2 WHERE collection_id = ?1",
    ] {
        sqlx::query(sql)
            .bind(loser.id)
            .bind(winner.id)
            .execute(&mut *tx)
            .await?;
    }
    if let (Some(lnorm), Some(wnorm)) = (
        loser.parsed_title_normalized.as_deref(),
        winner.parsed_title_normalized.as_deref(),
    ) {
        carry_name_keyed_rows(&mut tx, lnorm, wnorm).await?;
    }
    sqlx::query("DELETE FROM collections WHERE id = ?1")
        .bind(loser.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await
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
    .bind(kind.as_wire())
    .bind(now)
    .execute(pool)
    .await?;
    get(pool, id).await?.ok_or(sqlx::Error::RowNotFound)
}

/// A collection's episodes on disk: distinct (season, episode) of its live
/// releases. Not every `episode_files` row: two releases of one episode are one
/// episode, a reclaimed release keeps its rows (the ghost list), and episode 0
/// is the season-pack sentinel, no episode. A macro so it stays a literal
/// inside `concat!` (sqlx 0.9 only takes `&'static str`).
macro_rules! episode_count_column {
    () => {
        "(SELECT COUNT(DISTINCT ef.season || ':' || ef.episode) FROM episode_files ef \
          WHERE ef.collection_id = c.id AND ef.episode > 0 \
            AND EXISTS (SELECT 1 FROM torrents te \
                        WHERE te.infohash = ef.infohash AND te.deleted_at IS NULL)) AS episode_count"
    };
}

/// [`episode_count_column`] for a reclaimed collection: its releases are all
/// gone, so their episodes count (what it held before the GC).
macro_rules! ghost_episode_count_column {
    () => {
        "(SELECT COUNT(DISTINCT ef.season || ':' || ef.episode) FROM episode_files ef \
          WHERE ef.collection_id = c.id AND ef.episode > 0) AS episode_count"
    };
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
/// What the household watched last comes first (anyone's latest play in the
/// series; opening a page doesn't count), then the newest series.
pub async fn list_tv_with_episodes(pool: &SqlitePool) -> Result<Vec<CollectionRow>, sqlx::Error> {
    sqlx::query_as::<_, CollectionRow>(concat!(
        "SELECT ",
        collection_columns!(),
        " FROM collections c \
         WHERE c.kind = 'tv' \
           AND c.parsed_title_normalized IS NOT NULL \
           AND EXISTS ( \
             SELECT 1 FROM episode_files ef \
             JOIN torrents t ON t.infohash = ef.infohash \
             WHERE ef.collection_id = c.id AND t.deleted_at IS NULL \
           ) \
         ORDER BY COALESCE( \
             (SELECT MAX(p.last_watched_at) FROM playback_progress p \
              JOIN torrents t ON t.infohash = p.infohash \
              WHERE t.collection_id = c.id), \
             c.created_at) DESC, c.created_at DESC"
    ))
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
    sqlx::query_as::<_, CollectionRow>(concat!(
        "SELECT ",
        collection_columns!(),
        " FROM collections \
         WHERE kind = 'tv' \
           AND parsed_title_normalized IS NOT NULL \
           AND (last_indexer_scan_at IS NULL \
                OR last_indexer_scan_at < ?1) \
         ORDER BY last_indexer_scan_at IS NOT NULL, last_indexer_scan_at"
    ))
    // The column holds chrono's RFC 3339 (`…T…+00:00`); SQLite's
    // `datetime('now')` (`… …`) would sort after it all day long.
    .bind(Utc::now() - chrono::TimeDelta::seconds(cooldown_seconds))
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
        concat!("SELECT \
            c.id, \
            c.tmdb_id AS tmdb_id, \
            c.display_title, c.kind, c.is_anime, c.created_at, \
            COUNT(DISTINCT t.id) AS torrent_count, \
            COALESCE(SUM(t.total_size_bytes), 0) AS total_size_bytes, \
            ", episode_count_column!(), ", \
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
         LIMIT ?3"),
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
        concat!("SELECT \
            c.id, \
            c.tmdb_id AS tmdb_id, \
            c.display_title, c.kind, c.is_anime, c.created_at, \
            COUNT(DISTINCT t.id) AS torrent_count, \
            COALESCE(SUM(t.total_size_bytes), 0) AS total_size_bytes, \
            ", episode_count_column!(), ", \
            (SELECT t2.infohash FROM torrents t2 \
             WHERE t2.collection_id = c.id AND t2.deleted_at IS NULL \
             ORDER BY COALESCE(t2.last_played_at, t2.added_at) DESC LIMIT 1) AS representative_infohash \
         FROM collections c \
         LEFT JOIN torrents t ON t.collection_id = c.id AND t.deleted_at IS NULL \
         GROUP BY c.id \
         HAVING torrent_count > 0 \
         ORDER BY MAX(t.last_played_at) DESC NULLS LAST, c.created_at DESC"),
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
/// Dismissed ghosts stay hidden until newer watch activity.
pub async fn list_ghost_summaries_for_user(
    pool: &SqlitePool,
    user_id: iris_core::ids::UserId,
) -> Result<Vec<CollectionSummary>, sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query_as::<_, CollectionSummary>(concat!(
        "SELECT \
            c.id, \
            c.tmdb_id AS tmdb_id, \
            c.display_title, c.kind, c.is_anime, c.created_at, \
            0 AS torrent_count, \
            0 AS total_size_bytes, \
            ",
        ghost_episode_count_column!(),
        ", \
            NULL AS representative_infohash \
         FROM collections c \
         WHERE NOT EXISTS ( \
             SELECT 1 FROM torrents t \
             WHERE t.collection_id = c.id AND t.deleted_at IS NULL) \
           AND EXISTS ( \
             SELECT 1 FROM playback_progress p \
             JOIN torrents pt ON pt.infohash = p.infohash \
             WHERE p.user_id = ?1 AND pt.collection_id = c.id) \
           AND NOT EXISTS ( \
             SELECT 1 FROM ghost_dismissed gh \
             WHERE gh.user_id = ?1 AND gh.collection_id = c.id \
               AND gh.dismissed_at >= (SELECT MAX(p3.last_watched_at) \
                                       FROM playback_progress p3 \
                                       JOIN torrents pt3 ON pt3.infohash = p3.infohash \
                                       WHERE p3.user_id = ?1 AND pt3.collection_id = c.id)) \
         ORDER BY (SELECT MAX(p2.last_watched_at) FROM playback_progress p2 \
                   JOIN torrents pt2 ON pt2.infohash = p2.infohash \
                   WHERE p2.user_id = ?1 AND pt2.collection_id = c.id) DESC"
    ))
    .bind(user)
    .fetch_all(pool)
    .await
}

/// Hide a ghost collection from the CALLER's Library grid.
/// Timestamped like `cw_dismissed` — newer watch activity makes it
/// stale and the card returns. Never touches `playback_progress`.
pub async fn dismiss_ghost(
    pool: &SqlitePool,
    user_id: iris_core::ids::UserId,
    collection_id: Uuid,
) -> Result<(), sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query(
        "INSERT INTO ghost_dismissed (user_id, collection_id, dismissed_at) \
         VALUES (?1, ?2, ?3) \
         ON CONFLICT(user_id, collection_id) DO UPDATE SET dismissed_at = excluded.dismissed_at",
    )
    .bind(user)
    .bind(collection_id)
    .bind(chrono::Utc::now())
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{make_user, migrated_pool};

    async fn ef_count(pool: &SqlitePool, cid: Uuid) -> i64 {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM episode_files WHERE collection_id = ?1")
            .bind(cid)
            .fetch_one(pool)
            .await
            .expect("count episode_files")
    }

    /// The library's "N episodes" counts episodes, not rows: one per (season,
    /// episode) of the live releases, never the season-pack sentinel.
    #[tokio::test]
    async fn summary_counts_distinct_episodes_on_disk() {
        let pool = migrated_pool().await;
        let user = make_user(&pool).await;
        let col = find_or_create(&pool, "severance", "Severance", Kind::Tv, false)
            .await
            .unwrap();
        let mut hashes = Vec::new();
        for name in [
            "Severance.S01.FRENCH",
            "Severance.S01.MULTi",
            "Severance.S02E01",
        ] {
            let t = crate::torrents::upsert(
                &pool,
                crate::torrents::NewTorrent {
                    infohash: Uuid::new_v4().to_string(),
                    name: name.into(),
                    total_size_bytes: 1_000,
                    source_provider: None,
                    source_external_id: None,
                    tracker_tmdb_id: None,
                    added_by: user,
                },
            )
            .await
            .unwrap();
            crate::torrents::set_collection(&pool, &t.infohash, Some(col.id))
                .await
                .unwrap();
            hashes.push(t.infohash);
        }
        let file = |infohash: &str, season: i64, episode: i64, file_idx: i64| {
            crate::episode_files::UpsertEpisodeFile {
                collection_id: col.id,
                season,
                episode,
                infohash: infohash.into(),
                file_idx,
                derived_from: crate::episode_files::DerivedFrom::SceneParse,
                absolute_episode: None,
            }
        };
        for e in [1, 2] {
            crate::episode_files::upsert(&pool, file(&hashes[0], 1, e, e - 1))
                .await
                .unwrap();
            crate::episode_files::upsert(&pool, file(&hashes[1], 1, e, e - 1))
                .await
                .unwrap();
        }
        crate::episode_files::upsert(&pool, file(&hashes[1], 1, 0, 9))
            .await
            .unwrap();
        crate::episode_files::upsert(&pool, file(&hashes[2], 2, 1, 0))
            .await
            .unwrap();
        let count = |pool: SqlitePool| async move {
            list_summaries(&pool)
                .await
                .unwrap()
                .into_iter()
                .find(|s| s.id == col.id)
                .unwrap()
                .episode_count
        };
        assert_eq!(count(pool.clone()).await, 3);
        assert_eq!(
            crate::episode_files::count_owned_in_season(&pool, col.id, 1)
                .await
                .unwrap(),
            2
        );
        // A reclaimed release's episode is no longer on disk.
        sqlx::query("UPDATE torrents SET deleted_at = ?1 WHERE infohash = ?2")
            .bind(Utc::now())
            .bind(&hashes[2])
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(count(pool.clone()).await, 2);
    }

    #[tokio::test]
    async fn first_tv_tmdb_id_skips_movies_and_unresolved() {
        let pool = migrated_pool().await;
        assert_eq!(first_tv_tmdb_id(&pool, "dune").await.unwrap(), None);
        let film = find_or_create(&pool, "dune", "Dune", Kind::Movie, false)
            .await
            .unwrap();
        set_tmdb_id(&pool, film.id, 438_631).await.unwrap();
        find_or_create(&pool, "dune", "Dune", Kind::Tv, false)
            .await
            .unwrap();
        assert_eq!(first_tv_tmdb_id(&pool, "dune").await.unwrap(), None);
        let show = find_by_parsed_title(&pool, "dune", Kind::Tv)
            .await
            .unwrap()
            .unwrap();
        set_tmdb_id(&pool, show.id, 90_228).await.unwrap();
        assert_eq!(first_tv_tmdb_id(&pool, "dune").await.unwrap(), Some(90_228));
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
        // A per-series language choice made on the plain twin.
        let user = make_user(&pool).await;
        let french = crate::playback_preferences::PlaybackPreferences {
            audio_language: Some("fre".into()),
            subtitle_language: None,
        };
        crate::playback_preferences::set_for_collection(&pool, user, plain.id, &french)
            .await
            .unwrap();
        let plain = get(&pool, plain.id).await.unwrap().unwrap();
        let anime = get(&pool, anime.id).await.unwrap().unwrap();

        merge_into(&pool, &plain, &anime).await.unwrap();

        assert!(get(&pool, plain.id).await.unwrap().is_none());
        // The survivor keeps the moved episode file (no cascade wipe)…
        assert_eq!(ef_count(&pool, anime.id).await, 1);
        // …and the user's series preference.
        let prefs = crate::playback_preferences::get_for_collection(&pool, user, anime.id)
            .await
            .unwrap();
        assert!(prefs.for_collection());
        assert_eq!(prefs.merged.audio_language.as_deref(), Some("fre"));
    }

    /// Ghosts are scoped to the user who watched them: a fully-GC'd
    /// collection appears in the watcher's ghost list only, and stops
    /// being a ghost as soon as any live torrent re-attaches.
    #[tokio::test]
    async fn a_renamed_collection_takes_its_follows_along() {
        let pool = migrated_pool().await;
        let ana = make_user(&pool).await;
        let bo = make_user(&pool).await;
        let col = find_or_create(&pool, "dr", "Dr", Kind::Tv, false)
            .await
            .unwrap();
        crate::follows::add(&pool, ana, "dr", "Dr", None)
            .await
            .unwrap();
        crate::follows::add(&pool, bo, "dr", "Dr", None)
            .await
            .unwrap();
        crate::follows::add(&pool, bo, "dr stone", "Dr. Stone", None)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO available_episodes (id, normalized_name, season, episode, \
             indexer_provider, indexer_torrent_id, magnet, found_at) \
             VALUES (?1, 'dr', 1, 1, 'p', 'x', '', ?2)",
        )
        .bind(Uuid::new_v4())
        .bind(Utc::now())
        .execute(&pool)
        .await
        .unwrap();

        set_parsed_title_normalized(&pool, col.id, "dr stone")
            .await
            .unwrap();

        let keys = |user| {
            let pool = pool.clone();
            async move {
                crate::follows::list_for_user(&pool, user)
                    .await
                    .unwrap()
                    .into_iter()
                    .map(|f| f.normalized_name)
                    .collect::<Vec<_>>()
            }
        };
        assert_eq!(keys(ana).await, ["dr stone"]);
        assert_eq!(keys(bo).await, ["dr stone"], "one follow per user and name");
        let offers: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM available_episodes")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(offers, 0, "offers for the dead name go");
    }

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
                tracker_tmdb_id: None,
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

        // Ghosts never surface as search "In library" matches — the
        // dismissal story depends on it (a hidden entry must not come
        // back through the search page; only new watch/download
        // activity resurfaces it).
        assert!(
            search_summaries(&pool, "goblin", 8)
                .await
                .unwrap()
                .is_empty(),
            "fully-reclaimed collections stay out of search matches",
        );

        // Dismissal hides the ghost card for the watcher…
        dismiss_ghost(&pool, watcher, col.id).await.unwrap();
        assert!(
            list_ghost_summaries_for_user(&pool, watcher)
                .await
                .unwrap()
                .is_empty(),
            "dismissed ghost leaves the Library grid",
        );

        // …until newer watch activity makes it stale (same Netflix-style
        // staleness as cw_dismissed): re-watching bumps last_watched_at
        // past dismissed_at and the ghost returns.
        crate::playback::upsert(
            &pool,
            crate::playback::UpsertProgress {
                user_id: watcher,
                infohash: t.infohash.clone(),
                file_idx: 0,
                position_seconds: 300.0,
                duration_seconds: Some(1_200.0),
                audio_track_idx: None,
                subtitle_track_idx: None,
                completed: false,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            list_ghost_summaries_for_user(&pool, watcher)
                .await
                .unwrap()
                .len(),
            1,
            "newer watch activity resurfaces the dismissed ghost",
        );
    }

    #[tokio::test]
    async fn replacing_the_tmdb_id_unverifies_member_torrents() {
        let pool = migrated_pool().await;
        let user = make_user(&pool).await;
        let film = find_or_create(&pool, "heat 1995", "Heat", Kind::Movie, false)
            .await
            .unwrap();
        set_tmdb_id(&pool, film.id, 949).await.unwrap();
        let t = crate::torrents::upsert(
            &pool,
            crate::torrents::NewTorrent {
                infohash: "ab".repeat(20),
                name: "Heat.1995.1080p".into(),
                total_size_bytes: 1,
                source_provider: None,
                source_external_id: None,
                tracker_tmdb_id: None,
                added_by: user,
            },
        )
        .await
        .unwrap();
        crate::torrents::set_collection(&pool, &t.infohash, Some(film.id))
            .await
            .unwrap();
        crate::torrents::set_tmdb_verified(&pool, &t.infohash, true)
            .await
            .unwrap();
        let verified = |pool: SqlitePool, ih: String| async move {
            crate::torrents::find_by_infohash(&pool, &ih)
                .await
                .unwrap()
                .unwrap()
                .tmdb_verified
        };

        set_tmdb_id(&pool, film.id, 949).await.unwrap();
        assert!(verified(pool.clone(), t.infohash.clone()).await, "same id");
        set_tmdb_id(&pool, film.id, 11).await.unwrap();
        assert!(!verified(pool.clone(), t.infohash.clone()).await, "new id");
    }

    #[tokio::test]
    async fn a_scan_is_due_once_its_cooldown_has_passed_the_same_day() {
        let pool = migrated_pool().await;
        let show = find_or_create(&pool, "severance", "Severance", Kind::Tv, false)
            .await
            .unwrap();
        let due = |pool: SqlitePool| async move {
            list_due_for_scan(&pool, 7_200)
                .await
                .unwrap()
                .iter()
                .map(|c| c.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(due(pool.clone()).await, vec![show.id], "never scanned");
        touch_scanned(&pool, show.id).await.unwrap();
        assert!(due(pool.clone()).await.is_empty(), "inside the cooldown");
        sqlx::query("UPDATE collections SET last_indexer_scan_at = ?1 WHERE id = ?2")
            .bind(Utc::now() - chrono::TimeDelta::hours(3))
            .bind(show.id)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(due(pool.clone()).await, vec![show.id], "cooldown elapsed");
    }

    #[tokio::test]
    async fn the_watchlist_puts_what_the_household_watched_last_first() {
        let pool = migrated_pool().await;
        let user = make_user(&pool).await;
        let mut shows = Vec::new();
        for (i, name) in ["older", "watched"].into_iter().enumerate() {
            let c = find_or_create(&pool, name, name, Kind::Tv, false)
                .await
                .unwrap();
            let ih = format!("{i:040}");
            crate::torrents::upsert(
                &pool,
                crate::torrents::NewTorrent {
                    infohash: ih.clone(),
                    name: name.into(),
                    total_size_bytes: 1,
                    source_provider: None,
                    source_external_id: None,
                    tracker_tmdb_id: None,
                    added_by: user,
                },
            )
            .await
            .unwrap();
            crate::torrents::set_collection(&pool, &ih, Some(c.id))
                .await
                .unwrap();
            crate::episode_files::upsert(
                &pool,
                crate::episode_files::UpsertEpisodeFile {
                    collection_id: c.id,
                    season: 1,
                    episode: 1,
                    infohash: ih.clone(),
                    file_idx: 0,
                    derived_from: crate::episode_files::DerivedFrom::SceneParse,
                    absolute_episode: None,
                },
            )
            .await
            .unwrap();
            shows.push((c.id, ih));
        }
        // Nobody has watched yet: the newest series ("watched") leads.
        let order = |rows: Vec<CollectionRow>| rows.into_iter().map(|c| c.id).collect::<Vec<_>>();
        assert_eq!(
            order(list_tv_with_episodes(&pool).await.unwrap()),
            vec![shows[1].0, shows[0].0]
        );
        // Opening a page no longer reorders the list.
        touch_visited(&pool, shows[0].0).await.unwrap();
        assert_eq!(
            order(list_tv_with_episodes(&pool).await.unwrap())[0],
            shows[1].0
        );
        crate::playback::mark_completed(&pool, user, &shows[0].1, 0)
            .await
            .unwrap();
        assert_eq!(
            order(list_tv_with_episodes(&pool).await.unwrap())[0],
            shows[0].0
        );
    }
}
