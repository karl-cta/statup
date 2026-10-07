//! Notification repository: the destinations and the queue of each.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::clock;
use crate::db::DbPool;
use crate::models::{Audience, Channel, ChannelInput, Delivery, Happening, Notification};

const DELIVERY_COLUMNS: &str = "d.id, d.channel_id, d.happening, d.event_id, d.update_id, \
     d.lifecycle, d.service_id, d.status, d.attempts, d.next_attempt_at, d.failure, \
     d.created_at, d.sent_at";

/// The newest finished message of each destination.
const LATEST_OUTCOMES: &str = "SELECT MAX(id) FROM notification_deliveries \
     WHERE status != 'pending' GROUP BY channel_id";

pub struct NotificationRepository;

impl NotificationRepository {
    /// Every destination, by name.
    pub async fn list_channels(pool: &DbPool) -> Result<Vec<Channel>, sqlx::Error> {
        sqlx::query_as::<_, Channel>(
            "SELECT * FROM notification_channels ORDER BY name COLLATE NOCASE, id",
        )
        .fetch_all(pool)
        .await
    }

    pub async fn find_channel(pool: &DbPool, id: i64) -> Result<Option<Channel>, sqlx::Error> {
        sqlx::query_as::<_, Channel>("SELECT * FROM notification_channels WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
    }

    pub async fn create_channel(
        pool: &DbPool,
        input: &ChannelInput,
    ) -> Result<Channel, sqlx::Error> {
        sqlx::query_as::<_, Channel>(
            "INSERT INTO notification_channels (name, kind, target, locale, on_incidents, \
             on_maintenances, on_publications, on_detected) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?) RETURNING *",
        )
        .bind(&input.name)
        .bind(input.kind)
        .bind(&input.target)
        .bind(&input.locale)
        .bind(input.on_incidents)
        .bind(input.on_maintenances)
        .bind(input.on_publications)
        .bind(input.on_detected)
        .fetch_one(pool)
        .await
    }

    /// Returns whether the destination exists.
    pub async fn update_channel(
        pool: &DbPool,
        id: i64,
        input: &ChannelInput,
    ) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE notification_channels SET name = ?, kind = ?, target = ?, locale = ?, \
             on_incidents = ?, on_maintenances = ?, on_publications = ?, on_detected = ? \
             WHERE id = ?",
        )
        .bind(&input.name)
        .bind(input.kind)
        .bind(&input.target)
        .bind(&input.locale)
        .bind(input.on_incidents)
        .bind(input.on_maintenances)
        .bind(input.on_publications)
        .bind(input.on_detected)
        .bind(id)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Deletes a destination with its queue. Returns whether it existed.
    pub async fn delete_channel(pool: &DbPool, id: i64) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM notification_channels WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Queues `notification` for every destination that receives its part of
    /// the page. A service coming back is only told where its outage was.
    /// Returns how many messages were queued.
    pub async fn enqueue(
        pool: &DbPool,
        notification: &Notification,
        now: DateTime<Utc>,
    ) -> Result<u64, sqlx::Error> {
        let column = match notification.audience {
            Audience::Incidents => "on_incidents",
            Audience::Maintenances => "on_maintenances",
            Audience::Publications => "on_publications",
            Audience::DetectedOutages => "on_detected",
        };
        let coming_back = notification.happening == Happening::ServiceUp;
        let after_outage = if coming_back {
            " AND (SELECT d.happening FROM notification_deliveries d \
                   WHERE d.channel_id = c.id AND d.service_id = ? \
                   ORDER BY d.id DESC LIMIT 1) = 'service_down'"
        } else {
            ""
        };
        let sql = format!(
            "INSERT INTO notification_deliveries (channel_id, happening, event_id, update_id, \
             lifecycle, service_id, next_attempt_at, created_at) \
             SELECT c.id, ?, ?, ?, ?, ?, ?, ? FROM notification_channels c \
             WHERE c.{column} = 1{after_outage}"
        );
        let at = clock::db(now);
        let mut query = sqlx::query(&sql)
            .bind(notification.happening)
            .bind(notification.event_id)
            .bind(notification.update_id)
            .bind(notification.lifecycle)
            .bind(notification.service_id)
            .bind(&at)
            .bind(&at);
        if coming_back {
            query = query.bind(notification.service_id);
        }
        Ok(query.execute(pool).await?.rows_affected())
    }

