use chrono::{DateTime, Utc};
use iris_core::ids::UserId;
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RefreshToken {
    pub jti: Uuid,
    pub user_id: Uuid,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

pub async fn insert(
    pool: &SqlitePool,
    jti: Uuid,
    user_id: UserId,
    expires_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query(
        "INSERT INTO refresh_tokens (jti, user_id, issued_at, expires_at) VALUES (?1, ?2, ?3, ?4)",
    )
    .bind(jti)
    .bind(user)
    .bind(Utc::now())
    .bind(expires_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn is_active(pool: &SqlitePool, jti: Uuid) -> Result<bool, sqlx::Error> {
    let row: Option<RefreshToken> = sqlx::query_as(
        "SELECT jti, user_id, issued_at, expires_at, revoked_at FROM refresh_tokens \
         WHERE jti = ?1 AND revoked_at IS NULL AND expires_at > ?2",
    )
    .bind(jti)
    .bind(Utc::now())
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some())
}

pub async fn revoke(pool: &SqlitePool, jti: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE refresh_tokens SET revoked_at = ?1 WHERE jti = ?2 AND revoked_at IS NULL")
        .bind(Utc::now())
        .bind(jti)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn revoke_all_for_user(pool: &SqlitePool, user_id: UserId) -> Result<(), sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query(
        "UPDATE refresh_tokens SET revoked_at = ?1 WHERE user_id = ?2 AND revoked_at IS NULL",
    )
    .bind(Utc::now())
    .bind(user)
    .execute(pool)
    .await?;
    Ok(())
}
