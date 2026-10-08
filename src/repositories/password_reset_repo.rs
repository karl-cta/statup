//! The links sent to a member who forgot their password.

use crate::db::DbPool;

/// How long a link opens the account, as an `SQLite` date modifier.
const LINK_LIFETIME: &str = "+1 hour";

/// A link that still opens its account: the newest of the account, unused
/// and unexpired. The secret itself is never stored, only its hash.
#[derive(Debug, sqlx::FromRow)]
pub struct PasswordReset {
    pub id: i64,
    pub user_id: i64,
    pub secret_hash: String,
}

pub struct PasswordResetRepository;

impl PasswordResetRepository {
    /// Store a new link for an account and return its id.
    pub async fn insert(
        pool: &DbPool,
        user_id: i64,
        secret_hash: &str,
    ) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar(
            "INSERT INTO password_resets (user_id, secret_hash, expires_at) \
             VALUES (?, ?, datetime('now', ?)) RETURNING id",
        )
        .bind(user_id)
        .bind(secret_hash)
        .bind(LINK_LIFETIME)
        .fetch_one(pool)
        .await
    }

    /// The link with this id, if it still opens its account. A newer link
    /// of the same account replaces it.
    pub async fn find_live(pool: &DbPool, id: i64) -> Result<Option<PasswordReset>, sqlx::Error> {
        sqlx::query_as::<_, PasswordReset>(
            "SELECT r.id, r.user_id, r.secret_hash FROM password_resets r \
             WHERE r.id = ? AND r.expires_at > datetime('now') \
               AND NOT EXISTS (SELECT 1 FROM password_resets newer \
                               WHERE newer.user_id = r.user_id AND newer.id > r.id)",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
    }

    /// How many links the account was sent in the last hour.
    pub async fn sent_in_last_hour(pool: &DbPool, user_id: i64) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM password_resets \
             WHERE user_id = ? AND created_at > datetime('now', '-1 hour')",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
    }

    /// Forget every link of an account, as one of them is used. Returns
    /// whether there was any: of two uses of one link at the same moment,
    /// only the first finds it.
    pub async fn delete_for_user(pool: &DbPool, user_id: i64) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM password_resets WHERE user_id = ?")
            .bind(user_id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Forget the links older than a day: past their hour, and no longer
    /// counted against their account.
    pub async fn delete_stale(pool: &DbPool) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM password_resets WHERE created_at < datetime('now', '-1 day')")
            .execute(pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Role;
    use crate::repositories::UserRepository;
    use crate::test_helpers::test_pool;

    async fn member(pool: &DbPool, email: &str) -> i64 {
        UserRepository::create(pool, email, "hash", "Someone", Role::Reader)
            .await
            .unwrap()
            .id
    }

    async fn age(pool: &DbPool, id: i64, created: &str, expires: &str) {
        sqlx::query(
            "UPDATE password_resets SET created_at = datetime('now', ?), \
                                        expires_at = datetime('now', ?) WHERE id = ?",
        )
        .bind(created)
        .bind(expires)
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn a_new_link_opens_its_account() {
        let pool = test_pool().await;
        let user_id = member(&pool, "ana@example.com").await;

        let id = PasswordResetRepository::insert(&pool, user_id, "secret-hash")
            .await
            .unwrap();
        let link = PasswordResetRepository::find_live(&pool, id)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(link.user_id, user_id);
        assert_eq!(link.secret_hash, "secret-hash");
    }

    #[tokio::test]
    async fn an_expired_link_no_longer_opens() {
        let pool = test_pool().await;
        let user_id = member(&pool, "ana@example.com").await;
        let id = PasswordResetRepository::insert(&pool, user_id, "h")
            .await
            .unwrap();

        age(&pool, id, "-61 minutes", "-1 minute").await;

        assert!(
            PasswordResetRepository::find_live(&pool, id)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn a_newer_link_replaces_the_older_one() {
        let pool = test_pool().await;
        let ana = member(&pool, "ana@example.com").await;
        let ben = member(&pool, "ben@example.com").await;
        let first = PasswordResetRepository::insert(&pool, ana, "h1")
            .await
            .unwrap();
        let other = PasswordResetRepository::insert(&pool, ben, "h2")
            .await
            .unwrap();
        let second = PasswordResetRepository::insert(&pool, ana, "h3")
            .await
            .unwrap();

        let live = |id| PasswordResetRepository::find_live(&pool, id);
        assert!(live(first).await.unwrap().is_none());
        assert!(live(second).await.unwrap().is_some());
        assert!(
            live(other).await.unwrap().is_some(),
            "another account's link stays"
        );
    }

    #[tokio::test]
    async fn only_the_last_hour_is_counted() {
        let pool = test_pool().await;
        let user_id = member(&pool, "ana@example.com").await;
        let old = PasswordResetRepository::insert(&pool, user_id, "h")
            .await
            .unwrap();
        age(&pool, old, "-2 hours", "-1 hour").await;
        PasswordResetRepository::insert(&pool, user_id, "h")
            .await
            .unwrap();
        PasswordResetRepository::insert(&pool, user_id, "h")
            .await
            .unwrap();

        let sent = PasswordResetRepository::sent_in_last_hour(&pool, user_id)
            .await
            .unwrap();
        assert_eq!(sent, 2);
    }

    #[tokio::test]
    async fn links_are_forgotten_once_used_or_a_day_old() {
        let pool = test_pool().await;
        let ana = member(&pool, "ana@example.com").await;
        let ben = member(&pool, "ben@example.com").await;
        let stale = PasswordResetRepository::insert(&pool, ben, "h")
            .await
            .unwrap();
        age(&pool, stale, "-25 hours", "-24 hours").await;
        let recent = PasswordResetRepository::insert(&pool, ben, "h")
            .await
            .unwrap();
        PasswordResetRepository::insert(&pool, ana, "h")
            .await
            .unwrap();

        let used = |id| PasswordResetRepository::delete_for_user(&pool, id);
        assert!(used(ana).await.unwrap());
        assert!(!used(ana).await.unwrap(), "a link is used up once");
        PasswordResetRepository::delete_stale(&pool).await.unwrap();

        let left: Vec<i64> = sqlx::query_scalar("SELECT id FROM password_resets")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(left, [recent]);
    }

    #[tokio::test]
    async fn deleting_an_account_deletes_its_links() {
        let pool = test_pool().await;
        let user_id = member(&pool, "ana@example.com").await;
        PasswordResetRepository::insert(&pool, user_id, "h")
            .await
            .unwrap();

        sqlx::query("DELETE FROM users WHERE id = ?")
            .bind(user_id)
            .execute(&pool)
            .await
            .unwrap();

        let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM password_resets")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(left, 0);
    }
}
