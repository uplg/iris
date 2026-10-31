use chrono::{DateTime, Utc};
use iris_core::ids::UserId;
use iris_core::user::User;
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
struct UserRow {
    id: Uuid,
    email: String,
    password_hash: String,
    display_name: String,
    is_admin: bool,
    created_at: DateTime<Utc>,
    account_id: Option<Uuid>,
}

impl UserRow {
    fn into_domain(self) -> (User, String) {
        (
            User {
                id: UserId::from(self.id),
                email: self.email,
                display_name: self.display_name,
                is_admin: self.is_admin,
                created_at: self.created_at,
                account_id: self.account_id.map(UserId::from),
            },
            self.password_hash,
        )
    }
}

#[derive(Debug, Clone)]
pub struct NewUser {
    pub email: String,
    pub password_hash: String,
    pub is_admin: bool,
}

/// Derive a default display name from the email: take the local-part,
/// then truncate at the first dot. `leonard.apollo@uplg.xyz` →
/// `leonard`. Falls back to the full local-part if there's no dot
/// (`johndoe@example.com` → `johndoe`). Same rule applied SQL-side in
/// the migration 0006 backfill.
fn default_display_name(email: &str) -> String {
    let local = match email.find('@') {
        Some(at) if at > 0 => &email[..at],
        _ => email,
    };
    match local.find('.') {
        Some(dot) if dot > 0 => local[..dot].to_string(),
        _ => local.to_string(),
    }
}

pub async fn create<'e, E>(executor: E, new: NewUser) -> Result<User, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let id = Uuid::new_v4();
    let now = Utc::now();
    let display_name = default_display_name(&new.email);
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, display_name, is_admin, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )
    .bind(id)
    .bind(&new.email)
    .bind(&new.password_hash)
    .bind(&display_name)
    .bind(new.is_admin)
    .bind(now)
    .execute(executor)
    .await?;

    Ok(User {
        id: UserId::from(id),
        email: new.email,
        display_name,
        is_admin: new.is_admin,
        created_at: now,
        account_id: None,
    })
}

/// How many profiles an account holds, itself included.
pub const MAX_PROFILES: usize = 6;

/// A new profile under `account`, `None` once it holds [`MAX_PROFILES`]. Its
/// email only fills the unique column and its hash verifies nothing: a profile
/// is entered from its account, never signed in to. With `copy_history` it
/// starts from a copy of the account's watched titles, resume points, watchlist
/// and what was dismissed from them; each then lives its own life.
pub async fn create_profile(
    pool: &SqlitePool,
    account: UserId,
    name: &str,
    copy_history: bool,
) -> Result<Option<User>, sqlx::Error> {
    let owner: Uuid = account.into();
    let id = Uuid::new_v4();
    let now = Utc::now();
    let email = format!("{id}@profile.invalid");
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let (held,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM users WHERE id = ?1 OR account_id = ?1")
            .bind(owner)
            .fetch_one(&mut *tx)
            .await?;
    if usize::try_from(held).unwrap_or(usize::MAX) >= MAX_PROFILES {
        return Ok(None);
    }
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, display_name, is_admin, created_at, account_id) \
         VALUES (?1, ?2, '!', ?3, 0, ?4, ?5)",
    )
    .bind(id)
    .bind(&email)
    .bind(name)
    .bind(now)
    .bind(owner)
    .execute(&mut *tx)
    .await?;
    if copy_history {
        for sql in [
            "INSERT INTO playback_progress (user_id, infohash, file_idx, position_seconds, \
               duration_seconds, audio_track_idx, subtitle_track_idx, completed, last_watched_at) \
             SELECT ?1, infohash, file_idx, position_seconds, duration_seconds, audio_track_idx, \
               subtitle_track_idx, completed, last_watched_at \
             FROM playback_progress WHERE user_id = ?2",
            "INSERT INTO series_follows (id, user_id, normalized_name, name, tmdb_id, \
               last_checked_at, last_visited_at, created_at) \
             SELECT randomblob(16), ?1, normalized_name, name, tmdb_id, last_checked_at, \
               last_visited_at, created_at \
             FROM series_follows WHERE user_id = ?2",
            "INSERT INTO cw_dismissed (user_id, collection_id, dismissed_at) \
             SELECT ?1, collection_id, dismissed_at FROM cw_dismissed WHERE user_id = ?2",
            "INSERT INTO ghost_dismissed (user_id, collection_id, dismissed_at) \
             SELECT ?1, collection_id, dismissed_at FROM ghost_dismissed WHERE user_id = ?2",
            "INSERT INTO gone_release_dismissed (user_id, infohash, dismissed_at) \
             SELECT ?1, infohash, dismissed_at FROM gone_release_dismissed WHERE user_id = ?2",
        ] {
            sqlx::query(sql)
                .bind(id)
                .bind(owner)
                .execute(&mut *tx)
                .await?;
        }
    }
    tx.commit().await?;
    Ok(Some(User {
        id: UserId::from(id),
        email,
        display_name: name.to_owned(),
        is_admin: false,
        created_at: now,
        account_id: Some(account),
    }))
}

