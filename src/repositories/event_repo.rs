//! Event repository: SQL queries on events and their updates.
//!
//! List queries pick the page of ids first, then load those rows, so their
//! cost does not grow with the history.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use sqlx::{QueryBuilder, Sqlite};

use crate::clock;
use crate::db::DbPool;
use crate::models::{
    CreateEventInput, Event, EventFilters, EventSummary, EventUpdateWithAuthor, EventWithServices,
    Kind, Lifecycle, Service, Severity, UpdateEventInput,
};

const SUMMARY_COLUMNS: &str = "e.id, e.kind, e.severity, e.planned, e.lifecycle, e.category, \
     e.title, e.description, e.planned_start, e.planned_end, e.started_at, e.ended_at, \
     e.keeps_services_up, e.created_at, e.updated_at, e.author_id, \
     COALESCE((SELECT GROUP_CONCAT(s.name, char(31) ORDER BY s.name) \
               FROM event_services es JOIN services s ON s.id = es.service_id \
               WHERE es.event_id = e.id), '') AS service_names, \
     COALESCE((SELECT GROUP_CONCAT(COALESCE(s.icon_name, '') || char(30) || COALESCE(i.filename, ''), \
                                   char(31) ORDER BY s.name) \
               FROM event_services es JOIN services s ON s.id = es.service_id \
               LEFT JOIN icons i ON i.id = s.icon_id \
               WHERE es.event_id = e.id), '') AS service_icons";

const LATEST_UPDATE_COLUMNS: &str = "\
     (SELECT u.message FROM event_updates u WHERE u.event_id = e.id \
      ORDER BY u.created_at DESC, u.id DESC LIMIT 1) AS latest_update, \
     (SELECT u.created_at FROM event_updates u WHERE u.event_id = e.id \
      ORDER BY u.created_at DESC, u.id DESC LIMIT 1) AS latest_update_at";

const UPDATE_COLUMNS: &str = "eu.id, eu.event_id, eu.message, eu.author_id, eu.created_at, \
     u.display_name AS author_name";

/// Longest search text and number of words kept from it.
const MAX_QUERY_CHARS: usize = 200;
const MAX_QUERY_TERMS: usize = 10;

pub struct EventRepository;

