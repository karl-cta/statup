//! The private link of the status embed. The embed is shown in a frame on
//! another site, where the browser sends no Statup session: holding the
//! link stands for being a member.

use crate::db::DbPool;
use crate::repositories::SettingsRepository;

use super::link_token::{new_secret, same_secret};

const EMBED_KEY_SETTING: &str = "embed_key";

pub struct EmbedService;

impl EmbedService {
    /// The current key, made the first time it is asked for.
    pub async fn key(pool: &DbPool) -> Result<String, sqlx::Error> {
        if let Some(key) = SettingsRepository::get(pool, EMBED_KEY_SETTING).await? {
            return Ok(key);
        }
        SettingsRepository::insert_if_absent(pool, EMBED_KEY_SETTING, &new_secret()).await?;
        SettingsRepository::get(pool, EMBED_KEY_SETTING)
            .await?
            .ok_or(sqlx::Error::RowNotFound)
    }

    /// A new key: every embed pasted with the old one stops showing the
    /// state at once.
    pub async fn regenerate(pool: &DbPool) -> Result<String, sqlx::Error> {
        let key = new_secret();
        SettingsRepository::set(pool, EMBED_KEY_SETTING, &key).await?;
        Ok(key)
    }

    /// Whether the key an embed sent is the current one. Nothing is made
    /// here: an instance whose administrator never asked for the key has
    /// none to match.
    pub async fn grants(pool: &DbPool, sent: &str) -> Result<bool, sqlx::Error> {
        let stored = SettingsRepository::get(pool, EMBED_KEY_SETTING).await?;
        Ok(stored.is_some_and(|stored| same_secret(sent, &stored)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::test_pool;

    #[tokio::test]
    async fn the_key_is_made_once_then_kept() {
        let pool = test_pool().await;
        let first = EmbedService::key(&pool).await.unwrap();
        assert_eq!(first.len(), 32);
        assert!(first.bytes().all(|b| b.is_ascii_alphanumeric()));
        assert_eq!(EmbedService::key(&pool).await.unwrap(), first);
    }

    #[tokio::test]
    async fn only_the_current_key_grants_access() {
        let pool = test_pool().await;
        assert!(
            !EmbedService::grants(&pool, "").await.unwrap(),
            "no key yet, nothing matches"
        );
        let old = EmbedService::key(&pool).await.unwrap();
        assert!(EmbedService::grants(&pool, &old).await.unwrap());
        assert!(!EmbedService::grants(&pool, &old[..31]).await.unwrap());

        let new = EmbedService::regenerate(&pool).await.unwrap();
        assert_ne!(new, old);
        assert!(!EmbedService::grants(&pool, &old).await.unwrap());
        assert!(EmbedService::grants(&pool, &new).await.unwrap());
    }
}
