//! Repeating maintenances: announced once, each session added by the
//! schedule in silence.

use chrono::{DateTime, Utc};

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::MaintenanceSeries;
use crate::repositories::SeriesRepository;

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
        Scope,
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
        NotificationRepository::create_channel(&pool, &channel)
            .await
            .unwrap();
        let (author, service) = author_and_service(&pool).await;
        let series = weekly_by(&pool, author, service).await;
        let single = EventRepository::create(&pool, &maintenance(author, service, first_start()))
            .await
            .unwrap();

        for hours in [0, 2, 24 * 7, 24 * 7 + 2] {
            run_at(&pool, first_start() + Duration::hours(hours)).await;
        }

        assert_eq!(sessions(&pool, series).await.len(), 3);
        let queued: Vec<(i64, String)> =
            sqlx::query_as("SELECT event_id, happening FROM notification_deliveries ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            queued,
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
