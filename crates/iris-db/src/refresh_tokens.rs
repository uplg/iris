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

/// Device label / kind / `expires_at` attached to an active refresh token,
/// or `None` if the jti is unknown / revoked / expired. Used by `/auth/refresh`
/// to carry the device tagging forward when rotating the token — without
/// this, paired-device rows lose their `device_kind` after the first
/// rotation and the account-page listing (which filters on
/// `device_kind IS NOT NULL`) shows "no paired devices yet".
#[derive(Debug, Clone)]
pub struct ActiveDeviceInfo {
    pub device_label: Option<String>,
    pub device_kind: Option<String>,
    pub expires_at: DateTime<Utc>,
}

pub async fn get_active_device_info(
    pool: &SqlitePool,
    jti: Uuid,
) -> Result<Option<ActiveDeviceInfo>, sqlx::Error> {
    let row: Option<(Option<String>, Option<String>, DateTime<Utc>)> = sqlx::query_as(
        "SELECT device_label, device_kind, expires_at FROM refresh_tokens \
         WHERE jti = ?1 AND revoked_at IS NULL AND expires_at > ?2",
    )
    .bind(jti)
    .bind(Utc::now())
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

/// Mark a refresh token as ROTATED: revoked, but flagged `rotated_at` so a
/// near-simultaneous straggler refresh can be recognised and tolerated (see
/// [`recently_rotated`]). Used by `/auth/refresh` in place of [`revoke`] —
/// `revoke` (logout / device revoke) leaves `rotated_at` NULL so a session
/// the user deliberately killed is never resurrected by the grace window.
pub async fn mark_rotated(pool: &SqlitePool, jti: Uuid) -> Result<(), sqlx::Error> {
    let now = Utc::now();
    sqlx::query(
        "UPDATE refresh_tokens SET revoked_at = ?1, rotated_at = ?1 \
         WHERE jti = ?2 AND revoked_at IS NULL",
    )
    .bind(now)
    .bind(jti)
    .execute(pool)
    .await?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    /// Single-connection in-memory pool, migrated through the latest schema.
    async fn migrated_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("open in-memory sqlite");
        crate::migrate::run(&pool).await.expect("run migrations");
        pool
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
        let user = crate::users::create(
            &pool,
            crate::users::NewUser {
                email: "tv@example.com".into(),
                password_hash: "x".into(),
                is_admin: false,
            },
        )
        .await
        .unwrap()
        .id;

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
        assert!(
            get_active_device_info(&pool, rotated)
                .await
                .unwrap()
                .is_none()
        );
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
        assert!(
            get_active_device_info(&pool, revoked)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            recently_rotated(&pool, revoked, 60)
                .await
                .unwrap()
                .is_none()
        );
    }
}
