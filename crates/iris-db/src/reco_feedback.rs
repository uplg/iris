//! Per-user feedback on recommendation candidates (dismiss / follow /
//! grab signals). Slice 5 wires the dismiss path; the positive signals
//! are recorded for future weighting.

use chrono::Utc;
use iris_core::ids::UserId;
use sqlx::SqlitePool;
use uuid::Uuid;

/// Record a feedback action. Idempotent on `(user, catalog_id, action)`
/// — re-dismissing the same item is a no-op.
pub async fn record(
    pool: &SqlitePool,
    user_id: UserId,
    catalog_id: Uuid,
    action: &str,
) -> Result<(), sqlx::Error> {
    let uuid: Uuid = user_id.into();
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO reco_feedback (user_id, catalog_id, action, created_at) \
         VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT(user_id, catalog_id, action) DO NOTHING",
    )
    .bind(uuid)
    .bind(catalog_id)
    .bind(action)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

/// Catalogue ids this user has dismissed — excluded from future shelves.
pub async fn dismissed_ids(pool: &SqlitePool, user_id: UserId) -> Result<Vec<Uuid>, sqlx::Error> {
    let uuid: Uuid = user_id.into();
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT catalog_id FROM reco_feedback WHERE user_id = ?1 AND action = 'dismissed'",
    )
    .bind(uuid)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}
