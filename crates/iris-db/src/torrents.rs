use chrono::{DateTime, Utc};
use iris_core::ids::{TorrentId, UserId};
use serde::Serialize;
use sqlx::SqlitePool;
use uuid::Uuid;

/// `SELECT … FROM … JOIN` prefix for every `TorrentRow` read, so the column
/// list and its joins can't drift from the struct's `FromRow` fields. A macro
/// so it stays a literal inside `concat!` (sqlx 0.9 only takes `&'static str`).
macro_rules! select_torrent_rows {
    () => {
        concat!(
            "SELECT t.id, t.infohash, t.name, t.total_size_bytes, t.source_provider, \
             t.source_external_id, \
             CASE WHEN t.tmdb_id_source = 'tracker' THEN t.tmdb_id END AS tmdb_id, ",
            tmdb_verified_sql!("t", "c"),
            " AS tmdb_verified, t.collection_id, t.added_by, \
             u.display_name AS added_by_name, t.added_at, t.finished_at, t.last_played_at, \
             t.last_seed_activity_at, t.deleted_at, t.uploaded_bytes_total, \
             t.downloaded_bytes_total, c.kind AS kind, c.tmdb_id AS collection_tmdb_id, \
             c.tmdb_trust AS collection_tmdb_trust \
             FROM torrents t \
             JOIN users u ON u.id = t.added_by \
             LEFT JOIN collections c ON c.id = t.collection_id"
        )
    };
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct TorrentRow {
    pub id: Uuid,
    pub infohash: String,
    pub name: String,
    pub total_size_bytes: i64,
    pub source_provider: Option<String>,
    pub source_external_id: Option<String>,
    /// The TMDB id the TRACKER shipped for this release (trust signal T1),
    /// captured at grab time. Never displayed as is: it only feeds the
    /// collection's trust evaluation (`tmdb_trust`). Legacy values of the
    /// column (mixed provenance, before migration 0045) read as `None` —
    /// only rows marked `tmdb_id_source = 'tracker'` surface here.
    pub tmdb_id: Option<i64>,
    /// Whether clients may show the collection's TMDB artwork for this
    /// torrent. Derived: `true` when the collection carries a trusted id
    /// (`tmdb_trust` set); for a legacy collection (not yet through
    /// `tmdb-trust --apply`), the old per-torrent runtime check
    /// ([`set_tmdb_verified`]).
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
    /// Lifetime download counter — max progress ever observed for this
    /// infohash (progress is absolute on-disk state, so max-ever ≈ bytes
    /// fetched). Survives restarts and, via the soft-deleted row, GC
    /// eviction. Denominator of every ratio display.
    pub downloaded_bytes_total: i64,
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
    /// Parent collection's `tmdb_trust` (`None` = legacy or no trusted id).
    pub collection_tmdb_trust: Option<String>,
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
    /// The TMDB id the tracker attached to the release, when it ships one.
    pub tracker_tmdb_id: Option<i64>,
    pub added_by: UserId,
}

/// Insert if the infohash is new, otherwise return the existing row. A live
/// row is returned as is. A soft-deleted one is brought back as the new
/// grab: the re-grabber becomes `added_by` and the release's provenance is
/// the new one. One statement, so two concurrent grabs of the same
/// infohash both succeed. The torrent's own `tmdb_id` records the tracker's id
/// (`tmdb_id_source = 'tracker'`) — evidence for the collection's trust
/// evaluation, never displayed directly.
pub async fn upsert(pool: &SqlitePool, new: NewTorrent) -> Result<TorrentRow, sqlx::Error> {
    let id = Uuid::new_v4();
    let now = Utc::now();
    let added_by: Uuid = new.added_by.into();
    // On a resurrect the payload is gone from disk (librqbit re-preallocates
    // zero-filled files), so `finished_at` is reset: `play_asset` trusts it
    // as "complete on disk". `set_finished` re-stamps it later. `added_at`
    // moves to the re-grab, else the GC ranks it its oldest candidate and
    // evicts it mid-download; the engine's upload counter restarts at 0.
    sqlx::query(
        "INSERT INTO torrents (id, infohash, name, total_size_bytes, source_provider, \
         source_external_id, added_by, added_at, tmdb_id, tmdb_id_source) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, CASE WHEN ?9 IS NOT NULL THEN 'tracker' END) \
         ON CONFLICT(infohash) DO UPDATE SET \
            source_provider = COALESCE(excluded.source_provider, torrents.source_provider), \
            source_external_id = \
                COALESCE(excluded.source_external_id, torrents.source_external_id), \
            tmdb_id = COALESCE(excluded.tmdb_id, torrents.tmdb_id), \
            tmdb_id_source = COALESCE(excluded.tmdb_id_source, torrents.tmdb_id_source), \
            added_by = excluded.added_by, \
            added_at = excluded.added_at, \
            uploaded_bytes_session_seen = 0, \
            finished_at = NULL, \
            deleted_at = NULL \
         WHERE torrents.deleted_at IS NOT NULL",
    )
    .bind(id)
    .bind(&new.infohash)
    .bind(&new.name)
    .bind(i64::try_from(new.total_size_bytes).unwrap_or(i64::MAX))
    .bind(&new.source_provider)
    .bind(&new.source_external_id)
    .bind(added_by)
    .bind(now)
    .bind(new.tracker_tmdb_id)
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
    sqlx::query_as::<_, TorrentRow>(concat!(select_torrent_rows!(), " WHERE t.infohash = ?1"))
        .bind(infohash)
        .fetch_optional(pool)
        .await
}

/// The live torrent grabbed from this tracker release, when one is on disk:
/// a release page says "Play from disk" instead of offering a grab.
pub async fn find_live_by_source(
    pool: &SqlitePool,
    provider: &str,
    external_id: &str,
) -> Result<Option<TorrentRow>, sqlx::Error> {
    sqlx::query_as::<_, TorrentRow>(concat!(
        select_torrent_rows!(),
        " WHERE t.source_provider = ?1 AND t.source_external_id = ?2 AND t.deleted_at IS NULL \
         ORDER BY t.added_at DESC LIMIT 1"
    ))
    .bind(provider)
    .bind(external_id)
    .fetch_optional(pool)
    .await
}

pub async fn list_active(pool: &SqlitePool) -> Result<Vec<TorrentRow>, sqlx::Error> {
    sqlx::query_as::<_, TorrentRow>(concat!(
        select_torrent_rows!(),
        " WHERE t.deleted_at IS NULL ORDER BY t.added_at DESC"
    ))
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

/// How many torrents are still on disk (not soft-deleted).
pub async fn count_active(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM torrents WHERE deleted_at IS NULL")
        .fetch_one(pool)
        .await?;
    Ok(n)
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
    sqlx::query(RECONCILE_UPLOADED)
        .bind(now)
        .bind(infohash)
        .execute(pool)
        .await?;
    Ok(())
}

/// Single UPDATE so the read of the previous value and the write of the new
/// total can't interleave with a delete or another reconcile pass. An
/// unchanged session counter writes nothing.
const RECONCILE_UPLOADED: &str = "UPDATE torrents SET \
       uploaded_bytes_total = uploaded_bytes_total + \
         CASE WHEN ?1 >= uploaded_bytes_session_seen \
              THEN ?1 - uploaded_bytes_session_seen \
              ELSE ?1 END, \
       uploaded_bytes_session_seen = ?1 \
     WHERE infohash = ?2 AND uploaded_bytes_session_seen IS NOT ?1";

const RECONCILE_DOWNLOADED: &str = "UPDATE torrents SET downloaded_bytes_total = ?1 \
     WHERE infohash = ?2 AND downloaded_bytes_total < ?1";

const MARK_FINISHED: &str = "UPDATE torrents SET finished_at = ?1 \
     WHERE infohash = ?2 AND finished_at IS NULL AND deleted_at IS NULL";

/// One engine snapshot's counters, for [`reconcile_snapshots`].
pub struct SnapshotCounters<'a> {
    pub infohash: &'a str,
    pub uploaded_bytes: u64,
    pub progress_bytes: u64,
    pub finished: bool,
}

/// [`reconcile_uploaded`], [`reconcile_downloaded`] and [`mark_finished`]
/// for every live torrent in one transaction: one write-lock acquisition
/// per tick instead of up to three per torrent.
pub async fn reconcile_snapshots(
    pool: &SqlitePool,
    snapshots: &[SnapshotCounters<'_>],
) -> Result<(), sqlx::Error> {
    let now = Utc::now();
    let mut tx = pool.begin().await?;
    for s in snapshots {
        sqlx::query(RECONCILE_UPLOADED)
            .bind(i64::try_from(s.uploaded_bytes).unwrap_or(i64::MAX))
            .bind(s.infohash)
            .execute(&mut *tx)
            .await?;
        sqlx::query(RECONCILE_DOWNLOADED)
            .bind(i64::try_from(s.progress_bytes).unwrap_or(i64::MAX))
            .bind(s.infohash)
            .execute(&mut *tx)
            .await?;
        if s.finished {
            sqlx::query(MARK_FINISHED)
                .bind(now)
                .bind(s.infohash)
                .execute(&mut *tx)
                .await?;
        }
    }
    tx.commit().await
}

/// Reconcile the lifetime download counter from a live snapshot's
/// `progress_bytes`. Monotonic max: progress is an absolute on-disk
/// state (it climbs during the post-restart recheck without any network
/// traffic), so deltas would phantom-count every restart — max-ever is
/// the honest approximation. A re-download after GC eviction is
/// consciously not double-counted.
pub async fn reconcile_downloaded(
    pool: &SqlitePool,
    infohash: &str,
    progress_now: u64,
) -> Result<(), sqlx::Error> {
    let now = i64::try_from(progress_now).unwrap_or(i64::MAX);
    sqlx::query(RECONCILE_DOWNLOADED)
        .bind(now)
        .bind(infohash)
        .execute(pool)
        .await?;
    Ok(())
}

/// One-shot repair for inflated lifetime upload counters (e.g. after a
/// double-writer incident counted every delta twice): clamp any row whose
/// lifetime ratio exceeds `max_ratio` back down to
/// `downloaded_bytes_total * max_ratio`. The download side is trustworthy
/// (a monotonic max of absolute on-disk progress, idempotent under
/// concurrent writers), so the ratio ceiling only ever cuts phantom
/// upload. Rows with no recorded download are left alone — partial
/// seeders genuinely upload without completing. Idempotent: a second run
/// matches no rows.
pub async fn clamp_uploaded_ratios(pool: &SqlitePool, max_ratio: u32) -> Result<u64, sqlx::Error> {
    let res = sqlx::query(
        "UPDATE torrents SET uploaded_bytes_total = downloaded_bytes_total * ?1 \
          WHERE downloaded_bytes_total > 0 \
            AND uploaded_bytes_total > downloaded_bytes_total * ?1",
    )
    .bind(i64::from(max_ratio))
    .bind(i64::from(max_ratio))
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

/// Lifetime `(uploaded, downloaded)` byte totals across every torrent ever
/// ingested, soft-deleted ones included: an evicted torrent still represents
/// work the seedbox did for the swarm, and the global ratio must compare two
/// lifetime quantities instead of lifetime upload vs current disk.
pub async fn lifetime_bytes(pool: &SqlitePool) -> Result<(u64, u64), sqlx::Error> {
    let (up, down): (Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT SUM(uploaded_bytes_total), SUM(downloaded_bytes_total) FROM torrents",
    )
    .fetch_one(pool)
    .await?;
    let bytes = |v: Option<i64>| u64::try_from(v.unwrap_or(0)).unwrap_or(0);
    Ok((bytes(up), bytes(down)))
}

/// Stamp `finished_at` (idempotent — only fills a NULL slot; `upsert`
/// resets it to NULL on re-grab so a re-download re-stamps on completion).
/// Called from the 30 s seed-stats tick for every snapshot reporting
/// `finished`, so the flag converges shortly after completion and is
/// already in place when a later deploy puts the restored session
/// through its `initializing` re-check.
pub async fn mark_finished(pool: &SqlitePool, infohash: &str) -> Result<(), sqlx::Error> {
    sqlx::query(MARK_FINISHED)
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

/// All torrents currently attached to a collection. Powers the Series
/// detail page when it walks "every file across every torrent in the
/// collection" to build the merged episode list.
pub async fn list_in_collection(
    pool: &SqlitePool,
    collection_id: Uuid,
) -> Result<Vec<TorrentRow>, sqlx::Error> {
    sqlx::query_as::<_, TorrentRow>(concat!(
        select_torrent_rows!(),
        " WHERE t.collection_id = ?1 AND t.deleted_at IS NULL \
         ORDER BY t.added_at"
    ))
    .bind(collection_id)
    .fetch_all(pool)
    .await
}

/// The collection's RECLAIMED torrents, newest deletion first — the
/// ghost-resume path (movies have no `available_episodes` to re-grab
/// from). Per-caller, same rules as
/// [`crate::episode_files::list_gone_for_collection`]: only releases
/// the caller has playback on, minus dismissed ones (stale once a
/// re-reclaim stamps a newer `deleted_at`).
pub async fn list_deleted_in_collection(
    pool: &SqlitePool,
    collection_id: Uuid,
    user_id: UserId,
) -> Result<Vec<TorrentRow>, sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query_as::<_, TorrentRow>(concat!(
        select_torrent_rows!(),
        " WHERE t.collection_id = ?1 AND t.deleted_at IS NOT NULL \
           AND EXISTS (SELECT 1 FROM playback_progress pe \
                       WHERE pe.user_id = ?2 AND pe.infohash = t.infohash) \
           AND NOT EXISTS (SELECT 1 FROM gone_release_dismissed gd \
                           WHERE gd.user_id = ?2 AND gd.infohash = t.infohash \
                             AND gd.dismissed_at >= t.deleted_at) \
         ORDER BY t.deleted_at DESC"
    ))
    .bind(collection_id)
    .bind(user)
    .fetch_all(pool)
    .await
}

/// Hide one reclaimed release from the CALLER's gone surfaces.
/// Never touches `playback_progress` — History keeps every row.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{make_user, migrated_pool};

    #[tokio::test]
    async fn lifetime_bytes_sum_deleted_torrents_too() {
        let pool = migrated_pool().await;
        assert_eq!(lifetime_bytes(&pool).await.unwrap(), (0, 0));
        let user = make_user(&pool).await;
        for (hash, up, down) in [("aa", 300, 100), ("bb", 50, 200)] {
            let row = upsert(
                &pool,
                NewTorrent {
                    infohash: hash.repeat(20),
                    name: hash.into(),
                    total_size_bytes: 1024,
                    source_provider: None,
                    source_external_id: None,
                    tracker_tmdb_id: None,
                    added_by: user,
                },
            )
            .await
            .unwrap();
            reconcile_uploaded(&pool, &row.infohash, up).await.unwrap();
            reconcile_downloaded(&pool, &row.infohash, down)
                .await
                .unwrap();
            if hash == "bb" {
                soft_delete(&pool, TorrentId(row.id)).await.unwrap();
            }
        }
        assert_eq!(lifetime_bytes(&pool).await.unwrap(), (350, 300));
        assert_eq!(count_active(&pool).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn tmdb_verified_is_derived_from_trust_and_tracker_ids_are_marked() {
        let pool = migrated_pool().await;
        let user = make_user(&pool).await;
        let film = crate::collections::find_or_create(
            &pool,
            "dune 2021",
            "Dune (2021)",
            crate::collections::Kind::Movie,
            false,
        )
        .await
        .unwrap();
        let t = upsert(
            &pool,
            NewTorrent {
                infohash: "cc".repeat(20),
                name: "Dune.2021.1080p".into(),
                total_size_bytes: 1,
                source_provider: Some("p".into()),
                source_external_id: Some("1".into()),
                tracker_tmdb_id: Some(438_631),
                added_by: user,
            },
        )
        .await
        .unwrap();
        assert_eq!(t.tmdb_id, Some(438_631));
        set_collection(&pool, &t.infohash, Some(film.id))
            .await
            .unwrap();
        let verified = || async {
            find_by_infohash(&pool, &"cc".repeat(20))
                .await
                .unwrap()
                .unwrap()
                .tmdb_verified
        };
        assert!(!verified().await, "no id");
        sqlx::query("UPDATE collections SET tmdb_id = 438631 WHERE id = ?1")
            .bind(film.id)
            .execute(&pool)
            .await
            .unwrap();
        assert!(!verified().await, "legacy id, runtime check not passed");
        set_tmdb_verified(&pool, &t.infohash, true).await.unwrap();
        assert!(verified().await, "legacy id, runtime check passed");
        crate::collections::set_trusted_tmdb(&pool, film.id, 438_631, "scene")
            .await
            .unwrap();
        set_tmdb_verified(&pool, &t.infohash, false).await.unwrap();
        assert!(
            verified().await,
            "trusted id: the runtime flag no longer matters"
        );
        crate::collections::clear_tmdb_id(&pool, film.id)
            .await
            .unwrap();
        assert!(!verified().await);

        sqlx::query("UPDATE torrents SET tmdb_id = 99, tmdb_id_source = NULL WHERE infohash = ?1")
            .bind(&t.infohash)
            .execute(&pool)
            .await
            .unwrap();
        let legacy = find_by_infohash(&pool, &t.infohash).await.unwrap().unwrap();
        assert_eq!(
            legacy.tmdb_id, None,
            "an unmarked legacy id is not a tracker id"
        );
        assert_eq!(
            crate::collections::tracker_tmdb_ids(&pool, film.id)
                .await
                .unwrap(),
            Vec::<i64>::new()
        );
    }

    /// Re-grabbing an evicted torrent must reset `finished_at`: the payload
    /// is gone from disk, and endpoints (`play_asset` & co) trust
    /// `finished_at` as "complete on disk". A stale stamp made them probe a
    /// zero-filled librqbit preallocation and 500 instead of returning the
    /// retryable "still downloading" 400.
    #[tokio::test]
    async fn regrab_resets_finished_at() {
        let pool = migrated_pool().await;
        let user = crate::test_support::make_user(&pool).await;

        let new = NewTorrent {
            infohash: "aa".repeat(20),
            name: "Splash 1984".into(),
            total_size_bytes: 1024,
            source_provider: None,
            source_external_id: None,
            tracker_tmdb_id: None,
            added_by: user,
        };
        let row = upsert(&pool, new.clone()).await.unwrap();
        mark_finished(&pool, &row.infohash).await.unwrap();
        soft_delete(&pool, TorrentId(row.id)).await.unwrap();

        sqlx::query("UPDATE torrents SET added_at = ?1, uploaded_bytes_session_seen = 500")
            .bind(Utc::now() - chrono::Duration::days(30))
            .execute(&pool)
            .await
            .unwrap();
        let regrabbed = upsert(&pool, new).await.unwrap();
        assert_eq!(regrabbed.id, row.id, "re-grab reuses the row");
        assert!(
            regrabbed.added_at > Utc::now() - chrono::Duration::minutes(1),
            "a re-grab is a fresh add for the GC"
        );
        let seen: i64 = sqlx::query_scalar("SELECT uploaded_bytes_session_seen FROM torrents")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(seen, 0, "the new engine handle counts uploads from 0");
        assert!(regrabbed.deleted_at.is_none(), "re-grab un-soft-deletes");
        assert!(
            regrabbed.finished_at.is_none(),
            "re-grab must clear the stale finished_at"
        );

        // …and completion of the re-download re-stamps it.
        mark_finished(&pool, &regrabbed.infohash).await.unwrap();
        let done = find_by_infohash(&pool, &regrabbed.infohash)
            .await
            .unwrap()
            .unwrap();
        assert!(done.finished_at.is_some());
    }

    /// A live (non-deleted) duplicate grab keeps its `finished_at` — only
    /// the evicted→re-grab path resets it.
    #[tokio::test]
    async fn duplicate_grab_of_live_torrent_keeps_finished_at() {
        let pool = migrated_pool().await;
        let user = crate::test_support::make_user(&pool).await;

        let new = NewTorrent {
            infohash: "bb".repeat(20),
            name: "Still Here".into(),
            total_size_bytes: 2048,
            source_provider: None,
            source_external_id: None,
            tracker_tmdb_id: None,
            added_by: user,
        };
        let row = upsert(&pool, new.clone()).await.unwrap();
        mark_finished(&pool, &row.infohash).await.unwrap();

        let again = upsert(&pool, new).await.unwrap();
        assert_eq!(again.id, row.id);
        assert!(again.finished_at.is_some(), "live dup keeps finished_at");
    }

    #[tokio::test]
    async fn regrab_of_a_deleted_torrent_belongs_to_the_new_grab() {
        let pool = migrated_pool().await;
        let first = crate::test_support::make_named_user(&pool, "A").await;
        let second = crate::test_support::make_named_user(&pool, "B").await;
        let grab = |by: UserId, provider: &str| NewTorrent {
            infohash: "cc".repeat(20),
            name: "Heat 1995".into(),
            total_size_bytes: 1,
            source_provider: Some(provider.into()),
            source_external_id: Some(format!("{provider}-1")),
            tracker_tmdb_id: None,
            added_by: by,
        };
        let row = upsert(&pool, grab(first, "c411")).await.unwrap();

        let live = upsert(&pool, grab(second, "seedpool")).await.unwrap();
        assert_eq!(live.added_by, row.added_by, "a live row keeps its grabber");
        assert_eq!(live.source_provider.as_deref(), Some("c411"));

        soft_delete(&pool, TorrentId(row.id)).await.unwrap();
        let back = upsert(&pool, grab(second, "seedpool")).await.unwrap();
        assert_eq!(back.id, row.id);
        assert_eq!(back.added_by, Uuid::from(second));
        assert_eq!(back.source_provider.as_deref(), Some("seedpool"));
        assert_eq!(back.source_external_id.as_deref(), Some("seedpool-1"));
    }

    #[tokio::test]
    async fn snapshot_reconcile_counts_deltas_once_and_stamps_finished() {
        let pool = migrated_pool().await;
        let user = make_user(&pool).await;
        let row = upsert(
            &pool,
            NewTorrent {
                infohash: "ee".repeat(20),
                name: "Seeded".into(),
                total_size_bytes: 100,
                source_provider: None,
                source_external_id: None,
                tracker_tmdb_id: None,
                added_by: user,
            },
        )
        .await
        .unwrap();
        let tick = |uploaded, progress, finished| SnapshotCounters {
            infohash: &row.infohash,
            uploaded_bytes: uploaded,
            progress_bytes: progress,
            finished,
        };
        reconcile_snapshots(&pool, &[tick(100, 50, false)])
            .await
            .unwrap();
        reconcile_snapshots(&pool, &[tick(100, 40, true)])
            .await
            .unwrap();
        // A restart resets the session counter: 30 is a fresh delta.
        reconcile_snapshots(&pool, &[tick(30, 100, true)])
            .await
            .unwrap();
        let got = find_by_infohash(&pool, &row.infohash)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.uploaded_bytes_total, 130);
        assert_eq!(got.downloaded_bytes_total, 100);
        assert!(got.finished_at.is_some());
    }

    #[tokio::test]
    async fn concurrent_first_grabs_share_one_row() {
        let pool = migrated_pool().await;
        let user = make_user(&pool).await;
        let new = NewTorrent {
            infohash: "dd".repeat(20),
            name: "Twice".into(),
            total_size_bytes: 1,
            source_provider: None,
            source_external_id: None,
            tracker_tmdb_id: None,
            added_by: user,
        };
        let (a, b) = tokio::join!(upsert(&pool, new.clone()), upsert(&pool, new));
        assert_eq!(a.unwrap().id, b.unwrap().id);
    }

    /// Upsert a row then overwrite its lifetime counters — test helper
    /// for seeding exact counter states without fabricating sessions.
    async fn set_counters(
        pool: &SqlitePool,
        user: UserId,
        infohash: &str,
        up: i64,
        down: i64,
    ) -> String {
        let row = upsert(
            pool,
            NewTorrent {
                infohash: infohash.into(),
                name: "x".into(),
                total_size_bytes: 1024,
                source_provider: None,
                source_external_id: None,
                tracker_tmdb_id: None,
                added_by: user,
            },
        )
        .await
        .unwrap();
        sqlx::query(
            "UPDATE torrents SET uploaded_bytes_total = ?1, downloaded_bytes_total = ?2 \
              WHERE id = ?3",
        )
        .bind(up)
        .bind(down)
        .bind(row.id)
        .execute(pool)
        .await
        .unwrap();
        row.infohash
    }

    /// One-shot clamp: a 4411x row comes back to the ceiling, sane rows
    /// and download-less partial seeders are untouched, re-running is a
    /// no-op, and the next session delta accumulates cleanly on top.
    #[tokio::test]
    async fn clamp_uploaded_ratios_repairs_only_outliers() {
        let pool = migrated_pool().await;
        let user = crate::test_support::make_user(&pool).await;

        let wild = set_counters(&pool, user, &"cc".repeat(20), 4_411_000, 1_000).await;
        let sane = set_counters(&pool, user, &"dd".repeat(20), 5_000, 1_000).await;
        let partial = set_counters(&pool, user, &"ee".repeat(20), 9_000, 0).await;

        assert_eq!(clamp_uploaded_ratios(&pool, 10).await.unwrap(), 1);

        let row = find_by_infohash(&pool, &wild).await.unwrap().unwrap();
        assert_eq!(row.uploaded_bytes_total, 10_000);
        let row = find_by_infohash(&pool, &sane).await.unwrap().unwrap();
        assert_eq!(row.uploaded_bytes_total, 5_000);
        let row = find_by_infohash(&pool, &partial).await.unwrap().unwrap();
        assert_eq!(row.uploaded_bytes_total, 9_000);

        assert_eq!(
            clamp_uploaded_ratios(&pool, 10).await.unwrap(),
            0,
            "second run matches nothing"
        );

        // Session deltas keep accumulating honestly on the repaired base.
        reconcile_uploaded(&pool, &wild, 100).await.unwrap();
        let row = find_by_infohash(&pool, &wild).await.unwrap().unwrap();
        assert_eq!(row.uploaded_bytes_total, 10_100);
    }
}
