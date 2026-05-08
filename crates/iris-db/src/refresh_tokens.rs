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
    pub device_label: Option<String>,
    pub device_kind: Option<String>,
}

pub async fn insert(
    pool: &SqlitePool,
    jti: Uuid,
    user_id: UserId,
    expires_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    insert_with_device(pool, jti, user_id, expires_at, None, None).await
}

pub async fn insert_with_device(
    pool: &SqlitePool,
    jti: Uuid,
    user_id: UserId,
    expires_at: DateTime<Utc>,
    device_label: Option<&str>,
    device_kind: Option<&str>,
) -> Result<(), sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query(
        "INSERT INTO refresh_tokens (jti, user_id, issued_at, expires_at, device_label, device_kind) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )
    .bind(jti)
    .bind(user)
    .bind(Utc::now())
    .bind(expires_at)
    .bind(device_label)
    .bind(device_kind)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_devices_for_user(
    pool: &SqlitePool,
    user_id: UserId,
) -> Result<Vec<RefreshToken>, sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query_as::<_, RefreshToken>(
        "SELECT jti, user_id, issued_at, expires_at, revoked_at, device_label, device_kind \
         FROM refresh_tokens \
         WHERE user_id = ?1 AND revoked_at IS NULL AND device_kind IS NOT NULL \
         ORDER BY issued_at DESC",
    )
    .bind(user)
    .fetch_all(pool)
    .await
}

pub async fn revoke_for_user(
    pool: &SqlitePool,
    user_id: UserId,
    jti: Uuid,
) -> Result<bool, sqlx::Error> {
    let user: Uuid = user_id.into();
    let res = sqlx::query(
        "UPDATE refresh_tokens SET revoked_at = ?1 \
         WHERE jti = ?2 AND user_id = ?3 AND revoked_at IS NULL",
    )
    .bind(Utc::now())
    .bind(jti)
    .bind(user)
    .execute(pool)
    .await?;
    Ok(res.rows_affected() == 1)
}

pub async fn is_active(pool: &SqlitePool, jti: Uuid) -> Result<bool, sqlx::Error> {
    // EXISTS-style probe with `query_scalar` — we don't need to materialise
    // a full RefreshToken row, and `FromRow` is strict about every column
    // declared on the struct being present in the SELECT. Selecting a
    // narrow projection here used to crash with `ColumnNotFound("device_label")`
    // on every /auth/refresh after migration 0004 added those columns.
    let row: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM refresh_tokens \
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
