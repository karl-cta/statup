//! Event template repository: SQL queries on event presets.

use crate::db::DbPool;
use crate::models::{Category, EventTemplate, Kind, Severity};

pub struct EventTemplateRepository;

pub struct CreateTemplateInput<'a> {
    pub title: &'a str,
    pub description: &'a str,
    pub kind: Kind,
    pub severity: Option<Severity>,
    pub planned: bool,
    pub category: Option<Category>,
    pub icon_id: Option<i64>,
    pub created_by: i64,
}

impl EventTemplateRepository {
    pub async fn create(
        pool: &DbPool,
        input: CreateTemplateInput<'_>,
    ) -> Result<EventTemplate, sqlx::Error> {
        sqlx::query_as::<_, EventTemplate>(
            "INSERT INTO event_templates (title, description, kind, severity, planned, category, icon_id, created_by) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
             RETURNING *",
        )
        .bind(input.title)
        .bind(input.description)
        .bind(input.kind)
        .bind(input.severity)
        .bind(input.planned)
        .bind(input.category)
        .bind(input.icon_id)
        .bind(input.created_by)
        .fetch_one(pool)
        .await
    }

    pub async fn find_by_id(pool: &DbPool, id: i64) -> Result<Option<EventTemplate>, sqlx::Error> {
        sqlx::query_as::<_, EventTemplate>("SELECT * FROM event_templates WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
    }

    pub async fn list_all(pool: &DbPool) -> Result<Vec<EventTemplate>, sqlx::Error> {
        sqlx::query_as::<_, EventTemplate>(
            "SELECT * FROM event_templates ORDER BY usage_count DESC, created_at DESC",
        )
        .fetch_all(pool)
        .await
    }

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
            CreateTemplateInput {
                title: "Mise à jour ERP",
                description: "Mise à jour du logiciel ERP",
                kind: Kind::Maintenance,
                severity: Some(Severity::Minor),
                planned: true,
                category: None,
                icon_id: None,
                created_by: uid,
            },
        )
        .await
        .unwrap();

        assert_eq!(tpl.title, "Mise à jour ERP");
        assert_eq!(tpl.usage_count, 0);
        assert!(tpl.planned);

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
            CreateTemplateInput {
                title: "Mise à jour ERP",
                description: "desc",
                kind: Kind::Maintenance,
                severity: Some(Severity::Minor),
                planned: true,
                category: None,
                icon_id: None,
                created_by: uid,
            },
        )
        .await
        .unwrap();

        EventTemplateRepository::create(
            &pool,
            CreateTemplateInput {
                title: "Incident réseau",
                description: "desc",
                kind: Kind::Incident,
                severity: Some(Severity::Major),
                planned: false,
                category: None,
                icon_id: None,
                created_by: uid,
            },
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
            CreateTemplateInput {
                title: "Test",
                description: "desc",
                kind: Kind::Incident,
                severity: Some(Severity::Minor),
                planned: false,
                category: None,
                icon_id: None,
                created_by: uid,
            },
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
            CreateTemplateInput {
                title: "Del",
                description: "desc",
                kind: Kind::Publication,
                severity: None,
                planned: false,
                category: Some(Category::Info),
                icon_id: None,
                created_by: uid,
            },
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
