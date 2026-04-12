//! User repository - database queries for users.
//!
//! All methods return `sqlx::Error` on database failure.

use crate::db::DbPool;
use crate::models::{Role, User};

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
        let role_str = match role {
            Role::Reader => "reader",
            Role::Publisher => "publisher",
            Role::Admin => "admin",
        };

        sqlx::query_as::<_, User>(
            "INSERT INTO users (email, password_hash, display_name, role) \
             VALUES (?, ?, ?, ?) \
             RETURNING *",
        )
        .bind(email)
        .bind(password_hash)
        .bind(display_name)
        .bind(role_str)
        .fetch_one(pool)
        .await
    }

    /// Find a user by email address.
    pub async fn find_by_email(pool: &DbPool, email: &str) -> Result<Option<User>, sqlx::Error> {
        sqlx::query_as::<_, User>("SELECT * FROM users WHERE email = ? AND is_active = 1")
            .bind(email)
            .fetch_optional(pool)
            .await
    }

    /// Find a user by ID.
    pub async fn find_by_id(pool: &DbPool, id: i64) -> Result<Option<User>, sqlx::Error> {
        sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
    }

    /// Update the `last_seen_at` timestamp for a user.
    pub async fn update_last_seen(pool: &DbPool, user_id: i64) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE users SET last_seen_at = datetime('now') WHERE id = ?")
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

    /// Update the role of a user.
    pub async fn update_role(pool: &DbPool, user_id: i64, role: Role) -> Result<(), sqlx::Error> {
        let role_str = match role {
            Role::Reader => "reader",
            Role::Publisher => "publisher",
            Role::Admin => "admin",
        };

        sqlx::query("UPDATE users SET role = ? WHERE id = ?")
            .bind(role_str)
            .bind(user_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Set the `is_active` flag for a user.
    pub async fn set_active(
        pool: &DbPool,
        user_id: i64,
        is_active: bool,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE users SET is_active = ? WHERE id = ?")
            .bind(is_active)
            .bind(user_id)
            .execute(pool)
            .await?;
        Ok(())
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

    /// Update a user's password hash.
    pub async fn update_password(
        pool: &DbPool,
        user_id: i64,
        password_hash: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
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
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users WHERE email = ? AND id != ?")
            .bind(email)
            .bind(exclude_user_id)
            .fetch_one(pool)
            .await?;
        Ok(row.0 > 0)
    }

    /// Count total users in the database.
    pub async fn count_all(pool: &DbPool) -> Result<i64, sqlx::Error> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(pool)
            .await?;
        Ok(row.0)
    }

    /// Count users with the admin role.
    pub async fn count_admins(pool: &DbPool) -> Result<i64, sqlx::Error> {
        let row: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM users WHERE role = 'admin' AND is_active = 1")
                .fetch_one(pool)
                .await?;
        Ok(row.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::test_pool;

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

        let found = UserRepository::find_by_email(&pool, "a@b.com")
            .await
            .unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, user.id);
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

        UserRepository::update_role(&pool, user.id, Role::Publisher)
            .await
            .unwrap();

        let updated = UserRepository::find_by_id(&pool, user.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.role, Role::Publisher);
    }

    #[tokio::test]
    async fn set_active_disables_user() {
        let pool = test_pool().await;
        let user = UserRepository::create(&pool, "d@d.com", "hash", "D", Role::Reader)
            .await
            .unwrap();

        UserRepository::set_active(&pool, user.id, false)
            .await
            .unwrap();

        // find_by_email filters is_active=1, so disabled user should not be found
        let found = UserRepository::find_by_email(&pool, "d@d.com")
            .await
            .unwrap();
        assert!(found.is_none());

        // but find_by_id does not filter
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

    #[tokio::test]
    async fn update_last_seen() {
        let pool = test_pool().await;
        let user = UserRepository::create(&pool, "t@t.com", "h", "T", Role::Reader)
            .await
            .unwrap();

        assert!(user.last_seen_at.is_none());

        UserRepository::update_last_seen(&pool, user.id)
            .await
            .unwrap();

        let updated = UserRepository::find_by_id(&pool, user.id)
            .await
            .unwrap()
            .unwrap();
        assert!(updated.last_seen_at.is_some());
    }
}
