//! Library-wide mapping `(collection_id, season, episode) → (infohash, file_idx)`.
//!
//! Populated at ingest time from SCENE filename parsing and by the
//! on-demand grab endpoint when a user explicitly asks "prépare E06".
//! NOT per-user — a file lives once on disk and any authorised user
//! can play it. The "did I watch it" stays in `playback_progress`.
//!
//! Keyed on `collection_id` (SCENE-parsed identity) rather than
//! `tmdb_id`. Indexers occasionally mis-tag torrents with the wrong
//! TMDB id; if the table were keyed on `tmdb_id`, those rogue rows
//! would surface as "downloaded" episodes on unrelated Watchlist
//! follows. Going through the collection — which is itself
//! SCENE-anchored — keeps each show's episode list honest.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct EpisodeFileRow {
    pub id: Uuid,
    pub collection_id: Uuid,
    pub season: i64,
    pub episode: i64,
    pub infohash: String,
    pub file_idx: i64,
    pub derived_from: String,
    pub created_at: DateTime<Utc>,
    /// Absolute episode number for fleuve-style anime releases
    /// (`One Piece S01E1156` → 1156). `NULL` for ordinary seasonal
    /// releases — the client renders a flat "Episode N" list only when
    /// a collection's episodes are absolute-numbered. See
    /// `iris_media::filename::absolute_from_parsed`.
    #[serde(default)]
    pub absolute_episode: Option<i64>,
}

/// All known files for a collection. Used by the Series detail page
/// (resolved follow → collection → these rows) and by the collection
/// detail page directly.
pub async fn list_for_collection(
    pool: &SqlitePool,
    collection_id: Uuid,
) -> Result<Vec<EpisodeFileRow>, sqlx::Error> {
    sqlx::query_as::<_, EpisodeFileRow>(
        "SELECT id, collection_id, season, episode, infohash, file_idx, derived_from, created_at, absolute_episode \
         FROM episode_files \
         WHERE collection_id = ?1 \
           AND EXISTS (SELECT 1 FROM torrents t \
                       WHERE t.infohash = episode_files.infohash AND t.deleted_at IS NULL) \
         ORDER BY season, episode",
    )
    .bind(collection_id)
    .fetch_all(pool)
    .await
}

/// One episode file whose source torrent was RECLAIMED, joined with
/// the CALLER's playback state — the "as if it were still on disk"
/// rendering. `episode_files` rows survive the GC, so the pre-reclaim
/// layout is fully reconstructible.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct GoneEpisodeFileRow {
    pub season: i64,
    pub episode: i64,
    pub absolute_episode: Option<i64>,
    pub infohash: String,
    pub file_idx: i64,
    pub torrent_name: String,
    /// Whole-release size — re-ingest is release-level, so a pack leaf
    /// reports the pack's size.
    pub total_size_bytes: i64,
    pub source_provider: Option<String>,
    pub source_external_id: Option<String>,
    pub completed: bool,
    pub position_seconds: Option<f64>,
    pub duration_seconds: Option<f64>,
    pub last_watched_at: Option<DateTime<Utc>>,
}

/// Gone episode rows for a collection, per-caller in BOTH senses:
/// only releases the caller has playback on surface (`episode_files`
/// is library-wide — without the gate everyone would see "Re-grab" on
/// releases they never touched; release-level on purpose, so a
/// half-watched pack keeps ALL its leaves), minus dismissed releases
/// (stale once a re-reclaim stamps a newer `deleted_at`).
pub async fn list_gone_for_collection(
    pool: &SqlitePool,
    collection_id: Uuid,
    user_id: iris_core::ids::UserId,
) -> Result<Vec<GoneEpisodeFileRow>, sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query_as::<_, GoneEpisodeFileRow>(
        "SELECT ef.season, ef.episode, ef.absolute_episode, ef.infohash, ef.file_idx, \
            t.name AS torrent_name, t.total_size_bytes, t.source_provider, t.source_external_id, \
            COALESCE(p.completed, 0) AS completed, \
            p.position_seconds, p.duration_seconds, p.last_watched_at \
         FROM episode_files ef \
         JOIN torrents t ON t.infohash = ef.infohash \
         LEFT JOIN playback_progress p \
            ON p.infohash = ef.infohash AND p.file_idx = ef.file_idx AND p.user_id = ?2 \
         WHERE ef.collection_id = ?1 AND t.deleted_at IS NOT NULL \
           AND EXISTS (SELECT 1 FROM playback_progress pe \
                       WHERE pe.user_id = ?2 AND pe.infohash = t.infohash) \
           AND NOT EXISTS (SELECT 1 FROM gone_release_dismissed gd \
                           WHERE gd.user_id = ?2 AND gd.infohash = t.infohash \
                             AND gd.dismissed_at >= t.deleted_at) \
         ORDER BY ef.season, ef.episode, ef.file_idx",
    )
    .bind(collection_id)
    .bind(user)
    .fetch_all(pool)
    .await
}

