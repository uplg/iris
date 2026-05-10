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
    /// Set by the collection-assignment job (Phase 4.5) to group multi-
    /// torrent series under one library entity. NULL until the job runs.
    pub collection_id: Option<Uuid>,
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
    /// Lifetime upload counter. librqbit's per-torrent `uploaded_bytes` is
    /// a session value (resets on restart, vanishes on GC eviction); this
    /// column is reconciled by [`reconcile_uploaded`] from the live session
    /// counter and survives both events.
    pub uploaded_bytes_total: i64,
    /// `"movie"` / `"tv"` from the parent collection. Null for
    /// standalone torrents not yet attached to one. Clients pass
    /// this to `/api/metadata/tmdb/{id}?kind=` — TMDB has separate
    /// id namespaces for movies and TV, so the same numerical id
    /// can resolve to two unrelated entries; the kind hint picks
    /// the right one.
    pub kind: Option<String>,
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
         t.tmdb_id, t.tmdb_verified, t.collection_id, t.added_by, u.display_name AS added_by_name, \
         t.added_at, t.last_played_at, t.last_seed_activity_at, t.deleted_at, t.uploaded_bytes_total, \
         c.kind AS kind \
         FROM torrents t \
         JOIN users u ON u.id = t.added_by \
         LEFT JOIN collections c ON c.id = t.collection_id \
         WHERE t.infohash = ?1",
    )
    .bind(infohash)
    .fetch_optional(pool)
    .await
}

pub async fn list_active(pool: &SqlitePool) -> Result<Vec<TorrentRow>, sqlx::Error> {
    sqlx::query_as::<_, TorrentRow>(
        "SELECT t.id, t.infohash, t.name, t.total_size_bytes, t.source_provider, t.source_external_id, \
         t.tmdb_id, t.tmdb_verified, t.collection_id, t.added_by, u.display_name AS added_by_name, \
         t.added_at, t.last_played_at, t.last_seed_activity_at, t.deleted_at, t.uploaded_bytes_total, \
         c.kind AS kind \
         FROM torrents t \
         JOIN users u ON u.id = t.added_by \
         LEFT JOIN collections c ON c.id = t.collection_id \
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

/// Reconcile the lifetime upload counter for `infohash` with the current
/// session value reported by librqbit. Atomic at the SQL level so we don't
/// race with delete_by_infohash / soft_delete.
///
/// Logic: lifetime += max(0, session_now - session_seen). If the current
/// session value is *below* the last seen one (process restarted, librqbit
/// reset its counter) we treat the new value as fresh delta — the work
/// done since boot.
pub async fn reconcile_uploaded(
    pool: &SqlitePool,
    infohash: &str,
    session_now: u64,
) -> Result<(), sqlx::Error> {
    let now = i64::try_from(session_now).unwrap_or(i64::MAX);
    // Single UPDATE so the read of the previous value and the write of the
    // new total can't interleave with a delete or another reconcile pass.
    sqlx::query(
        "UPDATE torrents SET \
           uploaded_bytes_total = uploaded_bytes_total + \
             CASE WHEN ?1 >= uploaded_bytes_session_seen \
                  THEN ?1 - uploaded_bytes_session_seen \
                  ELSE ?1 END, \
           uploaded_bytes_session_seen = ?1 \
         WHERE infohash = ?2",
    )
    .bind(now)
    .bind(infohash)
    .execute(pool)
    .await?;
    Ok(())
}

/// Sum of `uploaded_bytes_total` across every torrent ever ingested,
/// including soft-deleted ones (a torrent we've already evicted still
/// represents work the seedbox did for the swarm).
pub async fn total_uploaded_bytes(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let row: (Option<i64>,) =
        sqlx::query_as("SELECT SUM(uploaded_bytes_total) FROM torrents")
            .fetch_one(pool)
            .await?;
    Ok(u64::try_from(row.0.unwrap_or(0)).unwrap_or(0))
}

pub async fn touch_played(pool: &SqlitePool, infohash: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE torrents SET last_played_at = ?1 WHERE infohash = ?2")
        .bind(Utc::now())
        .bind(infohash)
        .execute(pool)
        .await?;
    Ok(())
}

/// Attach a torrent to a collection. Set `collection_id = None` to
/// detach (sets the column to NULL). Used by the collection-assignment
/// job (Phase 4.5) at ingest and during the retroactive batch.
pub async fn set_collection(
    pool: &SqlitePool,
    infohash: &str,
    collection_id: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE torrents SET collection_id = ?1 WHERE infohash = ?2")
        .bind(collection_id)
        .bind(infohash)
        .execute(pool)
        .await?;
    Ok(())
}

/// All torrents currently attached to a collection. Powers the Series
/// detail page when it walks "every file across every torrent in the
/// collection" to build the merged episode list.
pub async fn list_in_collection(
    pool: &SqlitePool,
    collection_id: Uuid,
) -> Result<Vec<TorrentRow>, sqlx::Error> {
    sqlx::query_as::<_, TorrentRow>(
        "SELECT t.id, t.infohash, t.name, t.total_size_bytes, t.source_provider, t.source_external_id, \
         t.tmdb_id, t.tmdb_verified, t.collection_id, t.added_by, u.display_name AS added_by_name, \
         t.added_at, t.last_played_at, t.last_seed_activity_at, t.deleted_at, t.uploaded_bytes_total, \
         c.kind AS kind \
         FROM torrents t \
         JOIN users u ON u.id = t.added_by \
         LEFT JOIN collections c ON c.id = t.collection_id \
         WHERE t.collection_id = ?1 AND t.deleted_at IS NULL \
         ORDER BY t.added_at",
    )
    .bind(collection_id)
    .fetch_all(pool)
    .await
}