    /// The first waiting message of each destination, once its time has
    /// come. The ones behind it wait their turn, so each destination receives
    /// its messages in order.
    pub async fn due(pool: &DbPool, now: DateTime<Utc>) -> Result<Vec<Delivery>, sqlx::Error> {
        sqlx::query_as::<_, Delivery>(&format!(
            "SELECT {DELIVERY_COLUMNS} FROM notification_deliveries d \
             WHERE d.status = 'pending' AND d.next_attempt_at <= ? \
               AND d.id = (SELECT MIN(p.id) FROM notification_deliveries p \
                           WHERE p.channel_id = d.channel_id AND p.status = 'pending') \
             ORDER BY d.id"
        ))
        .bind(clock::db(now))
        .fetch_all(pool)
        .await
    }

    pub async fn mark_sent(pool: &DbPool, id: i64, now: DateTime<Utc>) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE notification_deliveries SET status = 'sent', attempts = attempts + 1, \
             failure = NULL, sent_at = ? WHERE id = ?",
        )
        .bind(clock::db(now))
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Counts a failed attempt and sets when the next one is made.
    pub async fn retry_later(
        pool: &DbPool,
        id: i64,
        failure: &str,
        next_attempt_at: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE notification_deliveries SET attempts = attempts + 1, failure = ?, \
             next_attempt_at = ? WHERE id = ? AND status = 'pending'",
        )
        .bind(failure)
        .bind(clock::db(next_attempt_at))
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Gives up on one message, the others of its destination going on.
    pub async fn fail(pool: &DbPool, id: i64, failure: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE notification_deliveries SET status = 'failed', attempts = attempts + 1, \
             failure = ? WHERE id = ? AND status = 'pending'",
        )
        .bind(failure)
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Gives up on every waiting message of a destination, for the reason the
    /// first one failed: behind a dead address, each would wait out all its
    /// attempts in turn and arrive hours late.
    pub async fn fail_waiting(
        pool: &DbPool,
        channel_id: i64,
        failure: &str,
    ) -> Result<u64, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE notification_deliveries SET status = 'failed', failure = ? \
             WHERE channel_id = ? AND status = 'pending'",
        )
        .bind(failure)
        .bind(channel_id)
        .execute(pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// The last message each destination sent or gave up on.
    pub async fn latest_outcomes(pool: &DbPool) -> Result<HashMap<i64, Delivery>, sqlx::Error> {
        let rows = sqlx::query_as::<_, Delivery>(&format!(
            "SELECT {DELIVERY_COLUMNS} FROM notification_deliveries d \
             WHERE d.id IN ({LATEST_OUTCOMES})"
        ))
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().map(|d| (d.channel_id, d)).collect())
    }

    /// Forgets the finished messages queued before `before`, except the last
    /// one of each destination, which its settings show.
    pub async fn purge(pool: &DbPool, before: DateTime<Utc>) -> Result<u64, sqlx::Error> {
        let result = sqlx::query(&format!(
            "DELETE FROM notification_deliveries \
             WHERE status != 'pending' AND created_at < ? AND id NOT IN ({LATEST_OUTCOMES})"
        ))
        .bind(clock::db(before))
        .execute(pool)
        .await?;
        Ok(result.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};

    use super::*;
    use crate::models::{
        ChannelKind, CreateEventInput, DeliveryStatus, Kind, Lifecycle, Role, Severity,
    };
    use crate::repositories::{EventRepository, ServiceRepository, UserRepository};
    use crate::test_helpers::test_pool;

    fn at(minute: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 7, 9, 0, 0).unwrap() + Duration::minutes(minute)
    }

    /// A destination ticking incidents, maintenances and detected outages as
    /// asked, announcements always.
    async fn channel(pool: &DbPool, name: &str, incidents: bool, maintenances: bool) -> i64 {
        let input = ChannelInput {
            name: name.to_string(),
            kind: ChannelKind::Slack,
            target: format!("https://hooks.slack.com/services/{name}"),
            locale: "fr".to_string(),
            on_incidents: incidents,
            on_maintenances: maintenances,
            on_publications: true,
            on_detected: incidents,
        };
        NotificationRepository::create_channel(pool, &input)
            .await
            .unwrap()
            .id
    }

    async fn incident(pool: &DbPool) -> i64 {
        let author = UserRepository::create(pool, "ops@example.com", "hash", "Ops", Role::Admin)
            .await
            .unwrap();
        let input = CreateEventInput {
            kind: Kind::Incident,
            severity: Some(Severity::Critical),
            planned: false,
            category: None,
            title: "Network down at the head office".to_string(),
            description: String::new(),
            planned_start: None,
            planned_end: None,
            started_at: None,
            opening_step: None,
            keeps_services_up: false,
            service_ids: Vec::new(),
            follows_event_id: None,
            author_id: author.id,
        };
        EventRepository::create(pool, &input).await.unwrap().id
    }

    fn about_event(happening: Happening, event_id: i64) -> Notification {
        Notification {
            happening,
            audience: Audience::Incidents,
            event_id: Some(event_id),
            update_id: None,
            lifecycle: None,
            service_id: None,
        }
    }

    fn about_service(happening: Happening, service_id: i64) -> Notification {
        Notification {
            happening,
            audience: Audience::DetectedOutages,
            event_id: None,
            update_id: None,
            lifecycle: None,
            service_id: Some(service_id),
        }
    }

    async fn queued(pool: &DbPool) -> Vec<(i64, Happening, DeliveryStatus)> {
        sqlx::query_as(
            "SELECT channel_id, happening, status FROM notification_deliveries ORDER BY id",
        )
        .fetch_all(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn destinations_are_listed_by_name_changed_and_deleted() {
        let pool = test_pool().await;
        let zulu = channel(&pool, "zulu", true, true).await;
        let alpha = channel(&pool, "Alpha", true, true).await;

        let names: Vec<String> = NotificationRepository::list_channels(&pool)
            .await
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert_eq!(names, ["Alpha", "zulu"]);

        let mut input = ChannelInput {
            name: "IT team".to_string(),
            kind: ChannelKind::Teams,
            target: "https://example.logic.azure.com/workflows/1".to_string(),
            locale: "en".to_string(),
            on_incidents: true,
            on_maintenances: false,
            on_publications: false,
            on_detected: true,
        };
        assert!(
            NotificationRepository::update_channel(&pool, alpha, &input)
                .await
                .unwrap()
        );
        let saved = NotificationRepository::find_channel(&pool, alpha)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.kind, ChannelKind::Teams);
        assert_eq!(saved.locale, "en");
        assert!(saved.on_detected && !saved.on_maintenances);

        input.name = "Gone".to_string();
        assert!(
            !NotificationRepository::update_channel(&pool, 999, &input)
                .await
                .unwrap()
        );
        assert!(
            NotificationRepository::delete_channel(&pool, zulu)
                .await
                .unwrap()
        );
        assert!(
            NotificationRepository::find_channel(&pool, zulu)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn a_message_reaches_the_destinations_that_tick_its_box() {
        let pool = test_pool().await;
        let incidents = channel(&pool, "incidents", true, false).await;
        channel(&pool, "maintenances", false, true).await;
        let event = incident(&pool).await;

        let mut notification = about_event(Happening::Closed, event);
        notification.lifecycle = Some(Lifecycle::Resolved);
        let count = NotificationRepository::enqueue(&pool, &notification, at(0))
            .await
            .unwrap();

        assert_eq!(count, 1);
        let due = NotificationRepository::due(&pool, at(0)).await.unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].channel_id, incidents);
        assert_eq!(due[0].event_id, Some(event));
        assert_eq!(due[0].lifecycle, Some(Lifecycle::Resolved));
        assert_eq!(due[0].status, DeliveryStatus::Pending);
    }

    #[tokio::test]
    async fn a_service_comes_back_only_where_its_outage_was_told() {
        let pool = test_pool().await;
        let service = ServiceRepository::create(&pool, "Mail", "mail", None, None, None)
            .await
            .unwrap();
        let early = channel(&pool, "early", true, false).await;
        NotificationRepository::enqueue(
            &pool,
            &about_service(Happening::ServiceDown, service.id),
            at(0),
        )
        .await
        .unwrap();
        channel(&pool, "late", true, false).await;

        NotificationRepository::enqueue(
            &pool,
            &about_service(Happening::ServiceUp, service.id),
            at(5),
        )
        .await
        .unwrap();

        let rows = queued(&pool).await;
        assert_eq!(
            rows,
            [
                (early, Happening::ServiceDown, DeliveryStatus::Pending),
                (early, Happening::ServiceUp, DeliveryStatus::Pending),
            ]
        );
    }

    #[tokio::test]
    async fn each_destination_sends_its_oldest_message_first() {
        let pool = test_pool().await;
        let first = channel(&pool, "first", true, false).await;
        let second = channel(&pool, "second", true, false).await;
        let event = incident(&pool).await;
        for minute in [0, 1] {
            NotificationRepository::enqueue(
                &pool,
                &about_event(Happening::Updated, event),
                at(minute),
            )
            .await
            .unwrap();
        }

        let heads = NotificationRepository::due(&pool, at(1)).await.unwrap();
        let channels: Vec<i64> = heads.iter().map(|d| d.channel_id).collect();
        assert_eq!(channels, [first, second]);

        NotificationRepository::mark_sent(&pool, heads[0].id, at(1))
            .await
            .unwrap();
        NotificationRepository::retry_later(&pool, heads[1].id, "timed_out", at(3))
            .await
            .unwrap();

        let next = NotificationRepository::due(&pool, at(2)).await.unwrap();
        assert_eq!(next.len(), 1, "the second destination waits for its retry");
        assert_eq!(next[0].channel_id, first);
        assert!(next[0].id > heads[0].id);

        let retried = NotificationRepository::due(&pool, at(3)).await.unwrap();
        let again = retried.iter().find(|d| d.channel_id == second).unwrap();
        assert_eq!(again.id, heads[1].id);
        assert_eq!(again.attempts, 1);
        assert_eq!(again.failure.as_deref(), Some("timed_out"));
    }

    #[tokio::test]
    async fn giving_up_fails_the_whole_queue_of_one_destination() {
        let pool = test_pool().await;
        let dead = channel(&pool, "dead", true, false).await;
        let alive = channel(&pool, "alive", true, false).await;
        let event = incident(&pool).await;
        for minute in [0, 1, 2] {
            NotificationRepository::enqueue(
                &pool,
                &about_event(Happening::Updated, event),
                at(minute),
            )
            .await
            .unwrap();
        }

        let failed = NotificationRepository::fail_waiting(&pool, dead, "status_404")
            .await
            .unwrap();

        assert_eq!(failed, 3);
        let heads = NotificationRepository::due(&pool, at(2)).await.unwrap();
        assert_eq!(heads.len(), 1);
        assert_eq!(heads[0].channel_id, alive);
        let outcomes = NotificationRepository::latest_outcomes(&pool)
            .await
            .unwrap();
        assert_eq!(outcomes[&dead].failure.as_deref(), Some("status_404"));
        assert!(!outcomes.contains_key(&alive));
    }

    #[tokio::test]
    async fn purge_keeps_waiting_messages_and_the_last_outcome() {
        let pool = test_pool().await;
        let id = channel(&pool, "team", true, false).await;
        let event = incident(&pool).await;
        for minute in [0, 1, 2] {
            NotificationRepository::enqueue(
                &pool,
                &about_event(Happening::Updated, event),
                at(minute),
            )
            .await
            .unwrap();
        }
        for _ in 0..2 {
            let head = NotificationRepository::due(&pool, at(2)).await.unwrap()[0].id;
            NotificationRepository::mark_sent(&pool, head, at(2))
                .await
                .unwrap();
        }

        let purged = NotificationRepository::purge(&pool, at(10)).await.unwrap();

        assert_eq!(purged, 1);
        let statuses: Vec<DeliveryStatus> =
            queued(&pool).await.into_iter().map(|row| row.2).collect();
        assert_eq!(statuses, [DeliveryStatus::Sent, DeliveryStatus::Pending]);
        let outcomes = NotificationRepository::latest_outcomes(&pool)
            .await
            .unwrap();
        assert_eq!(outcomes[&id].status, DeliveryStatus::Sent);
    }

    #[tokio::test]
    async fn deleting_an_event_drops_its_messages() {
        let pool = test_pool().await;
        channel(&pool, "team", true, false).await;
        let event = incident(&pool).await;
        NotificationRepository::enqueue(&pool, &about_event(Happening::Opened, event), at(0))
            .await
            .unwrap();

        EventRepository::delete(&pool, event).await.unwrap();

        assert_eq!(queued(&pool).await, Vec::new());
    }

    #[tokio::test]
    async fn failing_one_message_leaves_the_next_of_its_destination_waiting() {
        let pool = test_pool().await;
        let id = channel(&pool, "team", true, false).await;
        let event = incident(&pool).await;
        for minute in [0, 1] {
            NotificationRepository::enqueue(
                &pool,
                &about_event(Happening::Updated, event),
                at(minute),
            )
            .await
            .unwrap();
        }
        let head = NotificationRepository::due(&pool, at(1)).await.unwrap()[0].id;

        NotificationRepository::fail(&pool, head, "gone")
            .await
            .unwrap();

        let next = NotificationRepository::due(&pool, at(1)).await.unwrap();
        assert_eq!(next.len(), 1);
        assert!(next[0].id > head);
        let outcomes = NotificationRepository::latest_outcomes(&pool)
            .await
            .unwrap();
        assert_eq!(outcomes[&id].status, DeliveryStatus::Failed);
        assert_eq!(outcomes[&id].failure.as_deref(), Some("gone"));
        assert_eq!(outcomes[&id].attempts, 1);
    }
}
