//! Event service: lifecycle transitions, updates, and markdown handling.

use chrono::{DateTime, Utc};

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::{CreateEventInput, Event, EventUpdate, EventWithServices, Lifecycle, Role};
use crate::repositories::EventRepository;
use crate::services::ServiceService;

pub struct EventService;

impl EventService {
    pub async fn create(pool: &DbPool, input: CreateEventInput) -> Result<Event, AppError> {
        validate_event_input(&input)?;

        let event = EventRepository::create(pool, input).await?;

        if let Some(ews) = EventRepository::find_by_id_with_services(pool, event.id).await? {
            for svc in &ews.services {
                ServiceService::recalculate_status(pool, svc.id).await?;
            }
        }

        Ok(event)
    }

    /// Transition the lifecycle. Publications have no lifecycle and the call
    /// is rejected. `ended_at` is set automatically on resolution or
    /// completion (not on cancellation).
    pub async fn update_lifecycle(
        pool: &DbPool,
        event_id: i64,
        new_lifecycle: Lifecycle,
        user_role: Role,
    ) -> Result<Event, AppError> {
        let event = EventRepository::find_by_id(pool, event_id)
            .await?
            .ok_or(AppError::NotFound)?;

        check_modification_allowed(&event, user_role)?;

        let current = event.lifecycle.ok_or(AppError::Validation(
            "validation.event_has_no_lifecycle".to_string(),
        ))?;

        if !event.kind.can_transition(current, new_lifecycle) {
            return Err(AppError::Validation(
                "validation.invalid_transition".to_string(),
            ));
        }

        EventRepository::update_lifecycle(pool, event_id, new_lifecycle).await?;

        if matches!(new_lifecycle, Lifecycle::Resolved | Lifecycle::Completed) {
            EventRepository::set_ended_at(pool, event_id, Utc::now()).await?;
        }

        let ews = EventRepository::find_by_id_with_services(pool, event_id).await?;
        if let Some(ews) = &ews {
            for svc in &ews.services {
                ServiceService::recalculate_status(pool, svc.id).await?;
            }
        }

        ews.map(|e| e.event).ok_or(AppError::NotFound)
    }

    pub async fn add_update(
        pool: &DbPool,
        event_id: i64,
        message: &str,
        author_id: i64,
        user_role: Role,
    ) -> Result<EventUpdate, AppError> {
        let event = EventRepository::find_by_id(pool, event_id)
            .await?
            .ok_or(AppError::NotFound)?;

        check_modification_allowed(&event, user_role)?;

        let message = message.trim();
        if message.is_empty() {
            return Err(AppError::Validation(
                "validation.update_required".to_string(),
            ));
        }

        let sanitized = sanitize_markdown(message);

        let update = EventRepository::add_update(pool, event_id, &sanitized, author_id).await?;
        Ok(update)
    }

    /// Revert to the previous lifecycle (one-level undo). Only allowed when
    /// `previous_lifecycle` is set.
    pub async fn revert_lifecycle(
        pool: &DbPool,
        event_id: i64,
        user_role: Role,
    ) -> Result<Event, AppError> {
        let event = EventRepository::find_by_id(pool, event_id)
            .await?
            .ok_or(AppError::NotFound)?;

        check_modification_allowed(&event, user_role)?;

        if event.previous_lifecycle.is_none() {
            return Err(AppError::Validation(
                "validation.no_previous_lifecycle".to_string(),
            ));
        }

        EventRepository::revert_lifecycle(pool, event_id).await?;

        let ews = EventRepository::find_by_id_with_services(pool, event_id).await?;
        if let Some(ews) = &ews {
            for svc in &ews.services {
                ServiceService::recalculate_status(pool, svc.id).await?;
            }
        }

        ews.map(|e| e.event).ok_or(AppError::NotFound)
    }

    /// Delete an event. Closed (terminal) events can only be deleted by an
    /// admin.
    pub async fn delete(pool: &DbPool, event_id: i64, user_role: Role) -> Result<(), AppError> {
        let event = EventRepository::find_by_id(pool, event_id)
            .await?
            .ok_or(AppError::NotFound)?;

        if !is_modifiable(&event) && !user_role.can_admin() {
            return Err(AppError::Validation(
                "validation.event_closed_admin_only".to_string(),
            ));
        }

        let service_ids = EventRepository::delete(pool, event_id).await?;

        for sid in service_ids {
            ServiceService::recalculate_status(pool, sid).await?;
        }

        Ok(())
    }

    /// Count of unseen events. `last_seen_at = None` means since the epoch.
    pub async fn unread_count(
        pool: &DbPool,
        last_seen_at: Option<DateTime<Utc>>,
    ) -> Result<i64, AppError> {
        let since = last_seen_at.unwrap_or(DateTime::UNIX_EPOCH);
        let count = EventRepository::count_since(pool, since).await?;
        Ok(count)
    }

    pub async fn find_with_services(
        pool: &DbPool,
        event_id: i64,
    ) -> Result<EventWithServices, AppError> {
        EventRepository::find_by_id_with_services(pool, event_id)
            .await?
            .ok_or(AppError::NotFound)
    }
}

