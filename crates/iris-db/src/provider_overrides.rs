//! An admin's runtime on/off per tracker (`provider_overrides`, migration 0047).

use chrono::Utc;
use iris_core::ids::UserId;
use sqlx::SqlitePool;
use uuid::Uuid;

/// Every override, `(provider_id, enabled)`.
pub async fn list(pool: &SqlitePool) -> Result<Vec<(String, bool)>, sqlx::Error> {
    sqlx::query_as("SELECT provider_id, enabled FROM provider_overrides ORDER BY provider_id")
        .fetch_all(pool)
        .await
}

pub async fn set(
    pool: &SqlitePool,
    provider_id: &str,
    enabled: bool,
    by: UserId,
) -> Result<(), sqlx::Error> {
    let by: Uuid = by.into();
    sqlx::query(
        "INSERT INTO provider_overrides (provider_id, enabled, updated_at, updated_by) \
         VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT(provider_id) DO UPDATE SET \
           enabled = excluded.enabled, updated_at = excluded.updated_at, \
           updated_by = excluded.updated_by",
    )
    .bind(provider_id)
    .bind(enabled)
    .bind(Utc::now())
    .bind(by)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::test_support::{make_user, migrated_pool};

    #[tokio::test]
    async fn the_last_write_wins() {
        let pool = migrated_pool().await;
        let admin = make_user(&pool).await;
        super::set(&pool, "tr4ker", false, admin).await.unwrap();
        super::set(&pool, "c411", true, admin).await.unwrap();
        super::set(&pool, "tr4ker", true, admin).await.unwrap();
        super::set(&pool, "tr4ker", false, admin).await.unwrap();
        assert_eq!(
            super::list(&pool).await.unwrap(),
            [("c411".to_string(), true), ("tr4ker".to_string(), false)]
        );
    }
}
