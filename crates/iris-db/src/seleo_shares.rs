//! Films an admin shares with `SéléoTV` (`seleo_shares`, migration 0052): one
//! file of a finished movie torrent, for a limited time, with the metadata
//! snapshot its manifest entry is built from.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use iris_core::ids::UserId;
use sqlx::SqlitePool;
use uuid::Uuid;

/// `SELECT … FROM … JOIN` prefix of every [`ShareRow`] read. A macro so it
/// stays a literal inside `concat!` (sqlx 0.9 only takes `&'static str`).
macro_rules! select_shares {
    () => {
        "SELECT s.infohash, s.file_idx, s.shared_by, u.display_name AS shared_by_name, \
         t.collection_id, s.shared_at, s.until, s.path, s.size_bytes, s.tmdb_id, s.title, \
         s.year, s.overview, s.runtime_minutes, s.poster \
         FROM seleo_shares s \
         JOIN torrents t ON t.infohash = s.infohash \
         LEFT JOIN users u ON u.id = s.shared_by"
    };
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct ShareRow {
    pub infohash: String,
    pub file_idx: i64,
    /// `None` once the admin who shared it is deleted.
    pub shared_by: Option<Uuid>,
    pub shared_by_name: Option<String>,
    pub collection_id: Option<Uuid>,
    pub shared_at: DateTime<Utc>,
    pub until: DateTime<Utc>,
    /// The file on disk, relative to the download dir.
    pub path: String,
    pub size_bytes: i64,
    pub tmdb_id: Option<i64>,
    pub title: String,
    pub year: Option<i64>,
    pub overview: Option<String>,
    pub runtime_minutes: Option<i64>,
    /// File name under `share/posters/`.
    pub poster: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewShare<'a> {
    pub infohash: &'a str,
    pub file_idx: i64,
    pub shared_by: UserId,
    pub until: DateTime<Utc>,
    pub path: &'a str,
    pub size_bytes: i64,
    pub tmdb_id: Option<i64>,
    pub title: &'a str,
    pub year: Option<i64>,
    pub overview: Option<&'a str>,
    pub runtime_minutes: Option<i64>,
    pub poster: Option<&'a str>,
}

/// Share a file from now, replacing any earlier share of it whole (an
/// expired one the hourly sweep hasn't dropped yet).
pub async fn put(pool: &SqlitePool, new: &NewShare<'_>) -> Result<(), sqlx::Error> {
    let by: Uuid = new.shared_by.into();
    sqlx::query(
        "INSERT INTO seleo_shares (infohash, file_idx, shared_by, shared_at, until, path, \
           size_bytes, tmdb_id, title, year, overview, runtime_minutes, poster) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13) \
         ON CONFLICT(infohash, file_idx) DO UPDATE SET \
           shared_by = excluded.shared_by, shared_at = excluded.shared_at, \
           until = excluded.until, path = excluded.path, size_bytes = excluded.size_bytes, \
           tmdb_id = excluded.tmdb_id, title = excluded.title, year = excluded.year, \
           overview = excluded.overview, runtime_minutes = excluded.runtime_minutes, \
           poster = excluded.poster",
    )
    .bind(new.infohash)
    .bind(new.file_idx)
    .bind(by)
    .bind(Utc::now())
    .bind(new.until)
    .bind(new.path)
    .bind(new.size_bytes)
    .bind(new.tmdb_id)
    .bind(new.title)
    .bind(new.year)
    .bind(new.overview)
    .bind(new.runtime_minutes)
    .bind(new.poster)
    .execute(pool)
    .await?;
    Ok(())
}

/// Move a share's end; `false` when there is no such share.
pub async fn extend(
    pool: &SqlitePool,
    infohash: &str,
    file_idx: i64,
    until: DateTime<Utc>,
) -> Result<bool, sqlx::Error> {
    let res =
        sqlx::query("UPDATE seleo_shares SET until = ?1 WHERE infohash = ?2 AND file_idx = ?3")
            .bind(until)
            .bind(infohash)
            .bind(file_idx)
            .execute(pool)
            .await?;
    Ok(res.rows_affected() == 1)
}

/// A share, running or not, of a torrent still on disk.
pub async fn get(
    pool: &SqlitePool,
    infohash: &str,
    file_idx: i64,
) -> Result<Option<ShareRow>, sqlx::Error> {
    sqlx::query_as::<_, ShareRow>(concat!(
        select_shares!(),
        " WHERE s.infohash = ?1 AND s.file_idx = ?2 AND t.deleted_at IS NULL"
    ))
    .bind(infohash)
    .bind(file_idx)
    .fetch_optional(pool)
    .await
}

/// The shares running at `now` of torrents still on disk, newest first:
/// what the manifest lists.
pub async fn list_live(
    pool: &SqlitePool,
    now: DateTime<Utc>,
) -> Result<Vec<ShareRow>, sqlx::Error> {
    sqlx::query_as::<_, ShareRow>(concat!(
        select_shares!(),
        " WHERE s.until > ?1 AND t.deleted_at IS NULL \
         ORDER BY s.shared_at DESC, s.infohash, s.file_idx"
    ))
    .bind(now)
    .fetch_all(pool)
    .await
}

/// When the torrent's last running share ends; `None` when none runs.
pub async fn shared_until(
    pool: &SqlitePool,
    infohash: &str,
    now: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
    let row: Option<(DateTime<Utc>,)> = sqlx::query_as(
        "SELECT until FROM seleo_shares WHERE infohash = ?1 AND until > ?2 \
         ORDER BY until DESC LIMIT 1",
    )
    .bind(infohash)
    .bind(now)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(until,)| until))
}

