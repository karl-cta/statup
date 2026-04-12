//! Service repository - database queries for services.
//!
//! All methods return `sqlx::Error` on database failure.

use crate::db::DbPool;
use crate::models::{Service, ServiceStatus};

/// Encapsulates all service-related database queries.
pub struct ServiceRepository;

impl ServiceRepository {
    /// Create a new service and return the created record.
    pub async fn create(
        pool: &DbPool,
        name: &str,
        slug: &str,
        description: Option<&str>,
    ) -> Result<Service, sqlx::Error> {
        sqlx::query_as::<_, Service>(
            "INSERT INTO services (name, slug, description) \
             VALUES (?, ?, ?) \
             RETURNING *",
        )
        .bind(name)
        .bind(slug)
        .bind(description)
        .fetch_one(pool)
        .await
    }

    /// Create a new service with an icon and return the created record.
    pub async fn create_with_icon(
        pool: &DbPool,
        name: &str,
        slug: &str,
        description: Option<&str>,
        icon_id: Option<i64>,
        icon_name: Option<&str>,
    ) -> Result<Service, sqlx::Error> {
        sqlx::query_as::<_, Service>(
            "INSERT INTO services (name, slug, description, icon_id, icon_name) \
             VALUES (?, ?, ?, ?, ?) \
             RETURNING *",
        )
        .bind(name)
        .bind(slug)
        .bind(description)
        .bind(icon_id)
        .bind(icon_name)
        .fetch_one(pool)
        .await
    }

