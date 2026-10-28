//! "Add to my calendar" on a maintenance still to come: the iCalendar file
//! Outlook and Apple Calendar open, and the way to Google Calendar.

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::response::{IntoResponse, Redirect, Response};

use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::Viewer;
use crate::middleware::headers::no_store;
use crate::models::{CalendarEntry, Service, plain_text};
use crate::routes::dashboard::origin;
use crate::routes::members_only;
use crate::services::{EventService, sanitize_markdown};
use crate::state::AppState;

/// Long enough to say what will happen, short enough for a link.
const DESCRIPTION_CHARS: usize = 600;

pub async fn calendar_file(
    viewer: Viewer,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    if let Some(redirect) = members_only(&state, viewer.user.as_ref(), &headers) {
        return Ok(redirect);
    }
    let entry = entry(&state, &viewer, id, &headers, &i18n).await?;
    let disposition = format!("inline; filename=\"maintenance-{id}.ics\"");
    let response = (
        [
            (CONTENT_TYPE, "text/calendar; charset=utf-8".to_string()),
            (CONTENT_DISPOSITION, disposition),
        ],
        entry.ics(),
    );
    Ok(no_store(response.into_response()))
}

pub async fn calendar_google(
    viewer: Viewer,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    if let Some(redirect) = members_only(&state, viewer.user.as_ref(), &headers) {
        return Ok(redirect);
    }
    let entry = entry(&state, &viewer, id, &headers, &i18n).await?;
    Ok(no_store(Redirect::to(&entry.google_link()).into_response()))
}

/// The maintenance as the viewer may see it; not found when it is hidden
/// from them or no longer to come, as its page would say.
async fn entry(
    state: &AppState,
    viewer: &Viewer,
    id: i64,
    headers: &HeaderMap,
    i18n: &I18n,
) -> Result<CalendarEntry, AppError> {
    let found = EventService::find_with_services(&state.pool, id, &viewer.scope).await?;
    let event = found.event;
    let start = event.upcoming_start().ok_or(AppError::NotFound)?;
    let url = format!("{}/events/{id}", origin(state, headers));
    Ok(CalendarEntry {
        uid: url.clone(),
        title: i18n.tf("events.calendar_title", &[("title", &event.title)]),
        start,
        end: event.planned_end,
        stamp: event.updated_at,
        description: description(&event.description, &found.services, &url, i18n),
    })
}

/// Its text, the services it touches and the address of its page, one
/// paragraph each.
fn description(text: &str, services: &[Service], url: &str, i18n: &I18n) -> String {
    let mut parts = Vec::new();
    parts.extend(plain_text(&sanitize_markdown(text), DESCRIPTION_CHARS));
    if !services.is_empty() {
        let names: Vec<&str> = services.iter().map(|s| s.name.as_str()).collect();
        let names = names.join(", ");
        parts.push(i18n.tf("events.calendar_services", &[("names", &names)]));
    }
    parts.push(url.to_string());
    parts.join("\n\n")
}
