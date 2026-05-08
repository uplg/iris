use chrono::{DateTime, Utc};
use iris_core::ids::UserId;
use serde::Serialize;
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct DeviceCode {
    pub code: String,
    pub device_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub claimed_by: Option<Uuid>,
    pub label: Option<String>,
    pub kind: String,
}

pub async fn create(
    pool: &SqlitePool,
    code: &str,
    expires_at: DateTime<Utc>,
    kind: &str,
) -> Result<DeviceCode, sqlx::Error> {
    let device_id = Uuid::new_v4();
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO device_codes (code, device_id, created_at, expires_at, kind) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(code)
    .bind(device_id)
    .bind(now)
    .bind(expires_at)
    .bind(kind)
    .execute(pool)
    .await?;
    Ok(DeviceCode {
        code: code.to_string(),
        device_id,
        created_at: now,
        expires_at,
        claimed_at: None,
        claimed_by: None,
        label: None,
        kind: kind.to_string(),
    })
}

pub async fn find_by_device_id(
    pool: &SqlitePool,
    device_id: Uuid,
) -> Result<Option<DeviceCode>, sqlx::Error> {
    sqlx::query_as::<_, DeviceCode>(
        "SELECT code, device_id, created_at, expires_at, claimed_at, claimed_by, label, kind \
         FROM device_codes WHERE device_id = ?1",
    )
    .bind(device_id)
    .fetch_optional(pool)
    .await
}

pub async fn find_active_by_code(
    pool: &SqlitePool,
    code: &str,
) -> Result<Option<DeviceCode>, sqlx::Error> {
    sqlx::query_as::<_, DeviceCode>(
        "SELECT code, device_id, created_at, expires_at, claimed_at, claimed_by, label, kind \
         FROM device_codes \
         WHERE code = ?1 AND claimed_at IS NULL AND expires_at > ?2",
    )
    .bind(code)
    .bind(Utc::now())
    .fetch_optional(pool)
    .await
}

pub async fn claim(
    pool: &SqlitePool,
    code: &str,
    user_id: UserId,
    label: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let user: Uuid = user_id.into();
    let res = sqlx::query(
        "UPDATE device_codes SET claimed_at = ?1, claimed_by = ?2, label = ?3 \
         WHERE code = ?4 AND claimed_at IS NULL AND expires_at > ?5",
    )
    .bind(Utc::now())
    .bind(user)
    .bind(label)
    .bind(code)
    .bind(Utc::now())
    .execute(pool)
    .await?;
    Ok(res.rows_affected() == 1)
}

pub async fn cleanup_expired(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let res = sqlx::query(
        "DELETE FROM device_codes \
         WHERE expires_at < ?1 AND claimed_at IS NULL",
    )
    .bind(Utc::now())
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}
