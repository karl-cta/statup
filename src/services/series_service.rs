//! Repeating maintenances: announced once, each session added by the
//! schedule in silence.

use chrono::{DateTime, Utc};

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::{
    Happening, Kind, Lifecycle, MaintenanceSeries, NewSeries, Notification, Role, UpdateEventInput,
};
use crate::repositories::{EventRepository, SeriesRepository};
use crate::services::{EventService, notify};

pub struct SeriesService;

impl SeriesService {
    /// Gives every repeating maintenance with nothing to come its next
    /// session, and returns how many were added. Nothing is sent: the series
    /// was announced once. One series failing does not hold up the others.
    pub async fn add_due_sessions(pool: &DbPool, now: DateTime<Utc>) -> Result<usize, AppError> {
        let mut added = 0;
        for series in SeriesRepository::awaiting_session(pool).await? {
            match add_next_session(pool, &series, now).await {
                Ok(true) => added += 1,
                Ok(false) => {}
                Err(e) => tracing::warn!(
                    series = series.id,
                    error = %e,
                    "Repeating maintenance session not added"
                ),
            }
        }
        Ok(added)
    }

    /// Stops the maintenance a session belongs to from repeating. The
    /// session waiting to begin is cancelled and told, as a cancellation
    /// by hand would be; one under way runs to its end.
    pub async fn stop(pool: &DbPool, session_id: i64) -> Result<(), AppError> {
        let series_id = EventRepository::find_by_id(pool, session_id)
            .await?
            .and_then(|session| session.series_id)
            .ok_or(AppError::NotFound)?;
        let Some(cancelled) = SeriesRepository::stop(pool, series_id, Utc::now()).await? else {
            return Ok(());
        };
        let scheduled = Some(Lifecycle::Scheduled);
        let happening = Happening::of_move(Kind::Maintenance, scheduled, Lifecycle::Cancelled);
        for id in cancelled {
            let cancelled = Some(Lifecycle::Cancelled);
            notify(
                pool,
                Notification::event(happening, Kind::Maintenance, id, cancelled),
            )
            .await;
        }
        Ok(())
    }

    /// Saves the edit of a session, then carries its title, text,
    /// services, time of day and length to the sessions after it. The day
    /// it now falls on sets the rhythm's day, or its rank in the month.
    pub async fn update_with_following(
        pool: &DbPool,
        id: i64,
        changes: UpdateEventInput,
        role: Role,
    ) -> Result<(), AppError> {
        if changes.planned_end.is_none() {
            return Err(AppError::validation("validation.repeat_needs_duration"));
        }
        let service_ids = changes.service_ids.clone();
        EventService::update(pool, id, changes, role).await?;
        let session = EventRepository::find_by_id(pool, id)
            .await?
            .ok_or(AppError::NotFound)?;
        let Some(series_id) = session.series_id else {
            return Ok(());
        };
        let Some(series) = SeriesRepository::find(pool, series_id).await? else {
            return Ok(());
        };
        let start = session.planned_start.or(session.started_at);
        let carried = start.zip(session.planned_end).and_then(|(start, end)| {
            NewSeries::from_window(series.recurrence.rhythm(), start, end)
        });
        let following = carried
            .zip(start)
            .and_then(|(carried, start)| carried.recurrence.next_after(start.max(Utc::now())));
        let (Some(carried), Some(following)) = (carried, following) else {
            return Err(AppError::validation("validation.repeat_needs_duration"));
        };
        SeriesRepository::carry_forward(pool, &session, &carried, &service_ids, following).await?;
        Ok(())
    }
}