impl EventRepository {
    pub async fn create(pool: &DbPool, input: &CreateEventInput) -> Result<Event, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let event = sqlx::query_as::<_, Event>(
            "INSERT INTO events (kind, severity, planned, lifecycle, category, title, \
             description, planned_start, planned_end, started_at, follows_event_id, author_id, \
             keeps_services_up) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING *",
        )
        .bind(input.kind)
        .bind(input.severity)
        .bind(input.planned)
        .bind(input.initial_lifecycle())
        .bind(input.category)
        .bind(&input.title)
        .bind(&input.description)
        .bind(input.planned_start.map(clock::db))
        .bind(input.planned_end.map(clock::db))
        .bind(input.initial_started_at().map(clock::db))
        .bind(input.follows_event_id)
        .bind(input.author_id)
        .bind(input.kind == Kind::Maintenance && input.keeps_services_up)
        .fetch_one(&mut *tx)
        .await?;
        link_services(&mut tx, event.id, &input.service_ids).await?;
        tx.commit().await?;
        Ok(event)
    }

    /// Saves the editable fields and the linked services. Returns every
    /// service linked before or after, whose status may have changed.
    pub async fn update(
        pool: &DbPool,
        id: i64,
        input: &UpdateEventInput,
    ) -> Result<Vec<i64>, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let mut touched = service_ids_in(&mut tx, id).await?;
        sqlx::query(
            "UPDATE events SET title = ?, description = ?, severity = ?, planned = ?, \
             category = ?, planned_start = ?, planned_end = ?, follows_event_id = ? \
             WHERE id = ?",
        )
        .bind(&input.title)
        .bind(&input.description)
        .bind(input.severity)
        .bind(input.planned)
        .bind(input.category)
        .bind(input.planned_start.map(clock::db))
        .bind(input.planned_end.map(clock::db))
        .bind(input.follows_event_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM event_services WHERE event_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        link_services(&mut tx, id, &input.service_ids).await?;
        tx.commit().await?;
        touched.extend(input.service_ids.iter().copied());
        touched.sort_unstable();
        touched.dedup();
        Ok(touched)
    }

    pub async fn find_by_id(pool: &DbPool, id: i64) -> Result<Option<Event>, sqlx::Error> {
        sqlx::query_as::<_, Event>("SELECT * FROM events WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
    }

    pub async fn find_with_services(
        pool: &DbPool,
        id: i64,
    ) -> Result<Option<EventWithServices>, sqlx::Error> {
        let Some(event) = Self::find_by_id(pool, id).await? else {
            return Ok(None);
        };
        let services = sqlx::query_as::<_, Service>(
            "SELECT s.* FROM services s \
             INNER JOIN event_services es ON es.service_id = s.id \
             WHERE es.event_id = ? ORDER BY s.name ASC",
        )
        .bind(id)
        .fetch_all(pool)
        .await?;
        Ok(Some(EventWithServices { event, services }))
    }

    pub async fn service_ids(pool: &DbPool, event_id: i64) -> Result<Vec<i64>, sqlx::Error> {
        sqlx::query_scalar("SELECT service_id FROM event_services WHERE event_id = ?")
            .bind(event_id)
            .fetch_all(pool)
            .await
    }

    /// One page of the events list, newest first.
    pub async fn list_page(
        pool: &DbPool,
        filters: &EventFilters,
    ) -> Result<Vec<EventSummary>, sqlx::Error> {
        let mut qb = QueryBuilder::<Sqlite>::new(format!(
            "SELECT {SUMMARY_COLUMNS} FROM events e WHERE e.id IN (SELECT f.id FROM events f"
        ));
        push_filters(&mut qb, filters);
        qb.push(" ORDER BY f.created_at DESC, f.id DESC LIMIT ")
            .push_bind(filters.limit)
            .push(" OFFSET ")
            .push_bind(filters.offset)
            .push(") ORDER BY e.created_at DESC, e.id DESC");
        qb.build_query_as::<EventSummary>().fetch_all(pool).await
    }

    /// Number of events matching the filters, across every page.
    pub async fn count_filtered(pool: &DbPool, filters: &EventFilters) -> Result<i64, sqlx::Error> {
        let mut qb = QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM events f");
        push_filters(&mut qb, filters);
        qb.build_query_scalar::<i64>().fetch_one(pool).await
    }

    /// Latest incidents and announcements; maintenances have their own card.
    pub async fn list_recent_activity(
        pool: &DbPool,
        limit: i64,
    ) -> Result<Vec<EventSummary>, sqlx::Error> {
        sqlx::query_as::<_, EventSummary>(&format!(
            "SELECT {SUMMARY_COLUMNS} FROM events e WHERE e.id IN ( \
               SELECT id FROM events WHERE kind != 'maintenance' \
               ORDER BY created_at DESC, id DESC LIMIT ?) \
             ORDER BY e.created_at DESC, e.id DESC"
        ))
        .bind(limit)
        .fetch_all(pool)
        .await
    }

    /// Newest events with their last activity time, for the Atom feed. A
    /// posted update does not touch `events.updated_at`, so it is folded in.
    pub async fn list_for_feed(
        pool: &DbPool,
        limit: i64,
    ) -> Result<Vec<EventSummary>, sqlx::Error> {
        sqlx::query_as::<_, EventSummary>(&format!(
            "SELECT {SUMMARY_COLUMNS}, \
               MAX(e.updated_at, COALESCE((SELECT MAX(u.created_at) FROM event_updates u \
                   WHERE u.event_id = e.id), e.updated_at)) AS last_activity_at \
             FROM events e WHERE e.id IN ( \
               SELECT id FROM events ORDER BY created_at DESC, id DESC LIMIT ?) \
             ORDER BY e.created_at DESC, e.id DESC"
        ))
        .bind(limit)
        .fetch_all(pool)
        .await
    }

    /// The events that hold services in their state, the same ones as
    /// `status_drivers`, worst first: what the status banner cites.
    pub async fn list_open_for_banner(pool: &DbPool) -> Result<Vec<EventSummary>, sqlx::Error> {
        sqlx::query_as::<_, EventSummary>(&format!(
            "SELECT {SUMMARY_COLUMNS}, {LATEST_UPDATE_COLUMNS} FROM events e \
             WHERE (e.kind = 'incident' AND e.lifecycle IN ('investigating', 'in_progress')) \
                OR (e.kind = 'maintenance' AND e.lifecycle = 'in_progress' \
                    AND NOT e.keeps_services_up) \
             ORDER BY CASE e.severity WHEN 'critical' THEN 2 WHEN 'minor' THEN 1 ELSE 0 END DESC, \
                      e.created_at DESC"
        ))
        .fetch_all(pool)
        .await
    }

    /// Maintenances under way, then those still to come, soonest first.
    pub async fn list_open_maintenance(pool: &DbPool) -> Result<Vec<EventSummary>, sqlx::Error> {
        sqlx::query_as::<_, EventSummary>(&format!(
            "SELECT {SUMMARY_COLUMNS} FROM events e \
             WHERE e.kind = 'maintenance' AND e.lifecycle IN ('in_progress', 'scheduled') \
             ORDER BY CASE e.lifecycle WHEN 'in_progress' THEN 0 ELSE 1 END, \
                      COALESCE(e.planned_start, e.started_at, e.created_at) ASC"
        ))
        .fetch_all(pool)
        .await
    }

    /// Maintenances that ended or were called off, latest first.
    pub async fn list_finished_maintenance(
        pool: &DbPool,
        limit: i64,
    ) -> Result<Vec<EventSummary>, sqlx::Error> {
        sqlx::query_as::<_, EventSummary>(&format!(
            "SELECT {SUMMARY_COLUMNS} FROM events e WHERE e.id IN ( \
               SELECT id FROM events \
               WHERE kind = 'maintenance' AND lifecycle IN ('completed', 'cancelled') \
               ORDER BY updated_at DESC LIMIT ?) \
             ORDER BY COALESCE(e.ended_at, e.updated_at) DESC"
        ))
        .bind(limit)
        .fetch_all(pool)
        .await
    }

    /// For each service held in its state by open work, the event that sets
    /// it, the worst first when several do: what the services page names.
    pub async fn state_drivers(
        pool: &DbPool,
        service_id: Option<i64>,
    ) -> Result<HashMap<i64, (i64, String)>, sqlx::Error> {
        let rows: Vec<(i64, i64, String)> = sqlx::query_as(
            "SELECT es.service_id, e.id, e.title FROM events e \
             INNER JOIN event_services es ON es.event_id = e.id \
             WHERE (? IS NULL OR es.service_id = ?) \
               AND ((e.kind = 'incident' AND e.lifecycle IN ('investigating', 'in_progress')) \
                 OR (e.kind = 'maintenance' AND e.lifecycle = 'in_progress' \
                     AND NOT e.keeps_services_up)) \
             ORDER BY CASE e.severity WHEN 'critical' THEN 0 WHEN 'minor' THEN 1 ELSE 2 END, \
                      e.created_at DESC",
        )
        .bind(service_id)
        .bind(service_id)
        .fetch_all(pool)
        .await?;
        let mut drivers = HashMap::new();
        for (service, event_id, title) in rows {
            drivers.entry(service).or_insert((event_id, title));
        }
        Ok(drivers)
    }

    /// Kind and severity of the open work that sets a service's status:
    /// incidents being worked on and maintenance under way. An incident
    /// under watch leaves the service up.
    pub async fn status_drivers(
        pool: &DbPool,
        service_id: i64,
    ) -> Result<Vec<(Kind, Option<Severity>)>, sqlx::Error> {
        sqlx::query_as(
            "SELECT e.kind, e.severity FROM events e \
             INNER JOIN event_services es ON es.event_id = e.id \
             WHERE es.service_id = ? \
               AND ((e.kind = 'incident' AND e.lifecycle IN ('investigating', 'in_progress')) \
                 OR (e.kind = 'maintenance' AND e.lifecycle = 'in_progress' \
                     AND NOT e.keeps_services_up))",
        )
        .bind(service_id)
        .fetch_all(pool)
        .await
    }

    /// Starts every announced maintenance whose start has come. Returns the
    /// ids that changed.
    pub async fn start_due_maintenance(
        pool: &DbPool,
        now: DateTime<Utc>,
    ) -> Result<Vec<i64>, sqlx::Error> {
        sqlx::query_scalar(
            "UPDATE events SET previous_lifecycle = NULL, lifecycle = 'in_progress', \
               started_at = COALESCE(started_at, planned_start) \
             WHERE kind = 'maintenance' AND lifecycle = 'scheduled' \
               AND planned_start IS NOT NULL AND planned_start <= ? \
             RETURNING id",
        )
        .bind(clock::db(now))
        .fetch_all(pool)
        .await
    }

    /// Completes every maintenance under way whose announced end has come.
    pub async fn complete_due_maintenance(
        pool: &DbPool,
        now: DateTime<Utc>,
    ) -> Result<Vec<i64>, sqlx::Error> {
        sqlx::query_scalar(
            "UPDATE events SET previous_lifecycle = NULL, lifecycle = 'completed', \
               ended_at = COALESCE(ended_at, planned_end) \
             WHERE kind = 'maintenance' AND lifecycle = 'in_progress' \
               AND planned_end IS NOT NULL AND planned_end <= ? \
             RETURNING id",
        )
        .bind(clock::db(now))
        .fetch_all(pool)
        .await
    }

    /// Moves to `next`, keeping the current state for a one step undo. The
    /// start is recorded the first time work begins, the restoration when the
    /// team starts watching, the end on closing.
    pub async fn transition(
        pool: &DbPool,
        id: i64,
        from: Lifecycle,
        next: Lifecycle,
        now: DateTime<Utc>,
    ) -> Result<bool, sqlx::Error> {
        let at = clock::db(now);
        let result = sqlx::query(
            "UPDATE events SET previous_lifecycle = lifecycle, lifecycle = ?, \
               started_at = CASE WHEN ? = 'in_progress' THEN COALESCE(started_at, ?) \
                            ELSE started_at END, \
               ended_at = CASE WHEN ? IN ('resolved', 'completed') THEN ? ELSE ended_at END, \
               restored_at = CASE WHEN ? = 'monitoring' THEN COALESCE(restored_at, ?) \
                             WHEN ? IN ('investigating', 'in_progress') THEN NULL \
                             ELSE restored_at END \
             WHERE id = ? AND lifecycle = ?",
        )
        .bind(next)
        .bind(next.as_str())
        .bind(&at)
        .bind(next.as_str())
        .bind(&at)
        .bind(next.as_str())
        .bind(&at)
        .bind(next.as_str())
        .bind(id)
        .bind(from)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Undoes the last transition. A maintenance sent back to its schedule
    /// forgets its start.
    pub async fn revert_transition(pool: &DbPool, id: i64) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE events SET lifecycle = previous_lifecycle, previous_lifecycle = NULL, \
               ended_at = NULL, \
               restored_at = CASE WHEN lifecycle = 'monitoring' THEN NULL ELSE restored_at END, \
               started_at = CASE WHEN previous_lifecycle = 'scheduled' THEN NULL \
                            ELSE started_at END \
             WHERE id = ? AND previous_lifecycle IS NOT NULL",
        )
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Deletes an event with its links and updates. Returns the services it
    /// was linked to.
    pub async fn delete(pool: &DbPool, id: i64) -> Result<Vec<i64>, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let service_ids = service_ids_in(&mut tx, id).await?;
        sqlx::query("DELETE FROM events WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(service_ids)
    }

    pub async fn add_update(
        pool: &DbPool,
        event_id: i64,
        html: &str,
        author_id: i64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("INSERT INTO event_updates (event_id, message, author_id) VALUES (?, ?, ?)")
            .bind(event_id)
            .bind(html)
            .bind(author_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn find_update(
        pool: &DbPool,
        event_id: i64,
        update_id: i64,
    ) -> Result<Option<EventUpdateWithAuthor>, sqlx::Error> {
        sqlx::query_as::<_, EventUpdateWithAuthor>(&format!(
            "SELECT {UPDATE_COLUMNS} FROM event_updates eu \
             INNER JOIN users u ON u.id = eu.author_id \
             WHERE eu.event_id = ? AND eu.id = ?"
        ))
        .bind(event_id)
        .bind(update_id)
        .fetch_optional(pool)
        .await
    }

    pub async fn delete_update(pool: &DbPool, update_id: i64) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM event_updates WHERE id = ?")
            .bind(update_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Updates of one event, newest first.
    pub async fn list_updates(
        pool: &DbPool,
        event_id: i64,
    ) -> Result<Vec<EventUpdateWithAuthor>, sqlx::Error> {
        Self::list_updates_for_events(pool, &[event_id]).await
    }

    /// Updates of several events in one query, newest first.
    pub async fn list_updates_for_events(
        pool: &DbPool,
        event_ids: &[i64],
    ) -> Result<Vec<EventUpdateWithAuthor>, sqlx::Error> {
        if event_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut qb = QueryBuilder::<Sqlite>::new(format!(
            "SELECT {UPDATE_COLUMNS} FROM event_updates eu \
             INNER JOIN users u ON u.id = eu.author_id WHERE eu.event_id IN ("
        ));
        let mut ids = qb.separated(", ");
        for id in event_ids {
            ids.push_bind(*id);
        }
        qb.push(") ORDER BY eu.created_at DESC, eu.id DESC");
        qb.build_query_as::<EventUpdateWithAuthor>()
            .fetch_all(pool)
            .await
    }

    /// Incidents and maintenances created since `since`, for the unread
    /// badge. Announcements are not interventions.
    pub async fn count_since(pool: &DbPool, since: DateTime<Utc>) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM events WHERE created_at >= ? AND kind != 'publication'",
        )
        .bind(clock::db(since))
        .fetch_one(pool)
        .await
    }

    /// Incidents that were open at some point after `since`, per service,
    /// for the availability strip. A span ends when the service came back,
    /// even if the team kept watching before closing.
    pub async fn incident_spans(
        pool: &DbPool,
        since: DateTime<Utc>,
    ) -> Result<HashMap<i64, Vec<IncidentSpan>>, sqlx::Error> {
        let rows: Vec<SpanRow> = sqlx::query_as(
            "SELECT es.service_id, e.severity, COALESCE(e.started_at, e.created_at), \
                        COALESCE(e.restored_at, e.ended_at) \
                 FROM events e INNER JOIN event_services es ON es.event_id = e.id \
                 WHERE e.kind = 'incident' AND e.lifecycle != 'cancelled' \
                   AND (COALESCE(e.restored_at, e.ended_at) IS NULL \
                        OR COALESCE(e.restored_at, e.ended_at) >= ?)",
        )
        .bind(clock::db(since))
        .fetch_all(pool)
        .await?;
        let mut spans: HashMap<i64, Vec<IncidentSpan>> = HashMap::new();
        for (service_id, severity, start, end) in rows {
            spans.entry(service_id).or_default().push(IncidentSpan {
                severity,
                start,
                end,
            });
        }
        Ok(spans)
    }
}

/// One incident on one service, as the availability strip reads it.
pub struct IncidentSpan {
    pub severity: Option<Severity>,
    pub start: DateTime<Utc>,
    pub end: Option<DateTime<Utc>>,
}

/// Service id, severity, start and end of an incident span.
type SpanRow = (i64, Option<Severity>, DateTime<Utc>, Option<DateTime<Utc>>);

async fn link_services(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    event_id: i64,
    service_ids: &[i64],
) -> Result<(), sqlx::Error> {
    for service_id in service_ids {
        sqlx::query("INSERT OR IGNORE INTO event_services (event_id, service_id) VALUES (?, ?)")
            .bind(event_id)
            .bind(service_id)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

async fn service_ids_in(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    event_id: i64,
) -> Result<Vec<i64>, sqlx::Error> {
    sqlx::query_scalar("SELECT service_id FROM event_services WHERE event_id = ?")
        .bind(event_id)
        .fetch_all(&mut **tx)
        .await
}

fn push_filters(qb: &mut QueryBuilder<'_, Sqlite>, filters: &EventFilters) {
    qb.push(" WHERE 1 = 1");
    if let Some(kind) = filters.kind {
        qb.push(" AND f.kind = ").push_bind(kind);
    }
    if let Some(group) = filters.lifecycle_group {
        qb.push(" AND f.lifecycle IN (");
        let mut states = qb.separated(", ");
        for lifecycle in group.lifecycles() {
            states.push_bind(*lifecycle);
        }
        qb.push(")");
    }
    if let Some(service_id) = filters.service_id {
        qb.push(" AND EXISTS (SELECT 1 FROM event_services es WHERE es.event_id = f.id AND es.service_id = ")
            .push_bind(service_id)
            .push(")");
    }
    if let Some(from) = filters.from {
        qb.push(" AND f.created_at >= ").push_bind(clock::db(from));
    }
    if let Some(to) = filters.to {
        qb.push(" AND f.created_at <= ").push_bind(clock::db(to));
    }
    if let Some(query) = filters
        .q
        .as_deref()
        .map(fts_query)
        .filter(|q| !q.is_empty())
    {
        qb.push(" AND f.id IN (SELECT rowid FROM events_fts WHERE events_fts MATCH ")
            .push_bind(query)
            .push(")");
    }
}

/// Turns what a reader typed into a literal FTS5 query: punctuation
/// separates words, operators are dropped and each word matches as a
/// prefix, so "stock" finds "stockage".
pub fn fts_query(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .take(MAX_QUERY_CHARS)
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    cleaned
        .split_whitespace()
        .take(MAX_QUERY_TERMS)
        .map(|word| format!("\"{word}\"*"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Category, Role, Severity};
    use crate::repositories::{ServiceRepository, UserRepository};
    use crate::test_helpers::test_pool;

    async fn seed_user_and_service(pool: &DbPool) -> (i64, i64) {
        let user = UserRepository::create(pool, "t@t.com", "hash", "Tester", Role::Publisher)
            .await
            .unwrap();
        let svc = ServiceRepository::create(pool, "API", "api", None, None, None)
            .await
            .unwrap();
        (user.id, svc.id)
    }

    fn incident(title: &str, author_id: i64, service_ids: Vec<i64>) -> CreateEventInput {
        CreateEventInput {
            kind: Kind::Incident,
            severity: Some(Severity::Critical),
            planned: false,
            category: None,
            title: title.to_string(),
            description: "Something broke".to_string(),
            planned_start: None,
            planned_end: None,
            started_at: None,
            opening_step: None,
            keeps_services_up: false,
            service_ids,
            follows_event_id: None,
            author_id,
        }
    }

    fn maintenance(
        author_id: i64,
        service_ids: Vec<i64>,
        start: DateTime<Utc>,
    ) -> CreateEventInput {
        CreateEventInput {
            kind: Kind::Maintenance,
            severity: None,
            planned: true,
            category: None,
            title: "Upgrade".to_string(),
            description: String::new(),
            planned_start: Some(start),
            planned_end: Some(start + chrono::Duration::hours(1)),
            started_at: None,
            opening_step: None,
            keeps_services_up: false,
            service_ids,
            follows_event_id: None,
            author_id,
        }
    }

    fn page(limit: i64) -> EventFilters {
        EventFilters {
            limit,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn create_and_find_with_services() {
        let pool = test_pool().await;
        let (uid, sid) = seed_user_and_service(&pool).await;
        let event = EventRepository::create(&pool, &incident("DB Down", uid, vec![sid]))
            .await
            .unwrap();
        assert_eq!(event.lifecycle, Some(Lifecycle::Investigating));

        let found = EventRepository::find_with_services(&pool, event.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.services.len(), 1);
        assert_eq!(found.event.title, "DB Down");
    }

    #[tokio::test]
    async fn list_page_names_services_and_counts_all_pages() {
        let pool = test_pool().await;
        let (uid, sid) = seed_user_and_service(&pool).await;
        for i in 0..5 {
            EventRepository::create(&pool, &incident(&format!("Event {i}"), uid, vec![sid]))
                .await
                .unwrap();
        }
        let rows = EventRepository::list_page(&pool, &page(3)).await.unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].services(), vec!["API"]);
        assert_eq!(
            EventRepository::count_filtered(&pool, &page(3))
                .await
                .unwrap(),
            5
        );
    }

    #[tokio::test]
    async fn same_day_bounds_include_the_day() {
        let pool = test_pool().await;
        let (uid, sid) = seed_user_and_service(&pool).await;
        EventRepository::create(&pool, &incident("Today", uid, vec![sid]))
            .await
            .unwrap();
        let now = Utc::now();
        let filters = EventFilters {
            from: Some(now - chrono::Duration::minutes(5)),
            to: Some(now + chrono::Duration::minutes(5)),
            limit: 10,
            ..Default::default()
        };
        assert_eq!(
            EventRepository::list_page(&pool, &filters)
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn search_matches_description_and_prefixes() {
        let pool = test_pool().await;
        let (uid, sid) = seed_user_and_service(&pool).await;
        let mut input = incident("Network issue", uid, vec![sid]);
        input.description = "Le stockage répond lentement".to_string();
        EventRepository::create(&pool, &input).await.unwrap();
        EventRepository::create(&pool, &incident("Database outage", uid, vec![sid]))
            .await
            .unwrap();

        let filters = EventFilters {
            q: Some("stock".to_string()),
            limit: 10,
            ..Default::default()
        };
        let rows = EventRepository::list_page(&pool, &filters).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "Network issue");
    }

    #[tokio::test]
    async fn monitoring_incident_no_longer_drives_the_status() {
        let pool = test_pool().await;
        let (uid, sid) = seed_user_and_service(&pool).await;
        let open = EventRepository::create(&pool, &incident("Open", uid, vec![sid]))
            .await
            .unwrap();
        assert_eq!(
            EventRepository::status_drivers(&pool, sid)
                .await
                .unwrap()
                .len(),
            1
        );

        EventRepository::transition(
            &pool,
            open.id,
            Lifecycle::Investigating,
            Lifecycle::Monitoring,
            Utc::now(),
        )
        .await
        .unwrap();
        assert!(
            EventRepository::status_drivers(&pool, sid)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn due_maintenance_starts_then_completes_on_its_own() {
        let pool = test_pool().await;
        let (uid, sid) = seed_user_and_service(&pool).await;
        let start = Utc::now() - chrono::Duration::minutes(30);
        let event = EventRepository::create(&pool, &maintenance(uid, vec![sid], start))
            .await
            .unwrap();
        assert!(
            EventRepository::status_drivers(&pool, sid)
                .await
                .unwrap()
                .is_empty()
        );

        let started = EventRepository::start_due_maintenance(&pool, Utc::now())
            .await
            .unwrap();
        assert_eq!(started, vec![event.id]);
        let running = EventRepository::find_by_id(&pool, event.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(running.lifecycle, Some(Lifecycle::InProgress));
        assert_eq!(
            running.started_at.map(|d| d.timestamp()),
            Some(start.timestamp())
        );
        assert_eq!(
            EventRepository::status_drivers(&pool, sid)
                .await
                .unwrap()
                .len(),
            1
        );

        let later = start + chrono::Duration::hours(2);
        let completed = EventRepository::complete_due_maintenance(&pool, later)
            .await
            .unwrap();
        assert_eq!(completed, vec![event.id]);
        let done = EventRepository::find_by_id(&pool, event.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(done.lifecycle, Some(Lifecycle::Completed));
        assert!(done.ended_at.is_some());
    }

    #[tokio::test]
    async fn future_maintenance_waits() {
        let pool = test_pool().await;
        let (uid, sid) = seed_user_and_service(&pool).await;
        let start = Utc::now() + chrono::Duration::hours(3);
        EventRepository::create(&pool, &maintenance(uid, vec![sid], start))
            .await
            .unwrap();
        assert!(
            EventRepository::start_due_maintenance(&pool, Utc::now())
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn transition_records_times_and_revert_restores() {
        let pool = test_pool().await;
        let (uid, sid) = seed_user_and_service(&pool).await;
        let event = EventRepository::create(&pool, &incident("Issue", uid, vec![sid]))
            .await
            .unwrap();
        assert!(
            EventRepository::transition(
                &pool,
                event.id,
                Lifecycle::Investigating,
                Lifecycle::Resolved,
                Utc::now()
            )
            .await
            .unwrap()
        );
        assert!(
            !EventRepository::transition(
                &pool,
                event.id,
                Lifecycle::Investigating,
                Lifecycle::Resolved,
                Utc::now()
            )
            .await
            .unwrap(),
            "a transition from a state the event has left is refused"
        );
        let closed = EventRepository::find_by_id(&pool, event.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(closed.lifecycle, Some(Lifecycle::Resolved));
        assert!(closed.ended_at.is_some());

        EventRepository::revert_transition(&pool, event.id)
            .await
            .unwrap();
        let reopened = EventRepository::find_by_id(&pool, event.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(reopened.lifecycle, Some(Lifecycle::Investigating));
        assert!(reopened.ended_at.is_none());
    }

    #[tokio::test]
    async fn updates_list_newest_first() {
        let pool = test_pool().await;
        let (uid, sid) = seed_user_and_service(&pool).await;
        let event = EventRepository::create(&pool, &incident("Bug", uid, vec![sid]))
            .await
            .unwrap();
        EventRepository::add_update(&pool, event.id, "<p>first</p>", uid)
            .await
            .unwrap();
        EventRepository::add_update(&pool, event.id, "<p>second</p>", uid)
            .await
            .unwrap();
        let updates = EventRepository::list_updates(&pool, event.id)
            .await
            .unwrap();
        assert_eq!(updates[0].message, "<p>second</p>");
        assert_eq!(updates[0].author_name, "Tester");

        let banner = EventRepository::list_open_for_banner(&pool).await.unwrap();
        assert_eq!(banner[0].latest_update.as_deref(), Some("<p>second</p>"));
    }

    #[tokio::test]
    async fn unread_count_skips_announcements() {
        let pool = test_pool().await;
        let (uid, sid) = seed_user_and_service(&pool).await;
        let before = Utc::now() - chrono::Duration::minutes(1);
        EventRepository::create(&pool, &incident("Incident", uid, vec![sid]))
            .await
            .unwrap();
        let mut note = incident("Info", uid, vec![]);
        note.kind = Kind::Publication;
        note.severity = None;
        note.category = Some(Category::Info);
        EventRepository::create(&pool, &note).await.unwrap();

        assert_eq!(
            EventRepository::count_since(&pool, before).await.unwrap(),
            1
        );
    }

    #[test]
    fn fts_query_keeps_words_as_prefixes() {
        assert_eq!(fts_query("hello* +world"), "\"hello\"* \"world\"*");
        assert_eq!(fts_query("+ - * \"\""), "");
        assert_eq!(fts_query(""), "");
        assert_eq!(fts_query("l'hébergeur"), "\"l\"* \"hébergeur\"*");
    }
}
