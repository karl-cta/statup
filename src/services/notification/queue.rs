//! What happened, queued for the destinations that receive it.

use chrono::Utc;

use crate::db::DbPool;
use crate::models::Notification;
use crate::repositories::NotificationRepository;

/// Queues a message for every destination that receives it. A failure goes
/// to the log and no further: the change it reports is saved already, and a
/// message must never undo it.
pub async fn notify(pool: &DbPool, notification: Notification) {
    if let Err(e) = NotificationRepository::enqueue(pool, &notification, Utc::now()).await {
        tracing::warn!(
            error = %e,
            happening = notification.happening.as_str(),
            "Notification not queued"
        );
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Duration, DurationRound, Utc};

    use crate::db::DbPool;
    use crate::models::{
        Category, ChannelInput, ChannelKind, CreateEventInput, Happening, Kind, Lifecycle, Role,
        Severity, UpdateEventInput, User,
    };
    use crate::repositories::{NotificationRepository, UserRepository};
    use crate::services::EventService;
    use crate::test_helpers::test_pool;

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
            EventService::post_update(&pool, event.id, message, next, &author)
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
        EventService::apply_schedule(&pool).await.unwrap();

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
        EventService::post_update(&pool, note.id, "Now open on Mondays.", None, &author)
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
}