/// Single-season variant — saves a sort+filter when the caller knows
/// they only need one season's worth of rows.
pub async fn list_for_collection_season(
    pool: &SqlitePool,
    collection_id: Uuid,
    season: i64,
) -> Result<Vec<EpisodeFileRow>, sqlx::Error> {
    sqlx::query_as::<_, EpisodeFileRow>(
        "SELECT id, collection_id, season, episode, infohash, file_idx, derived_from, created_at, absolute_episode \
         FROM episode_files \
         WHERE collection_id = ?1 AND season = ?2 \
           AND EXISTS (SELECT 1 FROM torrents t \
                       WHERE t.infohash = episode_files.infohash AND t.deleted_at IS NULL) \
         ORDER BY episode",
    )
    .bind(collection_id)
    .bind(season)
    .fetch_all(pool)
    .await
}

/// All episode files for collections matching a SCENE-normalised
/// name. The Watchlist / Series page uses this — follows are
/// keyed on the normalised name, so we join via
/// `collections.parsed_title_normalized` and skip TMDB entirely.
pub async fn list_for_normalized(
    pool: &SqlitePool,
    normalized_name: &str,
) -> Result<Vec<EpisodeFileRow>, sqlx::Error> {
    sqlx::query_as::<_, EpisodeFileRow>(
        "SELECT ef.id, ef.collection_id, ef.season, ef.episode, ef.infohash, ef.file_idx, \
                ef.derived_from, ef.created_at, ef.absolute_episode \
         FROM episode_files ef \
         JOIN collections c ON c.id = ef.collection_id \
         WHERE c.parsed_title_normalized = ?1 AND c.kind = 'tv' \
           AND EXISTS (SELECT 1 FROM torrents t \
                       WHERE t.infohash = ef.infohash AND t.deleted_at IS NULL) \
         ORDER BY ef.season, ef.episode",
    )
    .bind(normalized_name)
    .fetch_all(pool)
    .await
}

/// The owned file for one exact episode of a collection — seasonal
/// `(season, episode)` match, or absolute-numbered match for fleuve
/// anime (`One Piece 1156` → `absolute_episode = 1156`). When the
/// query carried no season, only the absolute branch can hit, which
/// keeps a bare trailing number from matching `E05` of every season.
/// Seasonal hits win over absolute ones when both exist. Backs the
/// "you already have this exact episode" row on the search page.
pub async fn find_owned_episode(
    pool: &SqlitePool,
    collection_id: Uuid,
    season: Option<i64>,
    episode: i64,
) -> Result<Option<EpisodeFileRow>, sqlx::Error> {
    sqlx::query_as::<_, EpisodeFileRow>(
        "SELECT id, collection_id, season, episode, infohash, file_idx, derived_from, created_at, absolute_episode \
         FROM episode_files \
         WHERE collection_id = ?1 \
           AND ((?2 IS NOT NULL AND season = ?2 AND episode = ?3) OR absolute_episode = ?3) \
           AND EXISTS (SELECT 1 FROM torrents t \
                       WHERE t.infohash = episode_files.infohash AND t.deleted_at IS NULL) \
         ORDER BY (season IS ?2 AND episode = ?3) DESC \
         LIMIT 1",
    )
    .bind(collection_id)
    .bind(season)
    .bind(episode)
    .fetch_optional(pool)
    .await
}

