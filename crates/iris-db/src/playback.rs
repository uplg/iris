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
}

/// Recent in-progress items for the user (excluding completed). Joined with
/// torrents so we already have a display name and the `tmdb_id` (when known
/// AND `tmdb_verified`) for poster lookups.
pub async fn continue_watching(
    pool: &SqlitePool,
    user_id: UserId,
    limit: i64,
) -> Result<Vec<ContinueWatchingRow>, sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query_as::<_, ContinueWatchingRow>(
        "SELECT p.infohash, t.name as torrent_name, \
            COALESCE(c.tmdb_id, t.tmdb_id) as tmdb_id, \
            t.tmdb_verified, p.file_idx, \
            p.position_seconds, p.duration_seconds, p.last_watched_at, p.completed, \
            p.audio_track_idx, p.subtitle_track_idx, c.kind as kind \
         FROM playback_progress p \
         JOIN torrents t ON t.infohash = p.infohash AND t.deleted_at IS NULL \
         LEFT JOIN collections c ON c.id = t.collection_id \
         WHERE p.user_id = ?1 AND p.completed = FALSE \
         ORDER BY p.last_watched_at DESC \
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
        "SELECT DISTINCT COALESCE(c.tmdb_id, t.tmdb_id) AS tmdb_id \
         FROM playback_progress p \
         JOIN torrents t ON t.infohash = p.infohash \
         LEFT JOIN collections c ON c.id = t.collection_id \
         WHERE p.user_id = ?1 AND COALESCE(c.tmdb_id, t.tmdb_id) IS NOT NULL",
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
            COALESCE(c.tmdb_id, t.tmdb_id) as tmdb_id, \
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
            COALESCE(c.tmdb_id, t.tmdb_id) as tmdb_id, \
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
