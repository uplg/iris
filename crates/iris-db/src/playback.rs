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