/// How many episodes of one season the library holds (live torrents
/// only). Lets a season-scoped search query ("vikings s03") surface
/// the collection with an honest "N episodes of S03" instead of a
/// blanket "in library".
pub async fn count_owned_in_season(
    pool: &SqlitePool,
    collection_id: Uuid,
    season: i64,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM episode_files \
         WHERE collection_id = ?1 AND season = ?2 \
           AND EXISTS (SELECT 1 FROM torrents t \
                       WHERE t.infohash = episode_files.infohash AND t.deleted_at IS NULL)",
    )
    .bind(collection_id)
    .bind(season)
    .fetch_one(pool)
    .await
}

/// Library-wide `(collection_normalized_title, season, episode) → infohash`
/// index. Used by the search ranker to flag results whose SCENE
/// identity already maps to a file on disk, so the UI can disable
/// the "Add to library" CTA and prevent the user from downloading
/// the same episode twice under a different release group.
/// One round-trip per search request; the household scale keeps
/// this cheap (low thousands of rows max).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LibraryEpisodeKey {
    pub normalized_title: String,
    pub season: i64,
    pub episode: i64,
    pub infohash: String,
    pub file_idx: i64,
}

pub async fn list_library_keys(pool: &SqlitePool) -> Result<Vec<LibraryEpisodeKey>, sqlx::Error> {
    sqlx::query_as::<_, LibraryEpisodeKey>(
        "SELECT c.parsed_title_normalized AS normalized_title, \
                ef.season, ef.episode, ef.infohash, ef.file_idx \
         FROM episode_files ef \
         JOIN collections c ON c.id = ef.collection_id \
         WHERE c.kind = 'tv' \
           AND EXISTS (SELECT 1 FROM torrents t \
                       WHERE t.infohash = ef.infohash AND t.deleted_at IS NULL)",
    )
    .fetch_all(pool)
    .await
}

/// Lookup by physical file — used by the player's end-of-episode
/// "Préparer le suivant ?" flow to identify which episode just
/// finished playing.
pub async fn find_by_file(
    pool: &SqlitePool,
    infohash: &str,
    file_idx: i64,
) -> Result<Option<EpisodeFileRow>, sqlx::Error> {
    sqlx::query_as::<_, EpisodeFileRow>(
        "SELECT id, collection_id, season, episode, infohash, file_idx, derived_from, created_at, absolute_episode \
         FROM episode_files \
         WHERE infohash = ?1 AND file_idx = ?2 \
           AND EXISTS (SELECT 1 FROM torrents t \
                       WHERE t.infohash = episode_files.infohash AND t.deleted_at IS NULL)",
    )
    .bind(infohash)
    .bind(file_idx)
    .fetch_optional(pool)
    .await
}

