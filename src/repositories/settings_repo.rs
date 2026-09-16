//! Repository for the `settings` key/value table.

use crate::db::DbPool;

pub struct SettingsRepository;

impl SettingsRepository {
    /// Get a setting value by key. Returns `None` if the key does not exist.
    pub async fn get(pool: &DbPool, key: &str) -> Result<Option<String>, sqlx::Error> {
        sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(pool)
            .await
    }

    /// Insert or update a setting.
    pub async fn set(pool: &DbPool, key: &str, value: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?, ?) \
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::test_pool;

    #[tokio::test]
    async fn a_setting_is_created_then_replaced() {
        let pool = test_pool().await;
        assert_eq!(
            SettingsRepository::get(&pool, "public_mode").await.unwrap(),
            None
        );

        SettingsRepository::set(&pool, "public_mode", "true")
            .await
            .unwrap();
        SettingsRepository::set(&pool, "public_mode", "false")
            .await
            .unwrap();

        assert_eq!(
            SettingsRepository::get(&pool, "public_mode")
                .await
                .unwrap()
                .as_deref(),
            Some("false")
        );
    }
}
