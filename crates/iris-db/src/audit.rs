//! Persistent audit trail for sensitive actions — deletions, password
//! resets, display-name changes, admin-triggered GC. Until migration 0031
//! these only hit ephemeral `tracing::` logs, which rotate out and aren't
//! queryable from the admin UI. This is the durable "who changed/deleted
//! what" answer, not a full request log — instrument mutating endpoints
//! deliberately, not every GET. Readable only via the admin-gated
//! `/admin/audit-log` route, but `actor_id` is whichever user performed the
//! action — most household members can delete their own torrents.

use chrono::{DateTime, Utc};
use iris_core::ids::UserId;
use serde::Serialize;
use sqlx::SqlitePool;
use uuid::Uuid;

/// Record one audited action. `resource_id` and `details` are free-form —
/// callers pass whatever identifies the affected resource (infohash, user
/// id, remux cache key, …) and any extra context worth keeping.
pub async fn record(
    pool: &SqlitePool,
    actor_id: UserId,
    action: &str,
    resource_type: &str,
    resource_id: Option<&str>,
    details: Option<&str>,
) -> Result<(), sqlx::Error> {
    let actor: Uuid = actor_id.into();
    sqlx::query(
        "INSERT INTO audit_log (actor_id, action, resource_type, resource_id, details, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )
    .bind(actor)
    .bind(action)
    .bind(resource_type)
    .bind(resource_id)
    .bind(details)
    .bind(Utc::now())
    .execute(pool)
    .await?;
    Ok(())
}

/// One row of the audit log, newest first — the acting user's
/// `display_name` is joined in so the UI never has to round-trip a
/// separate user lookup per row.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct AuditLogRow {
    pub id: i64,
    pub actor_id: Uuid,
    pub actor_display_name: String,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub details: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub async fn list(
    pool: &SqlitePool,
    limit: i64,
    offset: i64,
) -> Result<Vec<AuditLogRow>, sqlx::Error> {
    // LEFT JOIN: audit rows outlive their actor's account (migration 0036
    // dropped the FK for exactly that), so a deleted user's actions keep
    // showing up under a placeholder name instead of vanishing.
    sqlx::query_as::<_, AuditLogRow>(
        "SELECT a.id, a.actor_id, \
            COALESCE(u.display_name, 'deleted user') as actor_display_name, a.action, \
            a.resource_type, a.resource_id, a.details, a.created_at \
         FROM audit_log a \
         LEFT JOIN users u ON u.id = a.actor_id \
         ORDER BY a.created_at DESC \
         LIMIT ?1 OFFSET ?2",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn migrated_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("open in-memory sqlite");
        crate::migrate::run(&pool).await.expect("run migrations");
        pool
    }

    async fn make_user(pool: &SqlitePool, display_name: &str) -> UserId {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO users (id, email, password_hash, display_name, is_admin, created_at) \
             VALUES (?1, ?2, '', ?3, 1, ?4)",
        )
        .bind(id)
        .bind(format!("{id}@t.test"))
        .bind(display_name)
        .bind(Utc::now())
        .execute(pool)
        .await
        .expect("insert user");
        UserId::from(id)
    }

    #[tokio::test]
    async fn record_then_list_newest_first() {
        let pool = migrated_pool().await;
        let actor = make_user(&pool, "Léonard").await;

        record(
            &pool,
            actor,
            "torrent.delete",
            "torrent",
            Some("abc123"),
            None,
        )
        .await
        .unwrap();
        record(
            &pool,
            actor,
            "user.password_reset",
            "user",
            Some("some-user-id"),
            Some("reset by admin"),
        )
        .await
        .unwrap();

        let rows = list(&pool, 50, 0).await.unwrap();
        assert_eq!(rows.len(), 2);
        // Newest first.
        assert_eq!(rows[0].action, "user.password_reset");
        assert_eq!(rows[0].actor_display_name, "Léonard");
        assert_eq!(rows[0].details.as_deref(), Some("reset by admin"));
        assert_eq!(rows[1].action, "torrent.delete");
        assert_eq!(rows[1].resource_id.as_deref(), Some("abc123"));
    }

    #[tokio::test]
    async fn list_is_paginated() {
        let pool = migrated_pool().await;
        let actor = make_user(&pool, "Admin").await;
        for i in 0..5 {
            record(
                &pool,
                actor,
                "gc.evict",
                "torrent",
                Some(&i.to_string()),
                None,
            )
            .await
            .unwrap();
        }
        assert_eq!(list(&pool, 2, 0).await.unwrap().len(), 2);
        assert_eq!(list(&pool, 2, 4).await.unwrap().len(), 1);
        assert_eq!(list(&pool, 50, 0).await.unwrap().len(), 5);
    }
}
