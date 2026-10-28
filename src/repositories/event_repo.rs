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
    ActivityKind, CreateEventInput, DueReminder, Event, EventFilters, EventSummary,
    EventUpdateWithAuthor, EventWithServices, Kind, Lifecycle, NextUpdate, Scope, Service,
    Severity, UpdateEventInput,
};
use crate::repositories::service_repo::CHOSEN_ORDER;

const SUMMARY_COLUMNS: &str = "e.id, e.kind, e.severity, e.planned, e.lifecycle, e.category, \
     e.title, e.description, e.planned_start, e.planned_end, e.started_at, e.ended_at, \
     e.keeps_services_up, e.next_update_at, e.created_at, e.updated_at, e.author_id, \
     EXISTS (SELECT 1 FROM event_reviews r WHERE r.event_id = e.id) AS has_review, \
     COALESCE((SELECT GROUP_CONCAT(s.name, char(31) ORDER BY s.position, s.name COLLATE NOCASE, s.id) \
               FROM event_services es JOIN services s ON s.id = es.service_id \
               WHERE es.event_id = e.id), '') AS service_names, \
     COALESCE((SELECT GROUP_CONCAT(s.id, ',' ORDER BY s.position, s.name COLLATE NOCASE, s.id) \
               FROM event_services es JOIN services s ON s.id = es.service_id \
               WHERE es.event_id = e.id), '') AS service_ids, \
     COALESCE((SELECT GROUP_CONCAT(COALESCE(s.icon_name, '') || char(30) || COALESCE(i.filename, ''), \
                                   char(31) ORDER BY s.position, s.name COLLATE NOCASE, s.id) \
               FROM event_services es JOIN services s ON s.id = es.service_id \
               LEFT JOIN icons i ON i.id = s.icon_id \
               WHERE es.event_id = e.id), '') AS service_icons";

const LATEST_UPDATE_COLUMNS: &str = "\
     (SELECT u.message FROM event_updates u WHERE u.event_id = e.id \
      ORDER BY u.created_at DESC, u.id DESC LIMIT 1) AS latest_update, \
     (SELECT u.created_at FROM event_updates u WHERE u.event_id = e.id \
      ORDER BY u.created_at DESC, u.id DESC LIMIT 1) AS latest_update_at";

/// An update is signed with the name of the API key that posted it, else
/// with its author's.
const UPDATE_COLUMNS: &str = "eu.id, eu.event_id, eu.message, eu.author_id, eu.created_at, \
     COALESCE(k.name, u.display_name) AS author_name";

const UPDATE_AUTHORS: &str = "INNER JOIN users u ON u.id = eu.author_id \
     LEFT JOIN api_keys k ON k.id = eu.api_key_id";

/// Longest search text and number of words kept from it.
const MAX_QUERY_CHARS: usize = 200;

/// [`drives_services`] in SQL, on events aliased `e`: an incident until it
/// is under watch, a maintenance while it runs unless it keeps its services
/// up. A test holds the two to the same answers.
///
/// [`drives_services`]: crate::models::drives_services
const DRIVES_SERVICES: &str = "((e.kind = 'incident' \
       AND e.lifecycle IN ('investigating', 'in_progress')) \
     OR (e.kind = 'maintenance' AND e.lifecycle = 'in_progress' \
       AND NOT e.keeps_services_up))";

/// The worst first, events aliased `e`: a critical incident, a minor one,
/// then a maintenance, the latest first among equals.
const WORST_FIRST: &str = "CASE e.severity WHEN 'critical' THEN 0 WHEN 'minor' THEN 1 ELSE 2 END, \
     e.created_at DESC";
const MAX_QUERY_TERMS: usize = 10;

pub struct EventRepository;

