//! Events, as the API reads them: incidents, maintenances and announcements.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::caller::ApiCaller;
use crate::clock;
use crate::db::DbPool;
use crate::error::AppError;
use crate::models::{
    ApiKeyHolder, Category, CreateEventInput, Event, EventFilters, EventSummary,
    EventUpdateWithAuthor, EventWithServices, JournalKind, Kind, Lifecycle, LifecycleGroup,
    NAME_SEPARATOR, Scope, Service, Severity, html_to_text,
};
use crate::repositories::{EventRepository, NewJournalEntry};
use crate::services::{EventService, clean_update, read_next_update};

const DEFAULT_LIMIT: i64 = 20;
const MAX_LIMIT: i64 = 100;

#[derive(Deserialize)]
pub(super) struct ListQuery {
    /// `true` for open events, `false` for closed ones, both when absent.
    open: Option<bool>,
    kind: Option<Kind>,
    limit: Option<i64>,
    offset: Option<i64>,
}

impl ListQuery {
    /// A page out of range is clamped, never refused.
    fn filters(&self) -> EventFilters {
        EventFilters {
            kind: self.kind,
            lifecycle_group: self.open.map(|open| {
                if open {
                    LifecycleGroup::Active
                } else {
                    LifecycleGroup::Closed
                }
            }),
            limit: self.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT),
            offset: self.offset.unwrap_or(0).max(0),
            ..Default::default()
        }
    }
}

#[derive(Serialize)]
pub(super) struct EventList {
    events: Vec<EventBody>,
    /// Events matching the filters, whatever the page.
    total: i64,
}

#[derive(Serialize)]
pub(super) struct EventDetail {
    #[serde(flatten)]
    event: EventBody,
    updates: Vec<UpdateBody>,
}

/// An event in the API. Its own shape, so the contract does not move with
/// the database.
#[derive(Serialize)]
pub(super) struct EventBody {
    id: i64,
    kind: &'static str,
    severity: Option<&'static str>,
    category: Option<&'static str>,
    /// The lifecycle step, none for an announcement.
    state: Option<&'static str>,
    title: String,
    /// The Markdown as written.
    description: String,
    services: Vec<ServiceRef>,
    planned_start: Option<String>,
    planned_end: Option<String>,
    started_at: Option<String>,
    ended_at: Option<String>,
    next_update_at: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize)]
pub(super) struct ServiceRef {
    id: i64,
    name: String,
}

#[derive(Serialize)]
pub(super) struct UpdateBody {
    id: i64,
    /// The message as plain text.
    message: String,
    message_html: String,
    author: String,
    created_at: String,
}

fn date(value: Option<DateTime<Utc>>) -> Option<String> {
    value.map(clock::rfc3339)
}

/// `Event` and `EventSummary` carry the same fields; only their services
/// differ.
macro_rules! event_body {
    ($source:expr, $services:expr) => {{
        let source = $source;
        EventBody {
            id: source.id,
            kind: source.kind.as_str(),
            severity: source.severity.map(Severity::as_str),
            category: source.category.map(Category::as_str),
            state: source.lifecycle.map(Lifecycle::as_str),
            title: source.title,
            description: source.description,
            services: $services,
            planned_start: date(source.planned_start),
            planned_end: date(source.planned_end),
            started_at: date(source.started_at),
            ended_at: date(source.ended_at),
            next_update_at: date(source.next_update_at),
            created_at: clock::rfc3339(source.created_at),
            updated_at: clock::rfc3339(source.updated_at),
        }
    }};
}

impl EventBody {
    fn of_event(event: Event, services: Vec<ServiceRef>) -> Self {
        event_body!(event, services)
    }
}

impl From<EventSummary> for EventBody {
    fn from(summary: EventSummary) -> Self {
        // The names and the ids are joined in the same order.
        let services = summary
            .service_ids
            .split(',')
            .zip(summary.service_names.split(NAME_SEPARATOR))
            .filter_map(|(id, name)| {
                Some(ServiceRef {
                    id: id.parse().ok()?,
                    name: name.to_string(),
                })
            })
            .collect();
        event_body!(summary, services)
    }
}

impl From<Service> for ServiceRef {
    fn from(service: Service) -> Self {
        Self {
            id: service.id,
            name: service.name,
        }
    }
}

impl From<EventUpdateWithAuthor> for UpdateBody {
    fn from(update: EventUpdateWithAuthor) -> Self {
        Self {
            id: update.id,
            message: html_to_text(&update.message),
            message_html: clean_update(&update.message),
            author: update.author_name,
            created_at: clock::rfc3339(update.created_at),
        }
    }
}

/// Events newest first, a page at a time. A key sees what an editor sees.
pub(super) async fn list(
    _caller: ApiCaller,
    State(pool): State<DbPool>,
    Query(query): Query<ListQuery>,
) -> Result<Json<EventList>, AppError> {
    let filters = query.filters();
    let events = EventRepository::list_page(&pool, &filters, &Scope::All).await?;
    let total = EventRepository::count_filtered(&pool, &filters, &Scope::All).await?;
    Ok(Json(EventList {
        events: events.into_iter().map(EventBody::from).collect(),
        total,
    }))
}

