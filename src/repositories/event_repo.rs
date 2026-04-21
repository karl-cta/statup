//! Event repository: SQL queries on events and their updates.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, Utc};

use crate::db::DbPool;
use crate::models::{
    CreateEventInput, Event, EventFilters, EventSummary, EventUpdate, EventUpdateWithAuthor,
    EventWithServices, Lifecycle, Service,
};

pub struct EventRepository;

impl EventRepository {
    pub async fn create(pool: &DbPool, input: CreateEventInput) -> Result<Event, sqlx::Error> {
        let mut tx = pool.begin().await?;

        let event = sqlx::query_as::<_, Event>(
            "INSERT INTO events (kind, severity, planned, lifecycle, category, title, description, \
             planned_start, planned_end, started_at, icon_id, author_id) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             RETURNING *",
        )
        .bind(input.kind)
        .bind(input.severity)
        .bind(input.planned)
        .bind(input.initial_lifecycle())
        .bind(input.category)
        .bind(&input.title)
        .bind(&input.description)
        .bind(input.planned_start)
        .bind(input.planned_end)
        .bind(input.initial_started_at())
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

    pub async fn find_by_id(pool: &DbPool, id: i64) -> Result<Option<Event>, sqlx::Error> {
        sqlx::query_as::<_, Event>("SELECT * FROM events WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
    }

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

    pub async fn list_recent(
        pool: &DbPool,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<EventSummary>, sqlx::Error> {
        sqlx::query_as::<_, EventSummary>(
            "SELECT e.id, e.kind, e.severity, e.planned, e.lifecycle, e.category, \
             e.title, e.description, e.created_at, e.updated_at, e.author_id, \
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

    pub async fn list_by_filters(
        pool: &DbPool,
        filters: EventFilters,
    ) -> Result<Vec<EventSummary>, sqlx::Error> {
        let mut sql = String::from(
            "SELECT e.id, e.kind, e.severity, e.planned, e.lifecycle, e.category, \
             e.title, e.description, e.created_at, e.updated_at, e.author_id, \
             COALESCE(GROUP_CONCAT(sv.name, ', '), '') as service_names, \
             ic.filename AS icon_filename \
             FROM events e \
             LEFT JOIN event_services esv ON esv.event_id = e.id \
             LEFT JOIN services sv ON sv.id = esv.service_id \
             LEFT JOIN icons ic ON ic.id = e.icon_id",
        );
        let mut conditions: Vec<String> = Vec::new();

        if filters.service_id.is_some() {
            conditions.push(
                "e.id IN (SELECT es2.event_id FROM event_services es2 WHERE es2.service_id = ?)"
                    .to_string(),
            );
        }

        if filters.kind.is_some() {
            conditions.push("e.kind = ?".to_string());
        }
        if filters.lifecycle.is_some() {
            conditions.push("e.lifecycle = ?".to_string());
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

        let mut query = sqlx::query_as::<_, EventSummary>(&sql);

        if let Some(sid) = filters.service_id {
            query = query.bind(sid);
        }
        if let Some(k) = filters.kind {
            query = query.bind(k);
        }
        if let Some(l) = filters.lifecycle {
            query = query.bind(l);
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

    /// Recent activity, everything except maintenances (incidents + publications).
    pub async fn list_recent_activity(
        pool: &DbPool,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<EventSummary>, sqlx::Error> {
        sqlx::query_as::<_, EventSummary>(
            "SELECT e.id, e.kind, e.severity, e.planned, e.lifecycle, e.category, \
             e.title, e.description, e.created_at, e.updated_at, e.author_id, \
             COALESCE(GROUP_CONCAT(s.name, ', '), '') as service_names, \
             ic.filename AS icon_filename \
             FROM events e \
             LEFT JOIN event_services es ON es.event_id = e.id \
             LEFT JOIN services s ON s.id = es.service_id \
             LEFT JOIN icons ic ON ic.id = e.icon_id \
             WHERE e.kind != 'maintenance' \
             GROUP BY e.id \
             ORDER BY e.created_at DESC \
             LIMIT ? OFFSET ?",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
    }

    /// Maintenances in progress or scheduled (non-terminal lifecycle).
    pub async fn list_active_maintenance(pool: &DbPool) -> Result<Vec<EventSummary>, sqlx::Error> {
        sqlx::query_as::<_, EventSummary>(
            "SELECT e.id, e.kind, e.severity, e.planned, e.lifecycle, e.category, \
             e.title, e.description, e.created_at, e.updated_at, e.author_id, \
             COALESCE(GROUP_CONCAT(s.name, ', '), '') as service_names, \
             ic.filename AS icon_filename, \
             e.planned_start \
             FROM events e \
             LEFT JOIN event_services es ON es.event_id = e.id \
             LEFT JOIN services s ON s.id = es.service_id \
             LEFT JOIN icons ic ON ic.id = e.icon_id \
             WHERE e.kind = 'maintenance' \
               AND e.lifecycle NOT IN ('completed', 'cancelled') \
             GROUP BY e.id \
             ORDER BY COALESCE(e.planned_start, e.created_at) ASC",
        )
        .fetch_all(pool)
        .await
    }

    /// Recently completed or cancelled maintenances.
    pub async fn list_recent_resolved_maintenance(
        pool: &DbPool,
        limit: i64,
    ) -> Result<Vec<EventSummary>, sqlx::Error> {
        sqlx::query_as::<_, EventSummary>(
            "SELECT e.id, e.kind, e.severity, e.planned, e.lifecycle, e.category, \
             e.title, e.description, e.created_at, e.updated_at, e.author_id, \
             COALESCE(GROUP_CONCAT(s.name, ', '), '') as service_names, \
             ic.filename AS icon_filename \
             FROM events e \
             LEFT JOIN event_services es ON es.event_id = e.id \
             LEFT JOIN services s ON s.id = es.service_id \
             LEFT JOIN icons ic ON ic.id = e.icon_id \
             WHERE e.kind = 'maintenance' \
               AND e.lifecycle IN ('completed', 'cancelled') \
             GROUP BY e.id \
             ORDER BY e.updated_at DESC \
             LIMIT ?",
        )
        .bind(limit)
        .fetch_all(pool)
        .await
    }

    /// Upcoming scheduled maintenances.
    pub async fn list_upcoming_maintenance(
        pool: &DbPool,
    ) -> Result<Vec<EventSummary>, sqlx::Error> {
        sqlx::query_as::<_, EventSummary>(
            "SELECT e.id, e.kind, e.severity, e.planned, e.lifecycle, e.category, \
             e.title, e.description, e.created_at, e.updated_at, e.author_id, \
             COALESCE(GROUP_CONCAT(s.name, ', '), '') as service_names, \
             ic.filename AS icon_filename, \
             e.planned_start \
             FROM events e \
             LEFT JOIN event_services es ON es.event_id = e.id \
             LEFT JOIN services s ON s.id = es.service_id \
             LEFT JOIN icons ic ON ic.id = e.icon_id \
             WHERE e.kind = 'maintenance' \
               AND e.planned = 1 \
               AND e.lifecycle = 'scheduled' \
               AND (e.planned_start IS NULL OR e.planned_start > datetime('now')) \
             GROUP BY e.id \
             ORDER BY COALESCE(e.planned_start, e.created_at) ASC",
        )
        .fetch_all(pool)
        .await
    }

    /// Disruptive active events for the public page: incidents + ongoing
    /// unplanned maintenances.
    pub async fn list_active_incidents(pool: &DbPool) -> Result<Vec<EventSummary>, sqlx::Error> {
        sqlx::query_as::<_, EventSummary>(
            "SELECT e.id, e.kind, e.severity, e.planned, e.lifecycle, e.category, \
             e.title, e.description, e.created_at, e.updated_at, e.author_id, \
             COALESCE(GROUP_CONCAT(s.name, ', '), '') as service_names, \
             ic.filename AS icon_filename \
             FROM events e \
             LEFT JOIN event_services es ON es.event_id = e.id \
             LEFT JOIN services s ON s.id = es.service_id \
             LEFT JOIN icons ic ON ic.id = e.icon_id \
             WHERE e.lifecycle NOT IN ('resolved', 'completed', 'cancelled') \
               AND (e.kind = 'incident' OR (e.kind = 'maintenance' AND e.planned = 0)) \
             GROUP BY e.id \
             ORDER BY e.created_at DESC",
        )
        .fetch_all(pool)
        .await
    }

    /// Active events for a given service. Future scheduled maintenances are
    /// excluded so the service status is not degraded before the deadline.
    pub async fn list_active_for_service(
        pool: &DbPool,
        service_id: i64,
    ) -> Result<Vec<Event>, sqlx::Error> {
        sqlx::query_as::<_, Event>(
            "SELECT e.* FROM events e \
             INNER JOIN event_services es ON es.event_id = e.id \
             WHERE es.service_id = ? \
               AND e.lifecycle NOT IN ('resolved', 'completed', 'cancelled') \
               AND NOT (e.kind = 'maintenance' \
                        AND e.planned = 1 \
                        AND e.planned_start IS NOT NULL \
                        AND e.planned_start > datetime('now')) \
             ORDER BY e.created_at DESC",
        )
        .bind(service_id)
        .fetch_all(pool)
        .await
    }

    /// Transition the lifecycle. The previous value is stored in
    /// `previous_lifecycle` to allow a one-level undo.
    pub async fn update_lifecycle(
        pool: &DbPool,
        event_id: i64,
        new_lifecycle: Lifecycle,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE events SET previous_lifecycle = lifecycle, lifecycle = ? WHERE id = ?")
            .bind(new_lifecycle)
            .bind(event_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Undo the last lifecycle transition. Also clears `ended_at` so the
    /// event exits a terminal state cleanly.
    pub async fn revert_lifecycle(pool: &DbPool, event_id: i64) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE events SET lifecycle = previous_lifecycle, previous_lifecycle = NULL, ended_at = NULL WHERE id = ? AND previous_lifecycle IS NOT NULL",
        )
        .bind(event_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Replace the services linked to an event. Returns the previously
    /// linked service IDs so the caller can recompute their status.
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

    /// Delete an event. `event_services` and `event_updates` rows are
    /// cleaned up by `ON DELETE CASCADE`.
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

    pub async fn set_ended_at(
        pool: &DbPool,
        event_id: i64,
        at: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE events SET ended_at = ? WHERE id = ?")
            .bind(at)
            .bind(event_id)
            .execute(pool)
            .await?;
        Ok(())
    }

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

    /// Count of "important" events created since `since`, used for the
    /// unread badge. Publications are excluded (ambient, not interventions).
    pub async fn count_since(pool: &DbPool, since: DateTime<Utc>) -> Result<i64, sqlx::Error> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM events WHERE created_at >= ? AND kind != 'publication'",
        )
        .bind(since)
        .fetch_one(pool)
        .await?;
        Ok(row.0)
    }

    /// Daily severity level per service for the sparkline rendering.
    /// Returns a map `service_id` -> `Vec<u8>` of length `days` (oldest
    /// first). Each value is the worst level for the day:
    /// 0 = none, 1 = minor, 2 = major, 3 = critical.
    pub async fn sparkline_data(
        pool: &DbPool,
        days: u32,
    ) -> Result<HashMap<i64, Vec<u8>>, sqlx::Error> {
        let modifier = format!("-{} days", days.saturating_sub(1));

        let rows = sqlx::query_as::<_, SparklineEventRow>(
            "SELECT es.service_id, \
             CASE e.severity \
                 WHEN 'critical' THEN 3 \
                 WHEN 'major' THEN 2 \
                 WHEN 'minor' THEN 1 \
                 ELSE 0 \
             END AS severity_level, \
             DATE(COALESCE(e.started_at, e.created_at)) AS start_date, \
             DATE(COALESCE(e.ended_at, datetime('now'))) AS end_date \
             FROM events e \
             INNER JOIN event_services es ON es.event_id = e.id \
             WHERE e.kind IN ('incident', 'maintenance') \
               AND e.lifecycle != 'cancelled' \
               AND DATE(COALESCE(e.started_at, e.created_at)) <= DATE('now') \
               AND DATE(COALESCE(e.ended_at, datetime('now'))) >= DATE('now', ?)",
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

            let level = u8::try_from(row.severity_level.clamp(0, 3)).unwrap_or(0);
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

    /// Full-text search on events via FTS5, combinable with filters.
    /// Results ordered by FTS5 relevance, then by creation date descending.
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
            "SELECT e.id, e.kind, e.severity, e.planned, e.lifecycle, e.category, \
             e.title, e.description, e.created_at, e.updated_at, e.author_id, \
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
        if filters.kind.is_some() {
            conditions.push("e.kind = ?".to_string());
        }
        if filters.lifecycle.is_some() {
            conditions.push("e.lifecycle = ?".to_string());
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

        q = q.bind(sanitized);
        if let Some(sid) = filters.service_id {
            q = q.bind(sid);
        }
        if let Some(k) = filters.kind {
            q = q.bind(k);
        }
        if let Some(l) = filters.lifecycle {
            q = q.bind(l);
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

    /// Timestamp of the last admin action (events, updates, services).
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

#[derive(sqlx::FromRow)]
struct SparklineEventRow {
    service_id: i64,
    severity_level: i64,
    start_date: String,
    end_date: String,
}

/// Sanitize an FTS5 query by stripping special operators and quoting each
/// token to force a literal search.
fn sanitize_fts_query(raw: &str) -> String {
    raw.split_whitespace()
        .map(|token| {
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
    use crate::models::{Category, CreateEventInput, Kind, Role, Severity};
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
            kind: Kind::Incident,
            severity: Some(Severity::Major),
            planned: false,
            category: None,
            title: title.to_string(),
            description: "Something broke".to_string(),
            planned_start: None,
            planned_end: None,
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
        assert_eq!(event.kind, Kind::Incident);
        assert_eq!(event.lifecycle, Some(Lifecycle::Investigating));

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
    async fn update_lifecycle_and_set_ended_at() {
        let pool = test_pool().await;
        let (uid, sid) = seed_user_and_service(&pool).await;

        let event = EventRepository::create(&pool, incident_input("Issue", uid, vec![sid]))
            .await
            .unwrap();

        EventRepository::update_lifecycle(&pool, event.id, Lifecycle::Resolved)
            .await
            .unwrap();

        let now = chrono::Utc::now();
        EventRepository::set_ended_at(&pool, event.id, now)
            .await
            .unwrap();

        let updated = EventRepository::find_by_id(&pool, event.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.lifecycle, Some(Lifecycle::Resolved));
        assert!(updated.ended_at.is_some());
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

        EventRepository::update_lifecycle(&pool, e2.id, Lifecycle::Resolved)
            .await
            .unwrap();

        let active = EventRepository::list_active_for_service(&pool, sid)
            .await
            .unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, e1.id);
    }

    #[tokio::test]
    async fn count_since_excludes_publications() {
        let pool = test_pool().await;
        let (uid, sid) = seed_user_and_service(&pool).await;

        let before = chrono::DateTime::UNIX_EPOCH;

        EventRepository::create(&pool, incident_input("Incident", uid, vec![sid]))
            .await
            .unwrap();
        EventRepository::create(
            &pool,
            CreateEventInput {
                kind: Kind::Publication,
                severity: None,
                planned: false,
                category: Some(Category::Info),
                title: "Info".to_string(),
                description: "FYI".to_string(),
                planned_start: None,
                planned_end: None,
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
