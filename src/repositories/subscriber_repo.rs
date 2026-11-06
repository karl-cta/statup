//! The members and visitors told by email about what the page publishes.

use chrono::{DateTime, Utc};

use crate::clock;
use crate::db::DbPool;
use crate::models::{
    Audience, Choices, Notification, Subscriber, SubscriberRow, SubscriptionRequest,
};

/// Setting that opens the page to visitors' subscriptions, `true` when open.
/// Absent means closed.
pub const VISITORS_SETTING: &str = "visitor_subscriptions";

const SUBSCRIBER_COLUMNS: &str = "id, user_id, email, on_incidents, on_maintenances, \
     on_publications, unsubscribe_secret, created_at";

pub struct SubscriberRepository;

impl SubscriberRepository {
    pub async fn find(pool: &DbPool, id: i64) -> Result<Option<Subscriber>, sqlx::Error> {
        sqlx::query_as::<_, Subscriber>(&format!(
            "SELECT {SUBSCRIBER_COLUMNS} FROM subscribers WHERE id = ?"
        ))
        .bind(id)
        .fetch_optional(pool)
        .await
    }

    pub async fn find_for_member(
        pool: &DbPool,
        user_id: i64,
    ) -> Result<Option<Subscriber>, sqlx::Error> {
        sqlx::query_as::<_, Subscriber>(&format!(
            "SELECT {SUBSCRIBER_COLUMNS} FROM subscribers WHERE user_id = ?"
        ))
        .bind(user_id)
        .fetch_optional(pool)
        .await
    }