/// Column list for `SELECT`ing a [`UserRow`]. A macro rather than a
/// `const &str` so it expands to a string literal usable inside
/// `concat!`, keeping every read query a compile-time `&'static str` —
/// the only type sqlx 0.9 accepts natively (`SqlSafeStr`) — instead of a
/// `format!`-built string that would need an `AssertSqlSafe` audit hatch.
/// The literal carries no runtime data, so injection is impossible by
/// construction.
macro_rules! user_columns {
    () => {
        "id, email, password_hash, display_name, is_admin, created_at, account_id"
    };
}

/// The account's profiles, itself first, then by age.
pub async fn profiles_of(pool: &SqlitePool, account: UserId) -> Result<Vec<User>, sqlx::Error> {
    let owner: Uuid = account.into();
    let rows: Vec<UserRow> = sqlx::query_as(concat!(
        "SELECT ",
        user_columns!(),
        " FROM users WHERE id = ?1 OR account_id = ?1 \
         ORDER BY account_id IS NOT NULL, created_at ASC"
    ))
    .bind(owner)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.into_domain().0).collect())
}

pub async fn find_by_email<'e, E>(
    executor: E,
    email: &str,
) -> Result<Option<(User, String)>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let row: Option<UserRow> = sqlx::query_as(concat!(
        "SELECT ",
        user_columns!(),
        " FROM users WHERE email = ?1 AND account_id IS NULL"
    ))
    .bind(email)
    .fetch_optional(executor)
    .await?;
    Ok(row.map(UserRow::into_domain))
}

pub async fn find_by_id(pool: &SqlitePool, id: UserId) -> Result<Option<User>, sqlx::Error> {
    let uuid: Uuid = id.into();
    let row: Option<UserRow> = sqlx::query_as(concat!(
        "SELECT ",
        user_columns!(),
        " FROM users WHERE id = ?1"
    ))
    .bind(uuid)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.into_domain().0))
}

pub async fn count(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users WHERE account_id IS NULL")
        .fetch_one(pool)
        .await?;
    Ok(n)
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<User>, sqlx::Error> {
    let rows: Vec<UserRow> = sqlx::query_as(concat!(
        "SELECT ",
        user_columns!(),
        " FROM users WHERE account_id IS NULL ORDER BY created_at ASC"
    ))
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.into_domain().0).collect())
}

pub async fn get_password_hash(
    pool: &SqlitePool,
    id: UserId,
) -> Result<Option<String>, sqlx::Error> {
    let uuid: Uuid = id.into();
    let row: Option<(String,)> = sqlx::query_as("SELECT password_hash FROM users WHERE id = ?1")
        .bind(uuid)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.0))
}

