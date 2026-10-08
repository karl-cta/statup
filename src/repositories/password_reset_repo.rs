//! The links sent to a member who forgot their password.

use super::UserRepository;
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
    /// Store a new link for an account and return its id, unless the
    /// account had `per_hour` links in the last hour. Counting and storing
    /// are one statement, so requests at the same moment cannot pass the
    /// limit together.
    pub async fn insert_within_limit(
        pool: &DbPool,
        user_id: i64,
        secret_hash: &str,
        per_hour: i64,
    ) -> Result<Option<i64>, sqlx::Error> {
        sqlx::query_scalar(
            "INSERT INTO password_resets (user_id, secret_hash, expires_at) \
             SELECT ?, ?, datetime('now', ?) \
             WHERE (SELECT COUNT(*) FROM password_resets \
                    WHERE user_id = ? AND created_at > datetime('now', '-1 hour')) < ? \
             RETURNING id",
        )
        .bind(user_id)
        .bind(secret_hash)
        .bind(LINK_LIFETIME)
        .bind(user_id)
        .bind(per_hour)
        .fetch_optional(pool)
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

    /// Uses up every link of the account and sets its new password, both or
    /// neither. Returns false, and changes nothing, when the account has no
    /// link left: of two uses of one link at the same moment, only the
    /// first finds it.
    pub async fn use_up(
        pool: &DbPool,
        user_id: i64,
        password_hash: &str,
    ) -> Result<bool, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let deleted = sqlx::query("DELETE FROM password_resets WHERE user_id = ?")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        if deleted.rows_affected() == 0 {
            return Ok(false);
        }
        UserRepository::update_password(&mut *tx, user_id, password_hash).await?;
        tx.commit().await?;
        Ok(true)
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
    use crate::test_helpers::test_pool;

    const NO_LIMIT: i64 = 100;

    async fn member(pool: &DbPool, email: &str) -> i64 {
        UserRepository::create(pool, email, "hash", "Someone", Role::Reader)
            .await
            .unwrap()
            .id
    }

    async fn insert(pool: &DbPool, user_id: i64, secret_hash: &str) -> i64 {
        PasswordResetRepository::insert_within_limit(pool, user_id, secret_hash, NO_LIMIT)
            .await
            .unwrap()
            .unwrap()
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

    async fn links(pool: &DbPool) -> Vec<i64> {
        sqlx::query_scalar("SELECT id FROM password_resets ORDER BY id")
            .fetch_all(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_new_link_opens_its_account() {
        let pool = test_pool().await;
        let user_id = member(&pool, "ana@example.com").await;

        let id = insert(&pool, user_id, "secret-hash").await;
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
        let id = insert(&pool, user_id, "h").await;

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
        let first = insert(&pool, ana, "h1").await;
        let other = insert(&pool, ben, "h2").await;
        let second = insert(&pool, ana, "h3").await;

        let live = |id| PasswordResetRepository::find_live(&pool, id);
        assert!(live(first).await.unwrap().is_none());
        assert!(live(second).await.unwrap().is_some());
        assert!(
            live(other).await.unwrap().is_some(),
            "another account's link stays"
        );
    }

    #[tokio::test]
    async fn the_limit_counts_the_last_hour_of_one_account() {
        let pool = test_pool().await;
        let ana = member(&pool, "ana@example.com").await;
        let ben = member(&pool, "ben@example.com").await;
        let old = insert(&pool, ana, "h").await;
        age(&pool, old, "-2 hours", "-1 hour").await;
        insert(&pool, ben, "h").await;
        let within = |user_id| PasswordResetRepository::insert_within_limit(&pool, user_id, "h", 2);

        assert!(within(ana).await.unwrap().is_some());
        assert!(within(ana).await.unwrap().is_some());
        assert!(within(ana).await.unwrap().is_none());
        assert!(within(ben).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn a_link_is_used_up_once_with_the_new_password() {
        let pool = test_pool().await;
        let ana = member(&pool, "ana@example.com").await;
        let ben = member(&pool, "ben@example.com").await;
        insert(&pool, ana, "h").await;
        insert(&pool, ana, "h").await;
        let bens = insert(&pool, ben, "h").await;

        let used = |id| PasswordResetRepository::use_up(&pool, id, "new-hash");
        assert!(used(ana).await.unwrap());
        assert!(!used(ana).await.unwrap(), "a link is used up once");

        let stored = UserRepository::find_by_id(&pool, ana)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.password_hash, "new-hash");
        assert_eq!(links(&pool).await, [bens]);
    }

    #[tokio::test]
    async fn without_a_link_the_password_stays() {
        let pool = test_pool().await;
        let ana = member(&pool, "ana@example.com").await;

        let used = PasswordResetRepository::use_up(&pool, ana, "new-hash")
            .await
            .unwrap();

        assert!(!used);
        let stored = UserRepository::find_by_id(&pool, ana)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.password_hash, "hash");
    }

    #[tokio::test]
    async fn links_a_day_old_are_forgotten() {
        let pool = test_pool().await;
        let ben = member(&pool, "ben@example.com").await;
        let stale = insert(&pool, ben, "h").await;
        age(&pool, stale, "-25 hours", "-24 hours").await;
        let recent = insert(&pool, ben, "h").await;

        PasswordResetRepository::delete_stale(&pool).await.unwrap();

        assert_eq!(links(&pool).await, [recent]);
    }

    #[tokio::test]
    async fn deleting_an_account_deletes_its_links() {
        let pool = test_pool().await;
        let user_id = member(&pool, "ana@example.com").await;
        insert(&pool, user_id, "h").await;

        sqlx::query("DELETE FROM users WHERE id = ?")
            .bind(user_id)
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(links(&pool).await, Vec::<i64>::new());
    }
}
