use chrono::{DateTime, Utc};
use iris_core::ids::UserId;
use serde::Serialize;
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct ProgressRow {
    pub user_id: Uuid,
    pub infohash: String,
    pub file_idx: i64,
    pub position_seconds: f64,
    pub duration_seconds: Option<f64>,
    pub audio_track_idx: Option<i64>,
    pub subtitle_track_idx: Option<i64>,
    pub completed: bool,
    pub last_watched_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct UpsertProgress {
    pub user_id: UserId,
    pub infohash: String,
    pub file_idx: i64,
    pub position_seconds: f64,
    pub duration_seconds: Option<f64>,
    pub audio_track_idx: Option<i64>,
    pub subtitle_track_idx: Option<i64>,
    pub completed: bool,
}

pub async fn get(
    pool: &SqlitePool,
    user_id: UserId,
    infohash: &str,
    file_idx: i64,
) -> Result<Option<ProgressRow>, sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query_as::<_, ProgressRow>(
        "SELECT user_id, infohash, file_idx, position_seconds, duration_seconds, \
         audio_track_idx, subtitle_track_idx, completed, last_watched_at \
         FROM playback_progress \
         WHERE user_id = ?1 AND infohash = ?2 AND file_idx = ?3",
    )
    .bind(user)
    .bind(infohash)
    .bind(file_idx)
    .fetch_optional(pool)
    .await
}

pub async fn list_for_torrent(
    pool: &SqlitePool,
    user_id: UserId,
    infohash: &str,
) -> Result<Vec<ProgressRow>, sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query_as::<_, ProgressRow>(
        "SELECT user_id, infohash, file_idx, position_seconds, duration_seconds, \
         audio_track_idx, subtitle_track_idx, completed, last_watched_at \
         FROM playback_progress \
         WHERE user_id = ?1 AND infohash = ?2",
    )
    .bind(user)
    .bind(infohash)
    .fetch_all(pool)
    .await
}

/// Remove the user's progress for one file — "remove from Continue Watching".
/// Deletes the row outright (per-user; the file on disk is untouched), so the
/// tile leaves both the shelf and the caller's history.
pub async fn delete(
    pool: &SqlitePool,
    user_id: UserId,
    infohash: &str,
    file_idx: i64,
) -> Result<(), sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query(
        "DELETE FROM playback_progress WHERE user_id = ?1 AND infohash = ?2 AND file_idx = ?3",
    )
    .bind(user)
    .bind(infohash)
    .bind(file_idx)
    .execute(pool)
    .await?;
    Ok(())
}

/// Mark one file watched — "mark as watched". Sets `completed = 1` on an
/// existing row, or inserts a completed row (position 0) when the user never
/// started it (e.g. skipping a "next up" episode). Idempotent.
pub async fn mark_completed(
    pool: &SqlitePool,
    user_id: UserId,
    infohash: &str,
    file_idx: i64,
) -> Result<(), sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query(
        "INSERT INTO playback_progress \
            (user_id, infohash, file_idx, position_seconds, completed, last_watched_at) \
         VALUES (?1, ?2, ?3, 0, 1, ?4) \
         ON CONFLICT(user_id, infohash, file_idx) DO UPDATE SET \
            completed = 1, last_watched_at = excluded.last_watched_at",
    )
    .bind(user)
    .bind(infohash)
    .bind(file_idx)
    .bind(Utc::now())
    .execute(pool)
    .await?;
    Ok(())
}

