//! Event rules: creation, edits, updates, state changes and the maintenance
//! schedule.

use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::task::AbortHandle;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::{
    CreateEventInput, Event, EventWithServices, Kind, Lifecycle, Role, Severity, UpdateEventInput,
    User,
};
use crate::repositories::EventRepository;
use crate::services::ServiceService;

/// How often announced maintenances are started and completed.
const SCHEDULE_INTERVAL: Duration = Duration::from_secs(60);

const MAX_TITLE_CHARS: usize = 200;

pub struct EventService;

impl EventService {
    pub async fn create(pool: &DbPool, input: CreateEventInput) -> Result<Event, AppError> {
        if let Some(key) = event_field_error(&input.title)
            .or_else(|| severity_error(input.kind, input.severity))
            .or_else(|| began_error(input.started_at))
            .or_else(|| {
                let start = if input.planned {
                    input.planned_start
                } else {
                    Some(Utc::now())
                };
                schedule_error(input.kind, input.planned, start, input.planned_end)
            })
        {
            return Err(AppError::validation(key));
        }
        let mut input = input;
        input.follows_event_id =
            followed_maintenance(pool, input.kind, input.follows_event_id).await?;
        let event = EventRepository::create(pool, &input).await?;
        if affects_services(event.kind) {
            ServiceService::recalculate_many(pool, &input.service_ids).await?;
        }
        Ok(event)
    }

    /// Saves an edit. The kind and the "announced ahead" flag stay what they
    /// were: the event's states only make sense for them.
    pub async fn update(
        pool: &DbPool,
        id: i64,
        mut input: UpdateEventInput,
        role: Role,
    ) -> Result<(), AppError> {
        let event = find(pool, id).await?;
        check_modification_allowed(&event, role)?;
        input.planned = event.planned;
        if let Some(key) = event_field_error(&input.title)
            .or_else(|| severity_error(event.kind, input.severity))
            .or_else(|| {
                let start = if event.planned {
                    input.planned_start
                } else {
                    event.started_at
                };
                schedule_error(event.kind, event.planned, start, input.planned_end)
            })
        {
            return Err(AppError::validation(key));
        }
        input.follows_event_id =
            followed_maintenance(pool, event.kind, input.follows_event_id).await?;
        let touched = EventRepository::update(pool, id, &input).await?;
        if affects_services(event.kind) {
            ServiceService::recalculate_many(pool, &touched).await?;
        }
        Ok(())
    }

    /// Posts a message, moves the event to `next`, or both at once.
    pub async fn post_update(
        pool: &DbPool,
        id: i64,
        message: &str,
        next: Option<Lifecycle>,
        author: &User,
    ) -> Result<(), AppError> {
        let event = find(pool, id).await?;
        check_modification_allowed(&event, author.role)?;
        let message = message.trim();
        if let Some(key) = update_error(&event, message, next) {
            return Err(AppError::validation(key));
        }
        if !message.is_empty() {
            EventRepository::add_update(pool, id, &sanitize_markdown(message), author.id).await?;
        }
        if let Some(next) = next {
            let current = event
                .lifecycle
                .ok_or_else(|| AppError::validation("validation.event_has_no_lifecycle"))?;
            // A second submit, or a colleague's, finds the event elsewhere.
            if !EventRepository::transition(pool, id, current, next, Utc::now()).await? {
                return Err(AppError::validation("validation.invalid_transition"));
            }
            recalculate_event_services(pool, id).await?;
        }
        Ok(())
    }

    /// Undoes the last state change.
    pub async fn revert(pool: &DbPool, id: i64, role: Role) -> Result<(), AppError> {
        let event = find(pool, id).await?;
        check_modification_allowed(&event, role)?;
        if event.previous_lifecycle.is_none() {
            return Err(AppError::validation("validation.no_previous_lifecycle"));
        }
        EventRepository::revert_transition(pool, id).await?;
        recalculate_event_services(pool, id).await
    }

