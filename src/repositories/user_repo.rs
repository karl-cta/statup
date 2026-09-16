//! User repository: database queries for accounts.
//!
//! All methods return `sqlx::Error` on database failure. Email comparisons
//! ignore case: the column is declared `COLLATE NOCASE`.

use crate::db::DbPool;
use crate::models::{Role, User};

/// An account to insert. The password is already hashed.
pub struct NewUser<'a> {
    pub email: &'a str,
    pub password_hash: &'a str,
    pub display_name: &'a str,
    pub role: Role,
    pub must_change_password: bool,
}

/// Encapsulates all user-related database queries.
pub struct UserRepository;

impl UserRepository {
    /// Create a new user and return the created record.
    pub async fn create(
        pool: &DbPool,
        email: &str,
        password_hash: &str,
        display_name: &str,
        role: Role,
    ) -> Result<User, sqlx::Error> {
        let user = NewUser {
            email,
            password_hash,
            display_name,
            role,
            must_change_password: false,
        };
        Self::insert(pool, &user).await
    }

    /// Insert an account and return the created record.
    pub async fn insert(pool: &DbPool, user: &NewUser<'_>) -> Result<User, sqlx::Error> {
        sqlx::query_as::<_, User>(
            "INSERT INTO users (email, password_hash, display_name, role, must_change_password) \
             VALUES (?, ?, ?, ?, ?) \
             RETURNING *",
        )
        .bind(user.email)
        .bind(user.password_hash)
        .bind(user.display_name)
        .bind(user.role)
        .bind(user.must_change_password)
        .fetch_one(pool)
        .await
    }

    /// Insert the account only while the instance has none, in a single
    /// statement, so two simultaneous first sign-ups cannot both succeed.
    /// Returns `None` when an account already exists.
    pub async fn insert_first(
        pool: &DbPool,
        user: &NewUser<'_>,
    ) -> Result<Option<User>, sqlx::Error> {
        sqlx::query_as::<_, User>(
            "INSERT INTO users (email, password_hash, display_name, role, must_change_password) \
             SELECT ?, ?, ?, ?, ? WHERE NOT EXISTS (SELECT 1 FROM users) \
             RETURNING *",
        )
        .bind(user.email)
        .bind(user.password_hash)
        .bind(user.display_name)
        .bind(user.role)
        .bind(user.must_change_password)
        .fetch_optional(pool)
        .await
    }

    /// Find an active user by email address. A disabled account is not found.
    pub async fn find_by_email(pool: &DbPool, email: &str) -> Result<Option<User>, sqlx::Error> {
        sqlx::query_as::<_, User>("SELECT * FROM users WHERE email = ? AND is_active = 1")
            .bind(email)
            .fetch_optional(pool)
            .await
    }

