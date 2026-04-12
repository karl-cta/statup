//! Event service - event creation, status transitions, updates, markdown sanitization.

use chrono::{DateTime, Utc};

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::{CreateEventInput, Event, EventStatus, EventUpdate, EventWithServices, Role};
use crate::repositories::EventRepository;
use crate::services::ServiceService;

/// Business logic for event lifecycle management.
pub struct EventService;

impl EventService {
    /// Create a new event, validate inputs, sanitize markdown, and
    /// recalculate affected service statuses.
    pub async fn create(pool: &DbPool, input: CreateEventInput) -> Result<Event, AppError> {
        validate_event_input(&input)?;

        let event = EventRepository::create(pool, input).await?;

        // Recalculate status for all affected services
        let ews = EventRepository::find_by_id_with_services(pool, event.id).await?;
        if let Some(ews) = ews {
            for svc in &ews.services {
                ServiceService::recalculate_status(pool, svc.id).await?;
            }
        }

        Ok(event)
    }

    /// Transition an event's status, enforcing allowed transitions.
    ///
    /// Sets `actual_end` when resolved. Recalculates affected service statuses.
    pub async fn update_status(
        pool: &DbPool,
        event_id: i64,
        new_status: EventStatus,
        user_role: Role,
    ) -> Result<Event, AppError> {
        let event = EventRepository::find_by_id(pool, event_id)
            .await?
            .ok_or(AppError::NotFound)?;

        check_modification_allowed(&event, user_role)?;

        if !event.status.can_transition_to(new_status) {
            return Err(AppError::Validation(
                "validation.invalid_transition".to_string(),
            ));
        }

        EventRepository::update_status(pool, event_id, new_status).await?;

        if new_status == EventStatus::Resolved {
            EventRepository::set_actual_end(pool, event_id, Utc::now()).await?;
        }

        // Recalculate service statuses
        let ews = EventRepository::find_by_id_with_services(pool, event_id).await?;
        if let Some(ews) = &ews {
            for svc in &ews.services {
                ServiceService::recalculate_status(pool, svc.id).await?;
            }
        }

        let updated = ews.map(|e| e.event).ok_or(AppError::NotFound)?;
        Ok(updated)
    }

    /// Add a status update message to an event.
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

    /// Revert an event to its previous status (one-level undo).
    ///
    /// Only allowed when `previous_status` is set. Recalculates affected
    /// service statuses after reverting.
    pub async fn revert_status(
        pool: &DbPool,
        event_id: i64,
        user_role: Role,
    ) -> Result<Event, AppError> {
        let event = EventRepository::find_by_id(pool, event_id)
            .await?
            .ok_or(AppError::NotFound)?;

        check_modification_allowed(&event, user_role)?;

        if event.previous_status.is_none() {
            return Err(AppError::Validation(
                "validation.no_previous_status".to_string(),
            ));
        }

        EventRepository::revert_status(pool, event_id).await?;

        let ews = EventRepository::find_by_id_with_services(pool, event_id).await?;
        if let Some(ews) = &ews {
            for svc in &ews.services {
                ServiceService::recalculate_status(pool, svc.id).await?;
            }
        }

        let updated = ews.map(|e| e.event).ok_or(AppError::NotFound)?;
        Ok(updated)
    }

