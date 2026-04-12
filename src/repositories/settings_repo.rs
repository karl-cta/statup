//! Repository for the `settings` key/value table.

use crate::db::DbPool;
use crate::error::AppError;

pub struct SettingsRepository;

impl SettingsRepository {
    /// Get a setting value by key. Returns `None` if the key does not exist.
    pub async fn get(pool: &DbPool, key: &str) -> Result<Option<String>, AppError> {
        let row = sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        Ok(row)
    }

    /// Insert or update a setting.
    pub async fn set(pool: &DbPool, key: &str, value: &str) -> Result<(), AppError> {
        sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
            .bind(key)
            .bind(value)
            .execute(pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        Ok(())
    }
}
