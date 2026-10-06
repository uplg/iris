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
    list_filtered(pool, AuditFilter::default(), limit, offset).await
}

/// Which entries of the log: one action or a family of them, one actor's.
#[derive(Debug, Clone, Copy, Default)]
pub struct AuditFilter<'a> {
    /// An exact action (`user.delete`) or its family, the part before the
    /// dot (`user` matches every `user.*`).
    pub action: Option<&'a str>,
    pub actor: Option<UserId>,
}

pub async fn list_filtered(
    pool: &SqlitePool,
    filter: AuditFilter<'_>,
    limit: i64,
    offset: i64,
) -> Result<Vec<AuditLogRow>, sqlx::Error> {
    let actor: Option<Uuid> = filter.actor.map(Into::into);
    // LEFT JOIN: audit rows outlive their actor's account (migration 0036
    // dropped the FK for exactly that), so a deleted user's actions keep
    // showing up under a placeholder name instead of vanishing. The family
    // match is a prefix compare, not LIKE: an action's `_` is no wildcard.
    sqlx::query_as::<_, AuditLogRow>(
        "SELECT a.id, a.actor_id, \
            COALESCE(u.display_name, 'deleted user') as actor_display_name, a.action, \
            a.resource_type, a.resource_id, a.details, a.created_at \
         FROM audit_log a \
         LEFT JOIN users u ON u.id = a.actor_id \
         WHERE (?3 IS NULL OR a.action = ?3 \
                OR substr(a.action, 1, length(?3) + 1) = ?3 || '.') \
           AND (?4 IS NULL OR a.actor_id = ?4) \
         ORDER BY a.created_at DESC \
         LIMIT ?1 OFFSET ?2",
    )
    .bind(limit)
    .bind(offset)
    .bind(filter.action)
    .bind(actor)
    .fetch_all(pool)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{make_named_user, migrated_pool};

    #[tokio::test]
    async fn record_then_list_newest_first() {
        let pool = migrated_pool().await;
        let actor = make_named_user(&pool, "Léonard").await;

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
        let actor = make_named_user(&pool, "Admin").await;
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

    #[tokio::test]
    async fn list_filters_by_action_family_and_actor() {
        let pool = migrated_pool().await;
        let ana = make_named_user(&pool, "Ana").await;
        let bo = make_named_user(&pool, "Bo").await;
        for (actor, action) in [
            (ana, "user.delete"),
            (ana, "user.password_reset"),
            (bo, "torrent.delete"),
            (bo, "userland.oddity"),
        ] {
            record(&pool, actor, action, "x", None, None).await.unwrap();
        }
        let actions = |filter| {
            let pool = pool.clone();
            async move {
                let mut got: Vec<String> = list_filtered(&pool, filter, 50, 0)
                    .await
                    .unwrap()
                    .into_iter()
                    .map(|r| r.action)
                    .collect();
                got.sort();
                got
            }
        };
        let family = AuditFilter {
            action: Some("user"),
            actor: None,
        };
        assert_eq!(
            actions(family).await,
            ["user.delete", "user.password_reset"]
        );
        let exact = AuditFilter {
            action: Some("user.delete"),
            actor: None,
        };
        assert_eq!(actions(exact).await, ["user.delete"]);
        let bos = AuditFilter {
            action: None,
            actor: Some(bo),
        };
        assert_eq!(actions(bos).await, ["torrent.delete", "userland.oddity"]);
        let none = AuditFilter {
            action: Some("torrent"),
            actor: Some(ana),
        };
        assert_eq!(actions(none).await, Vec::<String>::new());
    }
}