impl EventRepository {
    pub async fn create(pool: &DbPool, input: &CreateEventInput) -> Result<Event, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let event = Self::insert(&mut tx, input, None).await?;
        tx.commit().await?;
        Ok(event)
    }

    /// Writes a new event and its services inside `tx`, as a session of
    /// `series_id` when it repeats.
    pub(crate) async fn insert(
        tx: &mut sqlx::Transaction<'_, Sqlite>,
        input: &CreateEventInput,
        series_id: Option<i64>,
    ) -> Result<Event, sqlx::Error> {
        let event = sqlx::query_as::<_, Event>(
            "INSERT INTO events (kind, severity, planned, lifecycle, category, title, \
             description, planned_start, planned_end, started_at, follows_event_id, author_id, \
             api_key_id, keeps_services_up, next_update_at, update_every_minutes, next_update_by, \
             series_id) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING *",
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
        .bind(input.api_key_id)
        .bind(input.kind == Kind::Maintenance && input.keeps_services_up)
        .bind(input.next_update.map(|next| clock::db(next.at)))
        .bind(input.next_update.and_then(|next| next.every_minutes))
        .bind(input.next_update.map(|_| input.author_id))
        .bind(series_id)
        .fetch_one(&mut **tx)
        .await?;
        link_services(tx, event.id, &input.service_ids).await?;
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
        scope: &Scope,
    ) -> Result<Option<EventWithServices>, sqlx::Error> {
        let Some(event) = Self::find_by_id(pool, id).await? else {
            return Ok(None);
        };
        let mut services = sqlx::query_as::<_, Service>(&format!(
            "SELECT s.* FROM services s \
             INNER JOIN event_services es ON es.service_id = s.id \
             WHERE es.event_id = ? ORDER BY {CHOSEN_ORDER}"
        ))
        .bind(id)
        .fetch_all(pool)
        .await?;
        let ids: Vec<i64> = services.iter().map(|service| service.id).collect();
        if !scope.sees_event(&ids) {
            return Ok(None);
        }
        services.retain(|service| scope.sees_service(service.id));
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
        scope: &Scope,
    ) -> Result<Vec<EventSummary>, sqlx::Error> {
        let mut qb = QueryBuilder::<Sqlite>::new(format!(
            "SELECT {SUMMARY_COLUMNS} FROM events e WHERE e.id IN (SELECT f.id FROM events f"
        ));
        push_filters(&mut qb, filters, scope);
        qb.push(" ORDER BY f.created_at DESC, f.id DESC LIMIT ")
            .push_bind(filters.limit)
            .push(" OFFSET ")
            .push_bind(filters.offset)
            .push(") ORDER BY e.created_at DESC, e.id DESC");
        fetch_scoped(qb, pool, scope).await
    }

    /// Number of events matching the filters, across every page.
    pub async fn count_filtered(
        pool: &DbPool,
        filters: &EventFilters,
        scope: &Scope,
    ) -> Result<i64, sqlx::Error> {
        let mut qb = QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM events f");
        push_filters(&mut qb, filters, scope);
        qb.build_query_scalar::<i64>().fetch_one(pool).await
    }

    /// Latest incidents and announcements; maintenances have their own card.
    /// The latest events of the kinds the activity card shows.
    pub async fn list_recent_activity(
        pool: &DbPool,
        limit: i64,
        kinds: &[ActivityKind],
        scope: &Scope,
    ) -> Result<Vec<EventSummary>, sqlx::Error> {
        if kinds.is_empty() {
            return Ok(Vec::new());
        }
        let wanted = kinds
            .iter()
            .map(|kind| activity_condition(*kind))
            .collect::<Vec<_>>()
            .join(" OR ");
        let mut qb = QueryBuilder::<Sqlite>::new(format!(
            "SELECT {SUMMARY_COLUMNS} FROM events e WHERE e.id IN ( \
               SELECT x.id FROM events x WHERE ({wanted})"
        ));
        push_scope(&mut qb, scope, "x");
        qb.push(" ORDER BY x.created_at DESC, x.id DESC LIMIT ")
            .push_bind(limit)
            .push(") ORDER BY e.created_at DESC, e.id DESC");
        fetch_scoped(qb, pool, scope).await
    }

    /// Newest events with their last activity time and their review, for the
    /// Atom feed. A posted update does not touch `events.updated_at`, so it
    /// is folded in.
    pub async fn list_for_feed(
        pool: &DbPool,
        limit: i64,
        scope: &Scope,
    ) -> Result<Vec<EventSummary>, sqlx::Error> {
        let mut qb = QueryBuilder::<Sqlite>::new(format!(
            "SELECT {SUMMARY_COLUMNS}, \
               MAX(e.updated_at, COALESCE((SELECT MAX(u.created_at) FROM event_updates u \
                   WHERE u.event_id = e.id), e.updated_at)) AS last_activity_at, \
               (SELECT r.body FROM event_reviews r WHERE r.event_id = e.id) AS review_body \
             FROM events e WHERE e.id IN ( \
               SELECT x.id FROM events x WHERE 1 = 1"
        ));
        push_scope(&mut qb, scope, "x");
        qb.push(" ORDER BY x.created_at DESC, x.id DESC LIMIT ")
            .push_bind(limit)
            .push(") ORDER BY e.created_at DESC, e.id DESC");
        fetch_scoped(qb, pool, scope).await
    }

    /// The events that hold services in their state, the same ones as
    /// `status_drivers`, worst first: what the status banner cites.
    pub async fn list_open_for_banner(
        pool: &DbPool,
        scope: &Scope,
    ) -> Result<Vec<EventSummary>, sqlx::Error> {
        let mut qb = QueryBuilder::<Sqlite>::new(format!(
            "SELECT {SUMMARY_COLUMNS}, {LATEST_UPDATE_COLUMNS} FROM events e \
             WHERE {DRIVES_SERVICES}"
        ));
        push_scope(&mut qb, scope, "e");
        qb.push(format!(" ORDER BY {WORST_FIRST}"));
        fetch_scoped(qb, pool, scope).await
    }

    /// Maintenances under way, then those still to come, soonest first.
    pub async fn list_open_maintenance(
        pool: &DbPool,
        scope: &Scope,
    ) -> Result<Vec<EventSummary>, sqlx::Error> {
        let mut qb = QueryBuilder::<Sqlite>::new(format!(
            "SELECT {SUMMARY_COLUMNS} FROM events e \
             WHERE e.kind = 'maintenance' AND e.lifecycle IN ('in_progress', 'scheduled')"
        ));
        push_scope(&mut qb, scope, "e");
        qb.push(
            " ORDER BY CASE e.lifecycle WHEN 'in_progress' THEN 0 ELSE 1 END, \
             COALESCE(e.planned_start, e.started_at, e.created_at) ASC",
        );
        fetch_scoped(qb, pool, scope).await
    }

    /// Maintenances that ended or were called off, latest first.
    pub async fn list_finished_maintenance(
        pool: &DbPool,
        limit: i64,
        scope: &Scope,
    ) -> Result<Vec<EventSummary>, sqlx::Error> {
        let mut qb = QueryBuilder::<Sqlite>::new(format!(
            "SELECT {SUMMARY_COLUMNS} FROM events e WHERE e.id IN ( \
               SELECT x.id FROM events x \
               WHERE x.kind = 'maintenance' AND x.lifecycle IN ('completed', 'cancelled')"
        ));
        push_scope(&mut qb, scope, "x");
        qb.push(" ORDER BY x.updated_at DESC LIMIT ")
            .push_bind(limit)
            .push(") ORDER BY COALESCE(e.ended_at, e.updated_at) DESC");
        fetch_scoped(qb, pool, scope).await
    }

    /// For each service held in its state by open work, the event that sets
    /// it, the worst first when several do: what the services page names.
    /// Takes no scope: the event names the service, so it is visible
    /// whenever the service is.
    pub async fn state_drivers(
        pool: &DbPool,
        service_id: Option<i64>,
    ) -> Result<HashMap<i64, (i64, String)>, sqlx::Error> {
        let rows: Vec<(i64, i64, String)> = sqlx::query_as(&format!(
            "SELECT es.service_id, e.id, e.title FROM events e \
             INNER JOIN event_services es ON es.event_id = e.id \
             WHERE (? IS NULL OR es.service_id = ?) AND {DRIVES_SERVICES} \
             ORDER BY {WORST_FIRST}"
        ))
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
        sqlx::query_as(&format!(
            "SELECT e.kind, e.severity FROM events e \
             INNER JOIN event_services es ON es.event_id = e.id \
             WHERE es.service_id = ? AND {DRIVES_SERVICES}"
        ))
        .bind(service_id)
        .fetch_all(pool)
        .await
    }

    /// Services a maintenance under way takes down: the checks keep quiet
    /// about them.
    pub async fn services_under_maintenance(pool: &DbPool) -> Result<Vec<i64>, sqlx::Error> {
        sqlx::query_scalar(&format!(
            "SELECT DISTINCT es.service_id FROM events e \
             INNER JOIN event_services es ON es.event_id = e.id \
             WHERE e.kind = 'maintenance' AND {DRIVES_SERVICES}"
        ))
        .fetch_all(pool)
        .await
    }

    /// Starts every announced maintenance whose start has come.
    pub async fn start_due_maintenance(
        pool: &DbPool,
        now: DateTime<Utc>,
    ) -> Result<Vec<DueMaintenance>, sqlx::Error> {
        sqlx::query_as(
            "UPDATE events SET previous_lifecycle = NULL, lifecycle = 'in_progress', \
               started_at = COALESCE(started_at, planned_start) \
             WHERE kind = 'maintenance' AND lifecycle = 'scheduled' \
               AND planned_start IS NOT NULL AND planned_start <= ? \
             RETURNING id, series_id IS NOT NULL AS repeats",
        )
        .bind(clock::db(now))
        .fetch_all(pool)
        .await
    }

    /// Completes every maintenance under way whose announced end has come.
    pub async fn complete_due_maintenance(
        pool: &DbPool,
        now: DateTime<Utc>,
    ) -> Result<Vec<DueMaintenance>, sqlx::Error> {
        sqlx::query_as(
            "UPDATE events SET previous_lifecycle = NULL, lifecycle = 'completed', \
               ended_at = COALESCE(ended_at, planned_end) \
             WHERE kind = 'maintenance' AND lifecycle = 'in_progress' \
               AND planned_end IS NOT NULL AND planned_end <= ? \
             RETURNING id, series_id IS NOT NULL AS repeats",
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

    /// Replaces when the next update is due and who said so, ready to be
    /// reminded again. Leaves the row alone when nothing changes, so its
    /// last change time stays.
    pub async fn set_next_update(
        pool: &DbPool,
        id: i64,
        next: Option<NextUpdate>,
        by: i64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE events SET next_update_at = ?1, update_every_minutes = ?2, \
             next_update_by = ?3, next_update_reminded = 0 \
             WHERE id = ?4 AND (next_update_at IS NOT ?1 OR update_every_minutes IS NOT ?2 \
             OR next_update_by IS NOT ?3)",
        )
        .bind(next.map(|next| clock::db(next.at)))
        .bind(next.and_then(|next| next.every_minutes))
        .bind(next.map(|_| by))
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Open incidents whose announced update was due by `cutoff` and whose
    /// announcer has not been reminded yet.
    pub async fn due_reminders(
        pool: &DbPool,
        cutoff: DateTime<Utc>,
    ) -> Result<Vec<DueReminder>, sqlx::Error> {
        sqlx::query_as::<_, DueReminder>(
            "SELECT e.id AS event_id, e.title, e.next_update_at, u.id AS user_id, u.email, \
             u.preferred_locale FROM events e JOIN users u ON u.id = e.next_update_by \
             WHERE e.kind = 'incident' AND e.lifecycle NOT IN ('resolved', 'cancelled') \
             AND e.next_update_at <= ? AND e.next_update_reminded = 0 AND u.is_active = 1 \
             AND u.role IN ('publisher', 'admin')",
        )
        .bind(clock::db(cutoff))
        .fetch_all(pool)
        .await
    }

    /// Notes the reminder of the update due at `due_at`. False when a new
    /// time was announced meanwhile, or another round got there first.
    pub async fn mark_reminded(
        pool: &DbPool,
        id: i64,
        due_at: DateTime<Utc>,
    ) -> Result<bool, sqlx::Error> {
        let done = sqlx::query(
            "UPDATE events SET next_update_reminded = 1 \
             WHERE id = ? AND next_update_at = ? AND next_update_reminded = 0",
        )
        .bind(id)
        .bind(clock::db(due_at))
        .execute(pool)
        .await?;
        Ok(done.rows_affected() == 1)
    }

    /// Returns the id of the update. `api_key_id` is the key that posts it,
    /// which signs it.
    pub async fn add_update(
        pool: &DbPool,
        event_id: i64,
        html: &str,
        author_id: i64,
        api_key_id: Option<i64>,
    ) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar(
            "INSERT INTO event_updates (event_id, message, author_id, api_key_id) \
             VALUES (?, ?, ?, ?) RETURNING id",
        )
        .bind(event_id)
        .bind(html)
        .bind(author_id)
        .bind(api_key_id)
        .fetch_one(pool)
        .await
    }

    /// The name that signs an event: the API key that published it, else
    /// its author. `None` when no such event exists.
    pub async fn author_name(pool: &DbPool, event_id: i64) -> Result<Option<String>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT COALESCE(k.name, u.display_name) FROM events e \
             INNER JOIN users u ON u.id = e.author_id \
             LEFT JOIN api_keys k ON k.id = e.api_key_id \
             WHERE e.id = ?",
        )
        .bind(event_id)
        .fetch_optional(pool)
        .await
    }

    pub async fn find_update(
        pool: &DbPool,
        event_id: i64,
        update_id: i64,
    ) -> Result<Option<EventUpdateWithAuthor>, sqlx::Error> {
        sqlx::query_as::<_, EventUpdateWithAuthor>(&format!(
            "SELECT {UPDATE_COLUMNS} FROM event_updates eu {UPDATE_AUTHORS} \
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
            "SELECT {UPDATE_COLUMNS} FROM event_updates eu {UPDATE_AUTHORS} \
             WHERE eu.event_id IN ("
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
    /// badge. Announcements are not interventions, and the sessions a
    /// repeating maintenance adds after its first are not news.
    pub async fn count_since(
        pool: &DbPool,
        since: DateTime<Utc>,
        scope: &Scope,
    ) -> Result<i64, sqlx::Error> {
        let mut qb = QueryBuilder::<Sqlite>::new(
            "SELECT COUNT(*) FROM events e WHERE e.kind != 'publication' \
             AND NOT (e.series_id IS NOT NULL \
                      AND e.id > (SELECT MIN(f.id) FROM events f WHERE f.series_id = e.series_id)) \
             AND e.created_at >= ",
        );
        qb.push_bind(clock::db(since));
        push_scope(&mut qb, scope, "e");
        qb.build_query_scalar::<i64>().fetch_one(pool).await
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

/// The rows of one kind of activity; fixed text, never user input. An
/// incident without a severity counts as a minor one.
fn activity_condition(kind: ActivityKind) -> &'static str {
    match kind {
        ActivityKind::MinorIncidents => {
            "(kind = 'incident' AND (severity = 'minor' OR severity IS NULL))"
        }
        ActivityKind::MajorIncidents => "(kind = 'incident' AND severity = 'critical')",
        ActivityKind::Maintenance => "kind = 'maintenance'",
        ActivityKind::Announcements => "kind = 'publication'",
    }
}

/// A maintenance the schedule started or completed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::FromRow)]
pub struct DueMaintenance {
    pub id: i64,
    /// A session of a repeating maintenance, which passes in silence.
    pub repeats: bool,
}

/// One incident on one service, as the availability strip reads it.
pub struct IncidentSpan {
    pub severity: Option<Severity>,
    pub start: DateTime<Utc>,
    pub end: Option<DateTime<Utc>>,
}

/// Service id, severity, start and end of an incident span.
type SpanRow = (i64, Option<Severity>, DateTime<Utc>, Option<DateTime<Utc>>);

pub(crate) async fn link_services(
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

/// Runs a summary query and drops, from each row, the services the viewer
/// may not see.
async fn fetch_scoped(
    mut qb: QueryBuilder<'_, Sqlite>,
    pool: &DbPool,
    scope: &Scope,
) -> Result<Vec<EventSummary>, sqlx::Error> {
    let mut rows = qb.build_query_as::<EventSummary>().fetch_all(pool).await?;
    for row in &mut rows {
        row.restrict_to(scope);
    }
    Ok(rows)
}

/// Keeps the events the viewer may see, on the events table aliased
/// `alias`: those naming no service, or at least one visible service. Adds
/// nothing when nothing is hidden.
fn push_scope(qb: &mut QueryBuilder<'_, Sqlite>, scope: &Scope, alias: &str) {
    let Some(visible) = scope.json() else {
        return;
    };
    qb.push(format!(
        " AND (NOT EXISTS (SELECT 1 FROM event_services v WHERE v.event_id = {alias}.id) \
         OR EXISTS (SELECT 1 FROM event_services v WHERE v.event_id = {alias}.id \
                    AND v.service_id IN (SELECT value FROM json_each("
    ))
    .push_bind(visible)
    .push("))))");
}

/// Narrows to one service. Asking for a hidden one matches nothing, or an
/// event naming both would reveal that it concerns the hidden service.
fn push_service_filter(qb: &mut QueryBuilder<'_, Sqlite>, service_id: i64, scope: &Scope) {
    if !scope.sees_service(service_id) {
        qb.push(" AND 0");
        return;
    }
    qb.push(" AND EXISTS (SELECT 1 FROM event_services es WHERE es.event_id = f.id AND es.service_id = ")
        .push_bind(service_id)
        .push(")");
}

fn push_filters(qb: &mut QueryBuilder<'_, Sqlite>, filters: &EventFilters, scope: &Scope) {
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
        push_service_filter(qb, service_id, scope);
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
    push_scope(qb, scope, "f");
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
    use crate::models::{Category, ICON_SEPARATOR, NAME_SEPARATOR, Role, Severity};
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
            api_key_id: None,
            next_update: None,
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
            api_key_id: None,
            next_update: None,
        }
    }

    /// The SQL rule and `drives_services` answer alike for every kind,
    /// step and downtime flag the table accepts.
    #[tokio::test]
    async fn the_sql_rule_matches_drives_services() {
        let pool = test_pool().await;
        let (author, _) = seed_user_and_service(&pool).await;
        let steps = [
            ("incident", "investigating"),
            ("incident", "in_progress"),
            ("incident", "monitoring"),
            ("incident", "resolved"),
            ("incident", "cancelled"),
            ("maintenance", "scheduled"),
            ("maintenance", "in_progress"),
            ("maintenance", "completed"),
            ("maintenance", "cancelled"),
        ];
        for keeps_up in [false, true] {
            for (kind, lifecycle) in steps {
                sqlx::query(
                    "INSERT INTO events (kind, lifecycle, keeps_services_up, title, description, \
                                         author_id) VALUES (?, ?, ?, 't', '', ?)",
                )
                .bind(kind)
                .bind(lifecycle)
                .bind(keeps_up)
                .bind(author)
                .execute(&pool)
                .await
                .unwrap();
            }
        }
        sqlx::query(
            "INSERT INTO events (kind, category, title, description, author_id) \
             VALUES ('publication', 'info', 't', '', ?)",
        )
        .bind(author)
        .execute(&pool)
        .await
        .unwrap();

        let rows: Vec<(Kind, Option<Lifecycle>, bool, bool)> = sqlx::query_as(&format!(
            "SELECT e.kind, e.lifecycle, e.keeps_services_up, {DRIVES_SERVICES} FROM events e"
        ))
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(rows.len(), 19);
        for (kind, lifecycle, keeps_up, in_sql) in rows {
            assert_eq!(
                in_sql,
                crate::models::drives_services(kind, lifecycle, keeps_up),
                "{kind:?} {lifecycle:?} keeps up: {keeps_up}"
            );
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

        let found = EventRepository::find_with_services(&pool, event.id, &Scope::All)
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
        let rows = EventRepository::list_page(&pool, &page(3), &Scope::All)
            .await
            .unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].services(), vec!["API"]);
        assert_eq!(
            EventRepository::count_filtered(&pool, &page(3), &Scope::All)
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
            EventRepository::list_page(&pool, &filters, &Scope::All)
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
        let rows = EventRepository::list_page(&pool, &filters, &Scope::All)
            .await
            .unwrap();
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
        assert_eq!(
            EventRepository::status_drivers(&pool, sid).await.unwrap(),
            []
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
        assert_eq!(
            EventRepository::status_drivers(&pool, sid).await.unwrap(),
            []
        );

        let started = EventRepository::start_due_maintenance(&pool, Utc::now())
            .await
            .unwrap();
        let moved = DueMaintenance {
            id: event.id,
            repeats: false,
        };
        assert_eq!(started, vec![moved]);
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
        assert_eq!(completed, vec![moved]);
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
        assert_eq!(
            EventRepository::start_due_maintenance(&pool, Utc::now())
                .await
                .unwrap(),
            Vec::<DueMaintenance>::new()
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

    /// What a key publishes is signed with its name; what a member writes,
    /// with theirs.
    #[tokio::test]
    async fn a_key_signs_what_it_publishes() {
        let pool = test_pool().await;
        let (uid, sid) = seed_user_and_service(&pool).await;
        let key =
            crate::repositories::ApiKeyRepository::create(&pool, "Zabbix", "stp_a", "hash", uid)
                .await
                .unwrap();
        let by_key = EventRepository::create(
            &pool,
            &CreateEventInput {
                api_key_id: Some(key),
                ..incident("Mail down", uid, vec![sid])
            },
        )
        .await
        .unwrap();
        let by_member = EventRepository::create(&pool, &incident("VPN down", uid, vec![sid]))
            .await
            .unwrap();
        assert_eq!(by_key.author_id, uid, "the key's creator stays the author");
        let signed = |id| EventRepository::author_name(&pool, id);
        assert_eq!(signed(by_key.id).await.unwrap().as_deref(), Some("Zabbix"));
        assert_eq!(
            signed(by_member.id).await.unwrap().as_deref(),
            Some("Tester")
        );
        assert_eq!(signed(by_member.id + 100).await.unwrap(), None);

        EventRepository::add_update(&pool, by_key.id, "<p>member</p>", uid, None)
            .await
            .unwrap();
        let posted = EventRepository::add_update(&pool, by_key.id, "<p>key</p>", uid, Some(key))
            .await
            .unwrap();
        let updates = EventRepository::list_updates(&pool, by_key.id)
            .await
            .unwrap();
        let authors: Vec<&str> = updates.iter().map(|u| u.author_name.as_str()).collect();
        assert_eq!(authors, ["Zabbix", "Tester"]);
        let found = EventRepository::find_update(&pool, by_key.id, posted)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.author_name, "Zabbix");
    }

    #[tokio::test]
    async fn updates_list_newest_first() {
        let pool = test_pool().await;
        let (uid, sid) = seed_user_and_service(&pool).await;
        let event = EventRepository::create(&pool, &incident("Bug", uid, vec![sid]))
            .await
            .unwrap();
        EventRepository::add_update(&pool, event.id, "<p>first</p>", uid, None)
            .await
            .unwrap();
        EventRepository::add_update(&pool, event.id, "<p>second</p>", uid, None)
            .await
            .unwrap();
        let updates = EventRepository::list_updates(&pool, event.id)
            .await
            .unwrap();
        assert_eq!(updates[0].message, "<p>second</p>");
        assert_eq!(updates[0].author_name, "Tester");

        let banner = EventRepository::list_open_for_banner(&pool, &Scope::All)
            .await
            .unwrap();
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
            EventRepository::count_since(&pool, before, &Scope::All)
                .await
                .unwrap(),
            1
        );
    }

    async fn named_service(pool: &DbPool, name: &str) -> i64 {
        let icon = name.to_lowercase();
        ServiceRepository::create(pool, name, &icon, None, None, Some(&icon))
            .await
            .unwrap()
            .id
    }

    async fn create_event(pool: &DbPool, title: &str, author: i64, services: Vec<i64>) -> i64 {
        EventRepository::create(pool, &incident(title, author, services))
            .await
            .unwrap()
            .id
    }

    fn titles(rows: &[EventSummary]) -> Vec<&str> {
        rows.iter().map(|row| row.title.as_str()).collect()
    }

    #[tokio::test]
    async fn a_page_stays_full_past_hidden_rows() {
        let pool = test_pool().await;
        let (author, shown) = seed_user_and_service(&pool).await;
        let hidden = named_service(&pool, "Vault").await;
        for i in 0..35 {
            let service = if i % 7 >= 5 { hidden } else { shown };
            create_event(&pool, &format!("Event {i}"), author, vec![service]).await;
        }
        let scope = Scope::visible(vec![shown]);

        let first = EventRepository::list_page(&pool, &page(20), &scope)
            .await
            .unwrap();
        assert_eq!(first.len(), 20);
        assert!(first.iter().all(|row| row.services() == vec!["API"]));
        let filters = EventFilters {
            offset: 20,
            ..page(20)
        };
        let second = EventRepository::list_page(&pool, &filters, &scope)
            .await
            .unwrap();
        assert_eq!(second.len(), 5);
        assert_eq!(
            EventRepository::count_filtered(&pool, &page(20), &scope)
                .await
                .unwrap(),
            25
        );
    }

    #[tokio::test]
    async fn search_skips_a_hidden_event() {
        let pool = test_pool().await;
        let (author, shown) = seed_user_and_service(&pool).await;
        let hidden = named_service(&pool, "Vault").await;
        create_event(&pool, "Payroll outage", author, vec![shown]).await;
        create_event(&pool, "Payroll audit", author, vec![hidden]).await;
        let filters = EventFilters {
            q: Some("payroll".to_string()),
            ..page(10)
        };
        let scope = Scope::visible(vec![shown]);

        let rows = EventRepository::list_page(&pool, &filters, &scope)
            .await
            .unwrap();
        assert_eq!(titles(&rows), vec!["Payroll outage"]);
        assert_eq!(
            EventRepository::count_filtered(&pool, &filters, &scope)
                .await
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn filtering_on_a_hidden_service_finds_nothing() {
        let pool = test_pool().await;
        let (author, shown) = seed_user_and_service(&pool).await;
        let hidden = named_service(&pool, "Vault").await;
        create_event(&pool, "Both", author, vec![shown, hidden]).await;
        let scope = Scope::visible(vec![shown]);
        let on_hidden = EventFilters {
            service_id: Some(hidden),
            ..page(10)
        };
        let on_shown = EventFilters {
            service_id: Some(shown),
            ..page(10)
        };

        assert!(
            EventRepository::list_page(&pool, &on_hidden, &scope)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            EventRepository::count_filtered(&pool, &on_hidden, &scope)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            EventRepository::count_filtered(&pool, &on_shown, &scope)
                .await
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn a_mixed_event_keeps_only_the_visible_services() {
        let pool = test_pool().await;
        let (author, _) = seed_user_and_service(&pool).await;
        let mail = named_service(&pool, "Mail").await;
        let wiki = named_service(&pool, "Wiki").await;
        let zebra = named_service(&pool, "Zebra").await;
        let secret = named_service(&pool, "Vault").await;
        create_event(&pool, "Mixed", author, vec![zebra, secret, wiki, mail]).await;
        create_event(&pool, "Nobody", author, vec![]).await;
        create_event(&pool, "Closed", author, vec![secret]).await;
        let scope = Scope::visible(vec![wiki, zebra]);

        let rows = EventRepository::list_page(&pool, &page(10), &scope)
            .await
            .unwrap();
        assert_eq!(titles(&rows), vec!["Nobody", "Mixed"]);
        let mixed = &rows[1];
        assert_eq!(mixed.services(), vec!["Wiki", "Zebra"]);
        assert_eq!(mixed.service_ids, format!("{wiki},{zebra}"));
        assert_eq!(
            mixed.service_icons,
            format!("wiki{ICON_SEPARATOR}{NAME_SEPARATOR}zebra{ICON_SEPARATOR}")
        );
        assert!(!rows[0].has_services());
    }

    #[tokio::test]
    async fn limited_lists_return_visible_rows_when_the_newest_are_hidden() {
        let pool = test_pool().await;
        let (author, shown) = seed_user_and_service(&pool).await;
        let hidden = named_service(&pool, "Vault").await;
        for i in 0..3 {
            create_event(&pool, &format!("Shown {i}"), author, vec![shown]).await;
        }
        for i in 0..5 {
            create_event(&pool, &format!("Hidden {i}"), author, vec![hidden]).await;
        }
        let scope = Scope::visible(vec![shown]);

        let activity = EventRepository::list_recent_activity(
            &pool,
            3,
            &[ActivityKind::MajorIncidents],
            &scope,
        )
        .await
        .unwrap();
        let feed = EventRepository::list_for_feed(&pool, 3, &scope)
            .await
            .unwrap();
        for rows in [activity, feed] {
            assert_eq!(titles(&rows), vec!["Shown 2", "Shown 1", "Shown 0"]);
        }
    }

    #[tokio::test]
    async fn finished_maintenance_returns_visible_rows_when_the_newest_are_hidden() {
        let pool = test_pool().await;
        let (author, shown) = seed_user_and_service(&pool).await;
        let hidden = named_service(&pool, "Vault").await;
        let start = Utc::now() + chrono::Duration::hours(3);
        for (i, service) in (0_i64..).zip([shown, shown, hidden, hidden]) {
            let event = EventRepository::create(&pool, &maintenance(author, vec![service], start))
                .await
                .unwrap();
            let updated = Utc::now() + chrono::Duration::minutes(i);
            sqlx::query("UPDATE events SET lifecycle = 'completed', updated_at = ? WHERE id = ?")
                .bind(clock::db(updated))
                .bind(event.id)
                .execute(&pool)
                .await
                .unwrap();
        }

        let rows =
            EventRepository::list_finished_maintenance(&pool, 2, &Scope::visible(vec![shown]))
                .await
                .unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| row.services() == vec!["API"]));
    }

    #[tokio::test]
    async fn open_lists_and_the_unread_count_leave_out_hidden_events() {
        let pool = test_pool().await;
        let (author, shown) = seed_user_and_service(&pool).await;
        let hidden = named_service(&pool, "Vault").await;
        let before = Utc::now() - chrono::Duration::minutes(1);
        let start = Utc::now() + chrono::Duration::hours(3);
        create_event(&pool, "Shown incident", author, vec![shown]).await;
        create_event(&pool, "Hidden incident", author, vec![hidden]).await;
        for service in [shown, hidden] {
            EventRepository::create(&pool, &maintenance(author, vec![service], start))
                .await
                .unwrap();
        }
        let scope = Scope::visible(vec![shown]);

        let banner = EventRepository::list_open_for_banner(&pool, &scope)
            .await
            .unwrap();
        assert_eq!(titles(&banner), vec!["Shown incident"]);
        let upcoming = EventRepository::list_open_maintenance(&pool, &scope)
            .await
            .unwrap();
        assert_eq!(upcoming.len(), 1);
        assert_eq!(upcoming[0].service_ids, shown.to_string());
        assert_eq!(
            EventRepository::count_since(&pool, before, &scope)
                .await
                .unwrap(),
            2
        );
        assert_eq!(
            EventRepository::count_since(&pool, before, &Scope::All)
                .await
                .unwrap(),
            4
        );
    }

    #[tokio::test]
    async fn find_with_services_hides_events_and_strips_services() {
        let pool = test_pool().await;
        let (author, shown) = seed_user_and_service(&pool).await;
        let hidden = named_service(&pool, "Vault").await;
        let closed = create_event(&pool, "Closed", author, vec![hidden]).await;
        let mixed = create_event(&pool, "Mixed", author, vec![shown, hidden]).await;
        let scope = Scope::visible(vec![shown]);

        assert!(
            EventRepository::find_with_services(&pool, closed, &scope)
                .await
                .unwrap()
                .is_none()
        );
        let found = EventRepository::find_with_services(&pool, mixed, &scope)
            .await
            .unwrap()
            .unwrap();
        let ids: Vec<i64> = found.services.iter().map(|service| service.id).collect();
        assert_eq!(ids, vec![shown]);
        let all = EventRepository::find_with_services(&pool, mixed, &Scope::All)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(all.services.len(), 2);
    }

    #[test]
    fn fts_query_keeps_words_as_prefixes() {
        assert_eq!(fts_query("hello* +world"), "\"hello\"* \"world\"*");
        assert_eq!(fts_query("+ - * \"\""), "");
        assert_eq!(fts_query(""), "");
        assert_eq!(fts_query("l'hébergeur"), "\"l\"* \"hébergeur\"*");
    }

    fn announced(author_id: i64, minutes_ago: i64) -> CreateEventInput {
        CreateEventInput {
            next_update: Some(NextUpdate {
                at: Utc::now() - chrono::Duration::minutes(minutes_ago),
                every_minutes: None,
            }),
            ..incident("Mail is slow", author_id, Vec::new())
        }
    }

    fn five_minutes_ago() -> DateTime<Utc> {
        Utc::now() - chrono::Duration::minutes(5)
    }

    #[tokio::test]
    async fn an_overdue_update_is_reminded_once() {
        let pool = test_pool().await;
        let (uid, _) = seed_user_and_service(&pool).await;
        let event = EventRepository::create(&pool, &announced(uid, 10))
            .await
            .unwrap();

        let due = EventRepository::due_reminders(&pool, five_minutes_ago())
            .await
            .unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!((due[0].event_id, due[0].user_id), (event.id, uid));
        assert_eq!(due[0].email, "t@t.com");

        let at = due[0].next_update_at;
        assert!(
            EventRepository::mark_reminded(&pool, event.id, at)
                .await
                .unwrap()
        );
        assert!(
            !EventRepository::mark_reminded(&pool, event.id, at)
                .await
                .unwrap()
        );
        let due = EventRepository::due_reminders(&pool, five_minutes_ago())
            .await
            .unwrap();
        assert!(due.is_empty());
    }

    #[tokio::test]
    async fn an_update_barely_late_waits_for_its_margin() {
        let pool = test_pool().await;
        let (uid, _) = seed_user_and_service(&pool).await;
        EventRepository::create(&pool, &announced(uid, 2))
            .await
            .unwrap();

        let due = EventRepository::due_reminders(&pool, five_minutes_ago())
            .await
            .unwrap();
        assert!(due.is_empty());
    }

    #[tokio::test]
    async fn a_new_announcement_arms_the_reminder_again() {
        let pool = test_pool().await;
        let (uid, _) = seed_user_and_service(&pool).await;
        let event = EventRepository::create(&pool, &announced(uid, 10))
            .await
            .unwrap();
        let at = event.next_update_at.unwrap();
        EventRepository::mark_reminded(&pool, event.id, at)
            .await
            .unwrap();

        let later = NextUpdate {
            at: Utc::now() - chrono::Duration::minutes(8),
            every_minutes: None,
        };
        EventRepository::set_next_update(&pool, event.id, Some(later), uid)
            .await
            .unwrap();

        let due = EventRepository::due_reminders(&pool, five_minutes_ago())
            .await
            .unwrap();
        assert_eq!(due.len(), 1);
    }

    #[tokio::test]
    async fn a_resolved_incident_owes_no_reminder() {
        let pool = test_pool().await;
        let (uid, _) = seed_user_and_service(&pool).await;
        let event = EventRepository::create(&pool, &announced(uid, 10))
            .await
            .unwrap();
        sqlx::query("UPDATE events SET lifecycle = 'resolved' WHERE id = ?")
            .bind(event.id)
            .execute(&pool)
            .await
            .unwrap();

        let due = EventRepository::due_reminders(&pool, five_minutes_ago())
            .await
            .unwrap();
        assert!(due.is_empty());
    }
}
