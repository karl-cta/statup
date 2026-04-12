//! Event repository - database queries for events and updates.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, Utc};

use crate::db::DbPool;
use crate::models::{
    CreateEventInput, Event, EventFilters, EventStatus, EventSummary, EventUpdate,
    EventUpdateWithAuthor, EventWithServices, Impact, Service,
};

/// Encapsulates all event-related database queries.
pub struct EventRepository;

impl EventRepository {
    /// Create a new event with its service associations (in a transaction).
    pub async fn create(pool: &DbPool, input: CreateEventInput) -> Result<Event, sqlx::Error> {
        let mut tx = pool.begin().await?;

        let event = sqlx::query_as::<_, Event>(
            "INSERT INTO events (event_type, status, title, description, impact, \
             scheduled_start, scheduled_end, actual_start, icon_id, author_id) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             RETURNING *",
        )
        .bind(input.event_type)
        .bind(input.initial_status())
        .bind(&input.title)
        .bind(&input.description)
        .bind(input.impact)
        .bind(input.scheduled_start)
        .bind(input.scheduled_end)
        .bind(input.actual_start())
        .bind(input.icon_id)
        .bind(input.author_id)
        .fetch_one(&mut *tx)
        .await?;

        for service_id in &input.service_ids {
            sqlx::query("INSERT INTO event_services (event_id, service_id) VALUES (?, ?)")
                .bind(event.id)
                .bind(service_id)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(event)
    }

    /// Find a single event by ID.
    pub async fn find_by_id(pool: &DbPool, id: i64) -> Result<Option<Event>, sqlx::Error> {
        sqlx::query_as::<_, Event>("SELECT * FROM events WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
    }

    /// Find an event by ID together with its associated services.
    pub async fn find_by_id_with_services(
        pool: &DbPool,
        id: i64,
    ) -> Result<Option<EventWithServices>, sqlx::Error> {
        let Some(event) = Self::find_by_id(pool, id).await? else {
            return Ok(None);
        };

        let services = sqlx::query_as::<_, Service>(
            "SELECT s.* FROM services s \
             INNER JOIN event_services es ON es.service_id = s.id \
             WHERE es.event_id = ? \
             ORDER BY s.name ASC",
        )
        .bind(id)
        .fetch_all(pool)
        .await?;

        Ok(Some(EventWithServices { event, services }))
    }

    /// List recent events ordered by creation date (newest first).
    pub async fn list_recent(
        pool: &DbPool,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<EventSummary>, sqlx::Error> {
        sqlx::query_as::<_, EventSummary>(
            "SELECT e.id, e.event_type, e.status, e.title, e.description, \
             e.impact, e.created_at, e.updated_at, e.author_id, \
             COALESCE(GROUP_CONCAT(s.name, ', '), '') as service_names, \
             ic.filename AS icon_filename \
             FROM events e \
             LEFT JOIN event_services es ON es.event_id = e.id \
             LEFT JOIN services s ON s.id = es.service_id \
             LEFT JOIN icons ic ON ic.id = e.icon_id \
             GROUP BY e.id \
             ORDER BY e.created_at DESC \
             LIMIT ? OFFSET ?",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
    }

    /// List events matching the given filters.
    ///
    /// Builds the WHERE clause dynamically based on which filters are set.
    pub async fn list_by_filters(
        pool: &DbPool,
        filters: EventFilters,
    ) -> Result<Vec<EventSummary>, sqlx::Error> {
        let mut sql = String::from(
            "SELECT e.id, e.event_type, e.status, e.title, e.impact, \
             e.created_at, e.updated_at, e.author_id, \
             COALESCE(GROUP_CONCAT(sv.name, ', '), '') as service_names, \
             ic.filename AS icon_filename \
             FROM events e \
             LEFT JOIN event_services esv ON esv.event_id = e.id \
             LEFT JOIN services sv ON sv.id = esv.service_id \
             LEFT JOIN icons ic ON ic.id = e.icon_id",
        );
        let mut conditions: Vec<String> = Vec::new();

        if filters.service_id.is_some() {
            // Additional filter join, we keep the LEFT JOINs for service_names
            // and add a WHERE on a subquery or on esv.service_id.
            conditions.push(
                "e.id IN (SELECT es2.event_id FROM event_services es2 WHERE es2.service_id = ?)"
                    .to_string(),
            );
        }

        if filters.event_type.is_some() {
            conditions.push("e.event_type = ?".to_string());
        }
        if filters.status.is_some() {
            conditions.push("e.status = ?".to_string());
        }
        if filters.from.is_some() {
            conditions.push("e.created_at >= ?".to_string());
        }
        if filters.to.is_some() {
            conditions.push("e.created_at <= ?".to_string());
        }

        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }

        sql.push_str(" GROUP BY e.id ORDER BY e.created_at DESC LIMIT ? OFFSET ?");

        // Bind in the exact order the placeholders appear.
        let mut query = sqlx::query_as::<_, EventSummary>(&sql);

        if let Some(sid) = filters.service_id {
            query = query.bind(sid);
        }
        if let Some(et) = filters.event_type {
            query = query.bind(et);
        }
        if let Some(st) = filters.status {
            query = query.bind(st);
        }
        if let Some(from) = filters.from {
            query = query.bind(from);
        }
        if let Some(to) = filters.to {
            query = query.bind(to);
        }

        query = query.bind(filters.limit).bind(filters.offset);

        query.fetch_all(pool).await
    }

    /// List recent activity (everything except maintenances).
    pub async fn list_recent_activity(
        pool: &DbPool,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<EventSummary>, sqlx::Error> {
        sqlx::query_as::<_, EventSummary>(
            "SELECT e.id, e.event_type, e.status, e.title, e.description, \
             e.impact, e.created_at, e.updated_at, e.author_id, \
             COALESCE(GROUP_CONCAT(s.name, ', '), '') as service_names, \
             ic.filename AS icon_filename \
             FROM events e \
             LEFT JOIN event_services es ON es.event_id = e.id \
             LEFT JOIN services s ON s.id = es.service_id \
             LEFT JOIN icons ic ON ic.id = e.icon_id \
             WHERE e.event_type NOT IN ('maintenance_scheduled', 'maintenance_urgent') \
             GROUP BY e.id \
             ORDER BY e.created_at DESC \
             LIMIT ? OFFSET ?",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
    }

    /// List active maintenance events (scheduled or urgent, non-terminal).
    pub async fn list_active_maintenance(pool: &DbPool) -> Result<Vec<EventSummary>, sqlx::Error> {
        sqlx::query_as::<_, EventSummary>(
            "SELECT e.id, e.event_type, e.status, e.title, e.impact, \
             e.created_at, e.updated_at, e.author_id, \
             COALESCE(GROUP_CONCAT(s.name, ', '), '') as service_names, \
             ic.filename AS icon_filename, \
             e.scheduled_start \
             FROM events e \
             LEFT JOIN event_services es ON es.event_id = e.id \
             LEFT JOIN services s ON s.id = es.service_id \
             LEFT JOIN icons ic ON ic.id = e.icon_id \
             WHERE e.event_type IN ('maintenance_scheduled', 'maintenance_urgent') \
               AND e.status NOT IN ('resolved', 'cancelled') \
             GROUP BY e.id \
             ORDER BY COALESCE(e.scheduled_start, e.created_at) ASC",
        )
        .fetch_all(pool)
        .await
    }

    /// List recently resolved/cancelled maintenance events.
    pub async fn list_recent_resolved_maintenance(
        pool: &DbPool,
        limit: i64,
    ) -> Result<Vec<EventSummary>, sqlx::Error> {
        sqlx::query_as::<_, EventSummary>(
            "SELECT e.id, e.event_type, e.status, e.title, e.impact, \
             e.created_at, e.updated_at, e.author_id, \
             COALESCE(GROUP_CONCAT(s.name, ', '), '') as service_names, \
             ic.filename AS icon_filename \
             FROM events e \
             LEFT JOIN event_services es ON es.event_id = e.id \
             LEFT JOIN services s ON s.id = es.service_id \
             LEFT JOIN icons ic ON ic.id = e.icon_id \
             WHERE e.event_type IN ('maintenance_scheduled', 'maintenance_urgent') \
               AND e.status IN ('resolved', 'cancelled') \
             GROUP BY e.id \
             ORDER BY e.updated_at DESC \
             LIMIT ?",
        )
        .bind(limit)
        .fetch_all(pool)
        .await
    }

    /// List scheduled maintenance events in the future.
    pub async fn list_upcoming_maintenance(
        pool: &DbPool,
    ) -> Result<Vec<EventSummary>, sqlx::Error> {
        sqlx::query_as::<_, EventSummary>(
            "SELECT e.id, e.event_type, e.status, e.title, e.impact, \
             e.created_at, e.updated_at, e.author_id, \
             COALESCE(GROUP_CONCAT(s.name, ', '), '') as service_names, \
             ic.filename AS icon_filename, \
             e.scheduled_start \
             FROM events e \
             LEFT JOIN event_services es ON es.event_id = e.id \
             LEFT JOIN services s ON s.id = es.service_id \
             LEFT JOIN icons ic ON ic.id = e.icon_id \
             WHERE e.event_type = 'maintenance_scheduled' \
               AND e.status = 'scheduled' \
               AND (e.scheduled_start IS NULL OR e.scheduled_start > datetime('now')) \
             GROUP BY e.id \
             ORDER BY COALESCE(e.scheduled_start, e.created_at) ASC",
        )
        .fetch_all(pool)
        .await
    }

    /// List active incidents (non-terminal, non-maintenance, non-info events).
    pub async fn list_active_incidents(pool: &DbPool) -> Result<Vec<EventSummary>, sqlx::Error> {
        sqlx::query_as::<_, EventSummary>(
            "SELECT e.id, e.event_type, e.status, e.title, e.impact, \
             e.created_at, e.updated_at, e.author_id, \
             COALESCE(GROUP_CONCAT(s.name, ', '), '') as service_names, \
             ic.filename AS icon_filename \
             FROM events e \
             LEFT JOIN event_services es ON es.event_id = e.id \
             LEFT JOIN services s ON s.id = es.service_id \
             LEFT JOIN icons ic ON ic.id = e.icon_id \
             WHERE e.status NOT IN ('resolved', 'cancelled') \
               AND e.event_type IN ('incident', 'maintenance_urgent') \
             GROUP BY e.id \
             ORDER BY e.created_at DESC",
        )
        .fetch_all(pool)
        .await
    }

    /// List active (non-terminal) events for a given service.
    ///
    /// Scheduled maintenances with a future `scheduled_start` are excluded
    /// so they don't impact service status before the planned date.
    pub async fn list_active_for_service(
        pool: &DbPool,
        service_id: i64,
    ) -> Result<Vec<Event>, sqlx::Error> {
        sqlx::query_as::<_, Event>(
            "SELECT e.* FROM events e \
             INNER JOIN event_services es ON es.event_id = e.id \
             WHERE es.service_id = ? \
               AND e.status NOT IN ('resolved', 'cancelled') \
               AND NOT (e.event_type = 'maintenance_scheduled' \
                        AND e.scheduled_start IS NOT NULL \
                        AND e.scheduled_start > datetime('now')) \
             ORDER BY e.created_at DESC",
        )
        .bind(service_id)
        .fetch_all(pool)
        .await
    }

    /// Update the status of an event, saving the current status as `previous_status`.
    pub async fn update_status(
        pool: &DbPool,
        event_id: i64,
        new_status: EventStatus,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE events SET previous_status = status, status = ? WHERE id = ?")
            .bind(new_status)
            .bind(event_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Revert an event to its `previous_status`, clearing the saved value.
    pub async fn revert_status(pool: &DbPool, event_id: i64) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE events SET status = previous_status, previous_status = NULL, actual_end = NULL WHERE id = ? AND previous_status IS NOT NULL",
        )
        .bind(event_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Replace the service associations for an event (in a transaction).
    ///
    /// Returns the list of previously associated service IDs so the caller
    /// can recalculate their status after the change.
    pub async fn update_services(
        pool: &DbPool,
        event_id: i64,
        new_service_ids: &[i64],
    ) -> Result<Vec<i64>, sqlx::Error> {
        let old_ids: Vec<(i64,)> =
            sqlx::query_as("SELECT service_id FROM event_services WHERE event_id = ?")
                .bind(event_id)
                .fetch_all(pool)
                .await?;
        let old_service_ids: Vec<i64> = old_ids.into_iter().map(|(id,)| id).collect();

        let mut tx = pool.begin().await?;

        sqlx::query("DELETE FROM event_services WHERE event_id = ?")
            .bind(event_id)
            .execute(&mut *tx)
            .await?;

        for service_id in new_service_ids {
            sqlx::query("INSERT INTO event_services (event_id, service_id) VALUES (?, ?)")
                .bind(event_id)
                .bind(service_id)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(old_service_ids)
    }

    /// Delete an event by ID. Returns the IDs of services that were associated.
    ///
    /// Relies on `ON DELETE CASCADE` for `event_services` and `event_updates`.
    pub async fn delete(pool: &DbPool, event_id: i64) -> Result<Vec<i64>, sqlx::Error> {
        let rows: Vec<(i64,)> =
            sqlx::query_as("SELECT service_id FROM event_services WHERE event_id = ?")
                .bind(event_id)
                .fetch_all(pool)
                .await?;
        let service_ids: Vec<i64> = rows.into_iter().map(|(id,)| id).collect();

        sqlx::query("DELETE FROM events WHERE id = ?")
            .bind(event_id)
            .execute(pool)
            .await?;

        Ok(service_ids)
    }

    /// Set the `actual_end` timestamp on an event (e.g. when resolved).
    pub async fn set_actual_end(
        pool: &DbPool,
        event_id: i64,
        at: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE events SET actual_end = ? WHERE id = ?")
            .bind(at)
            .bind(event_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Add a status update message to an event.
    pub async fn add_update(
        pool: &DbPool,
        event_id: i64,
        message: &str,
        author_id: i64,
    ) -> Result<EventUpdate, sqlx::Error> {
        sqlx::query_as::<_, EventUpdate>(
            "INSERT INTO event_updates (event_id, message, author_id) \
             VALUES (?, ?, ?) \
             RETURNING *",
        )
        .bind(event_id)
        .bind(message)
        .bind(author_id)
        .fetch_one(pool)
        .await
    }

    /// List updates for an event, enriched with author names.
    pub async fn list_updates_with_author(
        pool: &DbPool,
        event_id: i64,
    ) -> Result<Vec<EventUpdateWithAuthor>, sqlx::Error> {
        sqlx::query_as::<_, EventUpdateWithAuthor>(
            "SELECT eu.id, eu.event_id, eu.message, eu.author_id, eu.created_at, \
                    u.display_name AS author_name \
             FROM event_updates eu \
             INNER JOIN users u ON u.id = eu.author_id \
             WHERE eu.event_id = ? \
             ORDER BY eu.created_at ASC",
        )
        .bind(event_id)
        .fetch_all(pool)
        .await
    }

    /// Count events created since the given timestamp.
    pub async fn count_since(pool: &DbPool, since: DateTime<Utc>) -> Result<i64, sqlx::Error> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM events WHERE created_at >= ? AND event_type != 'info'",
        )
        .bind(since)
        .fetch_one(pool)
        .await?;
        Ok(row.0)
    }

    /// Fetch per-service daily worst-impact data for sparkline rendering.
    ///
    /// Returns a map of `service_id` → `Vec<u8>` with `days` entries (oldest
    /// first).  Each value is the worst impact level for that day:
    /// 0 = operational, 1 = minor, 2 = major, 3 = critical.
    pub async fn sparkline_data(
        pool: &DbPool,
        days: u32,
    ) -> Result<HashMap<i64, Vec<u8>>, sqlx::Error> {
        let modifier = format!("-{} days", days.saturating_sub(1));

        let rows = sqlx::query_as::<_, SparklineEventRow>(
            "SELECT es.service_id, e.impact, \
             DATE(COALESCE(e.actual_start, e.created_at)) AS start_date, \
             DATE(COALESCE(e.actual_end, datetime('now'))) AS end_date \
             FROM events e \
             INNER JOIN event_services es ON es.event_id = e.id \
             WHERE e.event_type IN ('incident', 'maintenance_urgent', 'maintenance_scheduled') \
               AND e.status != 'cancelled' \
               AND DATE(COALESCE(e.actual_start, e.created_at)) <= DATE('now') \
               AND DATE(COALESCE(e.actual_end, datetime('now'))) >= DATE('now', ?)",
        )
        .bind(&modifier)
        .fetch_all(pool)
        .await?;

        let today = Utc::now().date_naive();
        let window_start = today - chrono::Duration::days(i64::from(days) - 1);

        let mut result: HashMap<i64, Vec<u8>> = HashMap::new();

        for row in &rows {
            let Some(start) = NaiveDate::parse_from_str(&row.start_date, "%Y-%m-%d").ok() else {
                continue;
            };
            let Some(end) = NaiveDate::parse_from_str(&row.end_date, "%Y-%m-%d").ok() else {
                continue;
            };

            let level = row.impact.level();
            let entry = result
                .entry(row.service_id)
                .or_insert_with(|| vec![0u8; days as usize]);

            let overlap_start = start.max(window_start);
            let overlap_end = end.min(today);

            let mut day = overlap_start;
            while day <= overlap_end {
                let Some(idx) = usize::try_from((day - window_start).num_days()).ok() else {
                    break;
                };
                if idx < entry.len() {
                    entry[idx] = entry[idx].max(level);
                }
                day += chrono::Duration::days(1);
            }
        }

        Ok(result)
    }

    /// Full-text search on events using FTS5, optionally combined with SQL
    /// filters (type, service, date range).
    ///
    /// Results are sorted by FTS5 relevance rank first, then by creation date
    /// descending as a tiebreaker.
    pub async fn search(
        pool: &DbPool,
        query: &str,
        filters: &EventFilters,
    ) -> Result<Vec<EventSummary>, sqlx::Error> {
        let sanitized = sanitize_fts_query(query);
        if sanitized.is_empty() {
            return Ok(Vec::new());
        }

        let mut sql = String::from(
            "SELECT e.id, e.event_type, e.status, e.title, e.impact, \
             e.created_at, e.updated_at, e.author_id, \
             COALESCE(GROUP_CONCAT(s.name, ', '), '') as service_names, \
             ic.filename AS icon_filename \
             FROM events_fts fts \
             INNER JOIN events e ON e.id = fts.rowid \
             LEFT JOIN event_services es_j ON es_j.event_id = e.id \
             LEFT JOIN services s ON s.id = es_j.service_id \
             LEFT JOIN icons ic ON ic.id = e.icon_id",
        );

        let mut conditions: Vec<String> = vec!["events_fts MATCH ?".to_string()];

        if filters.service_id.is_some() {
            conditions.push(
                "e.id IN (SELECT es2.event_id FROM event_services es2 WHERE es2.service_id = ?)"
                    .to_string(),
            );
        }
        if filters.event_type.is_some() {
            conditions.push("e.event_type = ?".to_string());
        }
        if filters.status.is_some() {
            conditions.push("e.status = ?".to_string());
        }
        if filters.from.is_some() {
            conditions.push("e.created_at >= ?".to_string());
        }
        if filters.to.is_some() {
            conditions.push("e.created_at <= ?".to_string());
        }

        sql.push_str(" WHERE ");
        sql.push_str(&conditions.join(" AND "));
        sql.push_str(" GROUP BY e.id ORDER BY rank, e.created_at DESC LIMIT ? OFFSET ?");

        let mut q = sqlx::query_as::<_, EventSummary>(&sql);

        // Bind in placeholder order.
        q = q.bind(sanitized);
        if let Some(sid) = filters.service_id {
            q = q.bind(sid);
        }
        if let Some(et) = filters.event_type {
            q = q.bind(et);
        }
        if let Some(st) = filters.status {
            q = q.bind(st);
        }
        if let Some(from) = filters.from {
            q = q.bind(from);
        }
        if let Some(to) = filters.to {
            q = q.bind(to);
        }
        q = q.bind(filters.limit).bind(filters.offset);

        q.fetch_all(pool).await
    }

    /// Get the timestamp of the most recent admin action across events,
    /// event updates, and services.
    pub async fn last_admin_action(pool: &DbPool) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
        let row: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT MAX(ts) FROM ( \
                 SELECT MAX(updated_at) AS ts FROM events \
                 UNION ALL \
                 SELECT MAX(created_at) FROM event_updates \
                 UNION ALL \
                 SELECT MAX(updated_at) FROM services \
             )",
        )
        .fetch_optional(pool)
        .await?;

        let Some((Some(ts),)) = row else {
            return Ok(None);
        };

        Ok(DateTime::parse_from_str(&ts, "%Y-%m-%d %H:%M:%S")
            .ok()
            .map(|dt| dt.with_timezone(&Utc)))
    }
}

/// Row returned by the sparkline aggregation query.
#[derive(sqlx::FromRow)]
struct SparklineEventRow {
    service_id: i64,
    impact: Impact,
    start_date: String,
    end_date: String,
}

/// Sanitize a user-supplied search string for FTS5 MATCH.
///
/// Strips FTS5 operators and wraps each token with `"…"` so the query is
/// always treated as a simple term search (implicit AND).
fn sanitize_fts_query(raw: &str) -> String {
    raw.split_whitespace()
        .map(|token| {
            // Remove characters that have special meaning in FTS5 queries.
            let clean: String = token
                .chars()
                .filter(|c| !matches!(c, '"' | '*' | '+' | '-' | '(' | ')' | ':' | '^'))
                .collect();
            clean
        })
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{t}\""))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CreateEventInput, EventType, Impact, Role};
    use crate::repositories::{ServiceRepository, UserRepository};
    use crate::test_helpers::test_pool;

    async fn seed_user_and_service(pool: &DbPool) -> (i64, i64) {
        let user = UserRepository::create(pool, "t@t.com", "hash", "Tester", Role::Publisher)
            .await
            .unwrap();
        let svc = ServiceRepository::create(pool, "API", "api", None)
            .await
            .unwrap();
        (user.id, svc.id)
    }

    fn incident_input(title: &str, author_id: i64, service_ids: Vec<i64>) -> CreateEventInput {
        CreateEventInput {
            event_type: EventType::Incident,
            title: title.to_string(),
            description: "Something broke".to_string(),
            impact: Impact::Major,
            scheduled_start: None,
            scheduled_end: None,
            service_ids,
            icon_id: None,
            author_id,
        }
    }

    #[tokio::test]
    async fn create_event_and_find_by_id() {
        let pool = test_pool().await;
        let (uid, sid) = seed_user_and_service(&pool).await;

        let event = EventRepository::create(&pool, incident_input("DB Down", uid, vec![sid]))
            .await
            .unwrap();

        assert_eq!(event.title, "DB Down");
        assert_eq!(event.event_type, EventType::Incident);
        assert_eq!(event.status, EventStatus::Investigating);

        let found = EventRepository::find_by_id(&pool, event.id).await.unwrap();
        assert!(found.is_some());
    }

    #[tokio::test]
    async fn find_by_id_with_services() {
        let pool = test_pool().await;
        let (uid, sid) = seed_user_and_service(&pool).await;

        let event = EventRepository::create(&pool, incident_input("Outage", uid, vec![sid]))
            .await
            .unwrap();

        let ews = EventRepository::find_by_id_with_services(&pool, event.id)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(ews.services.len(), 1);
        assert_eq!(ews.services[0].slug, "api");
    }

    #[tokio::test]
    async fn has_events_returns_true_after_event_creation() {
        let pool = test_pool().await;
        let (uid, sid) = seed_user_and_service(&pool).await;

        EventRepository::create(&pool, incident_input("X", uid, vec![sid]))
            .await
            .unwrap();

        assert!(ServiceRepository::has_events(&pool, sid).await.unwrap());
    }

    #[tokio::test]
    async fn update_status_and_set_actual_end() {
        let pool = test_pool().await;
        let (uid, sid) = seed_user_and_service(&pool).await;

        let event = EventRepository::create(&pool, incident_input("Issue", uid, vec![sid]))
            .await
            .unwrap();

        EventRepository::update_status(&pool, event.id, EventStatus::Resolved)
            .await
            .unwrap();

        let now = chrono::Utc::now();
        EventRepository::set_actual_end(&pool, event.id, now)
            .await
            .unwrap();

        let updated = EventRepository::find_by_id(&pool, event.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, EventStatus::Resolved);
        assert!(updated.actual_end.is_some());
    }

    #[tokio::test]
    async fn add_update_and_list_with_author() {
        let pool = test_pool().await;
        let (uid, sid) = seed_user_and_service(&pool).await;

        let event = EventRepository::create(&pool, incident_input("Bug", uid, vec![sid]))
            .await
            .unwrap();

        let upd = EventRepository::add_update(&pool, event.id, "Investigating now", uid)
            .await
            .unwrap();
        assert_eq!(upd.message, "Investigating now");

        let updates = EventRepository::list_updates_with_author(&pool, event.id)
            .await
            .unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].author_name, "Tester");
    }

    #[tokio::test]
    async fn list_recent() {
        let pool = test_pool().await;
        let (uid, sid) = seed_user_and_service(&pool).await;

        for i in 0..5 {
            EventRepository::create(&pool, incident_input(&format!("Event {i}"), uid, vec![sid]))
                .await
                .unwrap();
        }

        let recent = EventRepository::list_recent(&pool, 3, 0).await.unwrap();
        assert_eq!(recent.len(), 3);
    }

    #[tokio::test]
    async fn list_active_for_service() {
        let pool = test_pool().await;
        let (uid, sid) = seed_user_and_service(&pool).await;

        let e1 = EventRepository::create(&pool, incident_input("Active", uid, vec![sid]))
            .await
            .unwrap();
        let e2 = EventRepository::create(&pool, incident_input("Resolved", uid, vec![sid]))
            .await
            .unwrap();

        EventRepository::update_status(&pool, e2.id, EventStatus::Resolved)
            .await
            .unwrap();

        let active = EventRepository::list_active_for_service(&pool, sid)
            .await
            .unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, e1.id);
    }