async fn add_next_session(
    pool: &DbPool,
    series: &MaintenanceSeries,
    now: DateTime<Utc>,
) -> Result<bool, AppError> {
    let start = series.next_session_start(now);
    let following = start.and_then(|start| series.recurrence.next_after(start));
    let (Some(start), Some(following)) = (start, following) else {
        tracing::warn!(series = series.id, "Repeating maintenance has no next date");
        return Ok(false);
    };
    let service_ids = SeriesRepository::service_ids(pool, series.id).await?;
    let session = series.session(start, service_ids);
    let added = SeriesRepository::add_session(pool, series, &session, following).await?;
    Ok(added.is_some())
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};

    use super::*;
    use crate::models::{
        ChannelInput, ChannelKind, CreateEventInput, Kind, Lifecycle, NewSeries, Rhythm, Role,
        Scope, UpdateEventInput,
    };
    use crate::repositories::{
        EventRepository, NotificationRepository, ServiceRepository, UserRepository,
    };
    use crate::services::EventService;
    use crate::test_helpers::test_pool;

    /// Sunday 15 November 2026: no change of clock for weeks after it.
    fn first_start() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 15, 2, 0, 0).unwrap()
    }

    fn weeks_after_first(weeks: i64) -> DateTime<Utc> {
        first_start() + Duration::weeks(weeks)
    }

    async fn author_and_service(pool: &DbPool) -> (i64, i64) {
        let user = UserRepository::create(pool, "ops@example.com", "hash", "Ops", Role::Admin)
            .await
            .unwrap();
        let service = ServiceRepository::create(pool, "Backups", "backups", None, None, None)
            .await
            .unwrap();
        (user.id, service.id)
    }

    fn maintenance(author_id: i64, service_id: i64, start: DateTime<Utc>) -> CreateEventInput {
        CreateEventInput {
            kind: Kind::Maintenance,
            severity: None,
            planned: true,
            category: None,
            title: "Sunday backups".to_string(),
            description: String::new(),
            planned_start: Some(start),
            planned_end: Some(start + Duration::hours(2)),
            started_at: None,
            opening_step: None,
            keeps_services_up: false,
            service_ids: vec![service_id],
            follows_event_id: None,
            author_id,
            api_key_id: None,
            next_update: None,
        }
    }

    /// A weekly maintenance whose first session is on `first_start`.
    async fn weekly(pool: &DbPool) -> i64 {
        let (author, service) = author_and_service(pool).await;
        weekly_by(pool, author, service).await
    }

    async fn weekly_by(pool: &DbPool, author: i64, service: i64) -> i64 {
        let first = maintenance(author, service, first_start());
        let series = NewSeries::from_first(Rhythm::Weekly, &first).unwrap();
        let event = SeriesRepository::create(pool, &series, &first)
            .await
            .unwrap();
        event.series_id.unwrap()
    }

    /// Start and state of each session of the series, oldest first.
    async fn sessions(pool: &DbPool, series_id: i64) -> Vec<(DateTime<Utc>, Lifecycle)> {
        sqlx::query_as(
            "SELECT planned_start, lifecycle FROM events WHERE series_id = ? ORDER BY id",
        )
        .bind(series_id)
        .fetch_all(pool)
        .await
        .unwrap()
    }

    async fn run_at(pool: &DbPool, now: DateTime<Utc>) {
        EventService::apply_schedule(pool, now).await.unwrap();
    }

    #[tokio::test]
    async fn the_next_session_is_added_once_the_one_ahead_begins() {
        let pool = test_pool().await;
        let series = weekly(&pool).await;

        run_at(&pool, first_start() - Duration::days(1)).await;
        assert_eq!(
            sessions(&pool, series).await,
            [(first_start(), Lifecycle::Scheduled)]
        );

        run_at(&pool, first_start() + Duration::minutes(1)).await;
        assert_eq!(
            sessions(&pool, series).await,
            [
                (first_start(), Lifecycle::InProgress),
                (weeks_after_first(1), Lifecycle::Scheduled),
            ]
        );

        run_at(&pool, first_start() + Duration::hours(2)).await;
        assert_eq!(
            sessions(&pool, series).await,
            [
                (first_start(), Lifecycle::Completed),
                (weeks_after_first(1), Lifecycle::Scheduled),
            ]
        );
        let found = SeriesRepository::find(&pool, series)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.next_start, weeks_after_first(2));
    }

    #[tokio::test]
    async fn no_session_is_caught_up_after_the_server_was_off() {
        let pool = test_pool().await;
        let series = weekly(&pool).await;

        run_at(&pool, weeks_after_first(3) + Duration::hours(1)).await;

        assert_eq!(
            sessions(&pool, series).await,
            [
                (first_start(), Lifecycle::Completed),
                (weeks_after_first(4), Lifecycle::Scheduled),
            ]
        );
    }

    #[tokio::test]
    async fn the_sessions_of_a_series_start_and_end_in_silence() {
        let pool = test_pool().await;
        everything(&pool).await;
        let (author, service) = author_and_service(&pool).await;
        let series = weekly_by(&pool, author, service).await;
        let single = EventRepository::create(&pool, &maintenance(author, service, first_start()))
            .await
            .unwrap();

        for hours in [0, 2, 24 * 7, 24 * 7 + 2] {
            run_at(&pool, first_start() + Duration::hours(hours)).await;
        }

        assert_eq!(sessions(&pool, series).await.len(), 3);
        assert_eq!(
            queued(&pool).await,
            [
                (single.id, "started".to_string()),
                (single.id, "closed".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn a_deleted_or_cancelled_session_is_skipped_not_added_again() {
        let pool = test_pool().await;
        let series = weekly(&pool).await;
        let first_id: i64 = sqlx::query_scalar("SELECT id FROM events WHERE series_id = ?")
            .bind(series)
            .fetch_one(&pool)
            .await
            .unwrap();

        EventRepository::delete(&pool, first_id).await.unwrap();
        run_at(&pool, first_start() - Duration::days(1)).await;
        assert_eq!(
            sessions(&pool, series).await,
            [(weeks_after_first(1), Lifecycle::Scheduled)]
        );

        let second_id: i64 = sqlx::query_scalar("SELECT id FROM events WHERE series_id = ?")
            .bind(series)
            .fetch_one(&pool)
            .await
            .unwrap();
        let cancelled = EventRepository::transition(
            &pool,
            second_id,
            Lifecycle::Scheduled,
            Lifecycle::Cancelled,
            first_start(),
        )
        .await
        .unwrap();
        assert!(cancelled);
        run_at(&pool, first_start()).await;
        assert_eq!(
            sessions(&pool, series).await,
            [
                (weeks_after_first(1), Lifecycle::Cancelled),
                (weeks_after_first(2), Lifecycle::Scheduled),
            ]
        );
    }

    #[tokio::test]
    async fn a_stopped_series_adds_no_session() {
        let pool = test_pool().await;
        let series = weekly(&pool).await;
        sqlx::query("UPDATE maintenance_series SET stopped_at = '2026-11-01 00:00:00'")
            .execute(&pool)
            .await
            .unwrap();

        run_at(&pool, first_start() + Duration::minutes(1)).await;

        assert_eq!(
            sessions(&pool, series).await,
            [(first_start(), Lifecycle::InProgress)]
        );
    }

    async fn queued(pool: &DbPool) -> Vec<(i64, String)> {
        sqlx::query_as("SELECT event_id, happening FROM notification_deliveries ORDER BY id")
            .fetch_all(pool)
            .await
            .unwrap()
    }

    async fn everything(pool: &DbPool) {
        let channel = ChannelInput {
            name: "Team".to_string(),
            kind: ChannelKind::Slack,
            target: "https://hooks.slack.com/services/team".to_string(),
            locale: "en".to_string(),
            on_incidents: true,
            on_maintenances: true,
            on_publications: true,
            on_detected: true,
        };
        NotificationRepository::create_channel(pool, &channel)
            .await
            .unwrap();
    }

    async fn session_ids(pool: &DbPool, series_id: i64) -> Vec<i64> {
        sqlx::query_scalar("SELECT id FROM events WHERE series_id = ? ORDER BY id")
            .bind(series_id)
            .fetch_all(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn stopping_cancels_the_session_ahead_once_and_adds_no_more() {
        let pool = test_pool().await;
        everything(&pool).await;
        let series = weekly(&pool).await;
        run_at(&pool, first_start() + Duration::minutes(1)).await;
        let ids = session_ids(&pool, series).await;

        SeriesService::stop(&pool, ids[0]).await.unwrap();
        SeriesService::stop(&pool, ids[0]).await.unwrap();
        run_at(&pool, weeks_after_first(2) + Duration::minutes(1)).await;

        assert_eq!(
            sessions(&pool, series).await,
            [
                (first_start(), Lifecycle::Completed),
                (weeks_after_first(1), Lifecycle::Cancelled),
            ]
        );
        assert_eq!(queued(&pool).await, [(ids[1], "closed".to_string())]);
        let found = SeriesRepository::find(&pool, series)
            .await
            .unwrap()
            .unwrap();
        assert!(found.stopped_at.is_some());
    }

    #[tokio::test]
    async fn an_edit_carried_forward_moves_the_session_ahead_and_the_rule() {
        let pool = test_pool().await;
        let (author, service) = author_and_service(&pool).await;
        let other = ServiceRepository::create(&pool, "Mail", "mail", None, None, None)
            .await
            .unwrap()
            .id;
        let series = weekly_by(&pool, author, service).await;
        run_at(&pool, first_start() + Duration::minutes(1)).await;
        let ids = session_ids(&pool, series).await;
        // The session under way keeps its start; its new end makes 3 hours.
        let monday = first_start() + Duration::days(1) + Duration::hours(1);
        let ahead = UpdateEventInput {
            severity: None,
            planned: true,
            category: None,
            title: "Monday backups".to_string(),
            description: "Copies and checks.".to_string(),
            planned_start: Some(monday + Duration::weeks(1)),
            planned_end: Some(monday + Duration::weeks(1) + Duration::hours(3)),
            service_ids: vec![service, other],
            follows_event_id: None,
        };

        SeriesService::update_with_following(&pool, ids[1], ahead, Role::Admin)
            .await
            .unwrap();

        let next = monday + Duration::weeks(2);
        assert_eq!(
            sessions(&pool, series).await,
            [
                (first_start(), Lifecycle::InProgress),
                (monday + Duration::weeks(1), Lifecycle::Scheduled),
            ]
        );
        let found = SeriesRepository::find(&pool, series)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.recurrence.weekday(), chrono::Weekday::Mon);
        assert_eq!(found.duration_minutes, 180);
        assert_eq!(found.title, "Monday backups");
        assert_eq!(found.next_start, next);
        assert_eq!(
            SeriesRepository::service_ids(&pool, series).await.unwrap(),
            vec![service, other]
        );
    }

    #[tokio::test]
    async fn an_edit_from_the_session_under_way_reaches_the_one_ahead() {
        let pool = test_pool().await;
        let series = weekly(&pool).await;
        run_at(&pool, first_start() + Duration::minutes(1)).await;
        let ids = session_ids(&pool, series).await;
        let running = EventRepository::find_by_id(&pool, ids[0])
            .await
            .unwrap()
            .unwrap();
        let changes = UpdateEventInput {
            severity: None,
            planned: true,
            category: None,
            title: "Longer backups".to_string(),
            description: String::new(),
            planned_start: running.planned_start,
            planned_end: Some(first_start() + Duration::hours(4)),
            service_ids: EventRepository::service_ids(&pool, ids[0]).await.unwrap(),
            follows_event_id: None,
        };

        SeriesService::update_with_following(&pool, ids[0], changes, Role::Admin)
            .await
            .unwrap();

        let ahead = EventRepository::find_by_id(&pool, ids[1])
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ahead.title, "Longer backups");
        assert_eq!(ahead.planned_start, Some(weeks_after_first(1)));
        assert_eq!(
            ahead.planned_end,
            Some(weeks_after_first(1) + Duration::hours(4))
        );
        let found = SeriesRepository::find(&pool, series)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.next_start, weeks_after_first(2));
        assert_eq!(found.duration_minutes, 240);
    }

    #[tokio::test]
    async fn an_edit_carried_forward_needs_an_end() {
        let pool = test_pool().await;
        let series = weekly(&pool).await;
        let ids = session_ids(&pool, series).await;
        let changes = UpdateEventInput {
            severity: None,
            planned: true,
            category: None,
            title: "Sunday backups".to_string(),
            description: String::new(),
            planned_start: Some(first_start()),
            planned_end: None,
            service_ids: Vec::new(),
            follows_event_id: None,
        };

        let refused =
            SeriesService::update_with_following(&pool, ids[0], changes, Role::Admin).await;

        assert!(matches!(refused, Err(AppError::Validation(_))));
    }

    #[tokio::test]
    async fn a_repeating_maintenance_lists_its_latest_finished_session_only() {
        let pool = test_pool().await;
        let (author, service) = author_and_service(&pool).await;
        let series = weekly_by(&pool, author, service).await;
        let single = EventRepository::create(&pool, &maintenance(author, service, first_start()))
            .await
            .unwrap();
        for week in 0..3 {
            let start = weeks_after_first(week);
            run_at(&pool, start + Duration::minutes(1)).await;
            run_at(&pool, start + Duration::hours(2) + Duration::minutes(1)).await;
        }
        let latest: i64 = sqlx::query_scalar(
            "SELECT id FROM events WHERE series_id = ? AND lifecycle = 'completed' \
             ORDER BY planned_start DESC LIMIT 1",
        )
        .bind(series)
        .fetch_one(&pool)
        .await
        .unwrap();

        let finished = EventRepository::list_finished_maintenance(&pool, 5, &Scope::All)
            .await
            .unwrap();

        let mut ids: Vec<i64> = finished.iter().map(|row| row.id).collect();
        ids.sort_unstable();
        assert_eq!(ids, [single.id, latest]);
    }

    #[tokio::test]
    async fn the_sessions_the_schedule_adds_are_not_news() {
        let pool = test_pool().await;
        weekly(&pool).await;

        run_at(&pool, first_start() + Duration::minutes(1)).await;

        let unread = EventRepository::count_since(&pool, DateTime::UNIX_EPOCH, &Scope::All)
            .await
            .unwrap();
        assert_eq!(unread, 1);
    }
}
