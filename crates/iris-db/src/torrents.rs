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
    /// DEPRECATED — no longer written (always NULL for torrents ingested after
    /// the collection-tmdb unification). The collection's id is the single source
    /// of truth ([`Self::effective_tmdb_id`] / `collection_tmdb_id`). Kept only so
    /// the admin diagnostic can still surface legacy rows' stale value; drop the
    /// column in a follow-up migration once the new grab flow is confirmed.
    pub tmdb_id: Option<i64>,
    /// Set true by [`set_tmdb_verified`] once we've matched the collection's `tmdb_id`'s
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
    /// First time the engine reported the torrent fully downloaded.
    /// Restart-proof "bytes on disk are final" flag — engine snapshots
    /// can't answer that during the post-deploy `initializing` re-check.
    /// Stamped by the seed-stats loop, never cleared.
    pub finished_at: Option<DateTime<Utc>>,
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
    /// Parent collection's resolved `tmdb_id` (via the `collections`
    /// LEFT JOIN). The collection is the single source of truth for
    /// poster/metadata — its id is resolved from the SCENE *identity*
    /// (`display_title`) and self-healed by `tmdb_backfill`, whereas a
    /// torrent's own `tmdb_id` is the unreliable ingest-time hint
    /// (a c411 season pack named "Saison 1" never resolves; a
    /// falsely-runtime-verified movie keeps a wrong id). NULL for
    /// standalone torrents or collections without a resolved id.
    /// Surfaced to clients via [`TorrentRow::effective_tmdb_id`] so
    /// every poster path converges on the same stable id.
    pub collection_tmdb_id: Option<i64>,
}

impl TorrentRow {
    /// The id clients render from: the parent collection's resolved id —
    /// the SINGLE source of truth. The torrent's own `tmdb_id` is the
    /// unreliable ingest-time hint (it disagreed with the collection in
    /// several prod rows) and is deliberately NOT consulted here, so every
    /// poster path (library shelf, collection page, per-torrent views) is
    /// one logic. `None` when the collection has no resolved id yet.
    pub fn effective_tmdb_id(&self) -> Option<i64> {
        self.collection_tmdb_id
    }
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

/// Insert if the infohash is new, otherwise return the existing row
/// (un-soft-deleting it). The torrent's own `tmdb_id` is no longer written —
/// the parent collection's id is the single source of truth (resolved from the
/// collection's SCENE identity); see `collection_assign::resolve_collection_tmdb`.
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
         source_external_id, added_by, added_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
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
        "SELECT t.id, t.infohash, t.name, t.total_size_bytes, t.source_provider, t.source_external_id, \
         t.tmdb_id, t.tmdb_verified, t.collection_id, t.added_by, u.display_name AS added_by_name, \
         t.added_at, t.finished_at, t.last_played_at, t.last_seed_activity_at, t.deleted_at, t.uploaded_bytes_total, \
         c.kind AS kind, c.tmdb_id AS collection_tmdb_id \
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
         t.added_at, t.finished_at, t.last_played_at, t.last_seed_activity_at, t.deleted_at, t.uploaded_bytes_total, \
         c.kind AS kind, c.tmdb_id AS collection_tmdb_id \
         FROM torrents t \
         JOIN users u ON u.id = t.added_by \
         LEFT JOIN collections c ON c.id = t.collection_id \
         WHERE t.deleted_at IS NULL ORDER BY t.added_at DESC",
    )
    .fetch_all(pool)
    .await
}

/// Infohashes of every torrent still on disk (not soft-deleted).
/// Search dedup uses this to flag a result as "already in library"
/// **only** when it is the exact same torrent — a different release
/// (or language) of the same episode has a different infohash and must
/// stay grabbable.
pub async fn list_active_infohashes(pool: &SqlitePool) -> Result<Vec<String>, sqlx::Error> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT infohash FROM torrents WHERE deleted_at IS NULL")
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|(h,)| h).collect())
}

