//! Atom feed of the instance's events, so feed readers, chat tools and on-call
//! tooling can follow incidents without anyone remembering to open the page.

use std::collections::HashMap;

use askama::Template;
use axum::extract::State;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, IF_MODIFIED_SINCE, LAST_MODIFIED, VARY};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use chrono::{DateTime, NaiveDateTime, Utc};

use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::OptionalUser;
use crate::models::{EventSummary, EventUpdateWithAuthor};
use crate::repositories::EventRepository;
use crate::services::sanitize_markdown;
use crate::state::AppState;

const FEED_SIZE: i64 = 50;

/// Feed readers poll: a minute of caching spares the database without
/// delaying news by much.
const MAX_AGE_SECS: u32 = 60;

/// The date format of HTTP headers (IMF-fixdate).
const HTTP_DATE: &str = "%a, %d %b %Y %H:%M:%S GMT";

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
    let public = state.is_public_mode();
    if user.is_none() && !public {
        return Ok(Redirect::to("/login").into_response());
    }

    let events = EventRepository::list_for_feed(&state.pool, FEED_SIZE).await?;
    let last_activity = events.iter().map(last_activity_of).max();
    if let Some(modified) = last_activity
        && not_modified_since(&headers, modified)
    {
        return Ok(with_cache_headers(
            StatusCode::NOT_MODIFIED.into_response(),
            public,
            last_activity,
        ));
    }

    let feed = FeedTemplate {
        base_url: base_url(&state, &headers),
        brand: crate::brand_name(),
        updated: last_activity.unwrap_or_else(Utc::now).to_rfc3339(),
        entries: render_entries(&state, &events, &i18n).await?,
        i18n,
    };
    let body = feed.render().map_err(|e| render_error(&e))?;
    let response = (
        [(CONTENT_TYPE, "application/atom+xml; charset=utf-8")],
        body,
    )
        .into_response();
    Ok(with_cache_headers(response, public, last_activity))
}

async fn render_entries(
    state: &AppState,
    events: &[EventSummary],
    i18n: &I18n,
) -> Result<Vec<FeedEntry>, AppError> {
    let ids: Vec<i64> = events.iter().map(|e| e.id).collect();
    let mut updates_by_event =
        group_by_event(EventRepository::list_updates_for_events(&state.pool, &ids).await?);
    events
        .iter()
        .map(|event| {
            let updates = updates_by_event.remove(&event.id).unwrap_or_default();
            render_entry(event, updates, i18n)
        })
        .collect()
}

fn last_activity_of(event: &EventSummary) -> DateTime<Utc> {
    event.last_activity_at.unwrap_or(event.updated_at)
}

/// Whether the client's copy, dated by `If-Modified-Since`, is current.
/// HTTP dates have whole seconds.
fn not_modified_since(headers: &HeaderMap, modified: DateTime<Utc>) -> bool {
    headers
        .get(IF_MODIFIED_SINCE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| NaiveDateTime::parse_from_str(value.trim(), HTTP_DATE).ok())
        .is_some_and(|since| modified.timestamp() <= since.and_utc().timestamp())
}

/// A shared cache may keep the feed of a public instance; a members-only
/// feed stays in the reader's own cache. The body depends on the language.
fn with_cache_headers(
    mut response: Response,
    public: bool,
    last_activity: Option<DateTime<Utc>>,
) -> Response {
    let scope = if public { "public" } else { "private" };
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(&format!("{scope}, max-age={MAX_AGE_SECS}")) {
        headers.insert(CACHE_CONTROL, value);
    }
    headers.insert(VARY, HeaderValue::from_static("Accept-Language, Cookie"));
    if let Some(value) = last_activity
        .and_then(|date| HeaderValue::from_str(&date.format(HTTP_DATE).to_string()).ok())
    {
        headers.insert(LAST_MODIFIED, value);
    }
    response
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
        updated: last_activity_of(event).to_rfc3339(),
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

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn with_since(value: &'static str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(IF_MODIFIED_SINCE, HeaderValue::from_static(value));
        headers
    }

    #[test]
    fn a_copy_as_recent_as_the_last_activity_is_current() {
        let modified = Utc.with_ymd_and_hms(2026, 9, 16, 10, 0, 0).unwrap();
        assert!(not_modified_since(
            &with_since("Wed, 16 Sep 2026 10:00:00 GMT"),
            modified
        ));
        assert!(not_modified_since(
            &with_since("Wed, 16 Sep 2026 11:00:00 GMT"),
            modified
        ));
        assert!(!not_modified_since(
            &with_since("Wed, 16 Sep 2026 09:59:59 GMT"),
            modified
        ));
        assert!(!not_modified_since(&with_since("yesterday"), modified));
        assert!(!not_modified_since(&HeaderMap::new(), modified));
    }

    #[test]
    fn dates_are_written_in_the_http_format() {
        let modified = Utc.with_ymd_and_hms(2026, 9, 6, 8, 5, 3).unwrap();
        let response = with_cache_headers(StatusCode::OK.into_response(), true, Some(modified));
        let header = |name| response.headers().get(name).and_then(|v| v.to_str().ok());
        assert_eq!(header(LAST_MODIFIED), Some("Sun, 06 Sep 2026 08:05:03 GMT"));
        assert_eq!(header(CACHE_CONTROL), Some("public, max-age=60"));
    }
}
