//! The list of events and its search.

use std::fmt::Write as _;

use askama::Template;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::Response;
use serde::Deserialize;

use crate::clock;
use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::headers::is_htmx;
use crate::middleware::{CsrfToken, OptionalUser};
use crate::models::{DayGroup, EventFilters, Kind, LifecycleGroup, Service, group_by_day};
use crate::repositories::{EventRepository, ServiceRepository, UserRepository};
use crate::routes::page::{TitlePart, title_parts};
use crate::routes::{Frame, members_only, render};
use crate::state::AppState;

const PAGE_SIZE: i64 = 20;
const MAX_PAGE: i64 = 10_000;

#[derive(Deserialize)]
pub struct ListQuery {
    page: Option<i64>,
    #[serde(default, deserialize_with = "super::deserialize_blank_as_none")]
    kind: Option<Kind>,
    #[serde(default, deserialize_with = "super::deserialize_blank_as_none")]
    service_id: Option<i64>,
    #[serde(default, deserialize_with = "super::deserialize_blank_as_none")]
    from: Option<String>,
    #[serde(default, deserialize_with = "super::deserialize_blank_as_none")]
    to: Option<String>,
    #[serde(default, deserialize_with = "super::deserialize_blank_as_none")]
    lifecycle: Option<String>,
    #[serde(default, deserialize_with = "super::deserialize_blank_as_none")]
    q: Option<String>,
}

impl ListQuery {
    fn page(&self) -> i64 {
        self.page.unwrap_or(1).clamp(1, MAX_PAGE)
    }

    fn group(&self) -> Option<LifecycleGroup> {
        self.lifecycle.as_deref().and_then(LifecycleGroup::parse)
    }

    fn query(&self) -> Option<String> {
        self.q
            .as_deref()
            .map(str::trim)
            .filter(|q| !q.is_empty())
            .map(String::from)
    }

    fn filters(&self) -> EventFilters {
        EventFilters {
            kind: self.kind,
            lifecycle_group: self.group(),
            service_id: self.service_id,
            from: self
                .from
                .as_deref()
                .and_then(|d| clock::parse_day_bound(d, false)),
            to: self
                .to
                .as_deref()
                .and_then(|d| clock::parse_day_bound(d, true)),
            q: self.query(),
            limit: PAGE_SIZE + 1,
            offset: (self.page() - 1) * PAGE_SIZE,
        }
    }

    fn has_filters(&self) -> bool {
        self.kind.is_some()
            || self.service_id.is_some()
            || self.group().is_some()
            || self.query().is_some()
            || self.from.is_some()
            || self.to.is_some()
    }

    /// Query string of the current filters, ready for a `page=` parameter.
    fn page_link_base(&self) -> String {
        let mut url = String::from("/events?");
        let pairs = [
            ("kind", self.kind.map(|k| k.as_str().to_string())),
            ("service_id", self.service_id.map(|id| id.to_string())),
            ("lifecycle", self.group().map(|g| g.as_str().to_string())),
            ("q", self.query()),
            ("from", self.from.clone()),
            ("to", self.to.clone()),
        ];
        for (name, value) in pairs {
            if let Some(value) = value {
                let encoded = percent_encoding::utf8_percent_encode(
                    &value,
                    percent_encoding::NON_ALPHANUMERIC,
                );
                let _ = write!(url, "{name}={encoded}&");
            }
        }
        url
    }
}

/// The part of the list that the filters refresh.
pub struct EventResults {
    pub groups: Vec<DayGroup>,
    pub total: i64,
    pub count_label: String,
    pub page: i64,
    pub page_label: String,
    pub has_next: bool,
    pub page_link_base: String,
    pub has_filters: bool,
    pub query: Option<String>,
}

impl EventResults {
    fn title_parts(&self, title: &str) -> Vec<TitlePart> {
        title_parts(title, self.query.as_deref())
    }
}

#[derive(Template)]
#[template(path = "events/list.html")]
struct EventListTemplate {
    frame: Frame,
    results: EventResults,
    query: ListQuery,
    services: Vec<Service>,
    i18n: I18n,
}

impl EventListTemplate {
    fn kind_is(&self, kind: &str) -> bool {
        self.query.kind.is_some_and(|k| k.as_str() == kind)
    }

    fn group_is(&self, group: &str) -> bool {
        self.query.group().is_some_and(|g| g.as_str() == group)
    }

    // Askama hands a field over by reference.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn service_is(&self, id: &i64) -> bool {
        self.query.service_id == Some(*id)
    }
}

#[derive(Template)]
#[template(path = "events/_results.html")]
struct EventResultsTemplate {
    results: EventResults,
    i18n: I18n,
}

async fn load_results(
    state: &AppState,
    query: &ListQuery,
    i18n: &I18n,
) -> Result<EventResults, AppError> {
    let filters = query.filters();
    let mut events = EventRepository::list_page(&state.pool, &filters).await?;
    let total = EventRepository::count_filtered(&state.pool, &filters).await?;
    let has_next = events.len() > usize::try_from(PAGE_SIZE).unwrap_or(usize::MAX);
    events.truncate(usize::try_from(PAGE_SIZE).unwrap_or(usize::MAX));
    Ok(EventResults {
        groups: group_by_day(events, i18n),
        total,
        count_label: i18n.plural("events.count", usize::try_from(total).unwrap_or(0)),
        page: query.page(),
        page_label: i18n.tf("common.page_n", &[("n", &query.page().to_string())]),
        has_next,
        page_link_base: query.page_link_base(),
        has_filters: query.has_filters(),
        query: query.query(),
    })
}

pub async fn list(
    OptionalUser(user): OptionalUser,
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
    headers: HeaderMap,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    if let Some(redirect) = members_only(&state, user.as_ref(), &headers) {
        return Ok(redirect);
    }
    let results = load_results(&state, &query, &i18n).await?;
    if is_htmx(&headers) {
        return render(&EventResultsTemplate { results, i18n });
    }
    let frame = Frame::load(&state.pool, user.as_ref(), csrf_token.0, &i18n).await?;
    if let Some(u) = &user {
        UserRepository::mark_seen(&state.pool, u.id).await;
    }
    render(&EventListTemplate {
        frame,
        results,
        query,
        services: ServiceRepository::list_all(&state.pool).await?,
        i18n,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(pairs: &str) -> ListQuery {
        serde_html_form::from_str(pairs).unwrap()
    }

    #[test]
    fn blank_filters_are_ignored() {
        let q = query("kind=&service_id=&q=%20%20&lifecycle=");
        assert!(!q.has_filters());
        assert_eq!(q.page_link_base(), "/events?");
    }

    #[test]
    fn page_links_keep_every_filter_encoded() {
        let q = query("kind=incident&q=d%C3%A9lai+long&from=2026-09-01&page=3");
        assert!(q.has_filters());
        assert_eq!(q.page(), 3);
        assert_eq!(
            q.page_link_base(),
            "/events?kind=incident&q=d%C3%A9lai%20long&from=2026%2D09%2D01&"
        );
    }

    #[test]
    fn page_is_clamped() {
        assert_eq!(query("page=0").page(), 1);
        assert_eq!(query("page=99999999").page(), MAX_PAGE);
    }
}
