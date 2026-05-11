use chrono::{DateTime, Utc};
use iris_core::ids::{InvitationId, UserId};
use serde::Serialize;
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Invitation {
    pub id: Uuid,
    pub token_hash: String,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub consumed_at: Option<DateTime<Utc>>,
    pub consumed_by: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct NewInvitation {
    pub token_hash: String,
    pub created_by: UserId,
    pub expires_at: DateTime<Utc>,
}

pub async fn create(pool: &SqlitePool, new: NewInvitation) -> Result<Invitation, sqlx::Error> {
    let id = Uuid::new_v4();
    let now = Utc::now();
    let creator: Uuid = new.created_by.into();
    sqlx::query(
        "INSERT INTO invitations (id, token_hash, created_by, created_at, expires_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(id)
    .bind(&new.token_hash)
    .bind(creator)
    .bind(now)
    .bind(new.expires_at)
    .execute(pool)
    .await?;

    Ok(Invitation {
        id,
        token_hash: new.token_hash,
        created_by: creator,
        created_at: now,
        expires_at: new.expires_at,
        consumed_at: None,
        consumed_by: None,
    })
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<Invitation>, sqlx::Error> {
    sqlx::query_as::<_, Invitation>(
        "SELECT id, token_hash, created_by, created_at, expires_at, consumed_at, consumed_by \
         FROM invitations ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await
}

pub async fn find_active_by_hash<'e, E>(
    executor: E,
    token_hash: &str,
) -> Result<Option<Invitation>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    sqlx::query_as::<_, Invitation>(
        "SELECT id, token_hash, created_by, created_at, expires_at, consumed_at, consumed_by \
         FROM invitations \
         WHERE token_hash = ?1 AND consumed_at IS NULL AND expires_at > ?2",
    )
    .bind(token_hash)
    .bind(Utc::now())
    .fetch_optional(executor)
    .await
}

pub async fn consume<'e, E>(
    executor: E,
    id: InvitationId,
    consumed_by: UserId,
) -> Result<bool, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let id: Uuid = id.into();
    let user: Uuid = consumed_by.into();
    let res = sqlx::query(
        "UPDATE invitations SET consumed_at = ?1, consumed_by = ?2 \
         WHERE id = ?3 AND consumed_at IS NULL",
    )
    .bind(Utc::now())
    .bind(user)
    .bind(id)
    .execute(executor)
    .await?;
    Ok(res.rows_affected() == 1)
}

pub async fn revoke(pool: &SqlitePool, id: InvitationId) -> Result<bool, sqlx::Error> {
    let id: Uuid = id.into();
    let res = sqlx::query("DELETE FROM invitations WHERE id = ?1 AND consumed_at IS NULL")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() == 1)
}
