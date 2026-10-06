//! An account's recent searches (`recent_searches`, migration 0041).

use chrono::{DateTime, Utc};
use iris_core::ids::UserId;
use sqlx::SqlitePool;
use uuid::Uuid;

/// How many searches an account keeps.
pub const KEEP: i64 = 10;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RecentSearch {
    pub query: String,
    pub searched_at: DateTime<Utc>,
}

fn key_of(query: &str) -> String {
    query.trim().to_lowercase()
}

pub async fn list(pool: &SqlitePool, user_id: UserId) -> Result<Vec<RecentSearch>, sqlx::Error> {
    let user: Uuid = user_id.into();
    sqlx::query_as(
        "SELECT query, searched_at FROM recent_searches \
         WHERE user_id = ?1 ORDER BY searched_at DESC LIMIT ?2",
    )
    .bind(user)
    .bind(KEEP)
    .fetch_all(pool)
    .await
}

/// Remember `query` as the latest search and forget the ones past [`KEEP`].
pub async fn record(pool: &SqlitePool, user_id: UserId, query: &str) -> Result<(), sqlx::Error> {
    let user: Uuid = user_id.into();
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO recent_searches (user_id, query_key, query, searched_at) \
         VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT(user_id, query_key) DO UPDATE SET \
           query = excluded.query, searched_at = excluded.searched_at",
    )
    .bind(user)
    .bind(key_of(query))
    .bind(query.trim())
    .bind(Utc::now())
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "DELETE FROM recent_searches WHERE user_id = ?1 AND query_key NOT IN ( \
           SELECT query_key FROM recent_searches WHERE user_id = ?1 \
           ORDER BY searched_at DESC LIMIT ?2)",
    )
    .bind(user)
    .bind(KEEP)
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

/// Forget one search, or every search when `query` is `None`.
pub async fn forget(
    pool: &SqlitePool,
    user_id: UserId,
    query: Option<&str>,
) -> Result<(), sqlx::Error> {
    let user: Uuid = user_id.into();
    match query {
        Some(q) => {
            sqlx::query("DELETE FROM recent_searches WHERE user_id = ?1 AND query_key = ?2")
                .bind(user)
                .bind(key_of(q))
                .execute(pool)
                .await?;
        }
        None => {
            sqlx::query("DELETE FROM recent_searches WHERE user_id = ?1")
                .bind(user)
                .execute(pool)
                .await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{KEEP, forget, list, record};

    async fn pool_with_user() -> (sqlx::SqlitePool, iris_core::ids::UserId) {
        let pool = crate::test_support::migrated_pool().await;
        let user = crate::test_support::make_user(&pool).await;
        (pool, user)
    }

    #[tokio::test]
    async fn newest_first_one_row_per_query_capped() {
        let (pool, user) = pool_with_user().await;
        record(&pool, user, "the bear").await.unwrap();
        record(&pool, user, "Frieren").await.unwrap();
        record(&pool, user, "  THE BEAR ").await.unwrap();
        let got: Vec<_> = list(&pool, user)
            .await
            .unwrap()
            .into_iter()
            .map(|r| r.query)
            .collect();
        assert_eq!(got, ["THE BEAR", "Frieren"]);

        for i in 0..20 {
            record(&pool, user, &format!("q{i}")).await.unwrap();
        }
        let got = list(&pool, user).await.unwrap();
        assert_eq!(got.len(), usize::try_from(KEEP).unwrap());
        assert_eq!(got[0].query, "q19");

        forget(&pool, user, Some("Q19")).await.unwrap();
        assert_eq!(list(&pool, user).await.unwrap()[0].query, "q18");
        forget(&pool, user, None).await.unwrap();
        assert!(list(&pool, user).await.unwrap().is_empty());
    }
}