pub(super) async fn show(
    _caller: ApiCaller,
    State(pool): State<DbPool>,
    Path(id): Path<i64>,
) -> Result<Json<EventDetail>, AppError> {
    Ok(Json(detail(&pool, id).await?))
}

/// One event with its updates, as reading it and every write answer.
async fn detail(pool: &DbPool, id: i64) -> Result<EventDetail, AppError> {
    let EventWithServices { event, services } =
        EventService::find_with_services(pool, id, &Scope::All).await?;
    let updates = EventRepository::list_updates(pool, id).await?;
    let services = services.into_iter().map(ServiceRef::from).collect();
    Ok(EventDetail {
        event: EventBody::of_event(event, services),
        updates: updates.into_iter().map(UpdateBody::from).collect(),
    })
}

/// The fields the API takes on a new event. Each kind keeps only those that
/// fit it, as the form does; a field the API does not take is refused.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NewEvent {
    kind: Kind,
    /// A missing title is refused by the event rules, as an empty one is.
    #[serde(default)]
    title: String,
    /// Markdown, as on the event page.
    #[serde(default)]
    description: String,
    severity: Option<Severity>,
    category: Option<Category>,
    #[serde(default)]
    services: Vec<i64>,
    /// The step an incident opens at, when its team already works on it.
    state: Option<Lifecycle>,
    /// When an incident declared late began.
    started_at: Option<DateTime<Utc>>,
    /// A maintenance with a start is announced; without one it begins now.
    planned_start: Option<DateTime<Utc>>,
    planned_end: Option<DateTime<Utc>>,
    #[serde(default)]
    keeps_services_up: bool,
    /// A delay or a time of day, as the form takes it: "30 min", "14:30".
    #[serde(default)]
    next_update: String,
}

impl NewEvent {
    fn into_input(self, key: &ApiKeyHolder) -> Result<CreateEventInput, AppError> {
        let kind = self.kind;
        let incident = kind == Kind::Incident;
        let maintenance = kind == Kind::Maintenance;
        let planned = maintenance && self.planned_start.is_some();
        let next_update = if incident {
            read_next_update(&self.next_update)?
        } else {
            None
        };
        Ok(CreateEventInput {
            kind,
            severity: self.severity.filter(|_| incident),
            planned,
            category: (kind == Kind::Publication).then(|| self.category.unwrap_or(Category::Info)),
            title: self.title.trim().to_string(),
            description: self.description.trim().to_string(),
            planned_start: self.planned_start.filter(|_| planned),
            planned_end: self.planned_end.filter(|_| maintenance),
            started_at: self.started_at.filter(|_| incident),
            opening_step: self.state.filter(|_| incident),
            keeps_services_up: maintenance && self.keeps_services_up,
            service_ids: self.services,
            follows_event_id: None,
            author_id: key.created_by,
            api_key_id: Some(key.id),
            next_update,
        })
    }
}

/// What the API takes on an update: a message, a new state, or both.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NewUpdate {
    /// Markdown, as on the event page.
    #[serde(default)]
    message: String,
    state: Option<Lifecycle>,
    /// A delay or a time of day, as the form takes it: "30 min", "14:30".
    #[serde(default)]
    next_update: String,
}

/// Publishes an event signed with the key, with the notifications the
/// screen sends.
pub(super) async fn create(
    caller: ApiCaller,
    State(pool): State<DbPool>,
    Json(input): Json<NewEvent>,
) -> Result<(StatusCode, Json<EventDetail>), AppError> {
    let event = EventService::create(&pool, input.into_input(&caller.key)?).await?;
    NewJournalEntry::new(JournalKind::EventCreated)
        .by_key(&caller.key)
        .about(&event.title)
        .record(&pool)
        .await;
    Ok((StatusCode::CREATED, Json(detail(&pool, event.id).await?)))
}

/// Posts an update as an editor would, signed with the key.
pub(super) async fn post_update(
    caller: ApiCaller,
    State(pool): State<DbPool>,
    Path(id): Path<i64>,
    Json(input): Json<NewUpdate>,
) -> Result<(StatusCode, Json<EventDetail>), AppError> {
    let next_update = read_next_update(&input.next_update)?;
    let editor = caller.editor(&pool).await?;
    let key_id = Some(caller.key.id);
    EventService::post_update(
        &pool,
        id,
        &input.message,
        input.state,
        next_update,
        &editor,
        key_id,
    )
    .await?;
    let detail = detail(&pool, id).await?;
    let mut entry = NewJournalEntry::new(JournalKind::EventUpdatePosted)
        .by_key(&caller.key)
        .about(&detail.event.title);
    if let Some(state) = input.state {
        entry = entry.with_detail(state.as_str());
    }
    entry.record(&pool).await;
    Ok((StatusCode::CREATED, Json(detail)))
}
