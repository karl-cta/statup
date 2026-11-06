//! API keys: made once, shown once, then kept only as a hash. A request
//! proves itself by presenting the key; its hash finds the row.

use chrono::{DateTime, Utc};
use rand::Rng;
use rand::distributions::Alphanumeric;

use super::link_token::secret_hash as hash;
use crate::db::DbPool;
use crate::error::{AppError, name_taken_on_conflict};
use crate::models::{API_KEY_PREFIX, ApiKeyHolder, User, check_api_key_name};
use crate::repositories::ApiKeyRepository;

/// Characters after the prefix: about 238 bits, beyond any guessing.
const SECRET_LENGTH: usize = 40;

/// Characters of the key the keys page keeps, prefix included.
const SHOWN_LENGTH: usize = 8;

const NAME_TAKEN: &str = "validation.api_key_name_taken";

/// A key just made: its row and the only copy of the key itself.
pub struct NewApiKey {
    pub id: i64,
    pub name: String,
    pub key: String,
}

pub struct ApiKeyService;

impl ApiKeyService {
    pub async fn create(pool: &DbPool, name: &str, creator: &User) -> Result<NewApiKey, AppError> {
        let name = check_api_key_name(name).map_err(AppError::validation)?;
        if ApiKeyRepository::name_in_use(pool, &name).await? {
            return Err(AppError::validation(NAME_TAKEN));
        }
        let key = new_key();
        let id =
            ApiKeyRepository::create(pool, &name, &key[..SHOWN_LENGTH], &hash(&key), creator.id)
                .await
                .map_err(|e| name_taken_on_conflict(e, NAME_TAKEN))?;
        Ok(NewApiKey { id, name, key })
    }

    /// A new secret for a key in use, under the same name: the old secret
    /// stops working at once.
    pub async fn regenerate(pool: &DbPool, id: i64) -> Result<NewApiKey, AppError> {
        let key = new_key();
        let name = ApiKeyRepository::replace_secret(pool, id, &key[..SHOWN_LENGTH], &hash(&key))
            .await?
            .ok_or(AppError::NotFound)?;
        Ok(NewApiKey { id, name, key })
    }

    /// The key in use that a request presents. A malformed key is refused
    /// before any query.
    pub async fn holder_of(
        pool: &DbPool,
        presented: &str,
    ) -> Result<Option<ApiKeyHolder>, AppError> {
        if !well_formed(presented) {
            return Ok(None);
        }
        Ok(ApiKeyRepository::find_by_hash(pool, &hash(presented)).await?)
    }

    /// Records that the key was used, when the last use is old enough to be
    /// worth a write.
    pub async fn record_use(
        pool: &DbPool,
        holder: &ApiKeyHolder,
        now: DateTime<Utc>,
    ) -> Result<(), AppError> {
        if ApiKeyRepository::use_is_due(holder.last_used_at, now) {
            ApiKeyRepository::touch(pool, holder.id, now).await?;
        }
        Ok(())
    }
}

fn new_key() -> String {
    let secret: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(SECRET_LENGTH)
        .map(char::from)
        .collect();
    format!("{API_KEY_PREFIX}{secret}")
}

fn well_formed(key: &str) -> bool {
    key.strip_prefix(API_KEY_PREFIX).is_some_and(|secret| {
        secret.len() == SECRET_LENGTH && secret.bytes().all(|b| b.is_ascii_alphanumeric())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Role;
    use crate::repositories::UserRepository;
    use crate::test_helpers::test_pool;

    async fn admin(pool: &DbPool) -> User {
        UserRepository::create(pool, "admin@example.com", "hash", "Karl", Role::Admin)
            .await
            .unwrap()
    }

    #[test]
    fn a_new_key_is_long_random_and_well_formed() {
        let key = new_key();
        assert!(key.starts_with("stp_"));
        assert_eq!(key.len(), 4 + SECRET_LENGTH);
        assert!(well_formed(&key));
        assert_ne!(key, new_key());
    }

    #[test]
    fn a_malformed_key_is_told_apart_without_a_query() {
        assert!(!well_formed(""));
        assert!(!well_formed("stp_short"));
        assert!(!well_formed(&format!("key_{}", "a".repeat(SECRET_LENGTH))));
        assert!(!well_formed(&format!(
            "stp_{}!",
            "a".repeat(SECRET_LENGTH - 1)
        )));
        assert!(well_formed(&format!("stp_{}", "a".repeat(SECRET_LENGTH))));
    }

    #[test]
    fn the_hash_is_the_hexadecimal_sha256() {
        assert_eq!(
            hash("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[tokio::test]
    async fn a_key_is_kept_as_a_hash_and_found_by_presenting_it() {
        let pool = test_pool().await;
        let creator = admin(&pool).await;
        let made = ApiKeyService::create(&pool, "  Zabbix ", &creator)
            .await
            .unwrap();
        assert_eq!(made.name, "Zabbix");

        let stored: (String, String) =
            sqlx::query_as("SELECT prefix, secret_hash FROM api_keys WHERE id = ?")
                .bind(made.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(stored.0, made.key[..8]);
        assert_eq!(stored.1, hash(&made.key));
        assert!(!stored.1.contains(&made.key[4..]));

        let holder = ApiKeyService::holder_of(&pool, &made.key)
            .await
            .unwrap()
            .unwrap();
        assert_eq!((holder.id, holder.created_by), (made.id, creator.id));
        let other = format!("stp_{}", "a".repeat(SECRET_LENGTH));
        assert!(
            ApiKeyService::holder_of(&pool, &other)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn a_name_in_use_or_wrong_is_refused() {
        let pool = test_pool().await;
        let creator = admin(&pool).await;
        ApiKeyService::create(&pool, "Zabbix", &creator)
            .await
            .unwrap();

        let twin = ApiKeyService::create(&pool, "zabbix", &creator).await;
        assert!(matches!(twin, Err(AppError::Validation(key)) if key == NAME_TAKEN));
        let empty = ApiKeyService::create(&pool, " ", &creator).await;
        assert!(
            matches!(empty, Err(AppError::Validation(key)) if key == "validation.api_key_name_required")
        );
    }
}
