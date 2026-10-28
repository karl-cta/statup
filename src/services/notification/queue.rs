//! What happened, queued for the destinations and subscribers that receive it.

use chrono::{DateTime, Utc};

use crate::db::DbPool;
use crate::models::Notification;
use crate::repositories::{NotificationRepository, SubscriberRepository};

/// Queues a message for every destination and every subscriber that receives
/// it. A failure goes to the log and no further: the change it reports is
/// saved already, and a message must never undo it. Destinations and
/// subscribers are queued apart, so one failing does not stop the other.
pub async fn notify(pool: &DbPool, notification: Notification) {
    let now = Utc::now();
    notify_later(pool, notification, now, now).await;
}

/// Queues a message that happened at `now` and leaves no sooner than
/// `send_at`.
pub async fn notify_later(
    pool: &DbPool,
    notification: Notification,
    now: DateTime<Utc>,
    send_at: DateTime<Utc>,
) {
    if let Err(e) = NotificationRepository::enqueue_after(pool, &notification, now, send_at).await {
        tracing::warn!(
            error = %e,
            happening = notification.happening.as_str(),
            "Notification not queued"
        );
    }
    if let Err(e) = SubscriberRepository::enqueue(pool, &notification, now, send_at).await {
        tracing::warn!(
            error = %e,
            happening = notification.happening.as_str(),
            "Subscriber emails not queued"
        );
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Duration, DurationRound, Utc};

    use super::notify;
    use crate::db::DbPool;
    use crate::models::{
        Category, ChannelInput, ChannelKind, Choices, CreateEventInput, Happening, Kind, Lifecycle,
        Notification, Role, Severity, UpdateEventInput, User,
    };
    use crate::repositories::{
        NotificationRepository, ServiceRepository, SubscriberRepository, UserRepository,
    };
    use crate::services::{EventService, SubscriptionService};
    use crate::test_helpers::test_pool;

    const ALL: Choices = Choices {
        incidents: true,
        maintenances: true,
        publications: true,
    };
    const INCIDENTS: Choices = Choices {
        incidents: true,
        maintenances: false,
        publications: false,
    };
    const MAINTENANCES: Choices = Choices {
        incidents: false,
        maintenances: true,
        publications: false,
    };
    const PUBLICATIONS: Choices = Choices {
        incidents: false,
        maintenances: false,
        publications: true,
    };

    /// A destination that receives everything.
    async fn everything(pool: &DbPool) {
        let input = ChannelInput {
            name: "Team".to_string(),
            kind: ChannelKind::Slack,
            target: "https://hooks.slack.com/services/team".to_string(),
            locale: "fr".to_string(),
            on_incidents: true,
            on_maintenances: true,
            on_publications: true,
            on_detected: true,
        };
        NotificationRepository::create_channel(pool, &input)
            .await
            .unwrap();
    }

    async fn admin(pool: &DbPool) -> User {
        UserRepository::create(pool, "ops@example.com", "hash", "Ops", Role::Admin)
            .await
            .unwrap()
    }

    fn new_event(kind: Kind, author: &User) -> CreateEventInput {
        CreateEventInput {
            kind,
            severity: (kind == Kind::Incident).then_some(Severity::Critical),
            planned: false,
            category: (kind == Kind::Publication).then_some(Category::Info),
            title: "Network down at the head office".to_string(),
            description: "The provider is on site.".to_string(),
            planned_start: None,
            planned_end: None,
            started_at: None,
            opening_step: None,
            keeps_services_up: false,
            service_ids: Vec::new(),
            follows_event_id: None,
            author_id: author.id,
            api_key_id: None,
            next_update: None,
        }
    }

    fn edit(title: &str, window: (DateTime<Utc>, DateTime<Utc>)) -> UpdateEventInput {
        UpdateEventInput {
            severity: None,
            planned: true,
            category: None,
            title: title.to_string(),
            description: String::new(),
            planned_start: Some(window.0),
            planned_end: Some(window.1),
            service_ids: Vec::new(),
            follows_event_id: None,
        }
    }

    /// A time a form could send: to the minute.
    fn in_hours(hours: i64) -> DateTime<Utc> {
        (Utc::now() + Duration::hours(hours))
            .duration_trunc(Duration::minutes(1))
            .unwrap()
    }

    /// What was queued, in order: what happened, the state, and whether an
    /// update goes with it.
    async fn queued(pool: &DbPool) -> Vec<(Happening, Option<Lifecycle>, bool)> {
        sqlx::query_as(
            "SELECT happening, lifecycle, update_id IS NOT NULL \
             FROM notification_deliveries ORDER BY id",
        )
        .fetch_all(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn an_incident_tells_its_opening_its_news_and_its_end() {
        let pool = test_pool().await;
        everything(&pool).await;
        let author = admin(&pool).await;
        let event = EventService::create(&pool, new_event(Kind::Incident, &author))
            .await
            .unwrap();

        let steps = [
            ("Cable replaced.", None),
            ("", Some(Lifecycle::InProgress)),
            ("All good.", Some(Lifecycle::Resolved)),
        ];
        for (message, next) in steps {
            EventService::post_update(&pool, event.id, message, next, None, &author, None)
                .await
                .unwrap();
        }
        EventService::revert(&pool, event.id, Role::Admin)
            .await
            .unwrap();

        let investigating = Some(Lifecycle::Investigating);
        let in_progress = Some(Lifecycle::InProgress);
        assert_eq!(
            queued(&pool).await,
            [
                (Happening::Opened, investigating, false),
                (Happening::Updated, investigating, true),
                (Happening::Updated, in_progress, false),
                (Happening::Closed, Some(Lifecycle::Resolved), true),
                (Happening::Updated, in_progress, false),
            ]
        );
    }

    #[tokio::test]
    async fn a_review_is_sent_once_to_whoever_receives_incidents() {
        let pool = test_pool().await;
        everything(&pool).await;
        let works = ChannelInput {
            name: "Works".to_string(),
            kind: ChannelKind::Slack,
            target: "https://hooks.slack.com/services/works".to_string(),
            locale: "fr".to_string(),
            on_incidents: false,
            on_maintenances: true,
            on_publications: false,
            on_detected: false,
        };
        NotificationRepository::create_channel(&pool, &works)
            .await
            .unwrap();
        let author = admin(&pool).await;
        let event = EventService::create(&pool, new_event(Kind::Incident, &author))
            .await
            .unwrap();
        let resolved = Some(Lifecycle::Resolved);
        EventService::post_update(&pool, event.id, "Fixed.", resolved, None, &author, None)
            .await
            .unwrap();

        for text in ["A cable was cut.", "A cable was cut by road works."] {
            EventService::publish_review(&pool, event.id, text, &author)
                .await
                .unwrap();
        }

        assert_eq!(
            queued(&pool).await,
            [
                (Happening::Opened, Some(Lifecycle::Investigating), false),
                (Happening::Closed, resolved, true),
                (Happening::Reviewed, resolved, false),
            ]
        );
    }

    #[tokio::test]
    async fn a_maintenance_tells_its_announce_its_new_date_its_start_and_its_end() {
        let pool = test_pool().await;
        everything(&pool).await;
        let author = admin(&pool).await;
        let window = (in_hours(24), in_hours(25));
        let mut input = new_event(Kind::Maintenance, &author);
        input.planned = true;
        input.planned_start = Some(window.0);
        input.planned_end = Some(window.1);
        let event = EventService::create(&pool, input).await.unwrap();

        EventService::update(&pool, event.id, edit("Server move", window), Role::Admin)
            .await
            .unwrap();
        let past = (in_hours(-2), in_hours(-1));
        EventService::update(&pool, event.id, edit("Server move", past), Role::Admin)
            .await
            .unwrap();
        EventService::apply_schedule(&pool, Utc::now())
            .await
            .unwrap();

        let scheduled = Some(Lifecycle::Scheduled);
        assert_eq!(
            queued(&pool).await,
            [
                (Happening::Opened, scheduled, false),
                (Happening::Rescheduled, scheduled, false),
                (Happening::Started, Some(Lifecycle::InProgress), false),
                (Happening::Closed, Some(Lifecycle::Completed), false),
            ]
        );
    }

    #[tokio::test]
    async fn urgent_work_begins_and_an_announcement_is_published() {
        let pool = test_pool().await;
        everything(&pool).await;
        let author = admin(&pool).await;

        EventService::create(&pool, new_event(Kind::Maintenance, &author))
            .await
            .unwrap();
        let note = EventService::create(&pool, new_event(Kind::Publication, &author))
            .await
            .unwrap();
        EventService::post_update(
            &pool,
            note.id,
            "Now open on Mondays.",
            None,
            None,
            &author,
            None,
        )
        .await
        .unwrap();

        assert_eq!(
            queued(&pool).await,
            [
                (Happening::Started, Some(Lifecycle::InProgress), false),
                (Happening::Published, None, false),
                (Happening::Updated, None, true),
            ]
        );
    }

    #[tokio::test]
    async fn an_edit_or_a_deletion_tells_nobody() {
        let pool = test_pool().await;
        let author = admin(&pool).await;
        let event = EventService::create(&pool, new_event(Kind::Incident, &author))
            .await
            .unwrap();
        everything(&pool).await;

        let mut fix = edit(
            "Network down at the main office",
            (in_hours(1), in_hours(2)),
        );
        fix.planned = false;
        fix.planned_start = None;
        fix.planned_end = None;
        fix.severity = Some(Severity::Minor);
        EventService::update(&pool, event.id, fix, Role::Admin)
            .await
            .unwrap();
        EventService::delete(&pool, event.id, Role::Admin)
            .await
            .unwrap();

        assert_eq!(queued(&pool).await, Vec::new());
    }

    /// A visitor subscribed at `email`. Their subscriber id.
    async fn visitor(pool: &DbPool, email: &str, choices: Choices) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO subscribers (email, locale, on_incidents, on_maintenances, \
             on_publications, unsubscribe_secret) VALUES (?, 'en', ?, ?, ?, 'secret') \
             RETURNING id",
        )
        .bind(email)
        .bind(choices.incidents)
        .bind(choices.maintenances)
        .bind(choices.publications)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    /// A member with an account at `email`, not subscribed.
    async fn account(pool: &DbPool, email: &str) -> User {
        UserRepository::create(pool, email, "hash", "Member", Role::Reader)
            .await
            .unwrap()
    }

    /// A member subscribed at the address of their account. Their user id
    /// and subscriber id.
    async fn member(pool: &DbPool, email: &str, choices: Choices) -> (i64, i64) {
        let user = account(pool, email).await;
        SubscriptionService::save_member(pool, user.id, "en", choices)
            .await
            .unwrap();
        let subscriber = SubscriberRepository::find_for_member(pool, user.id)
            .await
            .unwrap()
            .unwrap();
        (user.id, subscriber.id)
    }

    async fn open_to_visitors(pool: &DbPool) {
        SubscriptionService::set_visitors_open(pool, true)
            .await
            .unwrap();
    }

    async fn open_incident(pool: &DbPool, author: &User) {
        EventService::create(pool, new_event(Kind::Incident, author))
            .await
            .unwrap();
    }

    /// What was queued for subscribers, in order: who, what happened, the
    /// state, and whether an update goes with it.
    async fn queued_for_subscribers(
        pool: &DbPool,
    ) -> Vec<(i64, Happening, Option<Lifecycle>, bool)> {
        sqlx::query_as(
            "SELECT subscriber_id, happening, lifecycle, update_id IS NOT NULL \
             FROM subscriber_deliveries ORDER BY id",
        )
        .fetch_all(pool)
        .await
        .unwrap()
    }

    async fn recipients(pool: &DbPool) -> Vec<i64> {
        queued_for_subscribers(pool)
            .await
            .into_iter()
            .map(|(subscriber_id, ..)| subscriber_id)
            .collect()
    }

    #[tokio::test]
    async fn an_incident_reaches_the_subscribers_of_incidents_to_its_review() {
        let pool = test_pool().await;
        open_to_visitors(&pool).await;
        let reader = visitor(&pool, "ana@example.com", INCIDENTS).await;
        visitor(&pool, "ben@example.com", MAINTENANCES).await;
        let author = admin(&pool).await;
        let event = EventService::create(&pool, new_event(Kind::Incident, &author))
            .await
            .unwrap();

        let resolved = Some(Lifecycle::Resolved);
        let steps = [
            ("Cable replaced.", None),
            ("", Some(Lifecycle::InProgress)),
            ("All good.", resolved),
        ];
        for (message, next) in steps {
            EventService::post_update(&pool, event.id, message, next, None, &author, None)
                .await
                .unwrap();
        }
        EventService::publish_review(&pool, event.id, "A cable was cut.", &author)
            .await
            .unwrap();

        let investigating = Some(Lifecycle::Investigating);
        let in_progress = Some(Lifecycle::InProgress);
        assert_eq!(
            queued_for_subscribers(&pool).await,
            [
                (reader, Happening::Opened, investigating, false),
                (reader, Happening::Updated, investigating, true),
                (reader, Happening::Updated, in_progress, false),
                (reader, Happening::Closed, resolved, true),
                (reader, Happening::Reviewed, resolved, false),
            ]
        );
    }

    #[tokio::test]
    async fn a_maintenance_and_an_announcement_reach_only_their_own_subscribers() {
        let pool = test_pool().await;
        open_to_visitors(&pool).await;
        visitor(&pool, "ana@example.com", INCIDENTS).await;
        let works = visitor(&pool, "ben@example.com", MAINTENANCES).await;
        let news = visitor(&pool, "cleo@example.com", PUBLICATIONS).await;
        let author = admin(&pool).await;
        let window = (in_hours(24), in_hours(25));
        let mut input = new_event(Kind::Maintenance, &author);
        input.planned = true;
        input.planned_start = Some(window.0);
        input.planned_end = Some(window.1);

        EventService::create(&pool, input).await.unwrap();
        EventService::create(&pool, new_event(Kind::Publication, &author))
            .await
            .unwrap();

        assert_eq!(
            queued_for_subscribers(&pool).await,
            [
                (works, Happening::Opened, Some(Lifecycle::Scheduled), false),
                (news, Happening::Published, None, false),
            ]
        );
    }

    #[tokio::test]
    async fn a_detected_outage_is_never_sent_to_subscribers() {
        let pool = test_pool().await;
        everything(&pool).await;
        open_to_visitors(&pool).await;
        visitor(&pool, "ana@example.com", ALL).await;
        member(&pool, "ben@example.com", ALL).await;
        let service = ServiceRepository::create(&pool, "API", "api", None, None, None)
            .await
            .unwrap();

        notify(
            &pool,
            Notification::service(Happening::ServiceDown, service.id),
        )
        .await;

        assert_eq!(queued(&pool).await.len(), 1, "the destination is told");
        assert_eq!(queued_for_subscribers(&pool).await, Vec::new());
    }

    #[tokio::test]
    async fn a_disabled_member_is_not_queued() {
        let pool = test_pool().await;
        let (gone, _) = member(&pool, "ana@example.com", ALL).await;
        let (_, staying) = member(&pool, "ben@example.com", ALL).await;
        UserRepository::set_active(&pool, gone, false)
            .await
            .unwrap();
        let author = admin(&pool).await;

        open_incident(&pool, &author).await;

        assert_eq!(recipients(&pool).await, [staying]);
    }

    #[tokio::test]
    async fn visitors_wait_for_the_switch_but_members_do_not() {
        let pool = test_pool().await;
        visitor(&pool, "ana@example.com", ALL).await;
        let (_, member_id) = member(&pool, "ben@example.com", ALL).await;
        let author = admin(&pool).await;

        open_incident(&pool, &author).await;

        assert_eq!(recipients(&pool).await, [member_id]);
    }

    #[tokio::test]
    async fn a_person_who_is_member_and_visitor_gets_one_email() {
        let pool = test_pool().await;
        open_to_visitors(&pool).await;
        let (_, member_id) = member(&pool, "ana@example.com", ALL).await;
        visitor(&pool, "ANA@example.com", ALL).await;
        let author = admin(&pool).await;

        open_incident(&pool, &author).await;

        assert_eq!(recipients(&pool).await, [member_id]);
    }

    #[tokio::test]
    async fn the_visitor_is_queued_when_the_member_would_not_get_the_email() {
        let pool = test_pool().await;
        open_to_visitors(&pool).await;
        let author = admin(&pool).await;
        account(&pool, "ana@example.com").await;
        member(&pool, "ben@example.com", MAINTENANCES).await;
        let disabled = member(&pool, "cleo@example.com", ALL).await;
        UserRepository::set_active(&pool, disabled.0, false)
            .await
            .unwrap();
        let ana = visitor(&pool, "ANA@example.com", ALL).await;
        let ben = visitor(&pool, "BEN@example.com", ALL).await;
        let cleo = visitor(&pool, "CLEO@example.com", ALL).await;

        open_incident(&pool, &author).await;

        assert_eq!(recipients(&pool).await, [ana, ben, cleo]);
    }
}
