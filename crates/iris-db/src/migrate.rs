use sqlx::SqlitePool;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

pub async fn run(pool: &SqlitePool) -> Result<(), sqlx::migrate::MigrateError> {
    MIGRATOR.run(pool).await
}

#[cfg(test)]
mod tests {
    use crate::test_support::migrated_pool;

    #[tokio::test]
    async fn index_hygiene_leaves_one_index_per_lookup() {
        let pool = migrated_pool().await;
        let names: Vec<(String,)> =
            sqlx::query_as("SELECT name FROM sqlite_master WHERE type = 'index'")
                .fetch_all(&pool)
                .await
                .unwrap();
        let has = |n: &str| names.iter().any(|(name,)| name == n);
        assert!(has("torrents_source_idx"));
        assert!(has("catalog_items_provider_external_idx"));
        assert!(has("available_episodes_dedup_idx"));
        assert!(!has("available_episodes_lookup_idx"));
        assert!(!has("users_email_idx"));
    }

    /// Prod had an invitation consumed by an account deleted while foreign
    /// keys weren't enforced: the 0051 rebuild must carry it as NULL, not
    /// abort the boot.
    #[tokio::test]
    async fn the_invitations_rebuild_survives_a_dangling_account() {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::str::FromStr;

        let opts = SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .foreign_keys(false);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .unwrap();
        for m in super::MIGRATOR.iter().filter(|m| m.version <= 50) {
            sqlx::raw_sql(m.sql.clone()).execute(&pool).await.unwrap();
        }
        let user = uuid::Uuid::new_v4();
        let gone = uuid::Uuid::new_v4();
        sqlx::query("INSERT INTO users (id, email, display_name, password_hash, is_admin, created_at) VALUES (?1, 'a@x', 'A', 'h', 1, '2026-01-01')")
            .bind(user)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO invitations (id, token_hash, created_by, created_at, expires_at, consumed_at, consumed_by) VALUES (?1, 't', ?2, '2026-01-01', '2026-02-01', '2026-01-02', ?3)")
            .bind(uuid::Uuid::new_v4())
            .bind(user)
            .bind(gone)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .unwrap();
        let rebuild = super::MIGRATOR.iter().find(|m| m.version == 51).unwrap();
        sqlx::raw_sql(rebuild.sql.clone())
            .execute(&pool)
            .await
            .unwrap();
        let (created_by, consumed_by): (Option<uuid::Uuid>, Option<uuid::Uuid>) =
            sqlx::query_as("SELECT created_by, consumed_by FROM invitations")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(created_by, Some(user));
        assert_eq!(consumed_by, None, "the deleted account is carried as NULL");
    }
}
