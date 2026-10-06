//! Test fixtures: a migrated in-memory pool and users.

use std::str::FromStr;

use chrono::Utc;
use iris_core::ids::UserId;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use uuid::Uuid;

/// Single-connection in-memory pool (every query hits the same DB), migrated
/// through the latest schema, foreign keys ON like `pool::connect` so the
/// cascades under test are the prod ones.
///
/// # Panics
/// When the in-memory database can't be opened or migrated.
pub async fn migrated_pool() -> SqlitePool {
    let opts = SqliteConnectOptions::from_str("sqlite::memory:")
        .expect("parse sqlite url")
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .expect("open in-memory sqlite");
    crate::migrate::run(&pool).await.expect("run migrations");
    pool
}

/// # Panics
/// When the insert fails.
pub async fn make_user(pool: &SqlitePool) -> UserId {
    make_named_user(pool, "T").await
}

/// # Panics
/// When the insert fails.
pub async fn make_named_user(pool: &SqlitePool, display_name: &str) -> UserId {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, display_name, is_admin, created_at) \
         VALUES (?1, ?2, '', ?3, 0, ?4)",
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
