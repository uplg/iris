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
}