/// Infohashes with a share running at `now`.
pub async fn shared_infohashes(
    pool: &SqlitePool,
    now: DateTime<Utc>,
) -> Result<HashSet<String>, sqlx::Error> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT DISTINCT infohash FROM seleo_shares WHERE until > ?1")
            .bind(now)
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|(h,)| h).collect())
}

/// Stop a share. `None`: there was none; else its poster, to delete.
pub async fn delete(
    pool: &SqlitePool,
    infohash: &str,
    file_idx: i64,
) -> Result<Option<Option<String>>, sqlx::Error> {
    let row: Option<(Option<String>,)> = sqlx::query_as(
        "DELETE FROM seleo_shares WHERE infohash = ?1 AND file_idx = ?2 RETURNING poster",
    )
    .bind(infohash)
    .bind(file_idx)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(poster,)| poster))
}

/// Drop every share of a torrent that left the disk; answers how many there
/// were and their posters.
pub async fn delete_for_infohash(
    pool: &SqlitePool,
    infohash: &str,
) -> Result<Vec<Option<String>>, sqlx::Error> {
    let rows: Vec<(Option<String>,)> =
        sqlx::query_as("DELETE FROM seleo_shares WHERE infohash = ?1 RETURNING poster")
            .bind(infohash)
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|(p,)| p).collect())
}

