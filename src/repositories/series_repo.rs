//! Series repository: maintenances announced once that come back on their
//! own, and the services their sessions concern.

use chrono::{DateTime, Utc};

use super::EventRepository;
use crate::clock;
use crate::db::DbPool;
use crate::models::{CreateEventInput, Event, MaintenanceSeries, NewSeries, Rank, Recurrence};

pub struct SeriesRepository;

const SERIES_COLUMNS: &str = "SELECT id, rhythm, weekday, rank, start_time, duration_minutes, \
     title, description, keeps_services_up, next_start, author_id, stopped_at \
     FROM maintenance_series";

#[derive(sqlx::FromRow)]
struct SeriesRow {
    id: i64,
    rhythm: String,
    weekday: i64,
    rank: Option<String>,
    start_time: String,
    duration_minutes: i64,
    title: String,
    description: String,
    keeps_services_up: bool,
    next_start: DateTime<Utc>,
    author_id: i64,
    stopped_at: Option<DateTime<Utc>>,
}

impl TryFrom<SeriesRow> for MaintenanceSeries {
    type Error = sqlx::Error;

    fn try_from(row: SeriesRow) -> Result<Self, Self::Error> {
        let recurrence = Recurrence::from_stored(
            &row.rhythm,
            row.weekday,
            row.rank.as_deref(),
            &row.start_time,
        )
        .ok_or_else(|| {
            sqlx::Error::Decode(format!("series {} has no valid rule", row.id).into())
        })?;
        Ok(Self {
            id: row.id,
            recurrence,
            duration_minutes: row.duration_minutes,
            title: row.title,
            description: row.description,
            keeps_services_up: row.keeps_services_up,
            next_start: row.next_start,
            author_id: row.author_id,
            stopped_at: row.stopped_at,
        })
    }
}

