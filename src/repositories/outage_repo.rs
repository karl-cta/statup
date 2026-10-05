//! Outage repository: the outages the checks detected on each service.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use sqlx::Sqlite;

use crate::clock;
use crate::db::DbPool;

pub struct OutageRepository;

impl OutageRepository {
    /// Outages under way or ended since `since`, by service, oldest first.
    pub async fn since(
        pool: &DbPool,
        since: DateTime<Utc>,
    ) -> Result<HashMap<i64, Vec<OutageSpan>>, sqlx::Error> {
        let rows: Vec<(i64, DateTime<Utc>, Option<DateTime<Utc>>)> = sqlx::query_as(
            "SELECT service_id, started_at, ended_at FROM service_outages \
             WHERE ended_at IS NULL OR ended_at >= ? ORDER BY started_at",
        )
        .bind(clock::db(since))
        .fetch_all(pool)
        .await?;
        let mut spans: HashMap<i64, Vec<OutageSpan>> = HashMap::new();
        for (service_id, start, end) in rows {
            spans
                .entry(service_id)
                .or_default()
                .push(OutageSpan { start, end });
        }
        Ok(spans)
    }

    /// When each outage under way began, by service.
    pub async fn open_starts(pool: &DbPool) -> Result<HashMap<i64, DateTime<Utc>>, sqlx::Error> {
        let rows: Vec<(i64, DateTime<Utc>)> = sqlx::query_as(
            "SELECT service_id, started_at FROM service_outages WHERE ended_at IS NULL",
        )
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().collect())
    }
}

/// One detected outage on one service, as the availability strip reads it.
pub struct OutageSpan {
    pub start: DateTime<Utc>,
    pub end: Option<DateTime<Utc>>,
}

pub(super) async fn open_outage(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    service_id: i64,
    started_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO service_outages (service_id, started_at) VALUES (?, ?)")
        .bind(service_id)
        .bind(clock::db(started_at))
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub(super) async fn delete_open_outage(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    service_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM service_outages WHERE service_id = ? AND ended_at IS NULL")
        .bind(service_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Never ends an outage before it started, should the clock step back.
pub(super) async fn close_outage(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    service_id: i64,
    ended_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE service_outages SET ended_at = MAX(started_at, ?) \
         WHERE service_id = ? AND ended_at IS NULL",
    )
    .bind(clock::db(ended_at))
    .bind(service_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};

    use super::*;
    use crate::repositories::ServiceRepository;
    use crate::test_helpers::test_pool;

    fn at(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 4, hour, 0, 0).unwrap()
    }

    #[tokio::test]
    async fn since_keeps_open_and_recent_outages() {
        let pool = test_pool().await;
        let old = ServiceRepository::create(&pool, "Old", "old", None, None, None)
            .await
            .unwrap();
        let recent = ServiceRepository::create(&pool, "Recent", "recent", None, None, None)
            .await
            .unwrap();
        let open = ServiceRepository::create(&pool, "Open", "open", None, None, None)
            .await
            .unwrap();
        for (id, start, end) in [(old.id, 1, Some(2)), (recent.id, 3, Some(5))] {
            ServiceRepository::mark_detected_down(&pool, id, at(start))
                .await
                .unwrap();
            if let Some(end) = end {
                ServiceRepository::mark_detected_up(&pool, id, at(end))
                    .await
                    .unwrap();
            }
        }
        ServiceRepository::mark_detected_down(&pool, open.id, at(6))
            .await
            .unwrap();

        let spans = OutageRepository::since(&pool, at(4)).await.unwrap();
        assert!(!spans.contains_key(&old.id));
        assert_eq!(spans[&recent.id][0].start, at(3));
        assert_eq!(spans[&recent.id][0].end, Some(at(5)));
        assert_eq!(spans[&open.id][0].end, None);
    }

    #[tokio::test]
    async fn an_outage_never_ends_before_it_starts() {
        let pool = test_pool().await;
        let svc = ServiceRepository::create(&pool, "Skew", "skew", None, None, None)
            .await
            .unwrap();
        ServiceRepository::mark_detected_down(&pool, svc.id, at(10))
            .await
            .unwrap();
        ServiceRepository::mark_detected_up(&pool, svc.id, at(10) - Duration::minutes(5))
            .await
            .unwrap();
        let spans = OutageRepository::since(&pool, at(0)).await.unwrap();
        assert_eq!(spans[&svc.id][0].end, Some(at(10)));
    }
}
