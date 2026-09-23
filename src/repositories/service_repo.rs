//! Service repository: SQL queries on services.

use crate::db::DbPool;
use crate::models::{Service, ServiceStatus};

pub struct ServiceRepository;

const WITH_ICON: &str = "SELECT s.*, i.filename AS icon_filename, \
     EXISTS(SELECT 1 FROM event_services es WHERE es.service_id = s.id) AS has_history \
     FROM services s LEFT JOIN icons i ON i.id = s.icon_id";

impl ServiceRepository {
    pub async fn create(
        pool: &DbPool,
        name: &str,
        slug: &str,
        description: Option<&str>,
        icon_id: Option<i64>,
        icon_name: Option<&str>,
    ) -> Result<Service, sqlx::Error> {
        sqlx::query_as::<_, Service>(
            "INSERT INTO services (name, slug, description, icon_id, icon_name) \
             VALUES (?, ?, ?, ?, ?) RETURNING *",
        )
        .bind(name)
        .bind(slug)
        .bind(description)
        .bind(icon_id)
        .bind(icon_name)
        .fetch_one(pool)
        .await
    }

    pub async fn find_by_id(pool: &DbPool, id: i64) -> Result<Option<Service>, sqlx::Error> {
        sqlx::query_as::<_, Service>(&format!("{WITH_ICON} WHERE s.id = ?"))
            .bind(id)
            .fetch_optional(pool)
            .await
    }

    pub async fn slug_exists(pool: &DbPool, slug: &str) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM services WHERE slug = ?)")
            .bind(slug)
            .fetch_one(pool)
            .await
    }

    /// Whether the instance watches anything yet.
    pub async fn any(pool: &DbPool) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM services)")
            .fetch_one(pool)
            .await
    }

    /// Every service with its uploaded icon, by name.
    pub async fn list_all(pool: &DbPool) -> Result<Vec<Service>, sqlx::Error> {
        sqlx::query_as::<_, Service>(&format!("{WITH_ICON} ORDER BY s.name COLLATE NOCASE ASC"))
            .fetch_all(pool)
            .await
    }

    pub async fn update(
        pool: &DbPool,
        id: i64,
        name: &str,
        description: Option<&str>,
        icon_id: Option<i64>,
        icon_name: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE services SET name = ?, description = ?, icon_id = ?, icon_name = ? \
             WHERE id = ?",
        )
        .bind(name)
        .bind(description)
        .bind(icon_id)
        .bind(icon_name)
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Writes the status only when it changes, so an unchanged service keeps
    /// its last update time.
    pub async fn update_status(
        pool: &DbPool,
        id: i64,
        status: ServiceStatus,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE services SET status = ? WHERE id = ? AND status != ?")
            .bind(status)
            .bind(id)
            .bind(status)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Records the state set by hand; the shown one follows from it and the
    /// open events.
    pub async fn update_manual_status(
        pool: &DbPool,
        id: i64,
        status: ServiceStatus,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE services SET manual_status = ? WHERE id = ?")
            .bind(status)
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn has_events(pool: &DbPool, service_id: i64) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM event_services WHERE service_id = ?)")
            .bind(service_id)
            .fetch_one(pool)
            .await
    }

    pub async fn delete(pool: &DbPool, id: i64) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM services WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::test_pool;

    async fn create(pool: &DbPool, name: &str, slug: &str) -> Service {
        ServiceRepository::create(pool, name, slug, None, None, None)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn create_and_find_by_id() {
        let pool = test_pool().await;
        let svc = ServiceRepository::create(&pool, "API", "api", Some("The API"), None, None)
            .await
            .unwrap();
        assert_eq!(svc.status, ServiceStatus::Operational);

        let found = ServiceRepository::find_by_id(&pool, svc.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.name, "API");
        assert_eq!(found.description.as_deref(), Some("The API"));
    }

    #[tokio::test]
    async fn slug_lookup() {
        let pool = test_pool().await;
        create(&pool, "Web", "web-app").await;
        assert!(
            ServiceRepository::slug_exists(&pool, "web-app")
                .await
                .unwrap()
        );
        assert!(!ServiceRepository::slug_exists(&pool, "nope").await.unwrap());
    }

    #[tokio::test]
    async fn list_all_ignores_case() {
        let pool = test_pool().await;
        create(&pool, "zzz", "zzz").await;
        create(&pool, "Aaa", "aaa").await;
        create(&pool, "Mmm", "mmm").await;
        let names: Vec<String> = ServiceRepository::list_all(&pool)
            .await
            .unwrap()
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, vec!["Aaa", "Mmm", "zzz"]);
    }

    #[tokio::test]
    async fn update_and_status() {
        let pool = test_pool().await;
        let svc = create(&pool, "Old", "old").await;
        ServiceRepository::update(&pool, svc.id, "New", Some("desc"), None, None)
            .await
            .unwrap();
        ServiceRepository::update_status(&pool, svc.id, ServiceStatus::MajorOutage)
            .await
            .unwrap();
        let updated = ServiceRepository::find_by_id(&pool, svc.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.name, "New");
        assert_eq!(updated.status, ServiceStatus::MajorOutage);
    }

    #[tokio::test]
    async fn delete_and_history() {
        let pool = test_pool().await;
        let svc = create(&pool, "Del", "del").await;
        assert!(!ServiceRepository::has_events(&pool, svc.id).await.unwrap());
        ServiceRepository::delete(&pool, svc.id).await.unwrap();
        assert!(
            ServiceRepository::find_by_id(&pool, svc.id)
                .await
                .unwrap()
                .is_none()
        );
    }
}
