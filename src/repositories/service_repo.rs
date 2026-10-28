//! Service repository: SQL queries on services.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use super::GrantTables;
use super::outage_repo::{OutageCause, close_outage, delete_open_outage, open_outage};
use crate::db::DbPool;
use crate::models::{
    CheckedService, Grants, Scope, Service, ServiceCheck, ServiceStatus, Visibility,
};

pub struct ServiceRepository;

const WITH_ICON: &str = "SELECT s.*, i.filename AS icon_filename, \
     EXISTS(SELECT 1 FROM event_services es WHERE es.service_id = s.id) AS has_history \
     FROM services s LEFT JOIN icons i ON i.id = s.icon_id";

/// The order chosen on the Services page: services without a folder first,
/// then each folder in its own order with its services by their position.
/// Services at the same position, as written by a seed that leaves it out,
/// fall back to their name.
pub(crate) const CHOSEN_ORDER: &str = "s.folder_id IS NOT NULL, \
     COALESCE((SELECT f.position FROM service_folders f WHERE f.id = s.folder_id), 0), \
     s.folder_id, s.position, s.name COLLATE NOCASE, s.id";

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
            "INSERT INTO services (name, slug, description, icon_id, icon_name, position) \
             VALUES (?, ?, ?, ?, ?, (SELECT COALESCE(MAX(position), 0) + 1 FROM services)) \
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

    /// Every service with its uploaded icon, in the chosen order.
    pub async fn list_all(pool: &DbPool) -> Result<Vec<Service>, sqlx::Error> {
        sqlx::query_as::<_, Service>(&format!("{WITH_ICON} ORDER BY {CHOSEN_ORDER}"))
            .fetch_all(pool)
            .await
    }

    /// The names of the services among `ids`.
    pub async fn names(pool: &DbPool, ids: &[i64]) -> Result<HashMap<i64, String>, sqlx::Error> {
        let ids = serde_json::to_string(ids).unwrap_or_default();
        let rows: Vec<(i64, String)> = sqlx::query_as(
            "SELECT id, name FROM services WHERE id IN (SELECT value FROM json_each(?))",
        )
        .bind(ids)
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().collect())
    }

    /// The services a viewer may see, in the chosen order.
    pub async fn list_visible(pool: &DbPool, scope: &Scope) -> Result<Vec<Service>, sqlx::Error> {
        let mut services = Self::list_all(pool).await?;
        services.retain(|service| scope.sees_service(service.id));
        Ok(services)
    }

    /// Saves the order of every service, first to last. Returns false, and
    /// changes nothing, when `ids` is not exactly the set of services: the
    /// page that sent it no longer shows what is there.
    pub async fn reorder(pool: &DbPool, ids: &[i64]) -> Result<bool, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let mut existing: Vec<i64> = sqlx::query_scalar("SELECT id FROM services")
            .fetch_all(&mut *tx)
            .await?;
        let mut sent = ids.to_vec();
        existing.sort_unstable();
        sent.sort_unstable();
        if existing != sent {
            return Ok(false);
        }
        for (position, id) in (1_i64..).zip(ids) {
            sqlx::query("UPDATE services SET position = ? WHERE id = ?")
                .bind(position)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(true)
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
            "SELECT id, name, check_kind, check_target, check_internal_cert, detected_status, \
                    outage_after_minutes \
             FROM services WHERE check_kind IS NOT NULL ORDER BY id",
        )
        .fetch_all(pool)
        .await
    }

    /// Saves how many minutes of failed checks make an outage. Unlike a
    /// changed check, it leaves an outage under way as it is.
    pub async fn set_outage_after(pool: &DbPool, id: i64, minutes: u8) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE services SET outage_after_minutes = ? WHERE id = ?")
            .bind(minutes)
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Saves what to check. A changed check drops what the old one detected
    /// and erases its outage under way: fixing a wrong address leaves no
    /// false outage on the public strip. Returns whether a detected state
    /// was dropped, which changes the status to show.
    pub async fn set_check(
        pool: &DbPool,
        id: i64,
        check: Option<&ServiceCheck>,
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
        delete_open_outage(&mut tx, id).await?;
        tx.commit().await?;
        Ok(detected)
    }

    /// Records that the service stopped answering at `started_at`.
    pub async fn mark_detected_down(
        pool: &DbPool,
        id: i64,
        started_at: DateTime<Utc>,
        cause: &OutageCause,
    ) -> Result<(), sqlx::Error> {
        let mut tx = pool.begin().await?;
        sqlx::query("UPDATE services SET detected_status = ? WHERE id = ?")
            .bind(ServiceStatus::MajorOutage)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        open_outage(&mut tx, id, started_at, cause).await?;
        tx.commit().await
    }

    /// Records that the service answers again since `ended_at`. Returns
    /// false when no outage was under way: two reports racing to end the
    /// same one tell it once.
    pub async fn mark_detected_up(
        pool: &DbPool,
        id: i64,
        ended_at: DateTime<Utc>,
    ) -> Result<bool, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let ended = sqlx::query(
            "UPDATE services SET detected_status = NULL \
             WHERE id = ? AND detected_status IS NOT NULL",
        )
        .bind(id)
        .execute(&mut *tx)
        .await?
        .rows_affected()
            == 1;
        if !ended {
            return Ok(false);
        }
        close_outage(&mut tx, id, ended_at).await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Named by an event, or by a repeating maintenance still running: its
    /// next sessions take their visibility from their services, and would be
    /// open to everyone once the last restricted one is gone.
    pub async fn has_events(pool: &DbPool, service_id: i64) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM event_services WHERE service_id = ?)
                 OR EXISTS(SELECT 1 FROM maintenance_series_services ss
                           JOIN maintenance_series s ON s.id = ss.series_id
                           WHERE ss.service_id = ? AND s.stopped_at IS NULL)",
        )
        .bind(service_id)
        .bind(service_id)
        .fetch_one(pool)
        .await
    }

    /// The ids of the services this reader may see, with the count of all
    /// services. `None` is a visitor who is not signed in. A `groups` service
    /// with neither group nor member left matches nobody here.
    pub async fn visible_ids(
        pool: &DbPool,
        reader_id: Option<i64>,
    ) -> Result<(Vec<i64>, usize), sqlx::Error> {
        let rows: Vec<(i64, bool)> = sqlx::query_as(
            "SELECT s.id, CASE \
                 WHEN s.visibility = 'everyone' THEN 1 \
                 WHEN ?1 IS NULL THEN 0 \
                 WHEN s.visibility = 'members' THEN 1 \
                 ELSE EXISTS(SELECT 1 FROM service_access a \
                             JOIN group_members m ON m.group_id = a.group_id \
                             WHERE a.service_id = s.id AND m.user_id = ?1) \
                 OR EXISTS(SELECT 1 FROM service_member_access ma \
                           WHERE ma.service_id = s.id AND ma.user_id = ?1) \
             END FROM services s",
        )
        .bind(reader_id)
        .fetch_all(pool)
        .await?;
        let total = rows.len();
        let visible = rows
            .into_iter()
            .filter_map(|(id, visible)| visible.then_some(id))
            .collect();
        Ok((visible, total))
    }

    /// Saves who sees the service. The grants only count for
    /// [`Visibility::Groups`]; ids of groups that do not exist are dropped,
    /// and so are members who are not active readers (publishers and admins
    /// see everything already). A setting made here is the service's own: it
    /// stops following its folder.
    pub async fn set_visibility(
        pool: &DbPool,
        service_id: i64,
        visibility: Visibility,
        grants: Grants<'_>,
    ) -> Result<(), sqlx::Error> {
        let mut tx = pool.begin().await?;
        sqlx::query("UPDATE services SET visibility = ?, follows_folder = 0 WHERE id = ?")
            .bind(visibility)
            .bind(service_id)
            .execute(&mut *tx)
            .await?;
        super::replace_grants(
            &mut tx,
            GrantTables::Service,
            service_id,
            visibility,
            grants,
        )
        .await?;
        tx.commit().await
    }

    pub async fn group_ids_of(pool: &DbPool, service_id: i64) -> Result<Vec<i64>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT group_id FROM service_access WHERE service_id = ? ORDER BY group_id",
        )
        .bind(service_id)
        .fetch_all(pool)
        .await
    }

    pub async fn member_ids_of(pool: &DbPool, service_id: i64) -> Result<Vec<i64>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT user_id FROM service_member_access WHERE service_id = ? ORDER BY user_id",
        )
        .bind(service_id)
        .fetch_all(pool)
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
    use crate::models::{CheckKind, Role, User};
    use crate::repositories::{GroupRepository, OutageRepository, UserRepository};
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

    async fn names(pool: &DbPool) -> Vec<String> {
        ServiceRepository::list_all(pool)
            .await
            .unwrap()
            .into_iter()
            .map(|s| s.name)
            .collect()
    }

    #[tokio::test]
    async fn a_new_service_goes_last() {
        let pool = test_pool().await;
        create(&pool, "zzz", "zzz").await;
        create(&pool, "Aaa", "aaa").await;
        assert_eq!(names(&pool).await, vec!["zzz", "Aaa"]);
    }

    #[tokio::test]
    async fn services_without_a_position_sort_by_name_ignoring_case() {
        let pool = test_pool().await;
        for (name, slug) in [("zzz", "zzz"), ("Aaa", "aaa"), ("Mmm", "mmm")] {
            sqlx::query("INSERT INTO services (name, slug) VALUES (?, ?)")
                .bind(name)
                .bind(slug)
                .execute(&pool)
                .await
                .unwrap();
        }
        assert_eq!(names(&pool).await, vec!["Aaa", "Mmm", "zzz"]);
    }

    #[tokio::test]
    async fn reorder_saves_the_order_without_touching_the_update_time() {
        let pool = test_pool().await;
        let a = create(&pool, "A", "a").await;
        let b = create(&pool, "B", "b").await;
        let c = create(&pool, "C", "c").await;
        // Moving them too keeps the update trigger from stamping the time.
        sqlx::query(
            "UPDATE services SET updated_at = '2026-01-01 00:00:00', position = position + 10",
        )
        .execute(&pool)
        .await
        .unwrap();

        assert!(
            ServiceRepository::reorder(&pool, &[c.id, a.id, b.id])
                .await
                .unwrap()
        );
        assert_eq!(names(&pool).await, vec!["C", "A", "B"]);
        let moved = ServiceRepository::find_by_id(&pool, c.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            moved.updated_at,
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap()
        );
    }

    #[tokio::test]
    async fn reorder_refuses_a_list_that_is_not_every_service() {
        let pool = test_pool().await;
        let a = create(&pool, "A", "a").await;
        let b = create(&pool, "B", "b").await;
        for ids in [vec![b.id], vec![b.id, a.id, 999], vec![b.id, b.id]] {
            assert!(!ServiceRepository::reorder(&pool, &ids).await.unwrap());
        }
        assert_eq!(names(&pool).await, vec!["A", "B"]);
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
        ServiceRepository::set_check(&pool, watched.id, Some(&check))
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
    async fn the_outage_delay_stays_between_one_minute_and_an_hour() {
        let pool = test_pool().await;
        let svc = create(&pool, "Mail", "mail").await;
        assert_eq!(svc.outage_after_minutes, 3);
        for refused in [0, 61] {
            let saved = ServiceRepository::set_outage_after(&pool, svc.id, refused).await;
            assert!(saved.is_err(), "{refused}");
        }
        ServiceRepository::set_outage_after(&pool, svc.id, 60)
            .await
            .unwrap();
        let svc = ServiceRepository::find_by_id(&pool, svc.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(svc.outage_after_minutes, 60);
    }

    #[tokio::test]
    async fn changing_the_outage_delay_keeps_a_detected_outage() {
        let pool = test_pool().await;
        let svc = create(&pool, "Wiki", "wiki").await;
        ServiceRepository::set_check(&pool, svc.id, Some(&web("https://wiki.example")))
            .await
            .unwrap();
        ServiceRepository::mark_detected_down(&pool, svc.id, at(9), &OutageCause::Report)
            .await
            .unwrap();

        ServiceRepository::set_outage_after(&pool, svc.id, 15)
            .await
            .unwrap();
        assert_eq!(
            detected(&pool, svc.id).await,
            Some(ServiceStatus::MajorOutage)
        );
        let checked = ServiceRepository::list_checked(&pool).await.unwrap();
        assert_eq!(checked[0].outage_after_minutes, 15);
    }

    #[tokio::test]
    async fn saving_the_same_check_keeps_a_detected_outage() {
        let pool = test_pool().await;
        let svc = create(&pool, "Wiki", "wiki").await;
        let check = web("https://wiki.example");
        ServiceRepository::set_check(&pool, svc.id, Some(&check))
            .await
            .unwrap();
        ServiceRepository::mark_detected_down(&pool, svc.id, at(9), &OutageCause::Report)
            .await
            .unwrap();

        let dropped = ServiceRepository::set_check(&pool, svc.id, Some(&check))
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
    async fn changing_or_removing_the_check_erases_the_outage_under_way() {
        let pool = test_pool().await;
        let svc = create(&pool, "Paie", "paie").await;
        ServiceRepository::set_check(&pool, svc.id, Some(&web("https://wrong")))
            .await
            .unwrap();
        ServiceRepository::mark_detected_down(&pool, svc.id, at(8), &OutageCause::Report)
            .await
            .unwrap();
        ServiceRepository::mark_detected_up(&pool, svc.id, at(9))
            .await
            .unwrap();
        ServiceRepository::mark_detected_down(&pool, svc.id, at(10), &OutageCause::Report)
            .await
            .unwrap();

        let dropped = ServiceRepository::set_check(&pool, svc.id, Some(&web("https://paie")))
            .await
            .unwrap();
        assert!(dropped);
        assert_eq!(detected(&pool, svc.id).await, None);
        let spans = OutageRepository::since(&pool, at(0)).await.unwrap();
        assert_eq!(spans[&svc.id].len(), 1, "the ended outage stays");
        assert_eq!(spans[&svc.id][0].end, Some(at(9)));

        ServiceRepository::mark_detected_down(&pool, svc.id, at(11), &OutageCause::Report)
            .await
            .unwrap();
        let dropped = ServiceRepository::set_check(&pool, svc.id, None)
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
        let spans = OutageRepository::since(&pool, at(0)).await.unwrap();
        assert_eq!(spans[&svc.id].len(), 1);
    }

    #[tokio::test]
    async fn a_service_has_one_outage_under_way_at_most() {
        let pool = test_pool().await;
        let svc = create(&pool, "VPN", "vpn").await;
        ServiceRepository::mark_detected_down(&pool, svc.id, at(9), &OutageCause::Report)
            .await
            .unwrap();
        assert!(
            ServiceRepository::mark_detected_down(&pool, svc.id, at(10), &OutageCause::Report)
                .await
                .is_err()
        );
        assert!(
            ServiceRepository::mark_detected_up(&pool, svc.id, at(11))
                .await
                .unwrap()
        );
        assert!(
            !ServiceRepository::mark_detected_up(&pool, svc.id, at(11))
                .await
                .unwrap(),
            "an outage ends once"
        );
        ServiceRepository::mark_detected_down(&pool, svc.id, at(12), &OutageCause::Report)
            .await
            .unwrap();
        let spans = OutageRepository::since(&pool, at(0)).await.unwrap();
        assert_eq!(spans[&svc.id].len(), 2);
        assert_eq!(spans[&svc.id][0].end, Some(at(11)));
    }

    #[tokio::test]
    async fn deleting_a_service_deletes_its_outages() {
        let pool = test_pool().await;
        let svc = create(&pool, "Gone", "gone").await;
        ServiceRepository::mark_detected_down(&pool, svc.id, at(9), &OutageCause::Report)
            .await
            .unwrap();
        ServiceRepository::delete(&pool, svc.id).await.unwrap();
        let spans = OutageRepository::since(&pool, at(0)).await.unwrap();
        assert!(spans.is_empty());
    }

    #[tokio::test]
    async fn a_service_inserted_without_visibility_is_seen_by_everyone() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO services (name, slug) VALUES ('Wiki', 'wiki')")
            .execute(&pool)
            .await
            .unwrap();
        let services = ServiceRepository::list_all(&pool).await.unwrap();
        assert_eq!(services[0].visibility, Visibility::Everyone);
    }

    #[tokio::test]
    async fn visible_ids_follow_the_reader_and_count_every_service() {
        let pool = test_pool().await;
        let open = create(&pool, "Open", "open").await;
        let members = create(&pool, "Members", "members").await;
        let closed = create(&pool, "Closed", "closed").await;
        ServiceRepository::set_visibility(
            &pool,
            members.id,
            Visibility::Members,
            Grants::default(),
        )
        .await
        .unwrap();
        ServiceRepository::set_visibility(&pool, closed.id, Visibility::Groups, Grants::default())
            .await
            .unwrap();

        let (anonymous, total) = ServiceRepository::visible_ids(&pool, None).await.unwrap();
        assert_eq!((anonymous, total), (vec![open.id], 3));
        let (reader, total) = ServiceRepository::visible_ids(&pool, Some(1))
            .await
            .unwrap();
        assert_eq!((reader, total), (vec![open.id, members.id], 3));
    }

    #[tokio::test]
    async fn only_the_groups_choice_stores_access_rows() {
        let pool = test_pool().await;
        let svc = create(&pool, "Wiki", "wiki").await;
        let group = GroupRepository::create(&pool, "Support").await.unwrap();

        ServiceRepository::set_visibility(
            &pool,
            svc.id,
            Visibility::Groups,
            Grants::groups(&[group, 999]),
        )
        .await
        .unwrap();
        assert_eq!(
            ServiceRepository::group_ids_of(&pool, svc.id)
                .await
                .unwrap(),
            vec![group]
        );
        let stored = ServiceRepository::find_by_id(&pool, svc.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.visibility, Visibility::Groups);

        for visibility in [Visibility::Members, Visibility::Everyone] {
            ServiceRepository::set_visibility(
                &pool,
                svc.id,
                Visibility::Groups,
                Grants::groups(&[group]),
            )
            .await
            .unwrap();
            ServiceRepository::set_visibility(&pool, svc.id, visibility, Grants::groups(&[group]))
                .await
                .unwrap();
            assert!(
                ServiceRepository::group_ids_of(&pool, svc.id)
                    .await
                    .unwrap()
                    .is_empty(),
                "{visibility:?}"
            );
        }
    }

    async fn account(pool: &DbPool, email: &str, role: Role) -> User {
        UserRepository::create(pool, email, "hash", "Someone", role)
            .await
            .unwrap()
    }

    fn members_only(ids: &[i64]) -> Grants<'_> {
        Grants {
            groups: &[],
            members: ids,
        }
    }

    async fn sees(pool: &DbPool, user: &User, service_id: i64) -> bool {
        crate::services::scope_of(pool, Some(user))
            .await
            .unwrap()
            .sees_service(service_id)
    }

    #[tokio::test]
    async fn a_reader_granted_by_name_sees_a_groups_service_and_another_does_not() {
        let pool = test_pool().await;
        let svc = create(&pool, "Wiki", "wiki").await;
        let granted = account(&pool, "in@x.com", Role::Reader).await;
        let other = account(&pool, "out@x.com", Role::Reader).await;

        ServiceRepository::set_visibility(
            &pool,
            svc.id,
            Visibility::Groups,
            members_only(&[granted.id]),
        )
        .await
        .unwrap();

        assert!(sees(&pool, &granted, svc.id).await);
        assert!(!sees(&pool, &other, svc.id).await);
        assert_eq!(
            ServiceRepository::member_ids_of(&pool, svc.id)
                .await
                .unwrap(),
            vec![granted.id]
        );
    }

    #[tokio::test]
    async fn a_group_and_a_member_both_open_the_service() {
        let pool = test_pool().await;
        let svc = create(&pool, "Wiki", "wiki").await;
        let group = GroupRepository::create(&pool, "Support").await.unwrap();
        let in_group = account(&pool, "g@x.com", Role::Reader).await;
        let by_name = account(&pool, "n@x.com", Role::Reader).await;
        let other = account(&pool, "o@x.com", Role::Reader).await;
        GroupRepository::add_members(&pool, group, &[in_group.id])
            .await
            .unwrap();

        ServiceRepository::set_visibility(
            &pool,
            svc.id,
            Visibility::Groups,
            Grants {
                groups: &[group],
                members: &[by_name.id],
            },
        )
        .await
        .unwrap();

        assert!(sees(&pool, &in_group, svc.id).await);
        assert!(sees(&pool, &by_name, svc.id).await);
        assert!(!sees(&pool, &other, svc.id).await);
    }

    #[tokio::test]
    async fn only_active_readers_are_stored_as_members() {
        let pool = test_pool().await;
        let svc = create(&pool, "Wiki", "wiki").await;
        let reader = account(&pool, "r@x.com", Role::Reader).await;
        let publisher = account(&pool, "p@x.com", Role::Publisher).await;
        let admin = account(&pool, "a@x.com", Role::Admin).await;
        let disabled = account(&pool, "d@x.com", Role::Reader).await;
        UserRepository::set_active(&pool, disabled.id, false)
            .await
            .unwrap();

        let ids = [reader.id, publisher.id, admin.id, disabled.id, 999];
        ServiceRepository::set_visibility(&pool, svc.id, Visibility::Groups, members_only(&ids))
            .await
            .unwrap();

        assert_eq!(
            ServiceRepository::member_ids_of(&pool, svc.id)
                .await
                .unwrap(),
            vec![reader.id]
        );
    }

    #[tokio::test]
    async fn deleting_a_user_removes_the_access_granted_to_them() {
        let pool = test_pool().await;
        let svc = create(&pool, "Wiki", "wiki").await;
        let reader = account(&pool, "r@x.com", Role::Reader).await;
        ServiceRepository::set_visibility(
            &pool,
            svc.id,
            Visibility::Groups,
            members_only(&[reader.id]),
        )
        .await
        .unwrap();

        sqlx::query("DELETE FROM users WHERE id = ?")
            .bind(reader.id)
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(
            ServiceRepository::member_ids_of(&pool, svc.id)
                .await
                .unwrap(),
            Vec::<i64>::new()
        );
    }

    #[tokio::test]
    async fn leaving_the_groups_choice_clears_groups_and_members() {
        let pool = test_pool().await;
        let svc = create(&pool, "Wiki", "wiki").await;
        let group = GroupRepository::create(&pool, "Support").await.unwrap();
        let reader = account(&pool, "r@x.com", Role::Reader).await;
        let grants = Grants {
            groups: &[group],
            members: &[reader.id],
        };

        for visibility in [Visibility::Members, Visibility::Everyone] {
            ServiceRepository::set_visibility(&pool, svc.id, Visibility::Groups, grants)
                .await
                .unwrap();
            ServiceRepository::set_visibility(&pool, svc.id, visibility, grants)
                .await
                .unwrap();
            assert!(
                ServiceRepository::group_ids_of(&pool, svc.id)
                    .await
                    .unwrap()
                    .is_empty(),
                "{visibility:?}"
            );
            assert!(
                ServiceRepository::member_ids_of(&pool, svc.id)
                    .await
                    .unwrap()
                    .is_empty(),
                "{visibility:?}"
            );
        }
    }
}
