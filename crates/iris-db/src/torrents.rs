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
    pub added_by: Uuid,
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
    pub added_by: UserId,
}

/// Insert if the infohash is new, otherwise return the existing row (un-soft-deleting it).
pub async fn upsert(pool: &SqlitePool, new: NewTorrent) -> Result<TorrentRow, sqlx::Error> {
    if let Some(existing) = find_by_infohash(pool, &new.infohash).await? {
        if existing.deleted_at.is_some() {
            sqlx::query("UPDATE torrents SET deleted_at = NULL WHERE id = ?1")
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
         source_external_id, added_by, added_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )
    .bind(id)
    .bind(&new.infohash)
    .bind(&new.name)
    .bind(i64::try_from(new.total_size_bytes).unwrap_or(i64::MAX))
    .bind(&new.source_provider)
    .bind(&new.source_external_id)
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
        "SELECT id, infohash, name, total_size_bytes, source_provider, source_external_id, \
         added_by, added_at, last_played_at, last_seed_activity_at, deleted_at \
         FROM torrents WHERE infohash = ?1",
    )
    .bind(infohash)
    .fetch_optional(pool)
    .await
}

pub async fn list_active(pool: &SqlitePool) -> Result<Vec<TorrentRow>, sqlx::Error> {
    sqlx::query_as::<_, TorrentRow>(
        "SELECT id, infohash, name, total_size_bytes, source_provider, source_external_id, \
         added_by, added_at, last_played_at, last_seed_activity_at, deleted_at \
         FROM torrents WHERE deleted_at IS NULL ORDER BY added_at DESC",
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

pub async fn touch_played(pool: &SqlitePool, infohash: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE torrents SET last_played_at = ?1 WHERE infohash = ?2")
        .bind(Utc::now())
        .bind(infohash)
        .execute(pool)
        .await?;
    Ok(())
}