/// Distinct TMDB ids currently in the library (not soft-deleted), from each
/// torrent's parent collection — the authoritative id. The torrent's own
/// `tmdb_id` is never consulted. Used to exclude already-owned titles from the
/// recommendation shelves.
pub async fn library_tmdb_ids(pool: &SqlitePool) -> Result<Vec<i64>, sqlx::Error> {
    let rows: Vec<(i64,)> = sqlx::query_as(
        "SELECT DISTINCT c.tmdb_id AS tmdb_id \
         FROM torrents t \
         LEFT JOIN collections c ON c.id = t.collection_id \
         WHERE t.deleted_at IS NULL AND c.tmdb_id IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

pub async fn soft_delete(pool: &SqlitePool, id: TorrentId) -> Result<bool, sqlx::Error> {
    let id: Uuid = id.into();
    let res =
        sqlx::query("UPDATE torrents SET deleted_at = ?1 WHERE id = ?2 AND deleted_at IS NULL")
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
/// race with `delete_by_infohash` / `soft_delete`.
///
/// Logic: lifetime += max(0, `session_now` - `session_seen`). If the current
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
    let row: (Option<i64>,) = sqlx::query_as("SELECT SUM(uploaded_bytes_total) FROM torrents")
        .fetch_one(pool)
        .await?;
    Ok(u64::try_from(row.0.unwrap_or(0)).unwrap_or(0))
}

/// Stamp `finished_at` (first completion only — never moves once set).
/// Called from the 30 s seed-stats tick for every snapshot reporting
/// `finished`, so the flag converges shortly after completion and is
/// already in place when a later deploy puts the restored session
/// through its `initializing` re-check.
pub async fn mark_finished(pool: &SqlitePool, infohash: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE torrents SET finished_at = ?1 \
         WHERE infohash = ?2 AND finished_at IS NULL AND deleted_at IS NULL",
    )
    .bind(Utc::now())
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

/// Move every torrent attached to collection `from` over to `to`. Used by
/// the anime noise-split merge (`collection_assign`) that collapses an
/// `anime:K` + plain `K` pair pointing at the same TMDB entity into one.
/// Returns the number of torrents re-homed.
pub async fn reassign_collection(
    pool: &SqlitePool,
    from: Uuid,
    to: Uuid,
) -> Result<u64, sqlx::Error> {
    let res = sqlx::query("UPDATE torrents SET collection_id = ?2 WHERE collection_id = ?1")
        .bind(from)
        .bind(to)
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
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
         t.added_at, t.finished_at, t.last_played_at, t.last_seed_activity_at, t.deleted_at, t.uploaded_bytes_total, \
         c.kind AS kind, c.tmdb_id AS collection_tmdb_id \
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

/// The collection's RECLAIMED torrents (soft-deleted by the GC / a
/// cleanup), newest deletion first. Powers the collection page's
/// "Download again" list — the ghost-resume path for movies, which
/// have no `available_episodes` offers to re-grab from. Callers keep
/// only rows with source provenance (the actionable ones).
///
/// Per-caller: releases the user dismissed (`gone_release_dismissed`
/// at-or-after the deletion) are hidden. A re-download + re-reclaim
/// stamps a newer `deleted_at`, making the dismissal stale so the row
/// returns — same staleness model as `cw_dismissed`.
pub async fn list_deleted_in_collection(
    pool: &SqlitePool,
    collection_id: Uuid,
    user_id: UserId,
) -> Result<Vec<TorrentRow>, sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query_as::<_, TorrentRow>(
        "SELECT t.id, t.infohash, t.name, t.total_size_bytes, t.source_provider, t.source_external_id, \
         t.tmdb_id, t.tmdb_verified, t.collection_id, t.added_by, u.display_name AS added_by_name, \
         t.added_at, t.finished_at, t.last_played_at, t.last_seed_activity_at, t.deleted_at, t.uploaded_bytes_total, \
         c.kind AS kind, c.tmdb_id AS collection_tmdb_id \
         FROM torrents t \
         JOIN users u ON u.id = t.added_by \
         LEFT JOIN collections c ON c.id = t.collection_id \
         WHERE t.collection_id = ?1 AND t.deleted_at IS NOT NULL \
           AND NOT EXISTS (SELECT 1 FROM gone_release_dismissed gd \
                           WHERE gd.user_id = ?2 AND gd.infohash = t.infohash \
                             AND gd.dismissed_at >= t.deleted_at) \
         ORDER BY t.deleted_at DESC",
    )
    .bind(collection_id)
    .bind(user)
    .fetch_all(pool)
    .await
}

/// Hide one reclaimed release from the CALLER's gone surfaces (the
/// collection page's per-episode gone rows + raw release rows). Upsert
/// refreshes `dismissed_at`, so re-dismissing after a re-reclaim works.
/// History is deliberately untouched — dismissing never erases
/// `playback_progress`.
pub async fn dismiss_gone_release(
    pool: &SqlitePool,
    user_id: UserId,
    infohash: &str,
) -> Result<(), sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query(
        "INSERT INTO gone_release_dismissed (user_id, infohash, dismissed_at) \
         VALUES (?1, ?2, ?3) \
         ON CONFLICT(user_id, infohash) DO UPDATE SET dismissed_at = excluded.dismissed_at",
    )
    .bind(user)
    .bind(infohash)
    .bind(Utc::now())
    .execute(pool)
    .await?;
    Ok(())
}
