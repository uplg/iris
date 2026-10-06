use chrono::{DateTime, Duration, Utc};
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
           AND expires_at > ?2 \
         ORDER BY issued_at DESC",
    )
    .bind(user)
    .bind(Utc::now())
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

/// Device label / kind / `expires_at` of the refresh token [`mark_rotated`]
/// just retired. Used by `/auth/refresh` to carry the device tagging forward
/// when rotating the token — without
/// this, paired-device rows lose their `device_kind` after the first
/// rotation and the account-page listing (which filters on
/// `device_kind IS NOT NULL`) shows "no paired devices yet".
#[derive(Debug, Clone)]
pub struct ActiveDeviceInfo {
    pub device_label: Option<String>,
    pub device_kind: Option<String>,
    pub expires_at: DateTime<Utc>,
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

/// Mark an active refresh token as ROTATED and return its device tagging,
/// in one statement: of two near-simultaneous refreshes exactly one gets
/// `Some`, the other falls through to [`recently_rotated`]. Rotated means
/// revoked but flagged `rotated_at`, so a straggler can be recognised and
/// tolerated. `revoke` (logout / device revoke) leaves `rotated_at` NULL so
/// a session the user deliberately killed is never resurrected by the
/// grace window. `None`: the token wasn't active.
pub async fn mark_rotated(
    pool: &SqlitePool,
    jti: Uuid,
) -> Result<Option<ActiveDeviceInfo>, sqlx::Error> {
    let now = Utc::now();
    let row: Option<(Option<String>, Option<String>, DateTime<Utc>)> = sqlx::query_as(
        "UPDATE refresh_tokens SET revoked_at = ?1, rotated_at = ?1 \
         WHERE jti = ?2 AND revoked_at IS NULL AND expires_at > ?1 \
         RETURNING device_label, device_kind, expires_at",
    )
    .bind(now)
    .bind(jti)
    .fetch_optional(pool)
    .await?;
    Ok(
        row.map(|(device_label, device_kind, expires_at)| ActiveDeviceInfo {
            device_label,
            device_kind,
            expires_at,
        }),
    )
}

/// Device tagging carried on a token ROTATED within the grace window — the
/// straggler-refresh recovery path. `Some` only when the jti was rotated (not
/// explicitly revoked) no longer ago than `grace_secs`; `None` otherwise, so
/// the caller falls back to a 401.
#[derive(Debug, Clone)]
pub struct RotatedInfo {
    pub device_label: Option<String>,
    pub device_kind: Option<String>,
}

pub async fn recently_rotated(
    pool: &SqlitePool,
    jti: Uuid,
    grace_secs: i64,
) -> Result<Option<RotatedInfo>, sqlx::Error> {
    // Text comparison on the RFC3339 `rotated_at` — same proven shape as the
    // `expires_at > ?` filters above (sqlx encodes DateTime<Utc> consistently,
    // and the format sorts lexicographically).
    let cutoff = Utc::now() - Duration::seconds(grace_secs);
    let row: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT device_label, device_kind FROM refresh_tokens \
         WHERE jti = ?1 AND rotated_at IS NOT NULL AND rotated_at >= ?2",
    )
    .bind(jti)
    .bind(cutoff)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(device_label, device_kind)| RotatedInfo {
        device_label,
        device_kind,
    }))
}

