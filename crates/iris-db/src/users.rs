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

pub async fn update_password_hash(
    pool: &SqlitePool,
    id: UserId,
    new_hash: &str,
) -> Result<bool, sqlx::Error> {
    let uuid: Uuid = id.into();
    let res = sqlx::query("UPDATE users SET password_hash = ?1 WHERE id = ?2")
        .bind(new_hash)
        .bind(uuid)
        .execute(pool)
        .await?;
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
