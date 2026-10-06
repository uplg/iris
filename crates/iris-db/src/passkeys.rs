//! Stored passkeys (`webauthn_credentials`, migration 0040).

use chrono::{DateTime, Utc};
use iris_core::ids::UserId;
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PasskeyRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub credential_id: String,
    pub passkey_json: String,
    pub name: String,
    pub backup_eligible: bool,
    pub backed_up: bool,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

pub struct NewPasskey<'a> {
    pub user_id: UserId,
    pub credential_id: &'a str,
    pub passkey_json: &'a str,
    pub name: &'a str,
    pub backup_eligible: bool,
    pub backed_up: bool,
}

macro_rules! columns {
    () => {
        "id, user_id, credential_id, passkey_json, name, backup_eligible, backed_up, \
         created_at, last_used_at"
    };
}

pub async fn insert(pool: &SqlitePool, new: &NewPasskey<'_>) -> Result<PasskeyRow, sqlx::Error> {
    let user: Uuid = new.user_id.into();
    sqlx::query_as(concat!(
        "INSERT INTO webauthn_credentials \
           (id, user_id, credential_id, passkey_json, name, backup_eligible, backed_up, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) RETURNING ",
        columns!()
    ))
    .bind(Uuid::new_v4())
    .bind(user)
    .bind(new.credential_id)
    .bind(new.passkey_json)
    .bind(new.name)
    .bind(new.backup_eligible)
    .bind(new.backed_up)
    .bind(Utc::now())
    .fetch_one(pool)
    .await
}

pub async fn list_for_user(
    pool: &SqlitePool,
    user_id: UserId,
) -> Result<Vec<PasskeyRow>, sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query_as(concat!(
        "SELECT ",
        columns!(),
        " FROM webauthn_credentials WHERE user_id = ?1 ORDER BY created_at"
    ))
    .bind(user)
    .fetch_all(pool)
    .await
}

/// Credential ids a user already holds, so a new registration can exclude
/// them (an authenticator never makes a second passkey for the same site).
pub async fn credential_ids_of(
    pool: &SqlitePool,
    user_id: UserId,
) -> Result<Vec<String>, sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query_scalar("SELECT credential_id FROM webauthn_credentials WHERE user_id = ?1")
        .bind(user)
        .fetch_all(pool)
        .await
}

pub async fn find_by_credential(
    pool: &SqlitePool,
    credential_id: &str,
) -> Result<Option<PasskeyRow>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT ",
        columns!(),
        " FROM webauthn_credentials WHERE credential_id = ?1"
    ))
    .bind(credential_id)
    .fetch_optional(pool)
    .await
}

/// Record a sign-in; `updated` carries the new serialised passkey and
/// backup flags when the authenticator's counter or state moved.
pub async fn mark_used(
    pool: &SqlitePool,
    id: Uuid,
    updated: Option<(&str, bool, bool)>,
) -> Result<(), sqlx::Error> {
    let now = Utc::now();
    match updated {
        Some((json, eligible, backed_up)) => {
            sqlx::query(
                "UPDATE webauthn_credentials \
                 SET last_used_at = ?1, passkey_json = ?2, backup_eligible = ?3, backed_up = ?4 \
                 WHERE id = ?5",
            )
            .bind(now)
            .bind(json)
            .bind(eligible)
            .bind(backed_up)
            .bind(id)
            .execute(pool)
            .await?;
        }
        None => {
            sqlx::query("UPDATE webauthn_credentials SET last_used_at = ?1 WHERE id = ?2")
                .bind(now)
                .bind(id)
                .execute(pool)
                .await?;
        }
    }
    Ok(())
}

/// Rename one of the user's passkeys; `false` when it isn't theirs.
pub async fn rename(
    pool: &SqlitePool,
    user_id: UserId,
    id: Uuid,
    name: &str,
) -> Result<bool, sqlx::Error> {
    let user: Uuid = user_id.into();
    let res =
        sqlx::query("UPDATE webauthn_credentials SET name = ?1 WHERE id = ?2 AND user_id = ?3")
            .bind(name)
            .bind(id)
            .bind(user)
            .execute(pool)
            .await?;
    Ok(res.rows_affected() == 1)
}

/// Delete one of the user's passkeys; `false` when it isn't theirs. The
/// password stays, so the last passkey can go too.
pub async fn delete(pool: &SqlitePool, user_id: UserId, id: Uuid) -> Result<bool, sqlx::Error> {
    let user: Uuid = user_id.into();
    let res = sqlx::query("DELETE FROM webauthn_credentials WHERE id = ?1 AND user_id = ?2")
        .bind(id)
        .bind(user)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() == 1)
}
