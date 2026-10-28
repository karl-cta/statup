//! Series repository: maintenances announced once that come back on their
//! own, and the services their sessions concern.

use chrono::{DateTime, TimeDelta, Utc};
use sqlx::{Sqlite, Transaction};

use super::EventRepository;
use super::event_repo::link_services;
use crate::clock;
use crate::db::DbPool;
use crate::models::{CreateEventInput, Event, MaintenanceSeries, NewSeries, Rank, Recurrence};

pub struct SeriesRepository;

const SERIES_COLUMNS: &str = "SELECT id, rhythm, weekday, rank, start_time, duration_minutes, \
     title, description, keeps_services_up, next_start, author_id, stopped_at \
     FROM maintenance_series";

/// The series has no session waiting to begin.
const NOTHING_SCHEDULED: &str = "NOT EXISTS (SELECT 1 FROM events e \
     WHERE e.series_id = maintenance_series.id AND e.lifecycle = 'scheduled')";

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
        link_series_services(&mut tx, series_id, &first.service_ids).await?;
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

    /// Series still repeating with no session to come: the one ahead has
    /// begun, or was cancelled or deleted.
    pub async fn awaiting_session(pool: &DbPool) -> Result<Vec<MaintenanceSeries>, sqlx::Error> {
        let rows: Vec<SeriesRow> = sqlx::query_as(&format!(
            "{SERIES_COLUMNS} WHERE stopped_at IS NULL AND {NOTHING_SCHEDULED} ORDER BY id"
        ))
        .fetch_all(pool)
        .await?;
        rows.into_iter().map(MaintenanceSeries::try_from).collect()
    }

    /// Adds `session` to the series and moves the series on to `following`,
    /// in one transaction. `None`, with nothing written, when the series
    /// changed since it was read: stopped, edited, or given a session.
    pub async fn add_session(
        pool: &DbPool,
        series: &MaintenanceSeries,
        session: &CreateEventInput,
        following: DateTime<Utc>,
    ) -> Result<Option<Event>, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let moved_on = sqlx::query(&format!(
            "UPDATE maintenance_series SET next_start = ? \
             WHERE id = ? AND next_start = ? AND stopped_at IS NULL AND {NOTHING_SCHEDULED}"
        ))
        .bind(clock::db(following))
        .bind(series.id)
        .bind(clock::db(series.next_start))
        .execute(&mut *tx)
        .await?;
        if moved_on.rows_affected() == 0 {
            return Ok(None);
        }
        let event = EventRepository::insert(&mut tx, session, Some(series.id)).await?;
        tx.commit().await?;
        Ok(Some(event))
    }

    /// The series while it still repeats.
    pub async fn find_active(
        pool: &DbPool,
        id: i64,
    ) -> Result<Option<MaintenanceSeries>, sqlx::Error> {
        Ok(Self::find(pool, id)
            .await?
            .filter(|series| series.stopped_at.is_none()))
    }

    /// The session the series was announced with, while it exists: the
    /// others were added by the schedule.
    pub async fn first_session(pool: &DbPool, id: i64) -> Result<Option<i64>, sqlx::Error> {
        sqlx::query_scalar("SELECT MIN(id) FROM events WHERE series_id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
    }

    /// When the session waiting to begin starts, if there is one.
    pub async fn scheduled_start(
        pool: &DbPool,
        id: i64,
    ) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT planned_start FROM events WHERE series_id = ? AND lifecycle = 'scheduled' \
             ORDER BY planned_start LIMIT 1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
    }

    /// Stops the series and cancels the session it had ahead, in one
    /// transaction: the schedule adds no session once it is stopped, and
    /// the cancellation cannot be undone into a session nobody repeats.
    /// Returns the cancelled sessions, or `None` when it was stopped already.
    pub async fn stop(
        pool: &DbPool,
        id: i64,
        now: DateTime<Utc>,
    ) -> Result<Option<Vec<i64>>, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let stopped = sqlx::query(
            "UPDATE maintenance_series SET stopped_at = ?, updated_at = ? \
             WHERE id = ? AND stopped_at IS NULL",
        )
        .bind(clock::db(now))
        .bind(clock::db(now))
        .bind(id)
        .execute(&mut *tx)
        .await?;
        if stopped.rows_affected() == 0 {
            return Ok(None);
        }
        let cancelled = sqlx::query_scalar(
            "UPDATE events SET previous_lifecycle = NULL, lifecycle = 'cancelled' \
             WHERE series_id = ? AND lifecycle = 'scheduled' RETURNING id",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(cancelled))
    }

    /// Gives the series the rule, length, title, text and services of
    /// `session`, in one transaction. The session already scheduled after
    /// it moves to `following` and takes them too; without one, the series
    /// waits for `following`. Nothing is written to a stopped series.
    pub async fn carry_forward(
        pool: &DbPool,
        session: &Event,
        rule: &NewSeries,
        service_ids: &[i64],
        following: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        let Some(series_id) = session.series_id else {
            return Ok(());
        };
        let mut tx = pool.begin().await?;
        let next_one: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM events WHERE series_id = ? AND lifecycle = 'scheduled' AND id != ? \
             ORDER BY planned_start LIMIT 1",
        )
        .bind(series_id)
        .bind(session.id)
        .fetch_optional(&mut *tx)
        .await?;
        let length = TimeDelta::minutes(rule.duration_minutes);
        let cursor = match next_one {
            Some(next_id) => {
                move_session(&mut tx, next_id, session, following, following + length).await?;
                link_session_services(&mut tx, next_id, service_ids).await?;
                rule.recurrence.next_after(following).unwrap_or(following)
            }
            None => following,
        };
        let updated = update_rule(&mut tx, series_id, rule, session, cursor).await?;
        if !updated {
            return Ok(());
        }
        sqlx::query("DELETE FROM maintenance_series_services WHERE series_id = ?")
            .bind(series_id)
            .execute(&mut *tx)
            .await?;
        link_series_services(&mut tx, series_id, service_ids).await?;
        tx.commit().await?;
        Ok(())
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