/// Replace a user's password and end every way into the account opened
/// under the old one, in one transaction: every session (a just-rotated
/// one loses its grace window too, and `sessions_valid_after` kills the
/// access tokens already out) and every passkey, since a stolen session
/// could have registered its own. The account's profiles go through the same
/// door, so their sessions end with it.
///
/// Returns `None` for an unknown user, else how many passkeys went.
pub async fn set_password(
    pool: &SqlitePool,
    id: UserId,
    new_hash: &str,
) -> Result<Option<u64>, sqlx::Error> {
    let uuid: Uuid = id.into();
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let now = Utc::now();
    let res =
        sqlx::query("UPDATE users SET password_hash = ?1, sessions_valid_after = ?2 WHERE id = ?3")
            .bind(new_hash)
            .bind(now)
            .bind(uuid)
            .execute(&mut *tx)
            .await?;
    if res.rows_affected() != 1 {
        return Ok(None);
    }
    sqlx::query("UPDATE users SET sessions_valid_after = ?1 WHERE account_id = ?2")
        .bind(now)
        .bind(uuid)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE refresh_tokens SET revoked_at = COALESCE(revoked_at, ?1), rotated_at = NULL \
         WHERE user_id IN (SELECT id FROM users WHERE id = ?2 OR account_id = ?2)",
    )
    .bind(now)
    .bind(uuid)
    .execute(&mut *tx)
    .await?;
    let passkeys = sqlx::query("DELETE FROM webauthn_credentials WHERE user_id = ?1")
        .bind(uuid)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    tx.commit().await?;
    Ok(Some(passkeys))
}

/// Delete an account. `reassign_torrents_to` (the acting admin) inherits
/// the target's grabs first — `torrents.added_by` is `ON DELETE CASCADE`
/// and the library is household-shared, so letting the cascade run would
/// silently evict every release the deleted user ever grabbed. Everything
/// personal (sessions, watch progress, follows, preferences, dismissals)
/// cascades away as intended. An account's profiles leave with it, and their
/// grabs are inherited the same way.
pub async fn delete(
    pool: &SqlitePool,
    id: UserId,
    reassign_torrents_to: UserId,
) -> Result<bool, sqlx::Error> {
    let target: Uuid = id.into();
    let heir: Uuid = reassign_torrents_to.into();
    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE torrents SET added_by = ?1 \
         WHERE added_by IN (SELECT id FROM users WHERE id = ?2 OR account_id = ?2)",
    )
    .bind(heir)
    .bind(target)
    .execute(&mut *tx)
    .await?;
    let res = sqlx::query("DELETE FROM users WHERE id = ?1")
        .bind(target)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(res.rows_affected() == 1)
}

/// `None`: no such user. `Some(None)`: a user whose sessions were never cut.
pub async fn sessions_valid_after(
    pool: &SqlitePool,
    id: UserId,
) -> Result<Option<Option<DateTime<Utc>>>, sqlx::Error> {
    let uuid: Uuid = id.into();
    let row: Option<(Option<DateTime<Utc>>,)> =
        sqlx::query_as("SELECT sessions_valid_after FROM users WHERE id = ?1")
            .bind(uuid)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|r| r.0))
}

