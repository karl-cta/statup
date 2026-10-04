//! Service repository: SQL queries on services.

use chrono::{DateTime, Utc};

use super::outage_repo::{close_outage, open_outage};
use crate::db::DbPool;
use crate::models::{CheckedService, Service, ServiceCheck, ServiceStatus};

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

    /// The services the checks watch, by id.
    pub async fn list_checked(pool: &DbPool) -> Result<Vec<CheckedService>, sqlx::Error> {
        sqlx::query_as::<_, CheckedService>(
            "SELECT id, check_kind, check_target, check_internal_cert, detected_status \
             FROM services WHERE check_kind IS NOT NULL ORDER BY id",
        )
        .fetch_all(pool)
        .await
    }

    /// Saves what to check. A changed check drops what the old one detected
    /// and ends its outage, so fixing a wrong address clears a false outage.
    /// Returns whether a detected state was dropped, which changes the
    /// status to show.
    pub async fn set_check(
        pool: &DbPool,
        id: i64,
        check: Option<&ServiceCheck>,
        now: DateTime<Utc>,
    ) -> Result<bool, sqlx::Error> {
        let kind = check.map(|c| c.kind);
        let target = check.map(|c| c.target.as_str());
        let internal_cert = check.is_some_and(|c| c.internal_cert);
        let mut tx = pool.begin().await?;
        let current: Option<(bool, bool)> = sqlx::query_as(
            "SELECT check_kind IS ? AND check_target IS ? AND check_internal_cert = ?, \
                    detected_status IS NOT NULL \
             FROM services WHERE id = ?",
        )
        .bind(kind)
        .bind(target)
        .bind(internal_cert)
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        // Nothing to save for a missing service or an unchanged check.
        let Some((false, detected)) = current else {
            return Ok(false);
        };
        sqlx::query(
            "UPDATE services SET check_kind = ?, check_target = ?, check_internal_cert = ?, \
             detected_status = NULL WHERE id = ?",
        )
        .bind(kind)
        .bind(target)
        .bind(internal_cert)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        close_outage(&mut tx, id, now).await?;
        tx.commit().await?;
        Ok(detected)
    }

    /// Records that the service stopped answering at `started_at`.
    pub async fn mark_detected_down(
        pool: &DbPool,
        id: i64,
        started_at: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        let mut tx = pool.begin().await?;
        sqlx::query("UPDATE services SET detected_status = ? WHERE id = ?")
            .bind(ServiceStatus::MajorOutage)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        open_outage(&mut tx, id, started_at).await?;
        tx.commit().await
    }

    /// Records that the service answers again since `ended_at`.
    pub async fn mark_detected_up(
        pool: &DbPool,
        id: i64,
        ended_at: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        let mut tx = pool.begin().await?;
        sqlx::query("UPDATE services SET detected_status = NULL WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        close_outage(&mut tx, id, ended_at).await?;
        tx.commit().await
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
    use chrono::TimeZone;

    use super::*;
    use crate::models::CheckKind;
    use crate::repositories::OutageRepository;
    use crate::test_helpers::test_pool;

    fn at(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 4, hour, 0, 0).unwrap()
    }

    fn web(target: &str) -> ServiceCheck {
        ServiceCheck {
            kind: CheckKind::Http,
            target: target.to_string(),
            internal_cert: false,
        }
    }

    async fn detected(pool: &DbPool, id: i64) -> Option<ServiceStatus> {
        ServiceRepository::find_by_id(pool, id)
            .await
            .unwrap()
            .unwrap()
            .detected_status
    }

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

    #[tokio::test]
    async fn the_schema_refuses_incoherent_checks() {
        let pool = test_pool().await;
        let svc = create(&pool, "Mail", "mail").await;
        for update in [
            "UPDATE services SET check_kind = 'ping', check_target = 'x' WHERE id = ?",
            "UPDATE services SET check_kind = 'http' WHERE id = ?",
            "UPDATE services SET check_target = 'x' WHERE id = ?",
            "UPDATE services SET detected_status = 'degraded' WHERE id = ?",
        ] {
            let refused = sqlx::query(update).bind(svc.id).execute(&pool).await;
            assert!(refused.is_err(), "{update}");
        }
    }

    #[tokio::test]
    async fn checked_services_carry_their_check() {
        let pool = test_pool().await;
        let watched = create(&pool, "Site", "site").await;
        create(&pool, "Unwatched", "unwatched").await;
        let check = ServiceCheck {
            kind: CheckKind::Tcp,
            target: "10.0.0.1:443".to_string(),
            internal_cert: true,
        };
        ServiceRepository::set_check(&pool, watched.id, Some(&check), at(9))
            .await
            .unwrap();

        let checked = ServiceRepository::list_checked(&pool).await.unwrap();
        assert_eq!(checked.len(), 1);
        assert_eq!(checked[0].id, watched.id);
        assert_eq!(checked[0].kind, CheckKind::Tcp);
        assert_eq!(checked[0].target, "10.0.0.1:443");
        assert!(checked[0].internal_cert);
        assert_eq!(checked[0].detected_status, None);
    }

    #[tokio::test]
    async fn saving_the_same_check_keeps_a_detected_outage() {
        let pool = test_pool().await;
        let svc = create(&pool, "Wiki", "wiki").await;
        let check = web("https://wiki.example");
        ServiceRepository::set_check(&pool, svc.id, Some(&check), at(8))
            .await
            .unwrap();
        ServiceRepository::mark_detected_down(&pool, svc.id, at(9))
            .await
            .unwrap();

        let dropped = ServiceRepository::set_check(&pool, svc.id, Some(&check), at(10))
            .await
            .unwrap();
        assert!(!dropped);
        assert_eq!(
            detected(&pool, svc.id).await,
            Some(ServiceStatus::MajorOutage)
        );
        let spans = OutageRepository::since(&pool, at(0)).await.unwrap();
        assert_eq!(spans[&svc.id][0].end, None);
    }

    #[tokio::test]
    async fn changing_or_removing_the_check_clears_a_detected_outage() {
        let pool = test_pool().await;
        let svc = create(&pool, "Paie", "paie").await;
        ServiceRepository::set_check(&pool, svc.id, Some(&web("https://wrong")), at(8))
            .await
            .unwrap();
        ServiceRepository::mark_detected_down(&pool, svc.id, at(9))
            .await
            .unwrap();

        let dropped =
            ServiceRepository::set_check(&pool, svc.id, Some(&web("https://paie")), at(10))
                .await
                .unwrap();
        assert!(dropped);
        assert_eq!(detected(&pool, svc.id).await, None);
        let spans = OutageRepository::since(&pool, at(0)).await.unwrap();
        assert_eq!(spans[&svc.id][0].end, Some(at(10)));

        ServiceRepository::mark_detected_down(&pool, svc.id, at(11))
            .await
            .unwrap();
        let dropped = ServiceRepository::set_check(&pool, svc.id, None, at(12))
            .await
            .unwrap();
        assert!(dropped);
        assert!(
            ServiceRepository::list_checked(&pool)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(detected(&pool, svc.id).await, None);
    }

    #[tokio::test]
    async fn a_service_has_one_outage_under_way_at_most() {
        let pool = test_pool().await;
        let svc = create(&pool, "VPN", "vpn").await;
        ServiceRepository::mark_detected_down(&pool, svc.id, at(9))
            .await
            .unwrap();
        assert!(
            ServiceRepository::mark_detected_down(&pool, svc.id, at(10))
                .await
                .is_err()
        );
        ServiceRepository::mark_detected_up(&pool, svc.id, at(11))
            .await
            .unwrap();
        ServiceRepository::mark_detected_down(&pool, svc.id, at(12))
            .await
            .unwrap();
        let spans = OutageRepository::since(&pool, at(0)).await.unwrap();
        assert_eq!(spans[&svc.id].len(), 2);
    }

    #[tokio::test]
    async fn deleting_a_service_deletes_its_outages() {
        let pool = test_pool().await;
        let svc = create(&pool, "Gone", "gone").await;
        ServiceRepository::mark_detected_down(&pool, svc.id, at(9))
            .await
            .unwrap();
        ServiceRepository::delete(&pool, svc.id).await.unwrap();
        let spans = OutageRepository::since(&pool, at(0)).await.unwrap();
        assert!(spans.is_empty());
    }
}