    /// Delete an event. Only admins can delete resolved/cancelled events.
    ///
    /// Recalculates the status of all previously associated services.
    pub async fn delete(pool: &DbPool, event_id: i64, user_role: Role) -> Result<(), AppError> {
        let event = EventRepository::find_by_id(pool, event_id)
            .await?
            .ok_or(AppError::NotFound)?;

        if !event.status.is_active() && !user_role.can_admin() {
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

    /// Count events the user hasn't seen yet.
    ///
    /// If `last_seen_at` is `None` (user never visited the dashboard), all
    /// non-info events are considered unread (capped via `count_since` epoch).
    pub async fn unread_count(
        pool: &DbPool,
        last_seen_at: Option<DateTime<Utc>>,
    ) -> Result<i64, AppError> {
        let since = last_seen_at.unwrap_or(DateTime::UNIX_EPOCH);
        let count = EventRepository::count_since(pool, since).await?;
        Ok(count)
    }

    /// Find an event by ID with its services, or return `NotFound`.
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
    // service_ids is optional, events like changelogs or info
    // may not be tied to a specific service.

    Ok(())
}

/// Check that an event can still be modified.
///
/// Resolved/cancelled events can only be modified by admins.
fn check_modification_allowed(event: &Event, user_role: Role) -> Result<(), AppError> {
    if !event.status.is_active() && !user_role.can_admin() {
        return Err(AppError::Validation(
            "validation.event_closed_admin_only".to_string(),
        ));
    }
    Ok(())
}

/// Render markdown to HTML and sanitize the output via ammonia.
///
/// Allows a safe subset of HTML tags (paragraphs, lists, code blocks, links,
/// emphasis) while stripping anything dangerous (scripts, iframes, etc.).
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
    use crate::models::{EventType, Impact};

    fn make_input(title: &str, description: &str, service_ids: Vec<i64>) -> CreateEventInput {
        CreateEventInput {
            event_type: EventType::Incident,
            title: title.to_string(),
            description: description.to_string(),
            impact: Impact::Major,
            scheduled_start: None,
            scheduled_end: None,
            service_ids,
            icon_id: None,
            author_id: 1,
        }
    }

    #[test]
    fn validate_rejects_empty_title() {
        let input = make_input("", "desc", vec![1]);
        assert!(validate_event_input(&input).is_err());
    }

    #[test]
    fn validate_rejects_whitespace_only_title() {
        let input = make_input("   ", "desc", vec![1]);
        assert!(validate_event_input(&input).is_err());
    }

    #[test]
    fn validate_rejects_title_over_200_chars() {
        let long_title = "a".repeat(201);
        let input = make_input(&long_title, "desc", vec![1]);
        assert!(validate_event_input(&input).is_err());
    }

    #[test]
    fn validate_accepts_title_at_200_chars() {
        let title = "a".repeat(200);
        let input = make_input(&title, "desc", vec![1]);
        assert!(validate_event_input(&input).is_ok());
    }

    #[test]
    fn validate_rejects_empty_description() {
        let input = make_input("title", "", vec![1]);
        assert!(validate_event_input(&input).is_err());
    }

    #[test]
    fn validate_accepts_empty_service_ids() {
        let input = make_input("title", "desc", vec![]);
        assert!(validate_event_input(&input).is_ok());
    }

    #[test]
    fn validate_accepts_valid_input() {
        let input = make_input("Incident DB", "La base est down", vec![1, 2]);
        assert!(validate_event_input(&input).is_ok());
    }

    fn make_event(status: EventStatus) -> Event {
        Event {
            id: 1,
            event_type: EventType::Incident,
            status,
            title: "test".to_string(),
            description: "test".to_string(),
            impact: Impact::Major,
            scheduled_start: None,
            scheduled_end: None,
            actual_start: None,
            actual_end: None,
            icon_id: None,
            author_id: 1,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            previous_status: None,
        }
    }

    #[test]
    fn active_event_can_be_modified_by_publisher() {
        let event = make_event(EventStatus::Investigating);
        assert!(check_modification_allowed(&event, Role::Publisher).is_ok());
    }

    #[test]
    fn resolved_event_cannot_be_modified_by_publisher() {
        let event = make_event(EventStatus::Resolved);
        assert!(check_modification_allowed(&event, Role::Publisher).is_err());
    }

    #[test]
    fn cancelled_event_cannot_be_modified_by_publisher() {
        let event = make_event(EventStatus::Cancelled);
        assert!(check_modification_allowed(&event, Role::Publisher).is_err());
    }

    #[test]
    fn resolved_event_can_be_modified_by_admin() {
        let event = make_event(EventStatus::Resolved);
        assert!(check_modification_allowed(&event, Role::Admin).is_ok());
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