    pub async fn delete(pool: &DbPool, id: i64, role: Role) -> Result<(), AppError> {
        let event = find(pool, id).await?;
        check_modification_allowed(&event, role)?;
        let service_ids = EventRepository::delete(pool, id).await?;
        if affects_services(event.kind) {
            ServiceService::recalculate_many(pool, &service_ids).await?;
        }
        Ok(())
    }

    /// Removes a posted update: an admin may remove any, an author their own
    /// while the event is open.
    pub async fn delete_update(
        pool: &DbPool,
        event_id: i64,
        update_id: i64,
        user: &User,
    ) -> Result<(), AppError> {
        let event = find(pool, event_id).await?;
        let update = EventRepository::find_update(pool, event_id, update_id)
            .await?
            .ok_or(AppError::NotFound)?;
        if !can_delete_update(&event, update.author_id, user) {
            return Err(AppError::Forbidden);
        }
        EventRepository::delete_update(pool, update_id).await?;
        Ok(())
    }

    /// Incidents and maintenances created since the reader's last visit.
    pub async fn unread_count(
        pool: &DbPool,
        last_seen_at: Option<DateTime<Utc>>,
    ) -> Result<i64, AppError> {
        let since = last_seen_at.unwrap_or(DateTime::UNIX_EPOCH);
        Ok(EventRepository::count_since(pool, since).await?)
    }

    pub async fn find_with_services(
        pool: &DbPool,
        event_id: i64,
    ) -> Result<EventWithServices, AppError> {
        EventRepository::find_with_services(pool, event_id)
            .await?
            .ok_or(AppError::NotFound)
    }

    /// Starts and completes the announced maintenances whose time has come,
    /// then refreshes the services they touch. Returns how many changed.
    pub async fn apply_schedule(pool: &DbPool) -> Result<usize, AppError> {
        let now = Utc::now();
        let mut changed = EventRepository::start_due_maintenance(pool, now).await?;
        changed.extend(EventRepository::complete_due_maintenance(pool, now).await?);
        changed.sort_unstable();
        changed.dedup();
        for id in &changed {
            recalculate_event_services(pool, *id).await?;
        }
        Ok(changed.len())
    }
}

/// Runs [`EventService::apply_schedule`] every minute for the life of the
/// server.
pub fn spawn_maintenance_schedule(pool: DbPool) -> AbortHandle {
    let task = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(SCHEDULE_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            match EventService::apply_schedule(&pool).await {
                Ok(0) => {}
                Ok(count) => tracing::info!(count, "Maintenance schedule applied"),
                Err(e) => tracing::warn!(error = %e, "Maintenance schedule failed"),
            }
        }
    });
    task.abort_handle()
}

/// Title rules as a message key, so a route can re-render the form with
/// what the author typed. The description is optional.
pub fn event_field_error(title: &str) -> Option<&'static str> {
    let title = title.trim();
    if title.is_empty() {
        return Some("validation.title_empty");
    }
    if title.chars().count() > MAX_TITLE_CHARS {
        return Some("validation.title_too_long");
    }
    None
}

/// An incident says how bad it is.
/// An incident declared late began in the past, not later on.
fn began_error(started_at: Option<DateTime<Utc>>) -> Option<&'static str> {
    started_at
        .filter(|at| *at > Utc::now())
        .map(|_| "validation.began_in_future")
}

fn severity_error(kind: Kind, severity: Option<Severity>) -> Option<&'static str> {
    (kind == Kind::Incident && severity.is_none()).then_some("validation.severity_required")
}

/// An announced maintenance needs a start; any maintenance ends after it
/// starts. `start` is when a maintenance begun right away started.
pub fn schedule_error(
    kind: Kind,
    planned: bool,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
) -> Option<&'static str> {
    if kind != Kind::Maintenance {
        return None;
    }
    match (start, end) {
        (None, _) if planned => Some("validation.planned_start_required"),
        (Some(start), Some(end)) if end <= start => Some("validation.planned_end_before_start"),
        _ => None,
    }
}

fn update_error(event: &Event, message: &str, next: Option<Lifecycle>) -> Option<&'static str> {
    let Some(next) = next else {
        return message.is_empty().then_some("validation.update_required");
    };
    let Some(current) = event.lifecycle else {
        return Some("validation.event_has_no_lifecycle");
    };
    (!event.kind.can_transition(current, next)).then_some("validation.invalid_transition")
}