async fn link_series_services(
    tx: &mut Transaction<'_, Sqlite>,
    series_id: i64,
    service_ids: &[i64],
) -> Result<(), sqlx::Error> {
    for service_id in service_ids {
        sqlx::query(
            "INSERT OR IGNORE INTO maintenance_series_services (series_id, service_id) \
             VALUES (?, ?)",
        )
        .bind(series_id)
        .bind(service_id)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn link_session_services(
    tx: &mut Transaction<'_, Sqlite>,
    event_id: i64,
    service_ids: &[i64],
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM event_services WHERE event_id = ?")
        .bind(event_id)
        .execute(&mut **tx)
        .await?;
    link_services(tx, event_id, service_ids).await
}

/// Moves a session to its new window, with the title and text of `like`.
async fn move_session(
    tx: &mut Transaction<'_, Sqlite>,
    id: i64,
    like: &Event,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE events SET title = ?, description = ?, planned_start = ?, planned_end = ? \
         WHERE id = ?",
    )
    .bind(&like.title)
    .bind(&like.description)
    .bind(clock::db(start))
    .bind(clock::db(end))
    .bind(id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Writes the rule and what each session repeats; `false` when the series
/// is stopped.
async fn update_rule(
    tx: &mut Transaction<'_, Sqlite>,
    id: i64,
    rule: &NewSeries,
    like: &Event,
    next_start: DateTime<Utc>,
) -> Result<bool, sqlx::Error> {
    let recurrence = rule.recurrence;
    let updated = sqlx::query(
        "UPDATE maintenance_series SET rhythm = ?, weekday = ?, rank = ?, start_time = ?, \
           duration_minutes = ?, title = ?, description = ?, next_start = ?, \
           updated_at = datetime('now') \
         WHERE id = ? AND stopped_at IS NULL",
    )
    .bind(recurrence.rhythm().as_str())
    .bind(i64::from(recurrence.weekday().num_days_from_monday()))
    .bind(recurrence.rank().map(Rank::as_str))
    .bind(recurrence.time().format("%H:%M").to_string())
    .bind(rule.duration_minutes)
    .bind(&like.title)
    .bind(&like.description)
    .bind(clock::db(next_start))
    .bind(id)
    .execute(&mut **tx)
    .await?;
    Ok(updated.rows_affected() > 0)
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
    async fn a_series_read_before_a_change_gets_no_session() {
        let pool = test_pool().await;
        let (author, services) = seed(&pool).await;
        let start = Utc.with_ymd_and_hms(2026, 11, 1, 2, 0, 0).unwrap();
        let input = first(author, services, start);
        let new = NewSeries::from_first(Rhythm::Weekly, &input).unwrap();
        let event = SeriesRepository::create(&pool, &new, &input).await.unwrap();
        EventRepository::delete(&pool, event.id).await.unwrap();
        let read = SeriesRepository::find(&pool, event.series_id.unwrap())
            .await
            .unwrap()
            .unwrap();
        let session = read.session(read.next_start, vec![]);
        let following = read.next_start + Duration::weeks(1);

        let added = SeriesRepository::add_session(&pool, &read, &session, following)
            .await
            .unwrap();
        let again = SeriesRepository::add_session(&pool, &read, &session, following)
            .await
            .unwrap();

        assert!(added.is_some());
        assert!(again.is_none());
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
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