pub async fn update_display_name(
    pool: &SqlitePool,
    id: UserId,
    new_name: &str,
) -> Result<bool, sqlx::Error> {
    let uuid: Uuid = id.into();
    let res = sqlx::query("UPDATE users SET display_name = ?1 WHERE id = ?2")
        .bind(new_name)
        .bind(uuid)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() == 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::migrated_pool;

    async fn make_user(pool: &SqlitePool, email: &str) -> UserId {
        create(
            pool,
            NewUser {
                email: email.to_owned(),
                password_hash: String::new(),
                is_admin: false,
            },
        )
        .await
        .expect("insert user")
        .id
    }

    async fn insert_torrent(pool: &SqlitePool, infohash: &str, added_by: UserId) {
        let owner: Uuid = added_by.into();
        sqlx::query(
            "INSERT INTO torrents (id, infohash, name, total_size_bytes, added_by, added_at) \
             VALUES (?1, ?2, ?3, 1024, ?4, ?5)",
        )
        .bind(Uuid::new_v4())
        .bind(infohash)
        .bind(format!("release-{infohash}"))
        .bind(owner)
        .bind(Utc::now())
        .execute(pool)
        .await
        .expect("insert torrent");
    }

    #[tokio::test]
    async fn set_password_ends_every_session() {
        let pool = migrated_pool().await;
        let user = make_user(&pool, "a@example.org").await;
        let jti = Uuid::new_v4();
        crate::refresh_tokens::insert(&pool, jti, user, Utc::now() + chrono::Duration::days(1))
            .await
            .unwrap();
        assert!(crate::refresh_tokens::is_active(&pool, jti).await.unwrap());
        let rotated = Uuid::new_v4();
        crate::refresh_tokens::insert(&pool, rotated, user, Utc::now() + chrono::Duration::days(1))
            .await
            .unwrap();
        let rotation = crate::refresh_tokens::rotate(&pool, rotated, 60, |_| {
            crate::refresh_tokens::Successor {
                jti: Uuid::new_v4(),
                issued_at: Utc::now(),
                expires_at: Utc::now() + chrono::Duration::days(1),
            }
        })
        .await
        .unwrap();
        let crate::refresh_tokens::Rotation::Rotated(head) = rotation else {
            panic!("{rotation:?}");
        };
        crate::passkeys::insert(
            &pool,
            &crate::passkeys::NewPasskey {
                user_id: user,
                credential_id: "cred",
                passkey_json: "{}",
                name: "Phone",
                backup_eligible: false,
                backed_up: false,
            },
        )
        .await
        .unwrap();

        assert_eq!(sessions_valid_after(&pool, user).await.unwrap(), Some(None));
        let before = Utc::now();
        assert_eq!(
            set_password(&pool, user, "new-hash").await.unwrap(),
            Some(1)
        );
        let cut = sessions_valid_after(&pool, user).await.unwrap().flatten();
        assert!(cut.is_some_and(|t| t >= before), "the cut is stamped");

        assert!(!crate::refresh_tokens::is_active(&pool, jti).await.unwrap());
        assert!(
            !crate::refresh_tokens::is_active(&pool, head.jti)
                .await
                .unwrap()
        );
        assert!(
            crate::refresh_tokens::rotate(&pool, rotated, 60, |_| panic!("no mint"))
                .await
                .unwrap()
                == crate::refresh_tokens::Rotation::Rejected,
            "a just-rotated token can't come back through the grace window"
        );
        assert!(
            crate::passkeys::list_for_user(&pool, user)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            set_password(&pool, UserId::from(Uuid::new_v4()), "x")
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            get_password_hash(&pool, user).await.unwrap().as_deref(),
            Some("new-hash")
        );
    }

    #[tokio::test]
    async fn delete_reassigns_torrents_and_keeps_audit() {
        let pool = migrated_pool().await;
        let admin = make_user(&pool, "admin@t.test").await;
        let target = make_user(&pool, "leaver@t.test").await;
        insert_torrent(&pool, "aaaa", target).await;
        crate::audit::record(&pool, target, "torrent.delete", "torrent", Some("x"), None)
            .await
            .unwrap();

        assert!(delete(&pool, target, admin).await.unwrap());

        assert!(find_by_id(&pool, target).await.unwrap().is_none());
        // The grab survives, re-attributed to the acting admin instead of
        // cascading away with the account.
        let (owner,): (Uuid,) =
            sqlx::query_as("SELECT added_by FROM torrents WHERE infohash = 'aaaa'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(UserId::from(owner), admin);
        // The audit trail outlives the account, under a placeholder name.
        let rows = crate::audit::list(&pool, 10, 0).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].actor_display_name, "deleted user");
    }

    #[tokio::test]
    async fn a_profile_cannot_be_signed_in_to_and_stays_out_of_the_accounts() {
        let pool = migrated_pool().await;
        let account = make_user(&pool, "shared@t.test").await;
        let profile = create_profile(&pool, account, "Muriel", false)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(profile.account(), account);

        assert!(
            find_by_email(&pool, &profile.email)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(count(&pool).await.unwrap(), 1);
        assert_eq!(list(&pool).await.unwrap().len(), 1);
        let names: Vec<_> = profiles_of(&pool, account)
            .await
            .unwrap()
            .into_iter()
            .map(|u| u.display_name)
            .collect();
        assert_eq!(names, ["shared", "Muriel"]);
    }

    #[tokio::test]
    async fn an_account_holds_a_bounded_number_of_profiles() {
        let pool = migrated_pool().await;
        let account = make_user(&pool, "shared@t.test").await;
        for _ in 1..MAX_PROFILES {
            assert!(
                create_profile(&pool, account, "p", false)
                    .await
                    .unwrap()
                    .is_some()
            );
        }
        assert!(
            create_profile(&pool, account, "p", false)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn a_password_change_ends_the_sessions_of_every_profile() {
        let pool = migrated_pool().await;
        let account = make_user(&pool, "shared@t.test").await;
        let profile = create_profile(&pool, account, "Muriel", false)
            .await
            .unwrap()
            .unwrap();
        let jti = Uuid::new_v4();
        crate::refresh_tokens::insert(
            &pool,
            jti,
            profile.id,
            Utc::now() + chrono::Duration::days(1),
        )
        .await
        .unwrap();

        set_password(&pool, account, "new-hash").await.unwrap();

        assert!(!crate::refresh_tokens::is_active(&pool, jti).await.unwrap());
        assert!(
            sessions_valid_after(&pool, profile.id)
                .await
                .unwrap()
                .flatten()
                .is_some()
        );
    }

    #[tokio::test]
    async fn deleting_an_account_keeps_the_grabs_of_its_profiles() {
        let pool = migrated_pool().await;
        let admin = make_user(&pool, "admin@t.test").await;
        let account = make_user(&pool, "shared@t.test").await;
        let profile = create_profile(&pool, account, "Muriel", false)
            .await
            .unwrap()
            .unwrap();
        insert_torrent(&pool, "bbbb", profile.id).await;

        assert!(delete(&pool, account, admin).await.unwrap());

        assert!(find_by_id(&pool, profile.id).await.unwrap().is_none());
        let (owner,): (Uuid,) =
            sqlx::query_as("SELECT added_by FROM torrents WHERE infohash = 'bbbb'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(UserId::from(owner), admin);
    }

    #[tokio::test]
    async fn a_profile_can_start_from_the_accounts_history_and_then_lives_its_own() {
        let pool = migrated_pool().await;
        let account = make_user(&pool, "shared@t.test").await;
        insert_torrent(&pool, "cccc", account).await;
        sqlx::query(
            "INSERT INTO playback_progress (user_id, infohash, file_idx, position_seconds, last_watched_at) \
             VALUES (?1, 'cccc', 0, 120, ?2)",
        )
        .bind(Uuid::from(account))
        .bind(Utc::now())
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO series_follows (id, user_id, normalized_name, name, created_at) \
             VALUES (?1, ?2, 'severance', 'Severance', ?3)",
        )
        .bind(Uuid::new_v4())
        .bind(Uuid::from(account))
        .bind(Utc::now())
        .execute(&pool)
        .await
        .unwrap();
        let count = async |table: &'static str, user: UserId| -> i64 {
            let sql = match table {
                "progress" => "SELECT COUNT(*) FROM playback_progress WHERE user_id = ?1",
                _ => "SELECT COUNT(*) FROM series_follows WHERE user_id = ?1",
            };
            sqlx::query_as::<_, (i64,)>(sql)
                .bind(Uuid::from(user))
                .fetch_one(&pool)
                .await
                .unwrap()
                .0
        };

        let empty = create_profile(&pool, account, "Muriel", false)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(count("progress", empty.id).await, 0);

        let copied = create_profile(&pool, account, "Lyros", true)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(count("progress", copied.id).await, 1);
        assert_eq!(count("follows", copied.id).await, 1);

        sqlx::query("DELETE FROM playback_progress WHERE user_id = ?1")
            .bind(Uuid::from(copied.id))
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            count("progress", account).await,
            1,
            "the account keeps its own"
        );
    }

    #[tokio::test]
    async fn delete_missing_user_is_false() {
        let pool = migrated_pool().await;
        let admin = make_user(&pool, "admin@t.test").await;
        assert!(!delete(&pool, UserId::new(), admin).await.unwrap());
    }
}
