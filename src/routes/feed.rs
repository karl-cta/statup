//! Atom feed of the instance's events, so feed readers, chat tools and on-call
//! tooling can follow incidents without anyone remembering to open the page.

use std::collections::HashMap;

use askama::Template;
use axum::extract::State;
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Redirect, Response};
use chrono::Utc;

use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::OptionalUser;
use crate::models::{EventSummary, EventUpdateWithAuthor};
use crate::repositories::EventRepository;
use crate::services::sanitize_markdown;
use crate::state::AppState;

const FEED_SIZE: i64 = 50;

#[derive(Template)]
#[template(path = "feed.xml")]
struct FeedTemplate {
    base_url: String,
    brand: String,
    updated: String,
    entries: Vec<FeedEntry>,
    i18n: I18n,
}

struct FeedEntry {
    id: i64,
    title: String,
    published: String,
    updated: String,
    /// Rendered HTML, escaped again by the XML template as Atom requires.
    content: String,
}

#[derive(Template)]
#[template(path = "feed_entry.html")]
struct FeedEntryTemplate<'a> {
    event: &'a EventSummary,
    description_html: String,
    updates: Vec<EventUpdateWithAuthor>,
    i18n: &'a I18n,
}

pub async fn atom(
    OptionalUser(user): OptionalUser,
    State(state): State<AppState>,
    headers: HeaderMap,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    if user.is_none() && !state.is_public_mode() {
        return Ok(Redirect::to("/login").into_response());
    }

    let events = EventRepository::list_for_feed(&state.pool, FEED_SIZE).await?;
    let ids: Vec<i64> = events.iter().map(|e| e.id).collect();
    let mut updates_by_event =
        group_by_event(EventRepository::list_updates_for_events(&state.pool, &ids).await?);

    let last_activity = events
        .iter()
        .map(|e| e.last_activity_at.unwrap_or(e.updated_at))
        .max()
        .unwrap_or_else(Utc::now);

    let entries = events
        .iter()
        .map(|event| {
            let updates = updates_by_event.remove(&event.id).unwrap_or_default();
            render_entry(event, updates, &i18n)
        })
        .collect::<Result<Vec<_>, _>>()?;

    let feed = FeedTemplate {
        base_url: base_url(&state, &headers),
        brand: crate::brand_name(),
        updated: last_activity.to_rfc3339(),
        entries,
        i18n,
    };
    let body = feed.render().map_err(|e| render_error(&e))?;
    Ok((
        [(header::CONTENT_TYPE, "application/atom+xml; charset=utf-8")],
        body,
    )
        .into_response())
}

fn render_entry(
    event: &EventSummary,
    updates: Vec<EventUpdateWithAuthor>,
    i18n: &I18n,
) -> Result<FeedEntry, AppError> {
    let content = FeedEntryTemplate {
        event,
        description_html: sanitize_markdown(&event.description),
        updates,
        i18n,
    }
    .render()
    .map_err(|e| render_error(&e))?;

    Ok(FeedEntry {
        id: event.id,
        title: event.title.clone(),
        published: event.created_at.to_rfc3339(),
        updated: event
            .last_activity_at
            .unwrap_or(event.updated_at)
            .to_rfc3339(),
        content,
    })
}

fn group_by_event(updates: Vec<EventUpdateWithAuthor>) -> HashMap<i64, Vec<EventUpdateWithAuthor>> {
    let mut grouped: HashMap<i64, Vec<EventUpdateWithAuthor>> = HashMap::new();
    for update in updates {
        grouped.entry(update.event_id).or_default().push(update);
    }
    grouped
}

/// Absolute origin for entry links. `PUBLIC_URL` wins when set; otherwise the
/// request host, with the scheme taken from the proxy only when its headers
/// are trusted, for the same reason as the rate limiter.
fn base_url(state: &AppState, headers: &HeaderMap) -> String {
    if let Some(url) = &state.public_url {
        return url.clone();
    }
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");
    let forwarded_https = state.trust_proxy_headers
        && headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|proto| proto.eq_ignore_ascii_case("https"));
    let scheme = if forwarded_https { "https" } else { "http" };
    format!("{scheme}://{host}")
}

fn render_error(e: &askama::Error) -> AppError {
    AppError::Internal(anyhow::anyhow!("template render error: {e}"))
}