/// "Moved on to the next episode" ⇒ the previous one is done.
///
/// Given the episode the user just sent a heartbeat for (`current_*`), find the
/// LATEST earlier episode of the same collection that the user had STARTED but
/// not finished, and mark it completed. This is what stops an episode lingering
/// at "97 %" when the viewer skips the credits and jumps to the next one — the
/// `position >= duration - 30s` rule never fires for a long outro.
///
/// Scoped to episodes the user actually has an in-progress row for, so it never
/// fabricates completion for a skipped/unwatched episode, and it's idempotent
/// (the `completed = 0` guards make it a no-op once the predecessor is done).
/// Runs server-side in the progress handler, so web AND TV get it for free.
/// No-op for movies / the first episode (no matching `episode_files` row).
pub async fn complete_previous_episode(
    pool: &SqlitePool,
    user_id: UserId,
    current_infohash: &str,
    current_file_idx: i64,
) -> Result<bool, sqlx::Error> {
    let user: Uuid = user_id.into();
    // 1. Find the latest earlier episode (same collection) the user STARTED
    //    but hasn't finished. Plain multi-column SELECT — no exotic SQL.
    let prev: Option<(String, i64)> = sqlx::query_as(
        "SELECT prev.infohash, prev.file_idx \
         FROM episode_files cur \
         JOIN episode_files prev ON prev.collection_id = cur.collection_id \
         JOIN playback_progress p \
           ON p.infohash = prev.infohash AND p.file_idx = prev.file_idx \
         WHERE cur.infohash = ?2 AND cur.file_idx = ?3 \
           AND p.user_id = ?1 AND p.completed = 0 \
           AND (prev.season < cur.season \
                OR (prev.season = cur.season AND prev.episode < cur.episode)) \
         ORDER BY prev.season DESC, prev.episode DESC \
         LIMIT 1",
    )
    .bind(user)
    .bind(current_infohash)
    .bind(current_file_idx)
    .fetch_optional(pool)
    .await?;

    let Some((infohash, file_idx)) = prev else {
        return Ok(false);
    };

    // 2. Mark it completed (idempotent via the `completed = 0` guard).
    let res = sqlx::query(
        "UPDATE playback_progress SET completed = 1 \
         WHERE user_id = ?1 AND infohash = ?2 AND file_idx = ?3 AND completed = 0",
    )
    .bind(user)
    .bind(&infohash)
    .bind(file_idx)
    .execute(pool)
    .await?;
    Ok(res.rows_affected() > 0)
}

