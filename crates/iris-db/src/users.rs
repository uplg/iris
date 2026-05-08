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
    is_admin: bool,
    created_at: DateTime<Utc>,
}

impl UserRow {
    fn into_domain(self) -> (User, String) {
        (
            User {
                id: UserId::from(self.id),
                email: self.email,
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

pub async fn create(pool: &SqlitePool, new: NewUser) -> Result<User, sqlx::Error> {
    let id = Uuid::new_v4();
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, is_admin, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(id)
    .bind(&new.email)
    .bind(&new.password_hash)
    .bind(new.is_admin)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(User {
        id: UserId::from(id),
        email: new.email,
        is_admin: new.is_admin,
        created_at: now,
    })
}

pub async fn find_by_email(
    pool: &SqlitePool,
    email: &str,
) -> Result<Option<(User, String)>, sqlx::Error> {
    let row: Option<UserRow> =
        sqlx::query_as("SELECT id, email, password_hash, is_admin, created_at FROM users WHERE email = ?1")
            .bind(email)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(UserRow::into_domain))
}

pub async fn find_by_id(pool: &SqlitePool, id: UserId) -> Result<Option<User>, sqlx::Error> {
    let uuid: Uuid = id.into();
    let row: Option<UserRow> =
        sqlx::query_as("SELECT id, email, password_hash, is_admin, created_at FROM users WHERE id = ?1")
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