/// Delete sessions nobody can use any more: expired, or revoked/rotated
/// long enough ago that the rotation grace window can't need them.
pub async fn prune(pool: &SqlitePool, before: DateTime<Utc>) -> Result<u64, sqlx::Error> {
    let res = sqlx::query("DELETE FROM refresh_tokens WHERE expires_at < ?1 OR revoked_at < ?1")
        .bind(before)
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::migrated_pool;

    #[tokio::test]
    async fn prune_keeps_live_sessions_only() {
        let pool = migrated_pool().await;
        let user = crate::test_support::make_user(&pool).await;
        let live = Uuid::new_v4();
        let expired = Uuid::new_v4();
        let revoked = Uuid::new_v4();
        insert(&pool, live, user, Utc::now() + Duration::days(7))
            .await
            .unwrap();
        insert(&pool, expired, user, Utc::now() - Duration::days(3))
            .await
            .unwrap();
        insert(&pool, revoked, user, Utc::now() + Duration::days(7))
            .await
            .unwrap();
        mark_rotated(&pool, revoked).await.unwrap();

        // Rotated a moment ago: still inside any grace window, kept.
        assert_eq!(
            prune(&pool, Utc::now() - Duration::days(1)).await.unwrap(),
            1
        );
        // A day on, the rotated one goes too.
        assert_eq!(
            prune(&pool, Utc::now() + Duration::days(1)).await.unwrap(),
            1
        );
        assert!(is_active(&pool, live).await.unwrap());
    }

    #[tokio::test]
    async fn only_one_of_two_rotations_wins() {
        let pool = migrated_pool().await;
        let user = crate::test_support::make_user(&pool).await;
        let jti = Uuid::new_v4();
        insert_with_device(
            &pool,
            jti,
            user,
            Utc::now() + Duration::hours(1),
            Some("TV"),
            Some("android-tv"),
        )
        .await
        .unwrap();
        let first = mark_rotated(&pool, jti).await.unwrap();
        let second = mark_rotated(&pool, jti).await.unwrap();
        assert_eq!(
            first.and_then(|i| i.device_kind).as_deref(),
            Some("android-tv")
        );
        assert!(second.is_none());
    }

    /// The rotation grace contract: a token ROTATED (the happy path of
    /// `/auth/refresh`) drops out of the active lookup but stays recoverable —
    /// with its device tagging — for a straggler refresh within the grace
    /// window; outside the window, and for an explicitly REVOKED token, it is
    /// gone for good. This is what stops a multi-tab / retry race from logging
    /// the user out while never resurrecting a session they deliberately killed.
    #[tokio::test]
    async fn rotation_is_recoverable_within_grace_but_revocation_is_not() {
        let pool = migrated_pool().await;
        let user = crate::test_support::make_user(&pool).await;

        // A device-tagged token, active for an hour, then rotated.
        let rotated = Uuid::new_v4();
        insert_with_device(
            &pool,
            rotated,
            user,
            Utc::now() + Duration::hours(1),
            Some("Living room"),
            Some("android-tv"),
        )
        .await
        .unwrap();
        mark_rotated(&pool, rotated).await.unwrap();

        // No longer active for the normal refresh lookup …
        assert!(!is_active(&pool, rotated).await.unwrap());

        // … but a straggler within the grace window recovers it, carrying the
        // device tagging forward.
        let info = recently_rotated(&pool, rotated, 60)
            .await
            .unwrap()
            .expect("rotated token recoverable within grace");
        assert_eq!(info.device_kind.as_deref(), Some("android-tv"));
        assert_eq!(info.device_label.as_deref(), Some("Living room"));

        // Backdate the rotation past the grace window → no longer recoverable.
        sqlx::query("UPDATE refresh_tokens SET rotated_at = ?1 WHERE jti = ?2")
            .bind(Utc::now() - Duration::minutes(2))
            .bind(rotated)
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            recently_rotated(&pool, rotated, 60)
                .await
                .unwrap()
                .is_none()
        );

        // An explicitly REVOKED token (logout / device revoke) leaves
        // `rotated_at` NULL and is never resurrected by the grace window.
        let revoked = Uuid::new_v4();
        insert_with_device(
            &pool,
            revoked,
            user,
            Utc::now() + Duration::hours(1),
            None,
            None,
        )
        .await
        .unwrap();
        revoke(&pool, revoked).await.unwrap();
        assert!(!is_active(&pool, revoked).await.unwrap());
        assert!(
            recently_rotated(&pool, revoked, 60)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn an_expired_device_session_is_not_listed() {
        let pool = migrated_pool().await;
        let user = crate::test_support::make_user(&pool).await;
        let live = Uuid::new_v4();
        for (jti, expires_at) in [
            (live, Utc::now() + Duration::days(1)),
            (Uuid::new_v4(), Utc::now() - Duration::minutes(1)),
        ] {
            insert_with_device(&pool, jti, user, expires_at, None, Some("android-tv"))
                .await
                .unwrap();
        }
        let listed = list_devices_for_user(&pool, user).await.unwrap();
        assert_eq!(listed.iter().map(|t| t.jti).collect::<Vec<_>>(), [live]);
    }
}