    /// Subscribes the visitor of a confirmed request, or gives the address
    /// already subscribed the choices of its newest request, and uses up
    /// every request of the address, all or nothing. A new subscriber gets
    /// `unsubscribe_secret`; one already there keeps theirs, so the links of
    /// the emails they received still work. Returns `None`, changing
    /// nothing, when the address has no request left: of two clicks at the
    /// same moment, only the first counts.
    pub async fn confirm_visitor(
        pool: &DbPool,
        request: &SubscriptionRequest,
        unsubscribe_secret: &str,
    ) -> Result<Option<i64>, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let used = sqlx::query("DELETE FROM subscription_requests WHERE email = ?")
            .bind(&request.email)
            .execute(&mut *tx)
            .await?;
        if used.rows_affected() == 0 {
            return Ok(None);
        }
        let id = sqlx::query_scalar(
            "INSERT INTO subscribers (email, locale, on_incidents, on_maintenances, \
             on_publications, unsubscribe_secret) VALUES (?, ?, ?, ?, ?, ?) \
             ON CONFLICT(email) DO UPDATE SET locale = excluded.locale, \
               on_incidents = excluded.on_incidents, \
               on_maintenances = excluded.on_maintenances, \
               on_publications = excluded.on_publications, \
               confirmed_at = datetime('now') \
             RETURNING id",
        )
        .bind(&request.email)
        .bind(&request.locale)
        .bind(request.choices.incidents)
        .bind(request.choices.maintenances)
        .bind(request.choices.publications)
        .bind(unsubscribe_secret)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(id))
    }

    /// Subscribes a member at the address of their account, or replaces
    /// their choices. Returns true when the subscription is new.
    pub async fn save_member(
        pool: &DbPool,
        user_id: i64,
        locale: &str,
        choices: Choices,
        unsubscribe_secret: &str,
    ) -> Result<bool, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let created = sqlx::query(
            "INSERT INTO subscribers (user_id, locale, on_incidents, on_maintenances, \
             on_publications, unsubscribe_secret) VALUES (?, ?, ?, ?, ?, ?) \
             ON CONFLICT(user_id) DO NOTHING",
        )
        .bind(user_id)
        .bind(locale)
        .bind(choices.incidents)
        .bind(choices.maintenances)
        .bind(choices.publications)
        .bind(unsubscribe_secret)
        .execute(&mut *tx)
        .await?
        .rows_affected()
            > 0;
        if !created {
            sqlx::query(
                "UPDATE subscribers SET locale = ?, on_incidents = ?, on_maintenances = ?, \
                 on_publications = ?, confirmed_at = datetime('now') WHERE user_id = ?",
            )
            .bind(locale)
            .bind(choices.incidents)
            .bind(choices.maintenances)
            .bind(choices.publications)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(created)
    }

    /// Queues `notification` for every subscriber who ticked its kind of
    /// event; the message leaves no sooner than `send_at`. Detected outages
    /// and tests are never sent to subscribers. A disabled member is left
    /// out, and so are visitors while the page is closed to them. A visitor
    /// whose address is the account address of an active member who receives
    /// the same kind of event is left out too, so that person gets one email.
    /// Returns how many messages were queued.
    pub async fn enqueue(
        pool: &DbPool,
        notification: &Notification,
        now: DateTime<Utc>,
        send_at: DateTime<Utc>,
    ) -> Result<u64, sqlx::Error> {
        if notification.audience == Audience::DetectedOutages {
            return Ok(0);
        }
        let column = notification.audience.column();
        let Some(event_id) = notification.event_id else {
            return Ok(0);
        };
        let sql = format!(
            "INSERT INTO subscriber_deliveries (subscriber_id, happening, event_id, update_id, \
             lifecycle, next_attempt_at, created_at) \
             SELECT s.id, ?, ?, ?, ?, ?, ? FROM subscribers s \
             LEFT JOIN users u ON u.id = s.user_id \
             WHERE s.{column} = 1 AND (\
               (s.user_id IS NOT NULL AND u.is_active = 1) \
               OR (s.user_id IS NULL \
                   AND (SELECT value FROM settings WHERE key = ?) = 'true' \
                   AND NOT EXISTS (SELECT 1 FROM subscribers m \
                                   JOIN users mu ON mu.id = m.user_id \
                                   WHERE mu.is_active = 1 AND m.{column} = 1 \
                                   AND s.email = mu.email)))"
        );
        let queued = sqlx::query(&sql)
            .bind(notification.happening)
            .bind(event_id)
            .bind(notification.update_id)
            .bind(notification.lifecycle)
            .bind(clock::db(send_at))
            .bind(clock::db(now))
            .bind(VISITORS_SETTING)
            .execute(pool)
            .await?;
        Ok(queued.rows_affected())
    }

    /// How many members and visitors are subscribed.
    pub async fn count(pool: &DbPool) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar("SELECT COUNT(*) FROM subscribers")
            .fetch_one(pool)
            .await
    }

    /// Every subscriber, members first, each group by address, with how
    /// their latest sent or failed email went.
    pub async fn list_for_admin(pool: &DbPool) -> Result<Vec<SubscriberRow>, sqlx::Error> {
        sqlx::query_as(
            "SELECT s.id, s.user_id, u.display_name AS name, \
                    COALESCE(u.email, s.email) AS email, \
                    s.on_incidents, s.on_maintenances, s.on_publications, s.created_at, \
                    d.status AS last_status, d.failure AS last_failure, \
                    CASE d.status WHEN 'sent' THEN d.sent_at ELSE d.next_attempt_at END \
                      AS last_at \
             FROM subscribers s \
             LEFT JOIN users u ON u.id = s.user_id \
             LEFT JOIN subscriber_deliveries d ON d.id = (\
               SELECT MAX(l.id) FROM subscriber_deliveries l \
               WHERE l.subscriber_id = s.id AND l.status IN ('sent', 'failed')) \
             ORDER BY s.user_id IS NULL, email",
        )
        .fetch_all(pool)
        .await
    }

    /// Ends a subscription, and with it the emails it still had to receive.
    pub async fn delete(pool: &DbPool, id: i64) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM subscribers WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CreateEventInput, Kind, Role, Severity};
    use crate::repositories::{
        EventRepository, NewSubscriptionRequest, RequestLimits, SubscriptionRequestRepository,
        UserRepository,
    };
    use crate::test_helpers::test_pool;

    const EVERYTHING: Choices = Choices {
        incidents: true,
        maintenances: true,
        publications: true,
    };
    const ONLY_INCIDENTS: Choices = Choices {
        incidents: true,
        maintenances: false,
        publications: false,
    };

    async fn request(pool: &DbPool, email: &str, choices: Choices) -> SubscriptionRequest {
        let new = NewSubscriptionRequest {
            email,
            secret_hash: "hash",
            locale: "fr",
            choices,
        };
        let limits = RequestLimits {
            per_address: 100,
            per_instance: 100,
        };
        let id = SubscriptionRequestRepository::insert_within_limits(pool, &new, limits)
            .await
            .unwrap()
            .unwrap();
        SubscriptionRequestRepository::find_live(pool, id)
            .await
            .unwrap()
            .unwrap()
    }

    async fn confirmed(pool: &DbPool, email: &str, choices: Choices, secret: &str) -> i64 {
        let request = request(pool, email, choices).await;
        SubscriberRepository::confirm_visitor(pool, &request, secret)
            .await
            .unwrap()
            .unwrap()
    }

    async fn member(pool: &DbPool, email: &str) -> i64 {
        UserRepository::create(pool, email, "hash", "Someone", Role::Reader)
            .await
            .unwrap()
            .id
    }

    async fn count(pool: &DbPool, table: &str) -> i64 {
        sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn incident(pool: &DbPool) -> i64 {
        let author_id = member(pool, "ops@example.com").await;
        let input = CreateEventInput {
            kind: Kind::Incident,
            severity: Some(Severity::Minor),
            planned: false,
            category: None,
            title: "Network down".to_string(),
            description: String::new(),
            planned_start: None,
            planned_end: None,
            started_at: None,
            opening_step: None,
            keeps_services_up: false,
            service_ids: Vec::new(),
            follows_event_id: None,
            author_id,
            api_key_id: None,
            next_update: None,
        };
        EventRepository::create(pool, &input).await.unwrap().id
    }

    async fn queue_for(pool: &DbPool, subscriber_id: i64, event_id: i64, happening: &str) -> bool {
        sqlx::query(
            "INSERT INTO subscriber_deliveries (subscriber_id, happening, event_id) \
             VALUES (?, ?, ?)",
        )
        .bind(subscriber_id)
        .bind(happening)
        .bind(event_id)
        .execute(pool)
        .await
        .is_ok()
    }

    async fn stored_locale(pool: &DbPool, id: i64) -> String {
        sqlx::query_scalar("SELECT locale FROM subscribers WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn confirming_subscribes_the_visitor_with_their_choices() {
        let pool = test_pool().await;

        let id = confirmed(&pool, "ana@example.com", ONLY_INCIDENTS, "secret").await;

        let subscriber = SubscriberRepository::find(&pool, id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(subscriber.email.as_deref(), Some("ana@example.com"));
        assert_eq!(subscriber.user_id, None);
        assert_eq!(stored_locale(&pool, id).await, "fr");
        assert_eq!(subscriber.choices, ONLY_INCIDENTS);
        assert_eq!(subscriber.unsubscribe_secret, "secret");
        assert_eq!(count(&pool, "subscription_requests").await, 0);
    }

    #[tokio::test]
    async fn a_second_click_on_a_used_link_changes_nothing() {
        let pool = test_pool().await;
        let request = request(&pool, "ana@example.com", EVERYTHING).await;
        SubscriberRepository::confirm_visitor(&pool, &request, "first")
            .await
            .unwrap();

        let again = SubscriberRepository::confirm_visitor(&pool, &request, "second")
            .await
            .unwrap();

        assert_eq!(again, None);
        assert_eq!(count(&pool, "subscribers").await, 1);
    }

    #[tokio::test]
    async fn subscribing_again_replaces_the_choices_and_keeps_the_secret() {
        let pool = test_pool().await;
        let first = confirmed(&pool, "ana@example.com", EVERYTHING, "kept").await;

        let second = confirmed(&pool, "ANA@example.com", ONLY_INCIDENTS, "new").await;

        assert_eq!(first, second, "one address, one subscriber");
        let subscriber = SubscriberRepository::find(&pool, first)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(subscriber.choices, ONLY_INCIDENTS);
        assert_eq!(subscriber.unsubscribe_secret, "kept");
    }

    #[tokio::test]
    async fn a_member_subscribes_once_and_changes_their_choices() {
        let pool = test_pool().await;
        let ana = member(&pool, "ana@example.com").await;

        let created = SubscriberRepository::save_member(&pool, ana, "fr", EVERYTHING, "s1")
            .await
            .unwrap();
        let again = SubscriberRepository::save_member(&pool, ana, "en", ONLY_INCIDENTS, "s2")
            .await
            .unwrap();

        assert!(created);
        assert!(!again);
        let subscriber = SubscriberRepository::find_for_member(&pool, ana)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(subscriber.email, None);
        assert_eq!(stored_locale(&pool, subscriber.id).await, "en");
        assert_eq!(subscriber.choices, ONLY_INCIDENTS);
        assert_eq!(subscriber.unsubscribe_secret, "s1");
    }

    #[tokio::test]
    async fn a_subscriber_is_a_member_or_an_address_never_both_nor_neither() {
        let pool = test_pool().await;
        let ana = member(&pool, "ana@example.com").await;
        let insert = |user_id: Option<i64>, email: Option<&'static str>| {
            let pool = pool.clone();
            async move {
                sqlx::query(
                    "INSERT INTO subscribers (user_id, email, locale, unsubscribe_secret) \
                     VALUES (?, ?, 'fr', 's')",
                )
                .bind(user_id)
                .bind(email)
                .execute(&pool)
                .await
                .is_ok()
            }
        };

        assert!(!insert(Some(ana), Some("ana@example.com")).await);
        assert!(!insert(None, None).await);
        assert!(insert(Some(ana), None).await);
        assert!(insert(None, Some("ana@example.com")).await);
    }

    #[tokio::test]
    async fn ending_a_subscription_drops_its_waiting_emails() {
        let pool = test_pool().await;
        let id = confirmed(&pool, "ana@example.com", EVERYTHING, "s").await;
        let event_id = incident(&pool).await;
        assert!(queue_for(&pool, id, event_id, "opened").await);

        assert!(SubscriberRepository::delete(&pool, id).await.unwrap());

        assert!(!SubscriberRepository::delete(&pool, id).await.unwrap());
        assert_eq!(count(&pool, "subscriber_deliveries").await, 0);
    }

    #[tokio::test]
    async fn deleting_a_member_or_an_event_takes_what_hangs_on_them() {
        let pool = test_pool().await;
        let ana = member(&pool, "ana@example.com").await;
        SubscriberRepository::save_member(&pool, ana, "fr", EVERYTHING, "s")
            .await
            .unwrap();
        let visitor = confirmed(&pool, "ben@example.com", EVERYTHING, "s").await;
        let event_id = incident(&pool).await;
        assert!(queue_for(&pool, visitor, event_id, "opened").await);

        sqlx::query("DELETE FROM events WHERE id = ?")
            .bind(event_id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM users WHERE id = ?")
            .bind(ana)
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(count(&pool, "subscriber_deliveries").await, 0);
        assert_eq!(count(&pool, "subscribers").await, 1, "the visitor stays");
    }

    async fn finish(
        pool: &DbPool,
        subscriber_id: i64,
        event_id: i64,
        status: &str,
        failure: Option<&str>,
        at: &str,
    ) {
        sqlx::query(
            "INSERT INTO subscriber_deliveries (subscriber_id, happening, event_id, status, \
             failure, sent_at, next_attempt_at) VALUES (?, 'opened', ?, ?, ?, ?, ?)",
        )
        .bind(subscriber_id)
        .bind(event_id)
        .bind(status)
        .bind(failure)
        .bind(at)
        .bind(at)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn members_and_visitors_are_counted_together() {
        let pool = test_pool().await;
        assert_eq!(SubscriberRepository::count(&pool).await.unwrap(), 0);
        let ana = member(&pool, "ana@example.com").await;
        SubscriberRepository::save_member(&pool, ana, "fr", EVERYTHING, "s")
            .await
            .unwrap();
        confirmed(&pool, "ben@example.com", EVERYTHING, "s").await;
        confirmed(&pool, "cat@example.com", ONLY_INCIDENTS, "s").await;

        assert_eq!(SubscriberRepository::count(&pool).await.unwrap(), 3);
    }

    #[tokio::test]
    async fn the_list_shows_members_first_then_visitors_by_address() {
        let pool = test_pool().await;
        confirmed(&pool, "zed@example.com", ONLY_INCIDENTS, "s").await;
        confirmed(&pool, "ben@example.com", EVERYTHING, "s").await;
        let ana = member(&pool, "ana@example.com").await;
        SubscriberRepository::save_member(&pool, ana, "fr", EVERYTHING, "s")
            .await
            .unwrap();

        let rows = SubscriberRepository::list_for_admin(&pool).await.unwrap();

        let listed: Vec<_> = rows.iter().map(|r| r.email.as_str()).collect();
        assert_eq!(
            listed,
            ["ana@example.com", "ben@example.com", "zed@example.com"]
        );
        assert_eq!(rows[0].user_id, Some(ana));
        assert_eq!(rows[0].name.as_deref(), Some("Someone"));
        assert_eq!(rows[1].name, None);
        assert_eq!(rows[2].choices, ONLY_INCIDENTS);
    }

    #[tokio::test]
    async fn the_list_shows_the_latest_email_that_was_sent_or_failed() {
        let pool = test_pool().await;
        let event_id = incident(&pool).await;
        let ana = confirmed(&pool, "ana@example.com", EVERYTHING, "s").await;
        let ben = confirmed(&pool, "ben@example.com", EVERYTHING, "s").await;
        confirmed(&pool, "cat@example.com", EVERYTHING, "s").await;
        finish(
            &pool,
            ana,
            event_id,
            "failed",
            Some("status_550"),
            "2026-10-01 08:00:00",
        )
        .await;
        finish(&pool, ana, event_id, "sent", None, "2026-10-02 08:00:00").await;
        finish(
            &pool,
            ana,
            event_id,
            "skipped",
            Some("hidden"),
            "2026-10-03 08:00:00",
        )
        .await;
        finish(&pool, ben, event_id, "sent", None, "2026-10-02 08:00:00").await;
        finish(
            &pool,
            ben,
            event_id,
            "failed",
            Some("status_550"),
            "2026-10-04 08:00:00",
        )
        .await;
        assert!(queue_for(&pool, ben, event_id, "closed").await);

        let rows = SubscriberRepository::list_for_admin(&pool).await.unwrap();

        assert_eq!(rows[0].last_status.as_deref(), Some("sent"));
        assert_eq!(rows[0].last_failure, None);
        assert_eq!(
            rows[0].last_at.map(|at| at.to_rfc3339()),
            Some("2026-10-02T08:00:00+00:00".to_string())
        );
        assert_eq!(rows[1].last_status.as_deref(), Some("failed"));
        assert_eq!(rows[1].last_failure.as_deref(), Some("status_550"));
        assert_eq!(
            rows[1].last_at.map(|at| at.to_rfc3339()),
            Some("2026-10-04T08:00:00+00:00".to_string())
        );
        assert_eq!(rows[2].last_status, None);
        assert_eq!(rows[2].last_at, None);
    }

    #[tokio::test]
    async fn only_published_news_can_be_queued_for_a_subscriber() {
        let pool = test_pool().await;
        let id = confirmed(&pool, "ana@example.com", EVERYTHING, "s").await;
        let event_id = incident(&pool).await;

        for happening in ["service_down", "service_up", "test"] {
            assert!(
                !queue_for(&pool, id, event_id, happening).await,
                "{happening}"
            );
        }
        assert!(queue_for(&pool, id, event_id, "reviewed").await);
    }
}