/// Hard-delete every episode-file row for a physical torrent. Called
/// from the torrent-removal path: removing a torrent soft-deletes its
/// `torrents` row, drops the librqbit handle and wipes its files — the
/// matching `episode_files` rows must go too, otherwise the collection /
/// series views keep listing episodes that point at an infohash whose
/// torrent no longer exists ("stale collection entities"). Re-grabbing
/// the same release re-`upsert`s the rows, so a hard delete here is safe.
/// Returns the number of rows removed.
pub async fn delete_for_infohash(pool: &SqlitePool, infohash: &str) -> Result<u64, sqlx::Error> {
    let res = sqlx::query("DELETE FROM episode_files WHERE infohash = ?1")
        .bind(infohash)
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

/// Re-home every episode-file row from collection `from` to `to`. Part of
/// the anime noise-split merge: the surviving (anime) collection absorbs the
/// plain twin's rows BEFORE the twin is deleted (its `collection_id` FK is
/// `ON DELETE CASCADE`, so a delete-first order would wipe them). Safe wrt
/// the `UNIQUE(infohash, file_idx)` constraint — a given infohash lives in
/// exactly one collection, so the winner can't already hold a moved row.
/// Returns the number of rows re-homed.
pub async fn reassign_collection(
    pool: &SqlitePool,
    from: Uuid,
    to: Uuid,
) -> Result<u64, sqlx::Error> {
    let res = sqlx::query("UPDATE episode_files SET collection_id = ?2 WHERE collection_id = ?1")
        .bind(from)
        .bind(to)
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

#[derive(Debug, Clone)]
pub struct UpsertEpisodeFile {
    pub collection_id: Uuid,
    pub season: i64,
    pub episode: i64,
    pub infohash: String,
    pub file_idx: i64,
    pub derived_from: DerivedFrom,
    /// Absolute episode number for fleuve anime (`None` for seasonal).
    pub absolute_episode: Option<i64>,
}

#[derive(Debug, Clone, Copy)]
pub enum DerivedFrom {
    /// User-driven on-demand grab — we know S/E because the grab
    /// params said so, not because a filename matched.
    TmdbMatch,
    /// SCENE regex hit on the filename.
    SceneParse,
    /// Future: user assigned via admin UI.
    Manual,
}

impl DerivedFrom {
    fn as_str(self) -> &'static str {
        match self {
            DerivedFrom::TmdbMatch => "tmdb_match",
            DerivedFrom::SceneParse => "scene_parse",
            DerivedFrom::Manual => "manual",
        }
    }
}

/// Re-point an existing `scene_parse` row to corrected
/// `(season, episode)` values. Used by the boot/periodic reconcile
/// pass so a parser improvement (e.g. learning that `S02 E02` with a
/// space is still S02E02) retro-corrects packs ingested under the old
/// logic — the insert-only [`upsert`] can't, since the stale row
/// already claims `(infohash, file_idx)`.
///
/// Scoped to `derived_from = 'scene_parse'`: `tmdb_match` (user-driven
/// on-demand grab) and `manual` rows are explicit truth and must never
/// be rewritten by a filename re-parse. The
/// `season <> ?3 OR episode <> ?4` guard makes the steady state a
/// no-op (`rows_affected = 0`), so calling this every backfill tick is
/// free once everything has converged. Returns `true` when a row was
/// actually corrected.
pub async fn correct_scene_parsed(
    pool: &SqlitePool,
    infohash: &str,
    file_idx: i64,
    season: i64,
    episode: i64,
) -> Result<bool, sqlx::Error> {
    let res = sqlx::query(
        "UPDATE episode_files \
            SET season = ?3, episode = ?4 \
          WHERE infohash = ?1 AND file_idx = ?2 \
            AND derived_from = 'scene_parse' \
            AND (season <> ?3 OR episode <> ?4)",
    )
    .bind(infohash)
    .bind(file_idx)
    .bind(season)
    .bind(episode)
    .execute(pool)
    .await?;
    Ok(res.rows_affected() > 0)
}

/// Insert if `(infohash, file_idx)` isn't already claimed; do nothing
/// otherwise. Same physical file can't map to two different episodes
/// (the UNIQUE index in the migration enforces this), and re-running
/// the assignment job at ingest time should be a no-op.
pub async fn upsert(pool: &SqlitePool, ef: UpsertEpisodeFile) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO episode_files \
            (id, collection_id, season, episode, infohash, file_idx, derived_from, created_at, absolute_episode) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
         ON CONFLICT(infohash, file_idx) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(ef.collection_id)
    .bind(ef.season)
    .bind(ef.episode)
    .bind(&ef.infohash)
    .bind(ef.file_idx)
    .bind(ef.derived_from.as_str())
    .bind(Utc::now())
    .bind(ef.absolute_episode)
    .execute(pool)
    .await?;
    Ok(())
}

/// Variant of [`correct_scene_parsed`] that also re-derives the
/// `absolute_episode`. Used by the boot self-heal to back-fill the
/// absolute number on already-ingested anime files (the original
/// insert predates the column). Same `scene_parse`-only scoping; the
/// guard makes the converged state a no-op. Returns `true` on a change.
pub async fn correct_scene_parsed_with_absolute(
    pool: &SqlitePool,
    infohash: &str,
    file_idx: i64,
    season: i64,
    episode: i64,
    absolute_episode: Option<i64>,
) -> Result<bool, sqlx::Error> {
    let res = sqlx::query(
        "UPDATE episode_files \
            SET season = ?3, episode = ?4, absolute_episode = ?5 \
          WHERE infohash = ?1 AND file_idx = ?2 \
            AND derived_from = 'scene_parse' \
            AND (season <> ?3 OR episode <> ?4 \
                 OR absolute_episode IS NOT ?5)",
    )
    .bind(infohash)
    .bind(file_idx)
    .bind(season)
    .bind(episode)
    .bind(absolute_episode)
    .execute(pool)
    .await?;
    Ok(res.rows_affected() > 0)
}