/// The maintenance an announcement follows, checked to be one; nothing
/// for the other kinds.
async fn followed_maintenance(
    pool: &DbPool,
    kind: Kind,
    id: Option<i64>,
) -> Result<Option<i64>, AppError> {
    let (Kind::Publication, Some(id)) = (kind, id) else {
        return Ok(None);
    };
    match EventRepository::find_by_id(pool, id).await? {
        Some(followed) if followed.kind == Kind::Maintenance => Ok(Some(id)),
        _ => Err(AppError::validation("error.invalid_data")),
    }
}

/// Announcements never change a service status.
fn affects_services(kind: Kind) -> bool {
    kind != Kind::Publication
}

async fn find(pool: &DbPool, id: i64) -> Result<Event, AppError> {
    EventRepository::find_by_id(pool, id)
        .await?
        .ok_or(AppError::NotFound)
}

async fn recalculate_event_services(pool: &DbPool, event_id: i64) -> Result<(), AppError> {
    let service_ids = EventRepository::service_ids(pool, event_id).await?;
    ServiceService::recalculate_many(pool, &service_ids).await
}

/// Closed events stay as they are, except for an admin. Announcements have
/// no state and remain editable.
pub fn is_modifiable(event: &Event) -> bool {
    event.lifecycle.is_none_or(Lifecycle::is_active)
}

pub fn can_modify(event: &Event, role: Role) -> bool {
    role.can_publish() && (is_modifiable(event) || role.can_admin())
}

fn check_modification_allowed(event: &Event, role: Role) -> Result<(), AppError> {
    if can_modify(event, role) {
        Ok(())
    } else {
        Err(AppError::validation("validation.event_closed_admin_only"))
    }
}

pub fn can_delete_update(event: &Event, author_id: i64, user: &User) -> bool {
    user.role.can_admin()
        || (author_id == user.id && user.role.can_publish() && is_modifiable(event))
}