    #[tokio::test]
    async fn count_since_excludes_info_events() {
        let pool = test_pool().await;
        let (uid, sid) = seed_user_and_service(&pool).await;

        let before = chrono::DateTime::UNIX_EPOCH;

        EventRepository::create(&pool, incident_input("Incident", uid, vec![sid]))
            .await
            .unwrap();
        EventRepository::create(
            &pool,
            CreateEventInput {
                event_type: EventType::Info,
                title: "Info".to_string(),
                description: "FYI".to_string(),
                impact: Impact::None,
                scheduled_start: None,
                scheduled_end: None,
                service_ids: vec![sid],
                icon_id: None,
                author_id: uid,
            },
        )
        .await
        .unwrap();

        let count = EventRepository::count_since(&pool, before).await.unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn search_fts() {
        let pool = test_pool().await;
        let (uid, sid) = seed_user_and_service(&pool).await;

        EventRepository::create(&pool, incident_input("Database outage", uid, vec![sid]))
            .await
            .unwrap();
        EventRepository::create(&pool, incident_input("Network issue", uid, vec![sid]))
            .await
            .unwrap();

        let filters = EventFilters {
            limit: 10,
            ..Default::default()
        };

        let results = EventRepository::search(&pool, "database", &filters)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Database outage");
    }

    #[test]
    fn sanitize_fts_strips_operators() {
        assert_eq!(sanitize_fts_query("hello* +world"), "\"hello\" \"world\"");
    }

    #[test]
    fn sanitize_fts_empty_input() {
        assert_eq!(sanitize_fts_query(""), "");
    }

    #[test]
    fn sanitize_fts_only_operators() {
        assert_eq!(sanitize_fts_query("+ - * \"\""), "");
    }

    #[test]
    fn sanitize_fts_normal_query() {
        assert_eq!(
            sanitize_fts_query("database outage"),
            "\"database\" \"outage\""
        );
    }
}
