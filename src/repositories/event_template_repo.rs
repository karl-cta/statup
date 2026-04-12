//! Event template repository - database queries for event templates.

use crate::db::DbPool;
use crate::models::{EventTemplate, EventType, Impact};

/// Encapsulates all event template database queries.
pub struct EventTemplateRepository;

impl EventTemplateRepository {
    /// Create a new event template.
    pub async fn create(
        pool: &DbPool,
        title: &str,
        description: &str,
        event_type: EventType,
        impact: Impact,
        icon_id: Option<i64>,
        created_by: i64,
    ) -> Result<EventTemplate, sqlx::Error> {
        sqlx::query_as::<_, EventTemplate>(
            "INSERT INTO event_templates (title, description, event_type, impact, icon_id, created_by) \
             VALUES (?, ?, ?, ?, ?, ?) \
             RETURNING *",
        )
        .bind(title)
        .bind(description)
        .bind(event_type)
        .bind(impact)
        .bind(icon_id)
        .bind(created_by)
        .fetch_one(pool)
        .await
    }

    /// Find a template by ID.
    pub async fn find_by_id(pool: &DbPool, id: i64) -> Result<Option<EventTemplate>, sqlx::Error> {
        sqlx::query_as::<_, EventTemplate>("SELECT * FROM event_templates WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
    }

    /// List all templates ordered by usage count (most used first).
    pub async fn list_all(pool: &DbPool) -> Result<Vec<EventTemplate>, sqlx::Error> {
        sqlx::query_as::<_, EventTemplate>(
            "SELECT * FROM event_templates ORDER BY usage_count DESC, created_at DESC",
        )
        .fetch_all(pool)
        .await
    }

    /// Search templates by title prefix (for autocomplete).
    pub async fn search_by_title(
        pool: &DbPool,
        query: &str,
        limit: i64,
    ) -> Result<Vec<EventTemplate>, sqlx::Error> {
        let pattern = format!("%{query}%");
        sqlx::query_as::<_, EventTemplate>(
            "SELECT * FROM event_templates \
             WHERE title LIKE ? \
             ORDER BY usage_count DESC, created_at DESC \
             LIMIT ?",
        )
        .bind(pattern)
        .bind(limit)
        .fetch_all(pool)
        .await
    }

    /// Increment usage count and update `last_used_at`.
    pub async fn increment_usage(pool: &DbPool, id: i64) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE event_templates SET usage_count = usage_count + 1, \
             last_used_at = datetime('now') WHERE id = ?",
        )
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Delete a template by ID.
    pub async fn delete(pool: &DbPool, id: i64) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM event_templates WHERE id = ?")
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
        let user = UserRepository::create(pool, "tpl@test.com", "hash", "Tester", Role::Publisher)
            .await
            .unwrap();
        user.id
    }

    #[tokio::test]
    async fn create_and_find_by_id() {
        let pool = test_pool().await;
        let uid = seed_user(&pool).await;

        let tpl = EventTemplateRepository::create(
            &pool,
            "Mise à jour ERP",
            "Mise à jour du logiciel ERP",
            EventType::MaintenanceScheduled,
            Impact::Minor,
            None,
            uid,
        )
        .await
        .unwrap();

        assert_eq!(tpl.title, "Mise à jour ERP");
        assert_eq!(tpl.usage_count, 0);

        let found = EventTemplateRepository::find_by_id(&pool, tpl.id)
            .await
            .unwrap();
        assert!(found.is_some());
    }

    #[tokio::test]
    async fn search_by_title() {
        let pool = test_pool().await;
        let uid = seed_user(&pool).await;

        EventTemplateRepository::create(
            &pool,
            "Mise à jour ERP",
            "desc",
            EventType::MaintenanceScheduled,
            Impact::Minor,
            None,
            uid,
        )
        .await
        .unwrap();

        EventTemplateRepository::create(
            &pool,
            "Incident réseau",
            "desc",
            EventType::Incident,
            Impact::Major,
            None,
            uid,
        )
        .await
        .unwrap();

        let results = EventTemplateRepository::search_by_title(&pool, "ERP", 10)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Mise à jour ERP");
    }

    #[tokio::test]
    async fn increment_usage() {
        let pool = test_pool().await;
        let uid = seed_user(&pool).await;

        let tpl = EventTemplateRepository::create(
            &pool,
            "Test",
            "desc",
            EventType::Incident,
            Impact::Minor,
            None,
            uid,
        )
        .await
        .unwrap();

        EventTemplateRepository::increment_usage(&pool, tpl.id)
            .await
            .unwrap();

        let updated = EventTemplateRepository::find_by_id(&pool, tpl.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.usage_count, 1);
        assert!(updated.last_used_at.is_some());
    }

    #[tokio::test]
    async fn delete_template() {
        let pool = test_pool().await;
        let uid = seed_user(&pool).await;

        let tpl = EventTemplateRepository::create(
            &pool,
            "Del",
            "desc",
            EventType::Info,
            Impact::None,
            None,
            uid,
        )
        .await
        .unwrap();

        EventTemplateRepository::delete(&pool, tpl.id)
            .await
            .unwrap();

        let found = EventTemplateRepository::find_by_id(&pool, tpl.id)
            .await
            .unwrap();
        assert!(found.is_none());
    }
}
