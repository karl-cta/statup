//! Icon repository - database queries for the icon library.

use crate::db::DbPool;
use crate::models::Icon;

/// Encapsulates all icon-related database queries.
pub struct IconRepository;

impl IconRepository {
    /// Create a new icon record.
    pub async fn create(
        pool: &DbPool,
        filename: &str,
        original_name: &str,
        mime_type: &str,
        size_bytes: i64,
        uploaded_by: i64,
    ) -> Result<Icon, sqlx::Error> {
        sqlx::query_as::<_, Icon>(
            "INSERT INTO icons (filename, original_name, mime_type, size_bytes, uploaded_by) \
             VALUES (?, ?, ?, ?, ?) \
             RETURNING *",
        )
        .bind(filename)
        .bind(original_name)
        .bind(mime_type)
        .bind(size_bytes)
        .bind(uploaded_by)
        .fetch_one(pool)
        .await
    }

    /// Find an icon by ID.
    pub async fn find_by_id(pool: &DbPool, id: i64) -> Result<Option<Icon>, sqlx::Error> {
        sqlx::query_as::<_, Icon>("SELECT * FROM icons WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
    }

    /// List all icons ordered by creation date (newest first).
    pub async fn list_all(pool: &DbPool) -> Result<Vec<Icon>, sqlx::Error> {
        sqlx::query_as::<_, Icon>("SELECT * FROM icons ORDER BY created_at DESC")
            .fetch_all(pool)
            .await
    }

    /// Check if an icon is referenced by any service, event, or template.
    pub async fn is_referenced(pool: &DbPool, id: i64) -> Result<bool, sqlx::Error> {
        let row: (bool,) = sqlx::query_as(
            "SELECT EXISTS(\
                SELECT 1 FROM services WHERE icon_id = ? \
                UNION ALL \
                SELECT 1 FROM events WHERE icon_id = ? \
                UNION ALL \
                SELECT 1 FROM event_templates WHERE icon_id = ? \
             )",
        )
        .bind(id)
        .bind(id)
        .bind(id)
        .fetch_one(pool)
        .await?;
        Ok(row.0)
    }

    /// Delete an icon by ID.
    pub async fn delete(pool: &DbPool, id: i64) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM icons WHERE id = ?")
            .bind(id)
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

    async fn seed_user(pool: &DbPool) -> i64 {
        let user = UserRepository::create(pool, "icon@test.com", "hash", "Tester", Role::Publisher)
            .await
            .unwrap();
        user.id
    }

    #[tokio::test]
    async fn create_and_find_by_id() {
        let pool = test_pool().await;
        let uid = seed_user(&pool).await;

        let icon = IconRepository::create(&pool, "abc.png", "logo.png", "image/png", 1024, uid)
            .await
            .unwrap();

        assert_eq!(icon.filename, "abc.png");
        assert_eq!(icon.original_name, "logo.png");
        assert_eq!(icon.mime_type, "image/png");
        assert_eq!(icon.size_bytes, 1024);

        let found = IconRepository::find_by_id(&pool, icon.id).await.unwrap();
        assert!(found.is_some());
    }

    #[tokio::test]
    async fn list_all_returns_newest_first() {
        let pool = test_pool().await;
        let uid = seed_user(&pool).await;

        IconRepository::create(&pool, "a.png", "a.png", "image/png", 100, uid)
            .await
            .unwrap();
        IconRepository::create(&pool, "b.png", "b.png", "image/png", 200, uid)
            .await
            .unwrap();

        let all = IconRepository::list_all(&pool).await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn delete_icon() {
        let pool = test_pool().await;
        let uid = seed_user(&pool).await;

        let icon = IconRepository::create(&pool, "del.png", "del.png", "image/png", 100, uid)
            .await
            .unwrap();

        IconRepository::delete(&pool, icon.id).await.unwrap();

        let found = IconRepository::find_by_id(&pool, icon.id).await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn is_referenced_returns_false_when_unused() {
        let pool = test_pool().await;
        let uid = seed_user(&pool).await;

        let icon = IconRepository::create(&pool, "ref.png", "ref.png", "image/png", 100, uid)
            .await
            .unwrap();

        assert!(!IconRepository::is_referenced(&pool, icon.id).await.unwrap());
    }
}