    /// Find a service by ID.
    pub async fn find_by_id(pool: &DbPool, id: i64) -> Result<Option<Service>, sqlx::Error> {
        sqlx::query_as::<_, Service>("SELECT * FROM services WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
    }

    /// Find a service by its unique slug.
    pub async fn find_by_slug(pool: &DbPool, slug: &str) -> Result<Option<Service>, sqlx::Error> {
        sqlx::query_as::<_, Service>("SELECT * FROM services WHERE slug = ?")
            .bind(slug)
            .fetch_optional(pool)
            .await
    }

    /// List all services ordered alphabetically by name.
    pub async fn list_all(pool: &DbPool) -> Result<Vec<Service>, sqlx::Error> {
        sqlx::query_as::<_, Service>("SELECT * FROM services ORDER BY name ASC")
            .fetch_all(pool)
            .await
    }

    /// List all services with their icon filenames (LEFT JOIN).
    pub async fn list_all_with_icons(pool: &DbPool) -> Result<Vec<Service>, sqlx::Error> {
        sqlx::query_as::<_, Service>(
            "SELECT s.*, i.filename AS icon_filename \
             FROM services s \
             LEFT JOIN icons i ON i.id = s.icon_id \
             ORDER BY s.name ASC",
        )
        .fetch_all(pool)
        .await
    }

    /// Find a service by ID with its icon filename.
    pub async fn find_by_id_with_icon(
        pool: &DbPool,
        id: i64,
    ) -> Result<Option<Service>, sqlx::Error> {
        sqlx::query_as::<_, Service>(
            "SELECT s.*, i.filename AS icon_filename \
             FROM services s \
             LEFT JOIN icons i ON i.id = s.icon_id \
             WHERE s.id = ?",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
    }

    /// Update a service's name, description, and icon.
    pub async fn update(
        pool: &DbPool,
        id: i64,
        name: &str,
        description: Option<&str>,
        icon_id: Option<i64>,
        icon_name: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE services SET name = ?, description = ?, icon_id = ?, icon_name = ? WHERE id = ?",
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

    /// Get the icon filename for a service (if any).
    pub async fn get_icon_filename(
        pool: &DbPool,
        icon_id: i64,
    ) -> Result<Option<String>, sqlx::Error> {
        let row: Option<(String,)> = sqlx::query_as("SELECT filename FROM icons WHERE id = ?")
            .bind(icon_id)
            .fetch_optional(pool)
            .await?;
        Ok(row.map(|r| r.0))
    }

    /// Update only the status of a service.
    pub async fn update_status(
        pool: &DbPool,
        id: i64,
        status: ServiceStatus,
    ) -> Result<(), sqlx::Error> {
        let status_str = match status {
            ServiceStatus::Operational => "operational",
            ServiceStatus::Degraded => "degraded",
            ServiceStatus::PartialOutage => "partial_outage",
            ServiceStatus::MajorOutage => "major_outage",
            ServiceStatus::Maintenance => "maintenance",
        };

        sqlx::query("UPDATE services SET status = ? WHERE id = ?")
            .bind(status_str)
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Check if a service has any associated events.
    pub async fn has_events(pool: &DbPool, service_id: i64) -> Result<bool, sqlx::Error> {
        let row: (bool,) =
            sqlx::query_as("SELECT EXISTS(SELECT 1 FROM event_services WHERE service_id = ?)")
                .bind(service_id)
                .fetch_one(pool)
                .await?;
        Ok(row.0)
    }

    /// Delete a service by ID.
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

    #[tokio::test]
    async fn create_and_find_by_id() {
        let pool = test_pool().await;

        let svc = ServiceRepository::create(&pool, "API", "api", Some("The API"))
            .await
            .unwrap();

        assert_eq!(svc.name, "API");
        assert_eq!(svc.slug, "api");
        assert_eq!(svc.description.as_deref(), Some("The API"));
        assert_eq!(svc.status, ServiceStatus::Operational);

        let found = ServiceRepository::find_by_id(&pool, svc.id).await.unwrap();
        assert!(found.is_some());
    }

    #[tokio::test]
    async fn find_by_slug() {
        let pool = test_pool().await;
        ServiceRepository::create(&pool, "Web", "web-app", None)
            .await
            .unwrap();

        let found = ServiceRepository::find_by_slug(&pool, "web-app")
            .await
            .unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "Web");

        let missing = ServiceRepository::find_by_slug(&pool, "nope")
            .await
            .unwrap();
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn list_all_ordered_alphabetically() {
        let pool = test_pool().await;
        ServiceRepository::create(&pool, "Zzz", "zzz", None)
            .await
            .unwrap();
        ServiceRepository::create(&pool, "Aaa", "aaa", None)
            .await
            .unwrap();
        ServiceRepository::create(&pool, "Mmm", "mmm", None)
            .await
            .unwrap();

        let all = ServiceRepository::list_all(&pool).await.unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].name, "Aaa");
        assert_eq!(all[1].name, "Mmm");
        assert_eq!(all[2].name, "Zzz");
    }

    #[tokio::test]
    async fn update_service() {
        let pool = test_pool().await;
        let svc = ServiceRepository::create(&pool, "Old", "old", None)
            .await
            .unwrap();

        ServiceRepository::update(&pool, svc.id, "New", Some("desc"), None, None)
            .await
            .unwrap();

        let updated = ServiceRepository::find_by_id(&pool, svc.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.name, "New");
        assert_eq!(updated.description.as_deref(), Some("desc"));
    }

    #[tokio::test]
    async fn update_status() {
        let pool = test_pool().await;
        let svc = ServiceRepository::create(&pool, "S", "s", None)
            .await
            .unwrap();

        ServiceRepository::update_status(&pool, svc.id, ServiceStatus::MajorOutage)
            .await
            .unwrap();

        let updated = ServiceRepository::find_by_id(&pool, svc.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, ServiceStatus::MajorOutage);
    }

    #[tokio::test]
    async fn delete_service() {
        let pool = test_pool().await;
        let svc = ServiceRepository::create(&pool, "Del", "del", None)
            .await
            .unwrap();

        ServiceRepository::delete(&pool, svc.id).await.unwrap();

        let found = ServiceRepository::find_by_id(&pool, svc.id).await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn has_events_returns_false_when_no_events() {
        let pool = test_pool().await;
        let svc = ServiceRepository::create(&pool, "X", "x", None)
            .await
            .unwrap();

        assert!(!ServiceRepository::has_events(&pool, svc.id).await.unwrap());
    }
}