/// Drop the shares over at `now` and those of torrents no longer on disk;
/// answers their posters, one entry per dropped share.
pub async fn take_stale(
    pool: &SqlitePool,
    now: DateTime<Utc>,
) -> Result<Vec<Option<String>>, sqlx::Error> {
    let rows: Vec<(Option<String>,)> = sqlx::query_as(
        "DELETE FROM seleo_shares WHERE until <= ?1 \
           OR infohash NOT IN (SELECT infohash FROM torrents WHERE deleted_at IS NULL) \
         RETURNING poster",
    )
    .bind(now)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(p,)| p).collect())
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};
    use iris_core::ids::{TorrentId, UserId};

    use super::NewShare;
    use crate::test_support::{make_named_user, migrated_pool};

    async fn torrent(pool: &sqlx::SqlitePool, by: UserId, c: char) -> crate::torrents::TorrentRow {
        crate::torrents::upsert(
            pool,
            crate::torrents::NewTorrent {
                infohash: c.to_string().repeat(40),
                name: format!("Film.{c}.2019.1080p-GRP"),
                total_size_bytes: 1_000,
                source_provider: None,
                source_external_id: None,
                tracker_tmdb_id: None,
                added_by: by,
            },
        )
        .await
        .unwrap()
    }

    fn share<'a>(
        infohash: &'a str,
        by: UserId,
        days: i64,
        poster: Option<&'a str>,
    ) -> NewShare<'a> {
        NewShare {
            infohash,
            file_idx: 0,
            shared_by: by,
            until: Utc::now() + Duration::days(days),
            path: "Film/film.mkv",
            size_bytes: 1_000,
            tmdb_id: Some(530_915),
            title: "1917",
            year: Some(2019),
            overview: Some("Deux soldats."),
            runtime_minutes: Some(119),
            poster,
        }
    }

    #[tokio::test]
    async fn a_share_runs_until_its_end_and_is_then_swept_with_its_poster() {
        let pool = migrated_pool().await;
        let admin = make_named_user(&pool, "Ana").await;
        let a = torrent(&pool, admin, 'a').await;
        let b = torrent(&pool, admin, 'b').await;
        super::put(&pool, &share(&a.infohash, admin, 7, Some("a.jpg")))
            .await
            .unwrap();
        super::put(&pool, &share(&b.infohash, admin, 7, None))
            .await
            .unwrap();
        let now = Utc::now();

        let live = super::list_live(&pool, now).await.unwrap();
        let order: Vec<&str> = live.iter().map(|s| s.infohash.as_str()).collect();
        assert_eq!(
            order,
            [b.infohash.as_str(), a.infohash.as_str()],
            "newest first"
        );
        assert_eq!(live[1].shared_by_name.as_deref(), Some("Ana"));
        assert_eq!(live[1].year, Some(2019));
        assert!(
            super::shared_until(&pool, &a.infohash, now)
                .await
                .unwrap()
                .is_some()
        );
        assert_eq!(super::shared_infohashes(&pool, now).await.unwrap().len(), 2);

        let ended = now - Duration::hours(1);
        assert!(super::extend(&pool, &a.infohash, 0, ended).await.unwrap());
        assert!(!super::extend(&pool, &a.infohash, 3, ended).await.unwrap());
        assert_eq!(
            super::shared_until(&pool, &a.infohash, now).await.unwrap(),
            None
        );
        assert_eq!(super::list_live(&pool, now).await.unwrap().len(), 1);
        assert!(
            super::get(&pool, &a.infohash, 0).await.unwrap().is_some(),
            "until swept"
        );

        assert_eq!(
            super::take_stale(&pool, now).await.unwrap(),
            [Some("a.jpg".to_string())]
        );
        assert!(super::get(&pool, &a.infohash, 0).await.unwrap().is_none());
        assert_eq!(
            super::delete(&pool, &b.infohash, 0).await.unwrap(),
            Some(None)
        );
        assert_eq!(super::delete(&pool, &b.infohash, 0).await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_share_of_a_torrent_gone_from_disk_is_never_listed_and_is_swept() {
        let pool = migrated_pool().await;
        let admin = make_named_user(&pool, "Ana").await;
        let a = torrent(&pool, admin, 'a').await;
        super::put(&pool, &share(&a.infohash, admin, 7, Some("a.jpg")))
            .await
            .unwrap();
        crate::torrents::soft_delete(&pool, TorrentId::from(a.id))
            .await
            .unwrap();
        let now = Utc::now();
        assert_eq!(super::list_live(&pool, now).await.unwrap(), Vec::new());
        assert!(super::get(&pool, &a.infohash, 0).await.unwrap().is_none());
        assert_eq!(super::take_stale(&pool, now).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_reshare_starts_afresh_and_outlives_the_admin_who_made_it() {
        let pool = migrated_pool().await;
        let owner = make_named_user(&pool, "Owner").await;
        let admin = make_named_user(&pool, "Ana").await;
        let a = torrent(&pool, owner, 'a').await;
        super::put(&pool, &share(&a.infohash, admin, 1, Some("a.jpg")))
            .await
            .unwrap();
        let first = super::get(&pool, &a.infohash, 0).await.unwrap().unwrap();
        super::put(&pool, &share(&a.infohash, admin, 30, None))
            .await
            .unwrap();
        let again = super::get(&pool, &a.infohash, 0).await.unwrap().unwrap();
        assert!(again.shared_at >= first.shared_at && again.until > first.until);
        assert_eq!(again.poster, None);

        crate::users::delete(&pool, admin, owner).await.unwrap();
        let kept = super::get(&pool, &a.infohash, 0).await.unwrap().unwrap();
        assert_eq!((kept.shared_by, kept.shared_by_name), (None, None));
        assert_eq!(
            super::delete_for_infohash(&pool, &a.infohash)
                .await
                .unwrap(),
            [None]
        );
    }
}