    /// Whether any account, disabled ones included, uses this email.
    pub async fn email_exists(pool: &DbPool, email: &str) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users WHERE email = ?)")
            .bind(email)
            .fetch_one(pool)
            .await
    }

    /// Find a user by ID.
    pub async fn find_by_id(pool: &DbPool, id: i64) -> Result<Option<User>, sqlx::Error> {
        sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
    }

    /// Record a visit. Written at most every five minutes per account, so
    /// browsing does not take the write lock on every page.
    pub async fn update_last_seen(pool: &DbPool, user_id: i64) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE users SET last_seen_at = datetime('now') \
             WHERE id = ? \
             AND COALESCE(datetime(last_seen_at), '') < datetime('now', '-5 minutes')",
        )
        .bind(user_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// List all users ordered by creation date.
    pub async fn list_all(pool: &DbPool) -> Result<Vec<User>, sqlx::Error> {
        sqlx::query_as::<_, User>("SELECT * FROM users ORDER BY created_at DESC")
            .fetch_all(pool)
            .await
    }

    /// Change a role, unless that would demote the last active admin.
    /// The check and the write are one statement, so two admins demoting
    /// each other at the same time cannot both succeed. Returns whether
    /// the role was changed.
    pub async fn update_role(pool: &DbPool, user_id: i64, role: Role) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE users SET role = ? \
             WHERE id = ? \
             AND (? = 'admin' OR role != 'admin' OR is_active = 0 \
                  OR (SELECT COUNT(*) FROM users WHERE role = 'admin' AND is_active = 1) > 1)",
        )
        .bind(role)
        .bind(user_id)
        .bind(role)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Enable or disable an account, unless that would disable the last
    /// active admin, in one statement like [`Self::update_role`]. Returns
    /// whether the account was changed.
    pub async fn set_active(
        pool: &DbPool,
        user_id: i64,
        is_active: bool,
    ) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE users SET is_active = ? \
             WHERE id = ? \
             AND (? OR role != 'admin' OR is_active = 0 \
                  OR (SELECT COUNT(*) FROM users WHERE role = 'admin' AND is_active = 1) > 1)",
        )
        .bind(is_active)
        .bind(user_id)
        .bind(is_active)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Update a user's display name and email.
    pub async fn update_profile(
        pool: &DbPool,
        user_id: i64,
        email: &str,
        display_name: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE users SET email = ?, display_name = ? WHERE id = ?")
            .bind(email)
            .bind(display_name)
            .bind(user_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Update a user's preferred UI locale. Pass `None` to clear it.
    pub async fn update_preferred_locale(
        pool: &DbPool,
        user_id: i64,
        locale: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE users SET preferred_locale = ? WHERE id = ?")
            .bind(locale)
            .bind(user_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Update a user's password hash. A password the person chose clears any
    /// pending request to replace it.
    pub async fn update_password(
        pool: &DbPool,
        user_id: i64,
        password_hash: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE users SET password_hash = ?, must_change_password = 0 WHERE id = ?")
            .bind(password_hash)
            .bind(user_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Replace the password with a temporary one the person must change.
    pub async fn set_temporary_password(
        pool: &DbPool,
        user_id: i64,
        password_hash: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE users SET password_hash = ?, must_change_password = 1 WHERE id = ?")
            .bind(password_hash)
            .bind(user_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Check if an email is already used by another user.
    pub async fn email_taken_by_other(
        pool: &DbPool,
        email: &str,
        exclude_user_id: i64,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users WHERE email = ? AND id != ?)")
            .bind(email)
            .bind(exclude_user_id)
            .fetch_one(pool)
            .await
    }

    /// Count total users in the database.
    pub async fn count_all(pool: &DbPool) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(pool)
            .await
    }

    /// Count active users with the admin role.
    pub async fn count_admins(pool: &DbPool) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role = 'admin' AND is_active = 1")
            .fetch_one(pool)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::test_pool;

    fn new_user(email: &str, role: Role) -> NewUser<'_> {
        NewUser {
            email,
            password_hash: "hash",
            display_name: "Someone",
            role,
            must_change_password: false,
        }
    }

    #[tokio::test]
    async fn create_and_find_by_email() {
        let pool = test_pool().await;

        let user = UserRepository::create(&pool, "a@b.com", "hash", "Alice", Role::Reader)
            .await
            .unwrap();

        assert_eq!(user.email, "a@b.com");
        assert_eq!(user.display_name, "Alice");
        assert_eq!(user.role, Role::Reader);
        assert!(user.is_active);
        assert!(!user.must_change_password);

        let found = UserRepository::find_by_email(&pool, "A@B.com")
            .await
            .unwrap();
        assert_eq!(found.map(|u| u.id), Some(user.id), "emails ignore case");
    }

    #[tokio::test]
    async fn find_by_email_returns_none_for_unknown() {
        let pool = test_pool().await;
        let found = UserRepository::find_by_email(&pool, "nope@nope.com")
            .await
            .unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn insert_sets_the_temporary_password_flag() {
        let pool = test_pool().await;
        let mut member = new_user("m@m.com", Role::Reader);
        member.must_change_password = true;

        let user = UserRepository::insert(&pool, &member).await.unwrap();

        assert!(user.must_change_password);
    }

    #[tokio::test]
    async fn only_the_first_account_is_inserted_by_insert_first() {
        let pool = test_pool().await;

        let first = UserRepository::insert_first(&pool, &new_user("1@a.com", Role::Admin))
            .await
            .unwrap();
        let second = UserRepository::insert_first(&pool, &new_user("2@a.com", Role::Admin))
            .await
            .unwrap();

        assert!(first.is_some());
        assert!(second.is_none());
        assert_eq!(UserRepository::count_all(&pool).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn find_by_id() {
        let pool = test_pool().await;
        let user = UserRepository::create(&pool, "x@y.com", "hash", "X", Role::Admin)
            .await
            .unwrap();

        let found = UserRepository::find_by_id(&pool, user.id).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().email, "x@y.com");
    }

    #[tokio::test]
    async fn update_role() {
        let pool = test_pool().await;
        let user = UserRepository::create(&pool, "r@r.com", "hash", "R", Role::Reader)
            .await
            .unwrap();

        assert!(
            UserRepository::update_role(&pool, user.id, Role::Publisher)
                .await
                .unwrap()
        );

        let updated = UserRepository::find_by_id(&pool, user.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.role, Role::Publisher);
    }

    #[tokio::test]
    async fn the_last_active_admin_cannot_be_demoted_or_disabled() {
        let pool = test_pool().await;
        let admin = UserRepository::insert(&pool, &new_user("a@a.com", Role::Admin))
            .await
            .unwrap();

        assert!(
            !UserRepository::update_role(&pool, admin.id, Role::Reader)
                .await
                .unwrap()
        );
        assert!(
            !UserRepository::set_active(&pool, admin.id, false)
                .await
                .unwrap()
        );

        let other = UserRepository::insert(&pool, &new_user("b@b.com", Role::Admin))
            .await
            .unwrap();
        assert!(
            UserRepository::update_role(&pool, admin.id, Role::Reader)
                .await
                .unwrap()
        );
        assert!(
            !UserRepository::set_active(&pool, other.id, false)
                .await
                .unwrap()
        );
        assert_eq!(UserRepository::count_admins(&pool).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn an_inactive_admin_can_be_demoted_while_one_admin_is_active() {
        let pool = test_pool().await;
        UserRepository::insert(&pool, &new_user("a@a.com", Role::Admin))
            .await
            .unwrap();
        let dormant = UserRepository::insert(&pool, &new_user("b@b.com", Role::Admin))
            .await
            .unwrap();
        assert!(
            UserRepository::set_active(&pool, dormant.id, false)
                .await
                .unwrap()
        );

        assert!(
            UserRepository::update_role(&pool, dormant.id, Role::Reader)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn disabled_accounts_are_hidden_from_sign_in_but_keep_their_email() {
        let pool = test_pool().await;
        let user = UserRepository::create(&pool, "d@d.com", "hash", "D", Role::Reader)
            .await
            .unwrap();

        assert!(
            UserRepository::set_active(&pool, user.id, false)
                .await
                .unwrap()
        );

        let found = UserRepository::find_by_email(&pool, "d@d.com")
            .await
            .unwrap();
        assert!(found.is_none());
        assert!(
            UserRepository::email_exists(&pool, "d@d.com")
                .await
                .unwrap()
        );

        let found = UserRepository::find_by_id(&pool, user.id)
            .await
            .unwrap()
            .unwrap();
        assert!(!found.is_active);
    }

    #[tokio::test]
    async fn count_all_and_count_admins() {
        let pool = test_pool().await;

        assert_eq!(UserRepository::count_all(&pool).await.unwrap(), 0);
        assert_eq!(UserRepository::count_admins(&pool).await.unwrap(), 0);

        UserRepository::create(&pool, "a@a.com", "h", "A", Role::Admin)
            .await
            .unwrap();
        UserRepository::create(&pool, "b@b.com", "h", "B", Role::Reader)
            .await
            .unwrap();

        assert_eq!(UserRepository::count_all(&pool).await.unwrap(), 2);
        assert_eq!(UserRepository::count_admins(&pool).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn list_all_returns_all_users() {
        let pool = test_pool().await;

        UserRepository::create(&pool, "first@a.com", "h", "First", Role::Reader)
            .await
            .unwrap();
        UserRepository::create(&pool, "second@a.com", "h", "Second", Role::Reader)
            .await
            .unwrap();

        let users = UserRepository::list_all(&pool).await.unwrap();
        assert_eq!(users.len(), 2);
    }

    /// Sets `last_seen_at` to the result of an SQL expression, then records
    /// a visit, and returns the stored value.
    async fn visit_after(pool: &DbPool, user_id: i64, previous_visit: &str) -> Option<String> {
        let set = format!("UPDATE users SET last_seen_at = {previous_visit} WHERE id = ?");
        sqlx::query(&set).bind(user_id).execute(pool).await.unwrap();
        UserRepository::update_last_seen(pool, user_id)
            .await
            .unwrap();
        sqlx::query_scalar("SELECT last_seen_at FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn last_seen_is_written_at_most_every_five_minutes() {
        let pool = test_pool().await;
        let user = UserRepository::create(&pool, "t@t.com", "h", "T", Role::Reader)
            .await
            .unwrap();
        assert!(user.last_seen_at.is_none());

        assert!(visit_after(&pool, user.id, "NULL").await.is_some());

        let old = "'2000-01-01 00:00:00'";
        let refreshed = visit_after(&pool, user.id, old).await;
        assert_ne!(refreshed.as_deref(), Some("2000-01-01 00:00:00"));

        let recent = "datetime('now', '-1 minute')";
        let kept = visit_after(&pool, user.id, recent).await;
        let expected: String = sqlx::query_scalar("SELECT datetime('now', '-1 minute')")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(
            kept.is_some_and(|seen| seen <= expected),
            "a visit a minute ago is kept"
        );
    }
}
