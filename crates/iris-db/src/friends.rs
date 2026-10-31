//! Friendships between profiles, what a profile shares with them, and the
//! activity they see.

use chrono::{DateTime, Utc};
use iris_core::ids::UserId;
use sqlx::SqlitePool;
use uuid::Uuid;

/// Who a profile's accepted friends are, for `?1` the profile.
macro_rules! friends_cte {
    () => {
        "WITH friends(id) AS ( \
           SELECT addressee_id FROM friendships WHERE requester_id = ?1 AND accepted_at IS NOT NULL \
           UNION \
           SELECT requester_id FROM friendships WHERE addressee_id = ?1 AND accepted_at IS NOT NULL) "
    };
}

/// Keeps a row of torrent `t` its owner `$owner` did not hide.
macro_rules! not_hidden {
    ($owner:literal) => {
        concat!(
            " NOT EXISTS (SELECT 1 FROM friends_hidden h WHERE h.user_id = ",
            $owner,
            " AND (h.collection_id = t.collection_id OR h.infohash = t.infohash)) "
        )
    };
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Person {
    pub id: Uuid,
    pub name: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Relation {
    pub id: Uuid,
    pub name: String,
    pub since: DateTime<Utc>,
    pub accepted: bool,
    pub incoming: bool,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct HiddenTitle {
    pub collection_id: Option<Uuid>,
    pub infohash: Option<String>,
    pub title: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Title {
    pub collection_id: Option<Uuid>,
    pub infohash: String,
    pub title: String,
    pub season: Option<i64>,
    pub episode: Option<i64>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Event {
    pub friend_id: Uuid,
    pub friend_name: String,
    pub at: DateTime<Utc>,
    #[sqlx(flatten)]
    pub title: Title,
}

/// Ask `to`; when `to` already asked, that is a yes both ways. Asking again
/// changes nothing, and a declined asker is told nothing.
pub async fn request(pool: &SqlitePool, from: UserId, to: UserId) -> Result<(), sqlx::Error> {
    let (from, to): (Uuid, Uuid) = (from.into(), to.into());
    let now = Utc::now();
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let answered = sqlx::query(
        "UPDATE friendships SET accepted_at = COALESCE(accepted_at, ?1), declined_at = NULL \
         WHERE requester_id = ?2 AND addressee_id = ?3",
    )
    .bind(now)
    .bind(to)
    .bind(from)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if answered == 0 {
        sqlx::query(
            "INSERT OR IGNORE INTO friendships (requester_id, addressee_id, created_at) \
             VALUES (?1, ?2, ?3)",
        )
        .bind(from)
        .bind(to)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await
}

/// `true` when `other` had asked `me`, and now they are friends.
pub async fn accept(pool: &SqlitePool, me: UserId, other: UserId) -> Result<bool, sqlx::Error> {
    let res = sqlx::query(
        "UPDATE friendships SET accepted_at = ?1 \
         WHERE requester_id = ?2 AND addressee_id = ?3 \
           AND accepted_at IS NULL AND declined_at IS NULL",
    )
    .bind(Utc::now())
    .bind(Uuid::from(other))
    .bind(Uuid::from(me))
    .execute(pool)
    .await?;
    Ok(res.rows_affected() == 1)
}

pub async fn decline(pool: &SqlitePool, me: UserId, other: UserId) -> Result<bool, sqlx::Error> {
    let res = sqlx::query(
        "UPDATE friendships SET declined_at = ?1 \
         WHERE requester_id = ?2 AND addressee_id = ?3 \
           AND accepted_at IS NULL AND declined_at IS NULL",
    )
    .bind(Utc::now())
    .bind(Uuid::from(other))
    .bind(Uuid::from(me))
    .execute(pool)
    .await?;
    Ok(res.rows_affected() == 1)
}

/// End a friendship, or take back one's own request. A request `me` declined
/// stays, or asking again would get past the decline.
pub async fn remove(pool: &SqlitePool, me: UserId, other: UserId) -> Result<bool, sqlx::Error> {
    let res = sqlx::query(
        "DELETE FROM friendships \
         WHERE (requester_id = ?1 AND addressee_id = ?2) \
            OR (requester_id = ?2 AND addressee_id = ?1 AND accepted_at IS NOT NULL)",
    )
    .bind(Uuid::from(me))
    .bind(Uuid::from(other))
    .execute(pool)
    .await?;
    Ok(res.rows_affected() > 0)
}

/// Friends and live requests. What `me` declined is gone from its side; what
/// was declined to `me` still reads as pending.
pub async fn relations(pool: &SqlitePool, me: UserId) -> Result<Vec<Relation>, sqlx::Error> {
    sqlx::query_as(
        "SELECT u.id, u.display_name AS name, COALESCE(f.accepted_at, f.created_at) AS since, \
                f.accepted_at IS NOT NULL AS accepted, f.addressee_id = ?1 AS incoming \
         FROM friendships f \
         JOIN users u ON u.id = CASE WHEN f.requester_id = ?1 THEN f.addressee_id ELSE f.requester_id END \
         WHERE (f.requester_id = ?1 OR f.addressee_id = ?1) \
           AND NOT (f.addressee_id = ?1 AND f.declined_at IS NOT NULL) \
         ORDER BY u.display_name COLLATE NOCASE",
    )
    .bind(Uuid::from(me))
    .fetch_all(pool)
    .await
}

/// How many requests wait for `me`.
pub async fn incoming_count(pool: &SqlitePool, me: UserId) -> Result<i64, sqlx::Error> {
    let (n,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM friendships \
         WHERE addressee_id = ?1 AND accepted_at IS NULL AND declined_at IS NULL",
    )
    .bind(Uuid::from(me))
    .fetch_one(pool)
    .await?;
    Ok(n)
}

/// Profiles by name for `me` to ask, none it is already linked to either way.
pub async fn search(
    pool: &SqlitePool,
    me: UserId,
    query: &str,
    limit: i64,
) -> Result<Vec<Person>, sqlx::Error> {
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    sqlx::query_as(
        "SELECT u.id, u.display_name AS name FROM users u \
         WHERE u.id <> ?1 AND u.display_name LIKE '%' || ?2 || '%' ESCAPE '\\' \
           AND NOT EXISTS (SELECT 1 FROM friendships f \
             WHERE (f.requester_id = ?1 AND f.addressee_id = u.id) \
                OR (f.requester_id = u.id AND f.addressee_id = ?1)) \
         ORDER BY u.display_name COLLATE NOCASE LIMIT ?3",
    )
    .bind(Uuid::from(me))
    .bind(escaped)
    .bind(limit)
    .fetch_all(pool)
    .await
}

pub async fn sharing(pool: &SqlitePool, me: UserId) -> Result<bool, sqlx::Error> {
    let (on,): (bool,) = sqlx::query_as("SELECT share_watching FROM users WHERE id = ?1")
        .bind(Uuid::from(me))
        .fetch_one(pool)
        .await?;
    Ok(on)
}

pub async fn set_sharing(pool: &SqlitePool, me: UserId, on: bool) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE users SET share_watching = ?1 WHERE id = ?2")
        .bind(on)
        .bind(Uuid::from(me))
        .execute(pool)
        .await?;
    Ok(())
}

/// Keep a release's whole title from friends. `false`: no such release.
pub async fn hide(pool: &SqlitePool, me: UserId, infohash: &str) -> Result<bool, sqlx::Error> {
    let res = sqlx::query(
        "INSERT OR IGNORE INTO friends_hidden (user_id, collection_id, infohash, created_at) \
         SELECT ?1, t.collection_id, CASE WHEN t.collection_id IS NULL THEN t.infohash END, ?2 \
         FROM torrents t WHERE t.infohash = ?3",
    )
    .bind(Uuid::from(me))
    .bind(Utc::now())
    .bind(infohash)
    .execute(pool)
    .await?;
    let (known,): (bool,) =
        sqlx::query_as("SELECT EXISTS (SELECT 1 FROM torrents WHERE infohash = ?1)")
            .bind(infohash)
            .fetch_one(pool)
            .await?;
    Ok(res.rows_affected() > 0 || known)
}

pub async fn unhide(
    pool: &SqlitePool,
    me: UserId,
    collection_id: Option<Uuid>,
    infohash: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DELETE FROM friends_hidden \
         WHERE user_id = ?1 AND (collection_id = ?2 OR infohash = ?3)",
    )
    .bind(Uuid::from(me))
    .bind(collection_id)
    .bind(infohash)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn hidden(pool: &SqlitePool, me: UserId) -> Result<Vec<HiddenTitle>, sqlx::Error> {
    sqlx::query_as(
        "SELECT h.collection_id, h.infohash, COALESCE(c.display_title, t.name, '') AS title \
         FROM friends_hidden h \
         LEFT JOIN collections c ON c.id = h.collection_id \
         LEFT JOIN torrents t ON t.infohash = h.infohash \
         WHERE h.user_id = ?1 ORDER BY h.created_at DESC",
    )
    .bind(Uuid::from(me))
    .fetch_all(pool)
    .await
}

/// `me`'s friends who share what they watch.
pub async fn sharing_friends(pool: &SqlitePool, me: UserId) -> Result<Vec<Person>, sqlx::Error> {
    sqlx::query_as(concat!(
        friends_cte!(),
        "SELECT u.id, u.display_name AS name FROM users u \
         JOIN friends ON friends.id = u.id WHERE u.share_watching"
    ))
    .bind(Uuid::from(me))
    .fetch_all(pool)
    .await
}

/// What `owner` is playing, unless kept from friends.
pub async fn shown_title(
    pool: &SqlitePool,
    owner: Uuid,
    infohash: &str,
    file_idx: i64,
) -> Result<Option<Title>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT t.collection_id, t.infohash, COALESCE(c.display_title, t.name) AS title, \
                ef.season, ef.episode \
         FROM torrents t \
         LEFT JOIN collections c ON c.id = t.collection_id \
         LEFT JOIN episode_files ef ON ef.infohash = t.infohash AND ef.file_idx = ?3 \
         WHERE t.infohash = ?2 AND",
        not_hidden!("?1")
    ))
    .bind(owner)
    .bind(infohash)
    .bind(file_idx)
    .fetch_optional(pool)
    .await
}

/// What friends who share watched last, newest first.
pub async fn recently_watched(
    pool: &SqlitePool,
    me: UserId,
    limit: i64,
) -> Result<Vec<Event>, sqlx::Error> {
    sqlx::query_as(concat!(
        friends_cte!(),
        "SELECT u.id AS friend_id, u.display_name AS friend_name, p.last_watched_at AS at, \
                t.collection_id, t.infohash, COALESCE(c.display_title, t.name) AS title, \
                ef.season, ef.episode \
         FROM playback_progress p \
         JOIN friends ON friends.id = p.user_id \
         JOIN users u ON u.id = p.user_id AND u.share_watching \
         JOIN torrents t ON t.infohash = p.infohash \
         LEFT JOIN collections c ON c.id = t.collection_id \
         LEFT JOIN episode_files ef ON ef.infohash = p.infohash AND ef.file_idx = p.file_idx \
         WHERE",
        not_hidden!("p.user_id"),
        "ORDER BY p.last_watched_at DESC LIMIT ?2"
    ))
    .bind(Uuid::from(me))
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// What friends added to the library last, newest first.
pub async fn recently_added(
    pool: &SqlitePool,
    me: UserId,
    limit: i64,
) -> Result<Vec<Event>, sqlx::Error> {
    sqlx::query_as(concat!(
        friends_cte!(),
        "SELECT u.id AS friend_id, u.display_name AS friend_name, t.added_at AS at, \
                t.collection_id, t.infohash, COALESCE(c.display_title, t.name) AS title, \
                NULL AS season, NULL AS episode \
         FROM torrents t \
         JOIN friends ON friends.id = t.added_by \
         JOIN users u ON u.id = t.added_by \
         LEFT JOIN collections c ON c.id = t.collection_id \
         WHERE t.deleted_at IS NULL AND",
        not_hidden!("t.added_by"),
        "ORDER BY t.added_at DESC LIMIT ?2"
    ))
    .bind(Uuid::from(me))
    .bind(limit)
    .fetch_all(pool)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{make_named_user, make_user, migrated_pool};

    async fn torrent(pool: &SqlitePool, infohash: &str, owner: UserId) {
        sqlx::query(
            "INSERT INTO torrents (id, infohash, name, total_size_bytes, added_by, added_at) \
             VALUES (?1, ?2, ?3, 1, ?4, ?5)",
        )
        .bind(Uuid::new_v4())
        .bind(infohash)
        .bind(format!("Release.{infohash}"))
        .bind(Uuid::from(owner))
        .bind(Utc::now())
        .execute(pool)
        .await
        .unwrap();
    }

    async fn watch(pool: &SqlitePool, user: UserId, infohash: &str) {
        sqlx::query(
            "INSERT INTO playback_progress (user_id, infohash, file_idx, last_watched_at) \
             VALUES (?1, ?2, 0, ?3)",
        )
        .bind(Uuid::from(user))
        .bind(infohash)
        .bind(Utc::now())
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn a_declined_asker_is_told_nothing_and_cannot_ask_again() {
        let pool = migrated_pool().await;
        let (a, b) = (make_user(&pool).await, make_user(&pool).await);
        request(&pool, a, b).await.unwrap();
        assert_eq!(incoming_count(&pool, b).await.unwrap(), 1);
        assert!(decline(&pool, b, a).await.unwrap());
        assert_eq!(incoming_count(&pool, b).await.unwrap(), 0);
        assert!(relations(&pool, b).await.unwrap().is_empty());
        let asker = relations(&pool, a).await.unwrap();
        assert!(!asker[0].accepted && !asker[0].incoming);

        request(&pool, a, b).await.unwrap();
        assert_eq!(incoming_count(&pool, b).await.unwrap(), 0);
        assert!(!remove(&pool, b, a).await.unwrap(), "the decline stays");
    }

    #[tokio::test]
    async fn asking_someone_who_asked_is_a_yes() {
        let pool = migrated_pool().await;
        let (a, b) = (make_user(&pool).await, make_user(&pool).await);
        request(&pool, a, b).await.unwrap();
        request(&pool, b, a).await.unwrap();
        assert!(relations(&pool, a).await.unwrap()[0].accepted);
        assert!(relations(&pool, b).await.unwrap()[0].accepted);
    }

    #[tokio::test]
    async fn search_finds_by_name_and_leaves_out_who_is_linked() {
        let pool = migrated_pool().await;
        let me = make_named_user(&pool, "Vincent").await;
        let muriel = make_named_user(&pool, "Muriel").await;
        make_named_user(&pool, "100%").await;
        let names = |v: Vec<Person>| v.into_iter().map(|p| p.name).collect::<Vec<_>>();
        assert_eq!(
            names(search(&pool, me, "uri", 20).await.unwrap()),
            ["Muriel"]
        );
        assert_eq!(names(search(&pool, me, "%", 20).await.unwrap()), ["100%"]);
        assert!(search(&pool, me, "vin", 20).await.unwrap().is_empty());
        request(&pool, muriel, me).await.unwrap();
        assert!(search(&pool, me, "uri", 20).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn watching_reaches_friends_only_once_shared_and_never_when_hidden() {
        let pool = migrated_pool().await;
        let (me, friend, stranger) = (
            make_user(&pool).await,
            make_user(&pool).await,
            make_user(&pool).await,
        );
        torrent(&pool, "aaaa", friend).await;
        torrent(&pool, "bbbb", stranger).await;
        watch(&pool, friend, "aaaa").await;
        watch(&pool, stranger, "bbbb").await;
        request(&pool, friend, me).await.unwrap();
        set_sharing(&pool, stranger, true).await.unwrap();

        assert!(
            recently_added(&pool, me, 10).await.unwrap().is_empty(),
            "asked, not accepted"
        );
        accept(&pool, me, friend).await.unwrap();

        assert_eq!(recently_added(&pool, me, 10).await.unwrap().len(), 1);
        assert!(recently_watched(&pool, me, 10).await.unwrap().is_empty());
        set_sharing(&pool, friend, true).await.unwrap();
        let watched = recently_watched(&pool, me, 10).await.unwrap();
        assert_eq!(watched[0].title.infohash, "aaaa");
        assert_eq!(watched.len(), 1, "a stranger shares nothing with me");
        assert_eq!(sharing_friends(&pool, me).await.unwrap().len(), 1);

        assert!(hide(&pool, friend, "aaaa").await.unwrap());
        assert!(recently_watched(&pool, me, 10).await.unwrap().is_empty());
        assert!(recently_added(&pool, me, 10).await.unwrap().is_empty());
        assert!(
            shown_title(&pool, friend.into(), "aaaa", 0)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(hidden(&pool, friend).await.unwrap().len(), 1);
        unhide(&pool, friend, None, Some("aaaa")).await.unwrap();
        assert_eq!(recently_watched(&pool, me, 10).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn hiding_an_unknown_release_is_refused() {
        let pool = migrated_pool().await;
        let me = make_user(&pool).await;
        assert!(!hide(&pool, me, "nope").await.unwrap());
    }
}