fn validate_event_input(input: &CreateEventInput) -> Result<(), AppError> {
    if input.title.trim().is_empty() {
        return Err(AppError::Validation("validation.title_empty".to_string()));
    }
    if input.title.len() > 200 {
        return Err(AppError::Validation(
            "validation.title_too_long".to_string(),
        ));
    }
    if input.description.trim().is_empty() {
        return Err(AppError::Validation(
            "validation.description_required".to_string(),
        ));
    }
    Ok(())
}

/// An event is modifiable as long as it is not in a terminal state.
/// Publications (no lifecycle) remain modifiable by their author.
fn is_modifiable(event: &Event) -> bool {
    event.lifecycle.is_none_or(Lifecycle::is_active)
}

fn check_modification_allowed(event: &Event, user_role: Role) -> Result<(), AppError> {
    if !is_modifiable(event) && !user_role.can_admin() {
        return Err(AppError::Validation(
            "validation.event_closed_admin_only".to_string(),
        ));
    }
    Ok(())
}

/// Render markdown to HTML and pass the output through ammonia to allow only
/// a safe subset (paragraphs, lists, links, code, emphasis).
pub fn sanitize_markdown(raw: &str) -> String {
    use ammonia::Builder;
    use pulldown_cmark::{Options, Parser, html};

    let parser = Parser::new_ext(raw, Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);

    Builder::default()
        .add_generic_attributes(std::iter::once("class"))
        .clean(&html_output)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Kind, Severity};

    fn make_input(title: &str, description: &str, service_ids: Vec<i64>) -> CreateEventInput {
        CreateEventInput {
            kind: Kind::Incident,
            severity: Some(Severity::Major),
            planned: false,
            category: None,
            title: title.to_string(),
            description: description.to_string(),
            planned_start: None,
            planned_end: None,
            service_ids,
            icon_id: None,
            author_id: 1,
        }
    }

    #[test]
    fn validate_rejects_empty_title() {
        assert!(validate_event_input(&make_input("", "desc", vec![1])).is_err());
    }

    #[test]
    fn validate_rejects_whitespace_only_title() {
        assert!(validate_event_input(&make_input("   ", "desc", vec![1])).is_err());
    }

    #[test]
    fn validate_rejects_title_over_200_chars() {
        let long_title = "a".repeat(201);
        assert!(validate_event_input(&make_input(&long_title, "desc", vec![1])).is_err());
    }

    #[test]
    fn validate_accepts_title_at_200_chars() {
        let title = "a".repeat(200);
        assert!(validate_event_input(&make_input(&title, "desc", vec![1])).is_ok());
    }

    #[test]
    fn validate_rejects_empty_description() {
        assert!(validate_event_input(&make_input("title", "", vec![1])).is_err());
    }

    #[test]
    fn validate_accepts_empty_service_ids() {
        assert!(validate_event_input(&make_input("title", "desc", vec![])).is_ok());
    }

    fn make_event(lifecycle: Option<Lifecycle>, kind: Kind) -> Event {
        Event {
            id: 1,
            kind,
            severity: Some(Severity::Major),
            planned: false,
            lifecycle,
            category: None,
            title: "test".to_string(),
            description: "test".to_string(),
            planned_start: None,
            planned_end: None,
            started_at: None,
            ended_at: None,
            icon_id: None,
            author_id: 1,
            previous_lifecycle: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn active_incident_can_be_modified_by_publisher() {
        let event = make_event(Some(Lifecycle::Investigating), Kind::Incident);
        assert!(check_modification_allowed(&event, Role::Publisher).is_ok());
    }

    #[test]
    fn resolved_incident_cannot_be_modified_by_publisher() {
        let event = make_event(Some(Lifecycle::Resolved), Kind::Incident);
        assert!(check_modification_allowed(&event, Role::Publisher).is_err());
    }

    #[test]
    fn completed_maintenance_cannot_be_modified_by_publisher() {
        let event = make_event(Some(Lifecycle::Completed), Kind::Maintenance);
        assert!(check_modification_allowed(&event, Role::Publisher).is_err());
    }

    #[test]
    fn resolved_event_can_be_modified_by_admin() {
        let event = make_event(Some(Lifecycle::Resolved), Kind::Incident);
        assert!(check_modification_allowed(&event, Role::Admin).is_ok());
    }

    #[test]
    fn publication_is_always_modifiable() {
        let event = make_event(None, Kind::Publication);
        assert!(check_modification_allowed(&event, Role::Publisher).is_ok());
    }

    #[test]
    fn sanitize_renders_basic_markdown() {
        let result = sanitize_markdown("**bold** and *italic*");
        assert!(result.contains("<strong>bold</strong>"));
        assert!(result.contains("<em>italic</em>"));
    }

    #[test]
    fn sanitize_strips_script_tags() {
        let result = sanitize_markdown("<script>alert('xss')</script>");
        assert!(!result.contains("<script>"));
        assert!(!result.contains("alert"));
    }

    #[test]
    fn sanitize_strips_onerror_attributes() {
        let result = sanitize_markdown("<img onerror=\"alert('xss')\" src=\"x\">");
        assert!(!result.contains("onerror"));
    }

    #[test]
    fn sanitize_allows_links() {
        let result = sanitize_markdown("[link](https://example.com)");
        assert!(result.contains("<a"));
        assert!(result.contains("https://example.com"));
    }

    #[test]
    fn sanitize_allows_code_blocks() {
        let result = sanitize_markdown("```\ncode\n```");
        assert!(result.contains("<code>"));
    }
}