impl SeriesRepository {
    /// Creates a series and its first session in one transaction: the
    /// session never exists without the rule that brings the next one.
    /// The series copies what each later session repeats from the first.
    pub async fn create(
        pool: &DbPool,
        series: &NewSeries,
        first: &CreateEventInput,
    ) -> Result<Event, sqlx::Error> {
        let rule = series.recurrence;
        let mut tx = pool.begin().await?;
        let series_id: i64 = sqlx::query_scalar(
            "INSERT INTO maintenance_series (rhythm, weekday, rank, start_time, \
             duration_minutes, title, description, keeps_services_up, next_start, author_id) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(rule.rhythm().as_str())
        .bind(i64::from(rule.weekday().num_days_from_monday()))
        .bind(rule.rank().map(Rank::as_str))
        .bind(rule.time().format("%H:%M").to_string())
        .bind(series.duration_minutes)
        .bind(&first.title)
        .bind(&first.description)
        .bind(first.keeps_services_up)
        .bind(clock::db(series.next_start))
        .bind(first.author_id)
        .fetch_one(&mut *tx)
        .await?;
        for service_id in &first.service_ids {
            sqlx::query(
                "INSERT OR IGNORE INTO maintenance_series_services (series_id, service_id) \
                 VALUES (?, ?)",
            )
            .bind(series_id)
            .bind(service_id)
            .execute(&mut *tx)
            .await?;
        }
        let event = EventRepository::insert(&mut tx, first, Some(series_id)).await?;
        tx.commit().await?;
        Ok(event)
    }

    pub async fn find(pool: &DbPool, id: i64) -> Result<Option<MaintenanceSeries>, sqlx::Error> {
        let row: Option<SeriesRow> = sqlx::query_as(&format!("{SERIES_COLUMNS} WHERE id = ?"))
            .bind(id)
            .fetch_optional(pool)
            .await?;
        row.map(MaintenanceSeries::try_from).transpose()
    }

    /// The services each session of the series concerns, by id.
    pub async fn service_ids(pool: &DbPool, id: i64) -> Result<Vec<i64>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT service_id FROM maintenance_series_services \
             WHERE series_id = ? ORDER BY service_id",
        )
        .bind(id)
        .fetch_all(pool)
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Kind, Lifecycle, Rhythm, Role};
    use crate::repositories::{ServiceRepository, UserRepository};
    use crate::test_helpers::test_pool;
    use chrono::{Duration, TimeZone};

    async fn seed(pool: &DbPool) -> (i64, Vec<i64>) {
        let user = UserRepository::create(pool, "t@t.com", "hash", "Tester", Role::Publisher)
            .await
            .unwrap();
        let mut services = Vec::new();
        for (name, slug) in [("Mail", "mail"), ("VPN", "vpn")] {
            let service = ServiceRepository::create(pool, name, slug, None, None, None)
                .await
                .unwrap();
            services.push(service.id);
        }
        (user.id, services)
    }

    fn first(author_id: i64, service_ids: Vec<i64>, start: DateTime<Utc>) -> CreateEventInput {
        CreateEventInput {
            kind: Kind::Maintenance,
            severity: None,
            planned: true,
            category: None,
            title: "Sunday backups".to_string(),
            description: "Copies of every disk.".to_string(),
            planned_start: Some(start),
            planned_end: Some(start + Duration::hours(2)),
            started_at: None,
            opening_step: None,
            keeps_services_up: true,
            service_ids,
            follows_event_id: None,
            author_id,
            api_key_id: None,
            next_update: None,
        }
    }

    #[tokio::test]
    async fn a_series_starts_with_its_first_session_and_keeps_what_repeats() {
        let pool = test_pool().await;
        let (author, services) = seed(&pool).await;
        let start = Utc.with_ymd_and_hms(2026, 11, 1, 2, 0, 0).unwrap();
        let input = first(author, services.clone(), start);
        let new = NewSeries::from_first(Rhythm::Monthly, &input).unwrap();

        let event = SeriesRepository::create(&pool, &new, &input).await.unwrap();

        assert_eq!(event.kind, Kind::Maintenance);
        assert_eq!(event.lifecycle, Some(Lifecycle::Scheduled));
        let series_id = event.series_id.unwrap();
        let series = SeriesRepository::find(&pool, series_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(series.recurrence, new.recurrence);
        assert_eq!(series.recurrence.rank(), Some(Rank::First));
        assert_eq!(series.duration_minutes, 120);
        assert_eq!(series.next_start, new.next_start);
        assert_eq!(series.title, "Sunday backups");
        assert_eq!(series.description, "Copies of every disk.");
        assert!(series.keeps_services_up);
        assert_eq!(series.author_id, author);
        assert_eq!(series.stopped_at, None);
        assert_eq!(
            SeriesRepository::service_ids(&pool, series_id)
                .await
                .unwrap(),
            services
        );
        let found = EventRepository::find_by_id(&pool, event.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.series_id, Some(series_id));
    }

    #[tokio::test]
    async fn a_deleted_service_leaves_the_series() {
        let pool = test_pool().await;
        let (author, services) = seed(&pool).await;
        let start = Utc.with_ymd_and_hms(2026, 11, 1, 2, 0, 0).unwrap();
        let input = first(author, services.clone(), start);
        let new = NewSeries::from_first(Rhythm::Weekly, &input).unwrap();
        let event = SeriesRepository::create(&pool, &new, &input).await.unwrap();
        let series_id = event.series_id.unwrap();
        // A service named by an event cannot be deleted: the session goes first.
        EventRepository::delete(&pool, event.id).await.unwrap();

        ServiceRepository::delete(&pool, services[1]).await.unwrap();

        assert_eq!(
            SeriesRepository::service_ids(&pool, series_id)
                .await
                .unwrap(),
            vec![services[0]]
        );
    }

    #[tokio::test]
    async fn the_schema_refuses_a_rule_that_mixes_the_rhythms() {
        let pool = test_pool().await;
        let (author, _) = seed(&pool).await;
        let refused = sqlx::query(
            "INSERT INTO maintenance_series (rhythm, weekday, rank, start_time, \
             duration_minutes, title, description, next_start, author_id) \
             VALUES ('weekly', 6, 'first', '02:00', 60, 't', '', '2026-11-01 02:00:00', ?)",
        )
        .bind(author)
        .execute(&pool)
        .await;
        assert!(refused.is_err());
    }
}
