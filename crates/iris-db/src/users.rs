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
    })
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
        "id, email, password_hash, display_name, is_admin, created_at"
    };
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
        " FROM users WHERE email = ?1"
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
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;
    Ok(n)
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<User>, sqlx::Error> {
    let rows: Vec<UserRow> = sqlx::query_as(concat!(
        "SELECT ",
        user_columns!(),
        " FROM users ORDER BY created_at ASC"
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
/// one loses its grace window too) and every passkey, since a stolen
/// session could have registered its own.
///
/// Returns `None` for an unknown user, else how many passkeys went.
pub async fn set_password(
    pool: &SqlitePool,
    id: UserId,
    new_hash: &str,
) -> Result<Option<u64>, sqlx::Error> {
    let uuid: Uuid = id.into();
    let mut tx = pool.begin().await?;
    let res = sqlx::query("UPDATE users SET password_hash = ?1 WHERE id = ?2")
        .bind(new_hash)
        .bind(uuid)
        .execute(&mut *tx)
        .await?;
    if res.rows_affected() != 1 {
        return Ok(None);
    }
    sqlx::query(
        "UPDATE refresh_tokens SET revoked_at = COALESCE(revoked_at, ?1), rotated_at = NULL \
         WHERE user_id = ?2",
    )
    .bind(Utc::now())
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
/// cascades away as intended.
pub async fn delete(
    pool: &SqlitePool,
    id: UserId,
    reassign_torrents_to: UserId,
) -> Result<bool, sqlx::Error> {
    let target: Uuid = id.into();
    let heir: Uuid = reassign_torrents_to.into();
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE torrents SET added_by = ?1 WHERE added_by = ?2")
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
        crate::refresh_tokens::mark_rotated(&pool, rotated)
            .await
            .unwrap();
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

        assert_eq!(
            set_password(&pool, user, "new-hash").await.unwrap(),
            Some(1)
        );

        assert!(!crate::refresh_tokens::is_active(&pool, jti).await.unwrap());
        assert!(
            crate::refresh_tokens::recently_rotated(&pool, rotated, 60)
                .await
                .unwrap()
                .is_none(),
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
    async fn delete_missing_user_is_false() {
        let pool = migrated_pool().await;
        let admin = make_user(&pool, "admin@t.test").await;
        assert!(!delete(&pool, UserId::new(), admin).await.unwrap());
    }
}