/// One-shot historical cleanup: complete every in-progress episode the user
/// has since moved PAST — i.e. there's a LATER episode of the same collection
/// that the same user has any playback for. Sweeps the backlog of episodes
/// left stuck at e.g. "97 %" before [`complete_previous_episode`] existed,
/// including ones several episodes behind the current frontier (which the
/// per-heartbeat hook, scoped to the immediate predecessor, won't reach).
///
/// Idempotent (the `completed = 0` guard) — safe to run on every boot; a no-op
/// once everything has converged. Returns the number of rows completed.
pub async fn backfill_complete_superseded_episodes(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let res = sqlx::query(
        "UPDATE playback_progress SET completed = 1 \
         WHERE completed = 0 \
           AND EXISTS ( \
             SELECT 1 \
             FROM episode_files ef \
             JOIN episode_files later ON later.collection_id = ef.collection_id \
             JOIN playback_progress lp \
               ON lp.infohash = later.infohash AND lp.file_idx = later.file_idx \
             WHERE ef.infohash = playback_progress.infohash \
               AND ef.file_idx = playback_progress.file_idx \
               AND lp.user_id = playback_progress.user_id \
               AND (later.season > ef.season \
                    OR (later.season = ef.season AND later.episode > ef.episode)) \
           )",
    )
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

pub async fn upsert(pool: &SqlitePool, p: UpsertProgress) -> Result<(), sqlx::Error> {
    let user: Uuid = p.user_id.into();
    sqlx::query(
        "INSERT INTO playback_progress \
            (user_id, infohash, file_idx, position_seconds, duration_seconds, \
             audio_track_idx, subtitle_track_idx, completed, last_watched_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
         ON CONFLICT(user_id, infohash, file_idx) DO UPDATE SET \
            position_seconds   = excluded.position_seconds, \
            duration_seconds   = COALESCE(excluded.duration_seconds, playback_progress.duration_seconds), \
            audio_track_idx    = COALESCE(excluded.audio_track_idx, playback_progress.audio_track_idx), \
            subtitle_track_idx = excluded.subtitle_track_idx, \
            completed          = excluded.completed, \
            last_watched_at    = excluded.last_watched_at",
    )
    .bind(user)
    .bind(&p.infohash)
    .bind(p.file_idx)
    .bind(p.position_seconds)
    .bind(p.duration_seconds)
    .bind(p.audio_track_idx)
    .bind(p.subtitle_track_idx)
    .bind(p.completed)
    .bind(Utc::now())
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct ContinueWatchingRow {
    pub infohash: String,
    pub torrent_name: String,
    pub tmdb_id: Option<i64>,
    pub tmdb_verified: bool,
    pub file_idx: i64,
    pub position_seconds: f64,
    pub duration_seconds: Option<f64>,
    pub last_watched_at: DateTime<Utc>,
    pub completed: bool,
    pub audio_track_idx: Option<i64>,
    pub subtitle_track_idx: Option<i64>,
    /// `"movie"` / `"tv"` from the parent collection. Null for
    /// standalone torrents not yet attached to a collection. Used
    /// by clients to pass `?kind=` to the TMDB lookup endpoint —
    /// without the hint, an id collision between TMDB's separate
    /// movie / tv namespaces returns the wrong entry's poster.
    pub kind: Option<String>,
    /// Parent collection id — de-duplicates the shelf to one tile per series
    /// and identifies the series for "remove from Continue Watching".
    pub collection_id: Option<Uuid>,
    /// True when this tile is the NEXT (not-yet-started) episode surfaced
    /// because the previous one is finished — vs a mid-way resume tile.
    /// Clients label it "Up next" and manage it accordingly.
    pub next_up: bool,
}

/// Recent in-progress items for the user (excluding completed). Joined with
/// torrents so we already have a display name and the `tmdb_id` (when known
/// AND `tmdb_verified`) for poster lookups. Deliberately excludes
/// soft-deleted torrents — a deleted file can't be resumed, so it has no
/// place in a "continue here" shelf. The full per-episode answer to "where
/// was I" (surviving deletion) lives in [`user_history`] instead.
pub async fn continue_watching(
    pool: &SqlitePool,
    user_id: UserId,
    limit: i64,
) -> Result<Vec<ContinueWatchingRow>, sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query_as::<_, ContinueWatchingRow>(
        "SELECT p.infohash, t.name as torrent_name, \
            c.tmdb_id as tmdb_id, \
            t.tmdb_verified, p.file_idx, \
            p.position_seconds, p.duration_seconds, p.last_watched_at, p.completed, \
            p.audio_track_idx, p.subtitle_track_idx, c.kind as kind, \
            t.collection_id as collection_id, 0 AS next_up \
         FROM playback_progress p \
         JOIN torrents t ON t.infohash = p.infohash AND t.deleted_at IS NULL \
         LEFT JOIN collections c ON c.id = t.collection_id \
         WHERE p.user_id = ?1 AND p.completed = FALSE \
           AND NOT EXISTS ( \
             SELECT 1 FROM cw_dismissed d \
             WHERE d.user_id = ?1 AND d.collection_id = t.collection_id \
               AND d.dismissed_at >= p.last_watched_at) \
         ORDER BY p.last_watched_at DESC \
         LIMIT ?2",
    )
    .bind(user)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Remove a whole series from the caller's Continue Watching. Recorded at the
/// collection level (see the `cw_dismissed` migration) so it survives the
/// frontier-regeneration problem; it self-expires once the user plays a newer
/// episode. Idempotent — re-dismissing just refreshes the timestamp.
pub async fn dismiss_collection(
    pool: &SqlitePool,
    user_id: UserId,
    collection_id: Uuid,
) -> Result<(), sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query(
        "INSERT INTO cw_dismissed (user_id, collection_id, dismissed_at) \
         VALUES (?1, ?2, ?3) \
         ON CONFLICT(user_id, collection_id) DO UPDATE SET dismissed_at = excluded.dismissed_at",
    )
    .bind(user)
    .bind(collection_id)
    .bind(Utc::now())
    .execute(pool)
    .await?;
    Ok(())
}

/// "Next up" episodes for the Continue Watching shelf: for every collection
/// whose MOST-RECENT playback is a COMPLETED episode, the next owned episode
/// the user hasn't started yet — surfaced at position 0. This is what makes
/// the shelf advance from "you finished S02E04" to "play S02E05" once an
/// episode crosses the watched threshold, instead of the series dropping off.
///
/// Each row inherits the completed episode's `last_watched_at` so it sorts
/// among the resume tiles by recency (and naturally ages out of the shelf).
/// "Next" = the smallest `(season, episode)` strictly greater than the
/// completed one that exists on disk — robust to episode/season gaps.
pub async fn continue_watching_next_up(
    pool: &SqlitePool,
    user_id: UserId,
    limit: i64,
) -> Result<Vec<ContinueWatchingRow>, sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query_as::<_, ContinueWatchingRow>(
        "SELECT nf.infohash AS infohash, nt.name AS torrent_name, \
            c.tmdb_id AS tmdb_id, nt.tmdb_verified AS tmdb_verified, \
            nf.file_idx AS file_idx, 0.0 AS position_seconds, \
            NULL AS duration_seconds, latest.last_watched_at AS last_watched_at, \
            0 AS completed, NULL AS audio_track_idx, NULL AS subtitle_track_idx, \
            c.kind AS kind, c.id AS collection_id, 1 AS next_up \
         FROM ( \
            SELECT ef.collection_id AS cid, ef.season AS s, ef.episode AS e, \
                   p.last_watched_at AS last_watched_at \
            FROM playback_progress p \
            JOIN episode_files ef ON ef.infohash = p.infohash AND ef.file_idx = p.file_idx \
            WHERE p.user_id = ?1 AND p.completed = 1 \
              AND NOT EXISTS ( \
                SELECT 1 FROM playback_progress p2 \
                JOIN episode_files ef2 ON ef2.infohash = p2.infohash AND ef2.file_idx = p2.file_idx \
                WHERE p2.user_id = ?1 AND ef2.collection_id = ef.collection_id \
                  AND p2.last_watched_at > p.last_watched_at) \
         ) latest \
         JOIN collections c ON c.id = latest.cid \
         JOIN episode_files nf ON nf.collection_id = latest.cid \
              AND (nf.season > latest.s OR (nf.season = latest.s AND nf.episode > latest.e)) \
         JOIN torrents nt ON nt.infohash = nf.infohash AND nt.deleted_at IS NULL \
         WHERE NOT EXISTS ( \
                SELECT 1 FROM playback_progress pp \
                WHERE pp.user_id = ?1 AND pp.infohash = nf.infohash AND pp.file_idx = nf.file_idx) \
           AND NOT EXISTS ( \
                SELECT 1 FROM episode_files nf2 \
                JOIN torrents nt2 ON nt2.infohash = nf2.infohash AND nt2.deleted_at IS NULL \
                WHERE nf2.collection_id = latest.cid \
                  AND (nf2.season > latest.s OR (nf2.season = latest.s AND nf2.episode > latest.e)) \
                  AND (nf2.season < nf.season OR (nf2.season = nf.season AND nf2.episode < nf.episode))) \
           AND NOT EXISTS ( \
                SELECT 1 FROM cw_dismissed d \
                WHERE d.user_id = ?1 AND d.collection_id = latest.cid \
                  AND d.dismissed_at >= latest.last_watched_at) \
         ORDER BY latest.last_watched_at DESC \
         LIMIT ?2",
    )
    .bind(user)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Distinct TMDB ids the user has any playback record for (watched or
/// in-progress), preferring the parent collection id. Used to exclude
/// already-seen titles from the recommendation shelves. Soft-deleted
/// torrents still count — the user saw it even if it's since been GC'd.
pub async fn watched_tmdb_ids(pool: &SqlitePool, user_id: UserId) -> Result<Vec<i64>, sqlx::Error> {
    let user: Uuid = user_id.into();
    let rows: Vec<(i64,)> = sqlx::query_as(
        "SELECT DISTINCT c.tmdb_id AS tmdb_id \
         FROM playback_progress p \
         JOIN torrents t ON t.infohash = p.infohash \
         LEFT JOIN collections c ON c.id = t.collection_id \
         WHERE p.user_id = ?1 AND c.tmdb_id IS NOT NULL",
    )
    .bind(user)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

/// One row of cross-user recent playback activity for the admin view.
/// Mirrors [`ContinueWatchingRow`] but spans every user (joined with
/// `users` for the display name) and keeps completed items so the admin
/// sees finished watches too.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct RecentActivityRow {
    pub user_id: Uuid,
    pub display_name: String,
    pub infohash: String,
    pub torrent_name: String,
    pub tmdb_id: Option<i64>,
    pub tmdb_verified: bool,
    pub file_idx: i64,
    pub position_seconds: f64,
    pub duration_seconds: Option<f64>,
    pub last_watched_at: DateTime<Utc>,
    pub completed: bool,
    pub kind: Option<String>,
}

/// Poster/title metadata for a single torrent, resolved with the same
/// `COALESCE(collection, torrent)` tmdb precedence as the watch shelves.
/// Used to decorate live presence sessions in the admin view.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct SessionCardRow {
    pub torrent_name: String,
    pub tmdb_id: Option<i64>,
    pub tmdb_verified: bool,
    pub kind: Option<String>,
}

pub async fn session_card(
    pool: &SqlitePool,
    infohash: &str,
) -> Result<Option<SessionCardRow>, sqlx::Error> {
    sqlx::query_as::<_, SessionCardRow>(
        "SELECT t.name as torrent_name, \
            c.tmdb_id as tmdb_id, \
            t.tmdb_verified, c.kind as kind \
         FROM torrents t \
         LEFT JOIN collections c ON c.id = t.collection_id \
         WHERE t.infohash = ?1 AND t.deleted_at IS NULL",
    )
    .bind(infohash)
    .fetch_optional(pool)
    .await
}

/// Most-recent playback activity across all users, newest first. Powers the
/// admin "Recent activity" list.
pub async fn recent_activity(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<RecentActivityRow>, sqlx::Error> {
    sqlx::query_as::<_, RecentActivityRow>(
        "SELECT p.user_id, u.display_name, p.infohash, t.name as torrent_name, \
            c.tmdb_id as tmdb_id, \
            t.tmdb_verified, p.file_idx, \
            p.position_seconds, p.duration_seconds, p.last_watched_at, p.completed, \
            c.kind as kind \
         FROM playback_progress p \
         JOIN users u ON u.id = p.user_id \
         JOIN torrents t ON t.infohash = p.infohash AND t.deleted_at IS NULL \
         LEFT JOIN collections c ON c.id = t.collection_id \
         ORDER BY p.last_watched_at DESC \
         LIMIT ?1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// One row of a user's complete watch history — in-progress AND completed,
/// including items whose source torrent has since been soft-deleted
/// (`deleted = true`). Unlike [`continue_watching`] / [`recent_activity`],
/// which both `JOIN torrents t ON … AND t.deleted_at IS NULL` and so drop a
/// row the moment its torrent is GC'd or admin-removed, this query keeps
/// every row — a deletion (disk-reclaim, admin cleanup) must never erase
/// "what did I watch and how far did I get" (`torrents` rows are only ever
/// soft-deleted, so `t.name` / `c.tmdb_id` stay resolvable).
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct HistoryRow {
    pub infohash: String,
    pub torrent_name: String,
    pub tmdb_id: Option<i64>,
    pub tmdb_verified: bool,
    pub file_idx: i64,
    pub position_seconds: f64,
    pub duration_seconds: Option<f64>,
    pub last_watched_at: DateTime<Utc>,
    pub completed: bool,
    pub kind: Option<String>,
    /// `true` when the source torrent has been soft-deleted — clients show
    /// a "no longer available" state instead of a resume link.
    pub deleted: bool,
    /// Parent collection — survives GC (torrents are only soft-deleted,
    /// collections are never dropped), so history can stay grouped by
    /// show/movie ("ghost collection") after a disk-reclaim pass.
    pub collection_id: Option<Uuid>,
    /// The collection's clean display title ("Goblin The Lonely and
    /// Great God") — the readable label history renders instead of the
    /// raw SCENE torrent name.
    pub collection_title: Option<String>,
    /// SCENE-derived (season, episode) of the exact file watched.
    /// Present while the `episode_files` row lives (survives GC; a
    /// manual per-torrent remove hard-deletes it).
    pub season: Option<i64>,
    pub episode: Option<i64>,
    pub absolute_episode: Option<i64>,
    /// Provenance of the source torrent — lets clients offer
    /// "download again" on a GC'd row (same release → same infohash →
    /// the resume position in `playback_progress` still applies).
    pub source_provider: Option<String>,
    pub source_external_id: Option<String>,
}

/// Full watch history for one user, newest first, paginated. Powers both
/// the user's own `/me/history` page and the admin per-user drill-down
/// (`user_id` is the caller's own id in the former, the target user's id
/// in the latter — same query, different caller).
pub async fn user_history(
    pool: &SqlitePool,
    user_id: UserId,
    limit: i64,
    offset: i64,
) -> Result<Vec<HistoryRow>, sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query_as::<_, HistoryRow>(
        "SELECT p.infohash, t.name as torrent_name, \
            c.tmdb_id as tmdb_id, \
            t.tmdb_verified, p.file_idx, \
            p.position_seconds, p.duration_seconds, p.last_watched_at, p.completed, \
            c.kind as kind, (t.deleted_at IS NOT NULL) as deleted, \
            t.collection_id as collection_id, c.display_title as collection_title, \
            ef.season as season, ef.episode as episode, \
            ef.absolute_episode as absolute_episode, \
            t.source_provider as source_provider, \
            t.source_external_id as source_external_id \
         FROM playback_progress p \
         JOIN torrents t ON t.infohash = p.infohash \
         LEFT JOIN collections c ON c.id = t.collection_id \
         LEFT JOIN episode_files ef \
            ON ef.infohash = p.infohash AND ef.file_idx = p.file_idx \
         WHERE p.user_id = ?1 \
         ORDER BY p.last_watched_at DESC \
         LIMIT ?2 OFFSET ?3",
    )
    .bind(user)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}

/// One playback row of the caller on a RECLAIMED (soft-deleted) torrent
/// of a collection. Enriches the collection page's gone-release rows
/// (movies / packs without `episode_files`) with "already watched"
/// state — the per-episode gone rows get theirs from
/// [`crate::episode_files::list_gone_for_collection`] instead.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct GoneWatchRow {
    pub infohash: String,
    pub file_idx: i64,
    pub position_seconds: f64,
    pub duration_seconds: Option<f64>,
    pub completed: bool,
    pub last_watched_at: DateTime<Utc>,
}

/// The caller's playback rows on the collection's reclaimed torrents,
/// newest first (so `find`-by-infohash yields the most recent file's
/// state — the meaningful one for a single-file movie).
pub async fn watch_state_for_deleted_in_collection(
    pool: &SqlitePool,
    user_id: UserId,
    collection_id: Uuid,
) -> Result<Vec<GoneWatchRow>, sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query_as::<_, GoneWatchRow>(
        "SELECT p.infohash, p.file_idx, p.position_seconds, p.duration_seconds, \
            p.completed, p.last_watched_at \
         FROM playback_progress p \
         JOIN torrents t ON t.infohash = p.infohash \
         WHERE p.user_id = ?1 AND t.collection_id = ?2 AND t.deleted_at IS NOT NULL \
         ORDER BY p.last_watched_at DESC",
    )
    .bind(user)
    .bind(collection_id)
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

    async fn make_user(pool: &SqlitePool) -> UserId {
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
        UserId::from(id)
    }

    async fn make_torrent(
        pool: &SqlitePool,
        owner: UserId,
        name: &str,
    ) -> crate::torrents::TorrentRow {
        crate::torrents::upsert(
            pool,
            crate::torrents::NewTorrent {
                infohash: Uuid::new_v4().to_string(),
                name: name.to_string(),
                total_size_bytes: 1_000,
                source_provider: None,
                source_external_id: None,
                added_by: owner,
            },
        )
        .await
        .expect("insert torrent")
    }

    fn progress(user: UserId, infohash: String, completed: bool) -> UpsertProgress {
        UpsertProgress {
            user_id: user,
            infohash,
            file_idx: 0,
            position_seconds: 120.0,
            duration_seconds: Some(1_300.0),
            audio_track_idx: None,
            subtitle_track_idx: None,
            completed,
        }
    }

    #[tokio::test]
    async fn user_history_includes_completed_unlike_continue_watching() {
        let pool = migrated_pool().await;
        let user = make_user(&pool).await;
        let t = make_torrent(&pool, user, "Show S01E01").await;
        upsert(&pool, progress(user, t.infohash.clone(), true))
            .await
            .unwrap();

        assert!(
            continue_watching(&pool, user, 10).await.unwrap().is_empty(),
            "continue_watching excludes completed items",
        );

        let hist = user_history(&pool, user, 10, 0).await.unwrap();
        assert_eq!(hist.len(), 1);
        assert!(hist[0].completed);
        assert!(!hist[0].deleted);
    }

    #[tokio::test]
    async fn user_history_survives_torrent_deletion() {
        let pool = migrated_pool().await;
        let user = make_user(&pool).await;
        let t = make_torrent(&pool, user, "Movie Night").await;
        upsert(&pool, progress(user, t.infohash.clone(), false))
            .await
            .unwrap();

        assert_eq!(continue_watching(&pool, user, 10).await.unwrap().len(), 1);
        assert_eq!(user_history(&pool, user, 10, 0).await.unwrap().len(), 1);

        crate::torrents::soft_delete(&pool, iris_core::ids::TorrentId::from(t.id))
            .await
            .unwrap();

        // `continue_watching` drops it — a deleted file can't be resumed, so
        // it has no place in a "continue here" shelf. `user_history` keeps
        // it and flags `deleted` so the UI can show "no longer available"
        // instead of just losing the entry — that's the dedicated per-episode
        // answer to "where was I" after a cleanup.
        assert!(continue_watching(&pool, user, 10).await.unwrap().is_empty());
        let hist = user_history(&pool, user, 10, 0).await.unwrap();
        assert_eq!(hist.len(), 1);
        assert!(hist[0].deleted);
        assert_eq!(hist[0].torrent_name, "Movie Night");
    }

    #[tokio::test]
    async fn user_history_carries_collection_grouping_and_provenance_after_gc() {
        let pool = migrated_pool().await;
        let user = make_user(&pool).await;
        let t = crate::torrents::upsert(
            &pool,
            crate::torrents::NewTorrent {
                infohash: Uuid::new_v4().to_string(),
                name: "Goblin.The.Lonely.and.Great.God.2016.COMPLETE.VOSTFR.1080p".to_string(),
                total_size_bytes: 1_000,
                source_provider: Some("c411".to_string()),
                source_external_id: Some("12345".to_string()),
                added_by: user,
            },
        )
        .await
        .expect("insert torrent");
        let col = crate::collections::find_or_create(
            &pool,
            "goblin the lonely and great god",
            "Goblin The Lonely and Great God",
            crate::collections::Kind::Tv,
            false,
        )
        .await
        .expect("collection");
        crate::torrents::set_collection(&pool, &t.infohash, Some(col.id))
            .await
            .expect("attach");
        crate::episode_files::upsert(
            &pool,
            crate::episode_files::UpsertEpisodeFile {
                collection_id: col.id,
                season: 1,
                episode: 3,
                infohash: t.infohash.clone(),
                file_idx: 0,
                derived_from: crate::episode_files::DerivedFrom::SceneParse,
                absolute_episode: None,
            },
        )
        .await
        .expect("episode file");
        upsert(&pool, progress(user, t.infohash.clone(), false))
            .await
            .unwrap();

        // GC = soft-delete only. The grouping identity (collection),
        // episode coordinates and re-grab provenance must all survive —
        // that's what lets History stay readable and resumable.
        crate::torrents::soft_delete(&pool, iris_core::ids::TorrentId::from(t.id))
            .await
            .unwrap();

        let hist = user_history(&pool, user, 10, 0).await.unwrap();
        assert_eq!(hist.len(), 1);
        let row = &hist[0];
        assert!(row.deleted);
        assert_eq!(row.collection_id, Some(col.id));
        assert_eq!(
            row.collection_title.as_deref(),
            Some("Goblin The Lonely and Great God"),
        );
        assert_eq!((row.season, row.episode), (Some(1), Some(3)));
        assert_eq!(row.source_provider.as_deref(), Some("c411"));
        assert_eq!(row.source_external_id.as_deref(), Some("12345"));
    }

    /// Release-level Gone dismissal: hides the row for the dismissing
    /// user only, and goes stale when the release is re-ingested then
    /// reclaimed AGAIN (newer `deleted_at` → the row returns).
    #[tokio::test]
    async fn gone_release_dismissal_is_per_user_and_stale_on_redelete() {
        let pool = migrated_pool().await;
        let user = make_user(&pool).await;
        let stranger = make_user(&pool).await;
        let t = crate::torrents::upsert(
            &pool,
            crate::torrents::NewTorrent {
                infohash: Uuid::new_v4().to_string(),
                name: "Movie.2024.MULTi.1080p".to_string(),
                total_size_bytes: 1_000,
                source_provider: Some("c411".to_string()),
                source_external_id: Some("777".to_string()),
                added_by: user,
            },
        )
        .await
        .unwrap();
        let col = crate::collections::find_or_create(
            &pool,
            "movie",
            "Movie",
            crate::collections::Kind::Movie,
            false,
        )
        .await
        .unwrap();
        crate::torrents::set_collection(&pool, &t.infohash, Some(col.id))
            .await
            .unwrap();
        crate::torrents::soft_delete(&pool, iris_core::ids::TorrentId::from(t.id))
            .await
            .unwrap();

        let gone = crate::torrents::list_deleted_in_collection(&pool, col.id, user)
            .await
            .unwrap();
        assert_eq!(gone.len(), 1);

        crate::torrents::dismiss_gone_release(&pool, user, &t.infohash)
            .await
            .unwrap();
        assert!(
            crate::torrents::list_deleted_in_collection(&pool, col.id, user)
                .await
                .unwrap()
                .is_empty(),
            "dismissed release is hidden for the dismissing user",
        );
        assert_eq!(
            crate::torrents::list_deleted_in_collection(&pool, col.id, stranger)
                .await
                .unwrap()
                .len(),
            1,
            "dismissals never leak to other users",
        );

        // Re-download (upsert clears deleted_at) then reclaim again:
        // the fresh deleted_at outdates the dismissal → row returns.
        crate::torrents::upsert(
            &pool,
            crate::torrents::NewTorrent {
                infohash: t.infohash.clone(),
                name: "Movie.2024.MULTi.1080p".to_string(),
                total_size_bytes: 1_000,
                source_provider: Some("c411".to_string()),
                source_external_id: Some("777".to_string()),
                added_by: user,
            },
        )
        .await
        .unwrap();
        crate::torrents::soft_delete(&pool, iris_core::ids::TorrentId::from(t.id))
            .await
            .unwrap();
        assert_eq!(
            crate::torrents::list_deleted_in_collection(&pool, col.id, user)
                .await
                .unwrap()
                .len(),
            1,
            "a newer reclaim makes the dismissal stale",
        );
    }

    /// Gone episode rows reconstruct the pre-GC episode list with the
    /// caller's watch state attached, and honour release dismissals.
    #[tokio::test]
    async fn gone_episode_rows_carry_watch_state_and_respect_dismissal() {
        let pool = migrated_pool().await;
        let user = make_user(&pool).await;
        let t = crate::torrents::upsert(
            &pool,
            crate::torrents::NewTorrent {
                infohash: Uuid::new_v4().to_string(),
                name: "Show.S01E03.VOSTFR.1080p".to_string(),
                total_size_bytes: 1_000,
                source_provider: Some("c411".to_string()),
                source_external_id: Some("42".to_string()),
                added_by: user,
            },
        )
        .await
        .unwrap();
        let col = crate::collections::find_or_create(
            &pool,
            "show",
            "Show",
            crate::collections::Kind::Tv,
            false,
        )
        .await
        .unwrap();
        crate::torrents::set_collection(&pool, &t.infohash, Some(col.id))
            .await
            .unwrap();
        // Season-pack shape: several episode files on ONE infohash. The
        // user watched only the first — the gone view must keep the
        // per-episode watched split, not collapse the release.
        for (episode, file_idx) in [(3_i64, 0_i64), (4, 1)] {
            crate::episode_files::upsert(
                &pool,
                crate::episode_files::UpsertEpisodeFile {
                    collection_id: col.id,
                    season: 1,
                    episode,
                    infohash: t.infohash.clone(),
                    file_idx,
                    derived_from: crate::episode_files::DerivedFrom::SceneParse,
                    absolute_episode: None,
                },
            )
            .await
            .unwrap();
        }
        upsert(&pool, progress(user, t.infohash.clone(), true))
            .await
            .unwrap();

        // Torrent still live → nothing is "gone".
        assert!(
            crate::episode_files::list_gone_for_collection(&pool, col.id, user)
                .await
                .unwrap()
                .is_empty()
        );

        crate::torrents::soft_delete(&pool, iris_core::ids::TorrentId::from(t.id))
            .await
            .unwrap();

        let rows = crate::episode_files::list_gone_for_collection(&pool, col.id, user)
            .await
            .unwrap();
        assert_eq!(rows.len(), 2, "one gone row per pack leaf");
        let e3 = &rows[0];
        assert_eq!((e3.season, e3.episode), (1, 3));
        assert_eq!(e3.torrent_name, "Show.S01E03.VOSTFR.1080p");
        assert_eq!(e3.source_provider.as_deref(), Some("c411"));
        assert!(e3.completed, "the caller's watched state is attached");
        assert!(e3.last_watched_at.is_some());
        let e4 = &rows[1];
        assert_eq!((e4.season, e4.episode), (1, 4));
        assert!(
            !e4.completed && e4.last_watched_at.is_none(),
            "the unwatched pack leaf stays unwatched",
        );

        crate::torrents::dismiss_gone_release(&pool, user, &t.infohash)
            .await
            .unwrap();
        assert!(
            crate::episode_files::list_gone_for_collection(&pool, col.id, user)
                .await
                .unwrap()
                .is_empty(),
            "dismissing the release hides its gone episode rows too",
        );
    }

    #[tokio::test]
    async fn user_history_is_scoped_per_user_and_paginated() {
        let pool = migrated_pool().await;
        let user_a = make_user(&pool).await;
        let user_b = make_user(&pool).await;
        for i in 0..3 {
            let t = make_torrent(&pool, user_a, &format!("A Show {i}")).await;
            upsert(&pool, progress(user_a, t.infohash, false))
                .await
                .unwrap();
        }
        let t_b = make_torrent(&pool, user_b, "B Show").await;
        upsert(&pool, progress(user_b, t_b.infohash, false))
            .await
            .unwrap();

        assert_eq!(user_history(&pool, user_b, 10, 0).await.unwrap().len(), 1);
        assert_eq!(user_history(&pool, user_a, 2, 0).await.unwrap().len(), 2);
        assert_eq!(user_history(&pool, user_a, 2, 2).await.unwrap().len(), 1);
    }
}
