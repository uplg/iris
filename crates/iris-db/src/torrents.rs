use chrono::{DateTime, Utc};
use iris_core::ids::{TorrentId, UserId};
use serde::Serialize;
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct TorrentRow {
    pub id: Uuid,
    pub infohash: String,
    pub name: String,
    pub total_size_bytes: i64,
    pub source_provider: Option<String>,
    pub source_external_id: Option<String>,
    pub tmdb_id: Option<i64>,
    /// Set true by [`set_tmdb_verified`] once we've matched `tmdb_id`'s
    /// declared runtime against the file's probed duration. Until then
    /// frontends ignore `tmdb_id` for display purposes — wrong posters
    /// are worse UX than no posters.
    pub tmdb_verified: bool,
    pub added_by: Uuid,
    /// Public-facing display name of the user that added this torrent
    /// (denormalised via JOIN in [`find_by_infohash`] / [`list_active`]).
    /// Always present — `added_by` is `NOT NULL` with `ON DELETE CASCADE`
    /// so the row can't outlive its owner. Email is intentionally NOT
    /// exposed here to avoid leaking PII to non-admin users.
    pub added_by_name: String,
    pub added_at: DateTime<Utc>,
    pub last_played_at: Option<DateTime<Utc>>,
    pub last_seed_activity_at: Option<DateTime<Utc>>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct NewTorrent {
    pub infohash: String,
    pub name: String,
    pub total_size_bytes: u64,
    pub source_provider: Option<String>,
    pub source_external_id: Option<String>,
    pub tmdb_id: Option<i64>,
    pub added_by: UserId,
}

/// Insert if the infohash is new, otherwise return the existing row
/// (un-soft-deleting it). If the existing row lacks `tmdb_id` and the new
/// payload has one, backfill it — handy for torrents ingested before the
/// 0005 migration whose `tmdb_id` is now available because they got
/// re-resolved through search.
pub async fn upsert(pool: &SqlitePool, new: NewTorrent) -> Result<TorrentRow, sqlx::Error> {
    if let Some(existing) = find_by_infohash(pool, &new.infohash).await? {
        if existing.deleted_at.is_some() {
            sqlx::query("UPDATE torrents SET deleted_at = NULL WHERE id = ?1")
                .bind(existing.id)
                .execute(pool)
                .await?;
        }
        if existing.tmdb_id.is_none() && new.tmdb_id.is_some() {
            sqlx::query("UPDATE torrents SET tmdb_id = ?1 WHERE id = ?2")
                .bind(new.tmdb_id)
                .bind(existing.id)
                .execute(pool)
                .await?;
        }
        return find_by_infohash(pool, &new.infohash)
            .await?
            .ok_or(sqlx::Error::RowNotFound);
    }
    let id = Uuid::new_v4();
    let now = Utc::now();
    let added_by: Uuid = new.added_by.into();
    sqlx::query(
        "INSERT INTO torrents (id, infohash, name, total_size_bytes, source_provider, \
         source_external_id, tmdb_id, added_by, added_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )
    .bind(id)
    .bind(&new.infohash)
    .bind(&new.name)
    .bind(i64::try_from(new.total_size_bytes).unwrap_or(i64::MAX))
    .bind(&new.source_provider)
    .bind(&new.source_external_id)
    .bind(new.tmdb_id)
    .bind(added_by)
    .bind(now)
    .execute(pool)
    .await?;
    find_by_infohash(pool, &new.infohash)
        .await?
        .ok_or(sqlx::Error::RowNotFound)
}

pub async fn find_by_infohash(
    pool: &SqlitePool,
    infohash: &str,
) -> Result<Option<TorrentRow>, sqlx::Error> {
    sqlx::query_as::<_, TorrentRow>(
        "SELECT t.id, t.infohash, t.name, t.total_size_bytes, t.source_provider, t.source_external_id, \
         t.tmdb_id, t.tmdb_verified, t.added_by, u.display_name AS added_by_name, t.added_at, t.last_played_at, \
         t.last_seed_activity_at, t.deleted_at \
         FROM torrents t \
         JOIN users u ON u.id = t.added_by \
         WHERE t.infohash = ?1",
    )
    .bind(infohash)
    .fetch_optional(pool)
    .await
}

pub async fn list_active(pool: &SqlitePool) -> Result<Vec<TorrentRow>, sqlx::Error> {
    sqlx::query_as::<_, TorrentRow>(
        "SELECT t.id, t.infohash, t.name, t.total_size_bytes, t.source_provider, t.source_external_id, \
         t.tmdb_id, t.tmdb_verified, t.added_by, u.display_name AS added_by_name, t.added_at, t.last_played_at, \
         t.last_seed_activity_at, t.deleted_at \
         FROM torrents t \
         JOIN users u ON u.id = t.added_by \
         WHERE t.deleted_at IS NULL ORDER BY t.added_at DESC",
    )
    .fetch_all(pool)
    .await
}

pub async fn soft_delete(
    pool: &SqlitePool,
    id: TorrentId,
) -> Result<bool, sqlx::Error> {
    let id: Uuid = id.into();
    let res = sqlx::query("UPDATE torrents SET deleted_at = ?1 WHERE id = ?2 AND deleted_at IS NULL")
        .bind(Utc::now())
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() == 1)
}

/// Flip the `tmdb_verified` bit for a torrent — called once we've matched
/// the source's probed runtime against TMDB's declared runtime within an
/// acceptable tolerance. Frontend rendering paths consume only the
/// `(tmdb_id, tmdb_verified=true)` pair; everything else is treated as
/// "no metadata, show the filename".
pub async fn set_tmdb_verified(
    pool: &SqlitePool,
    infohash: &str,
    verified: bool,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE torrents SET tmdb_verified = ?1 WHERE infohash = ?2")
        .bind(verified)
        .bind(infohash)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn touch_played(pool: &SqlitePool, infohash: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE torrents SET last_played_at = ?1 WHERE infohash = ?2")
        .bind(Utc::now())
        .bind(infohash)
        .execute(pool)
        .await?;
    Ok(())
}