/// Renders Markdown and keeps a safe subset: paragraphs, lists, links, code,
/// emphasis and tables. No class attributes and no images, so an author
/// cannot restyle the page or load a third-party image.
pub fn sanitize_markdown(raw: &str) -> String {
    use pulldown_cmark::{Options, Parser, html};

    let parser = Parser::new_ext(raw, Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES);
    let mut rendered = String::new();
    html::push_html(&mut rendered, parser);
    ammonia::Builder::default()
        .rm_tags(&["img"])
        .clean(&rendered)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Severity;
    use chrono::Duration as TimeDelta;

    fn event(lifecycle: Option<Lifecycle>, kind: Kind) -> Event {
        Event {
            id: 1,
            kind,
            severity: Some(Severity::Critical),
            planned: false,
            lifecycle,
            category: None,
            title: "test".to_string(),
            description: "test".to_string(),
            planned_start: None,
            planned_end: None,
            started_at: None,
            ended_at: None,
            restored_at: None,
            author_id: 1,
            previous_lifecycle: None,
            keeps_services_up: false,
            follows_event_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn user(id: i64, role: Role) -> User {
        User {
            id,
            email: format!("u{id}@example.test"),
            password_hash: String::new(),
            display_name: "User".to_string(),
            role,
            is_active: true,
            last_seen_at: None,
            preferred_locale: None,
            must_change_password: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn title_rules() {
        assert!(event_field_error("").is_some());
        assert!(event_field_error("   ").is_some());
        assert!(event_field_error(&"a".repeat(201)).is_some());
        assert!(event_field_error(&"é".repeat(200)).is_none());
        assert!(event_field_error("Payroll down").is_none());
    }

    #[test]
    fn incidents_say_how_bad_they_are() {
        assert_eq!(
            severity_error(Kind::Incident, None),
            Some("validation.severity_required")
        );
        assert!(severity_error(Kind::Incident, Some(Severity::Minor)).is_none());
        assert!(severity_error(Kind::Maintenance, None).is_none());
        assert!(severity_error(Kind::Publication, None).is_none());
    }

    #[test]
    fn announced_maintenance_needs_a_consistent_window() {
        let start = Utc::now();
        assert_eq!(
            schedule_error(Kind::Maintenance, true, None, None),
            Some("validation.planned_start_required")
        );
        assert_eq!(
            schedule_error(Kind::Maintenance, true, Some(start), Some(start)),
            Some("validation.planned_end_before_start")
        );
        assert!(schedule_error(Kind::Maintenance, true, Some(start), None).is_none());
        assert!(
            schedule_error(
                Kind::Maintenance,
                true,
                Some(start),
                Some(start + TimeDelta::hours(1))
            )
            .is_none()
        );
        assert!(schedule_error(Kind::Maintenance, false, None, None).is_none());
        assert_eq!(
            schedule_error(
                Kind::Maintenance,
                false,
                Some(start),
                Some(start - TimeDelta::minutes(5))
            ),
            Some("validation.planned_end_before_start")
        );
        assert!(schedule_error(Kind::Incident, true, None, None).is_none());
    }

    #[test]
    fn a_state_change_needs_no_message() {
        let open = event(Some(Lifecycle::Investigating), Kind::Incident);
        assert!(update_error(&open, "", Some(Lifecycle::Resolved)).is_none());
        assert!(update_error(&open, "Fixed", Some(Lifecycle::Resolved)).is_none());
        assert!(update_error(&open, "", Some(Lifecycle::Monitoring)).is_none());
        assert_eq!(
            update_error(&open, "", None),
            Some("validation.update_required")
        );
    }

    #[test]
    fn transitions_are_checked_before_anything_is_written() {
        let open = event(Some(Lifecycle::Investigating), Kind::Incident);
        assert_eq!(
            update_error(&open, "x", Some(Lifecycle::Completed)),
            Some("validation.invalid_transition")
        );
        let note = event(None, Kind::Publication);
        assert_eq!(
            update_error(&note, "x", Some(Lifecycle::Resolved)),
            Some("validation.event_has_no_lifecycle")
        );
    }

    #[test]
    fn closed_events_belong_to_admins() {
        let resolved = event(Some(Lifecycle::Resolved), Kind::Incident);
        assert!(!can_modify(&resolved, Role::Publisher));
        assert!(can_modify(&resolved, Role::Admin));
        let open = event(Some(Lifecycle::Investigating), Kind::Incident);
        assert!(can_modify(&open, Role::Publisher));
        assert!(!can_modify(&open, Role::Reader));
        assert!(can_modify(&event(None, Kind::Publication), Role::Publisher));
    }

    #[test]
    fn authors_delete_their_own_updates_while_open() {
        let open = event(Some(Lifecycle::Investigating), Kind::Incident);
        let closed = event(Some(Lifecycle::Resolved), Kind::Incident);
        let author = user(7, Role::Publisher);
        assert!(can_delete_update(&open, 7, &author));
        assert!(!can_delete_update(&open, 8, &author));
        assert!(!can_delete_update(&closed, 7, &author));
        assert!(can_delete_update(&closed, 8, &user(1, Role::Admin)));
    }

    #[test]
    fn sanitize_renders_basic_markdown() {
        let result = sanitize_markdown("**bold** and *italic*");
        assert!(result.contains("<strong>bold</strong>"));
        assert!(result.contains("<em>italic</em>"));
    }

    #[test]
    fn sanitize_strips_scripts_images_and_classes() {
        let result = sanitize_markdown(
            "<script>alert('xss')</script><img src=x onerror=\"alert(1)\"><div class=\"fixed inset-0\">x</div>",
        );
        assert!(!result.contains("<script"));
        assert!(!result.contains("alert"));
        assert!(!result.contains("<img"));
        assert!(!result.contains("class="));
    }

    #[test]
    fn sanitize_keeps_links_and_code() {
        let link = sanitize_markdown("[link](https://example.com)");
        assert!(link.contains("<a") && link.contains("https://example.com"));
        assert!(sanitize_markdown("```\ncode\n```").contains("<code>"));
    }
}
