//! API key repository: the keys in use, found by the hash of what a request
//! presents. The key itself is never stored.

use chrono::{DateTime, Duration, Utc};

use sqlx::AssertSqlSafe;

use crate::clock;
use crate::db::DbPool;
use crate::models::{ApiKey, ApiKeyHolder};

pub struct ApiKeyRepository;

/// A key with the name and state of the administrator who made it.
const KEY_COLUMNS: &str = "SELECT k.id, k.name, k.prefix, \
     u.display_name AS creator_name, u.is_active AS creator_active, \
     k.created_at, k.last_used_at \
     FROM api_keys k JOIN users u ON u.id = k.created_by";

/// A use is recorded at most this often: a busy tool would otherwise write
/// on every request.
const USE_PRECISION: Duration = Duration::minutes(1);

impl ApiKeyRepository {
    pub async fn create(
        pool: &DbPool,
        name: &str,
        prefix: &str,
        secret_hash: &str,
        created_by: i64,
    ) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar(
            "INSERT INTO api_keys (name, prefix, secret_hash, created_by) \
             VALUES (?, ?, ?, ?) RETURNING id",
        )
        .bind(name)
        .bind(prefix)
        .bind(secret_hash)
        .bind(created_by)
        .fetch_one(pool)
        .await
    }

    /// Whether a key in use already has this name, ignoring case, accents
    /// included: `SQLite` folds the case of ASCII letters only.
    pub async fn name_in_use(pool: &DbPool, name: &str) -> Result<bool, sqlx::Error> {
        let names: Vec<String> =
            sqlx::query_scalar("SELECT name FROM api_keys WHERE revoked_at IS NULL")
                .fetch_all(pool)
                .await?;
        let wanted = name.to_lowercase();
        Ok(names.iter().any(|taken| taken.to_lowercase() == wanted))
    }

    /// Every key in use, newest first.
    pub async fn list_in_use(pool: &DbPool) -> Result<Vec<ApiKey>, sqlx::Error> {
        sqlx::query_as::<_, ApiKey>(AssertSqlSafe(format!(
            "{KEY_COLUMNS} WHERE k.revoked_at IS NULL ORDER BY k.created_at DESC, k.id DESC"
        )))
        .fetch_all(pool)
        .await
    }

    pub async fn count_in_use(pool: &DbPool) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar("SELECT COUNT(*) FROM api_keys WHERE revoked_at IS NULL")
            .fetch_one(pool)
            .await
    }

    /// The key in use whose hash this is.
    pub async fn find_by_hash(
        pool: &DbPool,
        secret_hash: &str,
    ) -> Result<Option<ApiKeyHolder>, sqlx::Error> {
        sqlx::query_as::<_, ApiKeyHolder>(
            "SELECT id, name, created_by, last_used_at FROM api_keys \
             WHERE secret_hash = ? AND revoked_at IS NULL",
        )
        .bind(secret_hash)
        .fetch_optional(pool)
        .await
    }

    /// The name of the key revoked, `None` when no key in use has this id.
    /// The row stays: its name still signs what the key published.
    pub async fn revoke(
        pool: &DbPool,
        id: i64,
        now: DateTime<Utc>,
    ) -> Result<Option<String>, sqlx::Error> {
        sqlx::query_scalar(
            "UPDATE api_keys SET revoked_at = ? WHERE id = ? AND revoked_at IS NULL \
             RETURNING name",
        )
        .bind(clock::db(now))
        .bind(id)
        .fetch_optional(pool)
        .await
    }

    /// Gives a key in use a new secret, which stops the old one at once.
    /// Returns its name, `None` when no key in use has this id. The key
    /// keeps its row, so its name still signs what it published before.
    pub async fn replace_secret(
        pool: &DbPool,
        id: i64,
        prefix: &str,
        secret_hash: &str,
    ) -> Result<Option<String>, sqlx::Error> {
        sqlx::query_scalar(
            "UPDATE api_keys SET prefix = ?, secret_hash = ?, last_used_at = NULL \
             WHERE id = ? AND revoked_at IS NULL RETURNING name",
        )
        .bind(prefix)
        .bind(secret_hash)
        .bind(id)
        .fetch_optional(pool)
        .await
    }

    /// Records a use of the key, unless one was recorded in the last minute.
    pub async fn touch(pool: &DbPool, id: i64, now: DateTime<Utc>) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE api_keys SET last_used_at = ? \
             WHERE id = ? AND (last_used_at IS NULL OR last_used_at <= ?)",
        )
        .bind(clock::db(now))
        .bind(id)
        .bind(clock::db(now - USE_PRECISION))
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Whether a use at `now` would be recorded, read from what the key
    /// already holds, so a recent use costs no write.
    pub fn use_is_due(last_used_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
        last_used_at.is_none_or(|last| last <= now - USE_PRECISION)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Role;
    use crate::repositories::UserRepository;
    use crate::test_helpers::test_pool;

    async fn admin(pool: &DbPool) -> i64 {
        UserRepository::create(pool, "admin@example.com", "hash", "Karl", Role::Admin)
            .await
            .unwrap()
            .id
    }

    fn at(minute: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + minute * 60, 0).unwrap()
    }

    #[tokio::test]
    async fn a_key_is_found_by_its_hash_until_it_is_revoked() {
        let pool = test_pool().await;
        let creator = admin(&pool).await;
        let id = ApiKeyRepository::create(&pool, "Zabbix", "stp_Ab3x", "hash-1", creator)
            .await
            .unwrap();

        let holder = ApiKeyRepository::find_by_hash(&pool, "hash-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!((holder.id, holder.name.as_str()), (id, "Zabbix"));
        assert_eq!(holder.created_by, creator);
        assert!(
            ApiKeyRepository::find_by_hash(&pool, "hash-2")
                .await
                .unwrap()
                .is_none()
        );

        assert_eq!(
            ApiKeyRepository::revoke(&pool, id, at(0)).await.unwrap(),
            Some("Zabbix".to_string())
        );
        assert_eq!(
            ApiKeyRepository::revoke(&pool, id, at(1)).await.unwrap(),
            None
        );
        assert!(
            ApiKeyRepository::find_by_hash(&pool, "hash-1")
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(ApiKeyRepository::list_in_use(&pool).await.unwrap(), []);
        assert_eq!(ApiKeyRepository::count_in_use(&pool).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn two_keys_in_use_never_share_a_name_but_a_revoked_one_frees_it() {
        let pool = test_pool().await;
        let creator = admin(&pool).await;
        let first = ApiKeyRepository::create(&pool, "Zabbix", "stp_a", "hash-1", creator)
            .await
            .unwrap();

        assert!(
            ApiKeyRepository::name_in_use(&pool, "ZABBIX")
                .await
                .unwrap()
        );
        let twin = ApiKeyRepository::create(&pool, "zabbix", "stp_b", "hash-2", creator).await;
        assert!(matches!(twin, Err(sqlx::Error::Database(db)) if db.is_unique_violation()));

        ApiKeyRepository::revoke(&pool, first, at(0)).await.unwrap();
        assert!(
            !ApiKeyRepository::name_in_use(&pool, "Zabbix")
                .await
                .unwrap()
        );
        ApiKeyRepository::create(&pool, "Zabbix", "stp_c", "hash-3", creator)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn names_differing_only_by_accented_case_are_the_same_name() {
        let pool = test_pool().await;
        let creator = admin(&pool).await;
        ApiKeyRepository::create(&pool, "Équipe", "stp_a", "hash-1", creator)
            .await
            .unwrap();
        assert!(
            ApiKeyRepository::name_in_use(&pool, "équipe")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn the_list_names_the_creator_and_says_when_their_account_is_disabled() {
        let pool = test_pool().await;
        let creator = admin(&pool).await;
        let id = ApiKeyRepository::create(&pool, "Zabbix", "stp_Ab3x", "hash-1", creator)
            .await
            .unwrap();
        sqlx::query("UPDATE users SET is_active = 0 WHERE id = ?")
            .bind(creator)
            .execute(&pool)
            .await
            .unwrap();

        let keys = ApiKeyRepository::list_in_use(&pool).await.unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].id, id);
        assert_eq!(keys[0].creator_name, "Karl");
        assert!(!keys[0].creator_active);
        assert_eq!(keys[0].prefix, "stp_Ab3x");
        // A disabled creator does not stop the key.
        assert!(
            ApiKeyRepository::find_by_hash(&pool, "hash-1")
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn a_use_is_recorded_at_most_once_a_minute() {
        let pool = test_pool().await;
        let creator = admin(&pool).await;
        let id = ApiKeyRepository::create(&pool, "Zabbix", "stp_a", "hash-1", creator)
            .await
            .unwrap();
        let last_used = || async {
            ApiKeyRepository::find_by_hash(&pool, "hash-1")
                .await
                .unwrap()
                .unwrap()
                .last_used_at
        };

        assert!(ApiKeyRepository::use_is_due(None, at(0)));
        ApiKeyRepository::touch(&pool, id, at(0)).await.unwrap();
        assert_eq!(last_used().await, Some(at(0)));

        ApiKeyRepository::touch(&pool, id, at(0) + Duration::seconds(30))
            .await
            .unwrap();
        assert_eq!(last_used().await, Some(at(0)));
        assert!(!ApiKeyRepository::use_is_due(
            Some(at(0)),
            at(0) + Duration::seconds(30)
        ));

        ApiKeyRepository::touch(&pool, id, at(1)).await.unwrap();
        assert_eq!(last_used().await, Some(at(1)));
        assert!(ApiKeyRepository::use_is_due(Some(at(0)), at(1)));
    }
}
