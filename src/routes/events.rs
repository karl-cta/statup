//! Event pages: the list and its search, the event page and its side panel,
//! the forms, updates, state changes and templates.

use std::fmt::Write as _;

use askama::Template;
use axum::extract::{Path, Query, RawQuery, State};
use axum::http::{HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Redirect, Response};
use serde::{Deserialize, Deserializer};

use super::page::{TitlePart, title_parts};
use super::timeline::{self, TimelineEntry};
use super::{Frame, members_only, render};
use crate::clock;
use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::{CsrfToken, HtmlForm, OptionalUser, RequirePublisher};
use crate::models::{
    Category, CreateEventInput, DayGroup, Event, EventFilters, EventWithServices, Kind, Lifecycle,
    LifecycleGroup, Service, ServiceTag, Severity, UpdateEventInput, User, group_by_day,
};
use crate::repositories::{
    CreateTemplateInput, EventRepository, EventTemplateRepository, ServiceRepository,
    UserRepository,
};
use crate::services::{EventService, EventTemplateService, can_modify, sanitize_markdown};
use crate::state::AppState;

const PAGE_SIZE: i64 = 20;
const MAX_PAGE: i64 = 10_000;

/// How many recent maintenances the announcement form offers.
const MAINTENANCE_CHOICES: i64 = 20;

/// How many saved templates a new event offers to start from.
const TEMPLATE_CHOICES: i64 = 30;

fn deserialize_blank_as_none<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let raw = String::deserialize(deserializer)?;
    if raw.trim().is_empty() {
        return Ok(None);
    }
    T::deserialize(serde::de::value::StringDeserializer::<D::Error>::new(raw)).map(Some)
}

/// A number field left blank means none; a filled one is parsed, since a
/// form sends text.
fn deserialize_blank_as_none_id<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    trimmed.parse().map(Some).map_err(serde::de::Error::custom)
}

fn is_htmx(headers: &HeaderMap) -> bool {
    headers.contains_key("hx-request")
}

// ---- List and search ----

#[derive(Deserialize)]
pub struct ListQuery {
    page: Option<i64>,
    #[serde(default, deserialize_with = "deserialize_blank_as_none")]
    kind: Option<Kind>,
    #[serde(default, deserialize_with = "deserialize_blank_as_none")]
    service_id: Option<i64>,
    #[serde(default, deserialize_with = "deserialize_blank_as_none")]
    from: Option<String>,
    #[serde(default, deserialize_with = "deserialize_blank_as_none")]
    to: Option<String>,
    #[serde(default, deserialize_with = "deserialize_blank_as_none")]
    lifecycle: Option<String>,
    #[serde(default, deserialize_with = "deserialize_blank_as_none")]
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

/// The search page lives in the events list now; old links keep working.
pub async fn search(RawQuery(query): RawQuery) -> Redirect {
    match query.filter(|q| !q.is_empty()) {
        Some(q) => Redirect::permanent(&format!("/events?{q}")),
        None => Redirect::permanent("/events"),
    }
}

// ---- Event page and side panel ----

pub struct TransitionOption {
    pub value: &'static str,
    pub label: String,
    pub tone: &'static str,
    pub selected: bool,
}

/// What the author had typed when the composer is shown again.
#[derive(Default)]
pub struct Composer {
    pub message: String,
    pub lifecycle: Option<Lifecycle>,
    pub error: Option<String>,
}

#[derive(Template)]
#[template(path = "events/detail.html")]
struct EventDetailTemplate {
    frame: Frame,
    view: EventView,
    can_edit: bool,
    /// A finished maintenance offers a publisher the announcement of what
    /// is new after it.
    changelog_offer: bool,
    transitions: Vec<TransitionOption>,
    revert_label: Option<String>,
    published_notice: bool,
    composer: Composer,
    i18n: I18n,
}

#[derive(Template)]
#[template(path = "events/drawer_content.html")]
struct DrawerTemplate {
    view: EventView,
    can_edit: bool,
    csrf_token: String,
    transitions: Vec<TransitionOption>,
    /// Undoing a step is offered on the event's page only.
    revert_label: Option<String>,
    composer: Composer,
    i18n: I18n,
}

/// An event as its page and its side panel show it.
pub struct EventView {
    pub event: Event,
    pub services: Vec<Service>,
    /// The maintenance this announcement follows.
    pub follows: Option<MaintenanceChoice>,
    pub state: Option<StateLine>,
    pub timeline: Vec<TimelineEntry>,
}

impl EventView {
    pub fn service_tags(&self) -> Vec<ServiceTag> {
        self.services.iter().map(ServiceTag::from).collect()
    }

    /// A row of facts is worth drawing when there is one.
    pub fn has_facts(&self) -> bool {
        self.state.is_some() || !self.services.is_empty() || self.follows.is_some()
    }
}

/// Where the event stands, in words: the state and how long it has held.
pub struct StateLine {
    pub tone: &'static str,
    pub label: String,
    pub detail: Option<String>,
}

async fn load_view(
    state: &AppState,
    id: i64,
    user: Option<&User>,
    i18n: &I18n,
) -> Result<EventView, AppError> {
    let ews = EventService::find_with_services(&state.pool, id).await?;
    let updates = EventRepository::list_updates(&state.pool, id).await?;
    let author = match user {
        Some(_) => Some(author_name(state, ews.event.author_id, i18n).await?),
        None => None,
    };
    let follows = match ews.event.follows_event_id {
        Some(id) => EventRepository::find_by_id(&state.pool, id)
            .await?
            .map(|m| MaintenanceChoice { id, title: m.title }),
        None => None,
    };
    let timeline = timeline::build(
        &ews.event,
        sanitize_markdown(&ews.event.description),
        author,
        updates,
        user,
        i18n,
    );
    Ok(EventView {
        follows,
        state: state_line(&ews.event, i18n),
        event: ews.event,
        services: ews.services,
        timeline,
    })
}

async fn author_name(state: &AppState, author_id: i64, i18n: &I18n) -> Result<String, AppError> {
    Ok(
        crate::repositories::UserRepository::find_by_id(&state.pool, author_id)
            .await?
            .map_or_else(
                || i18n.t("date.unknown_author").to_string(),
                |u| u.display_name,
            ),
    )
}

fn state_line(event: &Event, i18n: &I18n) -> Option<StateLine> {
    let lifecycle = event.lifecycle?;
    let detail = match lifecycle {
        Lifecycle::Scheduled => i18n.format_countdown(event.countdown()),
        Lifecycle::Cancelled => None,
        open if open.is_active() => event.elapsed_text(i18n),
        _ => event.duration().map(|parts| {
            i18n.tf(
                "events.lasted",
                &[("duration", &i18n.format_duration(&parts))],
            )
        }),
    };
    Some(StateLine {
        tone: event.tone().as_str(),
        label: i18n.t(lifecycle.label_key(event.kind)).to_string(),
        detail,
    })
}

fn transition_options(
    event: &Event,
    selected: Option<Lifecycle>,
    i18n: &I18n,
) -> Vec<TransitionOption> {
    let Some(current) = event.lifecycle else {
        return Vec::new();
    };
    event
        .kind
        .allowed_transitions(current)
        .iter()
        .map(|next| TransitionOption {
            value: next.as_str(),
            label: i18n.t(next.label_key(event.kind)).to_string(),
            tone: event.tone_at(*next),
            selected: selected == Some(*next),
        })
        .collect()
}

#[derive(Deserialize)]
pub struct DetailQuery {
    #[serde(default)]
    published: Option<String>,
}

pub async fn detail(
    OptionalUser(user): OptionalUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<DetailQuery>,
    headers: HeaderMap,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    if let Some(redirect) = members_only(&state, user.as_ref(), &headers) {
        return Ok(redirect);
    }
    let page = detail_page(
        &state,
        id,
        user.as_ref(),
        csrf_token.0,
        i18n,
        Composer::default(),
    )
    .await?;
    render(&EventDetailTemplate {
        published_notice: query.published.is_some() && page.can_edit,
        ..page
    })
}

async fn detail_page(
    state: &AppState,
    id: i64,
    user: Option<&User>,
    csrf_token: String,
    i18n: I18n,
    composer: Composer,
) -> Result<EventDetailTemplate, AppError> {
    let view = load_view(state, id, user, &i18n).await?;
    let can_edit = user.is_some_and(|u| can_modify(&view.event, u.role));
    let changelog_offer = user.is_some_and(|u| u.role.can_publish())
        && view.event.kind == Kind::Maintenance
        && view.event.lifecycle == Some(Lifecycle::Completed);
    Ok(EventDetailTemplate {
        changelog_offer,
        frame: Frame::load(&state.pool, user, csrf_token, &i18n).await?,
        transitions: transition_options(&view.event, composer.lifecycle, &i18n),
        revert_label: revert_label(&view.event, &i18n),
        published_notice: false,
        can_edit,
        view,
        composer,
        i18n,
    })
}

/// "Back to Investigating", when the last step can be undone.
fn revert_label(event: &Event, i18n: &I18n) -> Option<String> {
    event
        .previous_lifecycle_key()
        .map(|key| i18n.tf("events.revert_to", &[("state", i18n.t(key))]))
}

async fn drawer_page(
    state: &AppState,
    id: i64,
    user: Option<&User>,
    csrf_token: String,
    i18n: I18n,
    composer: Composer,
) -> Result<DrawerTemplate, AppError> {
    let view = load_view(state, id, user, &i18n).await?;
    let can_edit = user.is_some_and(|u| can_modify(&view.event, u.role));
    Ok(DrawerTemplate {
        transitions: transition_options(&view.event, composer.lifecycle, &i18n),
        revert_label: None,
        can_edit,
        csrf_token,
        view,
        composer,
        i18n,
    })
}

pub async fn drawer_content(
    OptionalUser(user): OptionalUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    if let Some(redirect) = members_only(&state, user.as_ref(), &headers) {
        return Ok(redirect);
    }
    let page = drawer_page(
        &state,
        id,
        user.as_ref(),
        csrf_token.0,
        i18n,
        Composer::default(),
    )
    .await?;
    render(&page)
}

// ---- Updates and state changes ----

#[derive(Deserialize)]
pub struct UpdateInput {
    #[serde(default)]
    message: String,
    #[serde(default, deserialize_with = "deserialize_blank_as_none")]
    lifecycle: Option<Lifecycle>,
}

pub async fn add_update(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
    HtmlForm(input): HtmlForm<UpdateInput>,
) -> Result<Response, AppError> {
    match EventService::post_update(&state.pool, id, &input.message, input.lifecycle, &user).await {
        Ok(()) => Ok(Redirect::to(&format!("/events/{id}")).into_response()),
        Err(AppError::Validation(key)) => {
            let composer = Composer {
                error: Some(i18n.t(&key).to_string()),
                message: input.message,
                lifecycle: input.lifecycle,
            };
            let page = detail_page(&state, id, Some(&user), csrf_token.0, i18n, composer).await?;
            render(&page)
        }
        Err(e) => Err(e),
    }
}

/// An update posted from the side panel: the panel is drawn again in
/// place, and the page behind it learns that the event changed.
pub async fn add_update_in_panel(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
    HtmlForm(input): HtmlForm<UpdateInput>,
) -> Result<Response, AppError> {
    let result =
        EventService::post_update(&state.pool, id, &input.message, input.lifecycle, &user).await;
    let composer = match result {
        Ok(()) => Composer::default(),
        Err(AppError::Validation(key)) => Composer {
            error: Some(i18n.t(&key).to_string()),
            message: input.message,
            lifecycle: input.lifecycle,
        },
        Err(e) => return Err(e),
    };
    let posted = composer.error.is_none();
    let page = drawer_page(&state, id, Some(&user), csrf_token.0, i18n, composer).await?;
    let mut response = render(&page)?;
    if posted {
        response
            .headers_mut()
            .insert("HX-Trigger", HeaderValue::from_static("event-updated"));
    }
    Ok(response)
}

pub async fn delete_update(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Path((id, update_id)): Path<(i64, i64)>,
) -> Result<Response, AppError> {
    EventService::delete_update(&state.pool, id, update_id, &user).await?;
    Ok(Redirect::to(&format!("/events/{id}")).into_response())
}

pub async fn revert_lifecycle(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    EventService::revert(&state.pool, id, user.role).await?;
    Ok(Redirect::to(&format!("/events/{id}")).into_response())
}

pub async fn delete(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    EventService::delete(&state.pool, id, user.role).await?;
    Ok(Redirect::to("/events").into_response())
}

// ---- Forms ----

#[derive(Deserialize)]
pub struct EventInput {
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default, deserialize_with = "deserialize_blank_as_none")]
    kind: Option<Kind>,
    #[serde(default, deserialize_with = "deserialize_blank_as_none")]
    severity: Option<Severity>,
    #[serde(default, deserialize_with = "deserialize_blank_as_none")]
    category: Option<Category>,
    #[serde(default)]
    service_ids: Vec<i64>,
    #[serde(default)]
    save_as_template: Option<String>,
    #[serde(default, deserialize_with = "deserialize_blank_as_none_id")]
    template_id: Option<i64>,
    #[serde(default)]
    planned_start: String,
    #[serde(default)]
    planned_end: String,
    #[serde(default)]
    started_at: String,
    #[serde(default, deserialize_with = "deserialize_blank_as_none_id")]
    follows_event_id: Option<i64>,
}

impl EventInput {
    fn kind(&self) -> Kind {
        self.kind.unwrap_or(Kind::Incident)
    }

    /// Dimensions that belong to the kind; the others are dropped.
    fn severity(&self, kind: Kind) -> Option<Severity> {
        (kind == Kind::Incident).then_some(self.severity).flatten()
    }

    fn category(&self, kind: Kind) -> Option<Category> {
        (kind == Kind::Publication).then(|| self.category.unwrap_or(Category::Info))
    }

    /// A maintenance with a start is announced; without one it begins now.
    fn planned(&self, kind: Kind) -> bool {
        kind == Kind::Maintenance && !self.planned_start.trim().is_empty()
    }

    fn start(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        clock::parse_input(&self.planned_start)
    }

    fn end(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        clock::parse_input(&self.planned_end)
    }

    /// When an incident declared late really began; only an incident has one.
    fn began(&self, kind: Kind) -> Option<chrono::DateTime<chrono::Utc>> {
        (kind == Kind::Incident)
            .then(|| clock::parse_input(&self.started_at))
            .flatten()
    }
}

/// Values the form shows: an existing event, or what the author typed.
pub struct EventFormData {
    pub title: String,
    pub description: String,
    pub kind: Kind,
    pub severity: Option<Severity>,
    pub planned: bool,
    pub category: Option<Category>,
    pub service_ids: Vec<i64>,
    pub planned_start: String,
    pub planned_end: String,
    /// A maintenance under way keeps the start it had.
    pub start_locked: bool,
    /// When an incident being declared really began.
    pub started_at: String,
    pub follows_event_id: Option<i64>,
}

/// A maintenance an announcement may follow.
pub struct MaintenanceChoice {
    pub id: i64,
    pub title: String,
}

impl EventFormData {
    fn blank(kind: Kind) -> Self {
        Self {
            title: String::new(),
            description: String::new(),
            kind,
            severity: None,
            planned: true,
            category: (kind == Kind::Publication).then_some(Category::Info),
            service_ids: Vec::new(),
            planned_start: String::new(),
            planned_end: String::new(),
            start_locked: false,
            started_at: String::new(),
            follows_event_id: None,
        }
    }

    /// What is new after a maintenance: an announcement on the same
    /// services, its text opening with a link back to the work.
    fn changelog_after(ews: &EventWithServices, i18n: &I18n) -> Self {
        Self {
            title: i18n.tf("title.changelog_after", &[("title", &ews.event.title)]),
            description: String::new(),
            kind: Kind::Publication,
            severity: None,
            planned: false,
            category: Some(Category::Changelog),
            service_ids: ews.services.iter().map(|s| s.id).collect(),
            planned_start: String::new(),
            planned_end: String::new(),
            start_locked: false,
            started_at: String::new(),
            follows_event_id: Some(ews.event.id),
        }
    }

    fn from_event(event: &Event, service_ids: Vec<i64>) -> Self {
        let maintenance = event.kind == Kind::Maintenance;
        // A maintenance begun right away has no planned start: its window
        // is read from when it started.
        let start = event
            .planned_start
            .or(event.started_at.filter(|_| maintenance));
        Self {
            title: event.title.clone(),
            description: event.description.clone(),
            kind: event.kind,
            severity: event.severity,
            planned: event.planned,
            category: event.category,
            service_ids,
            planned_start: start.as_ref().map(clock::format_input).unwrap_or_default(),
            planned_end: event
                .planned_end
                .as_ref()
                .map(clock::format_input)
                .unwrap_or_default(),
            start_locked: maintenance && event.started_at.is_some(),
            started_at: String::new(),
            follows_event_id: event.follows_event_id,
        }
    }

    fn from_input(input: EventInput, kind: Kind, planned: bool) -> Self {
        Self {
            severity: input.severity(kind),
            category: input.category(kind),
            planned,
            kind,
            title: input.title,
            description: input.description,
            service_ids: input.service_ids,
            planned_start: input.planned_start,
            planned_end: input.planned_end,
            start_locked: false,
            started_at: input.started_at,
            follows_event_id: input.follows_event_id,
        }
    }

    // The template hands a field over by reference.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn follows(&self, id: &i64) -> bool {
        self.follows_event_id == Some(*id)
    }

    /// Opens the extra options when they already hold a choice.
    fn options_in_use(&self) -> bool {
        self.follows_event_id.is_some() || !self.started_at.is_empty()
    }

    fn kind_is(&self, kind: &str) -> bool {
        self.kind.as_str() == kind
    }

    fn severity_is(&self, severity: &str) -> bool {
        self.severity.is_some_and(|s| s.as_str() == severity)
    }

    fn category_is(&self, category: &str) -> bool {
        self.category.is_some_and(|c| c.as_str() == category)
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn has_service(&self, id: &i64) -> bool {
        self.service_ids.contains(id)
    }
}

#[derive(Template)]
#[template(path = "events/form.html")]
struct EventFormTemplate {
    frame: Frame,
    error: Option<String>,
    services: Vec<Service>,
    /// Recent maintenances an announcement may follow.
    maintenances: Vec<MaintenanceChoice>,
    edit_id: Option<i64>,
    form: EventFormData,
    /// "Times are in the instance zone (UTC+2)."
    zone_hint: String,
    /// The instance clock when the page was drawn, for the schedule script.
    now_input: String,
    /// Saved templates a new event may start from.
    templates: Vec<crate::models::EventTemplate>,
    i18n: I18n,
}

async fn render_form(
    state: &AppState,
    user: &User,
    csrf_token: String,
    i18n: I18n,
    edit_id: Option<i64>,
    form: EventFormData,
    error: Option<String>,
) -> Result<Response, AppError> {
    render(&EventFormTemplate {
        frame: Frame::load(&state.pool, Some(user), csrf_token, &i18n).await?,
        services: ServiceRepository::list_all(&state.pool).await?,
        maintenances: maintenance_choices(state, form.follows_event_id).await?,
        zone_hint: i18n.tf(
            "events.zone_hint",
            &[("zone", &clock::offset_label(&chrono::Utc::now()))],
        ),
        now_input: clock::format_input(&chrono::Utc::now()),
        templates: if edit_id.is_none() {
            EventTemplateRepository::list_most_used(&state.pool, TEMPLATE_CHOICES).await?
        } else {
            Vec::new()
        },
        error,
        edit_id,
        form,
        i18n,
    })
}

/// The last finished maintenances, and the one the form already follows
/// when it is older than those.
async fn maintenance_choices(
    state: &AppState,
    followed: Option<i64>,
) -> Result<Vec<MaintenanceChoice>, AppError> {
    let mut choices: Vec<MaintenanceChoice> =
        EventRepository::list_finished_maintenance(&state.pool, MAINTENANCE_CHOICES)
            .await?
            .into_iter()
            .map(|m| MaintenanceChoice {
                id: m.id,
                title: m.title,
            })
            .collect();
    if let Some(id) = followed
        && !choices.iter().any(|c| c.id == id)
        && let Some(event) = EventRepository::find_by_id(&state.pool, id).await?
    {
        choices.insert(
            0,
            MaintenanceChoice {
                id,
                title: event.title,
            },
        );
    }
    Ok(choices)
}

#[derive(Deserialize)]
pub struct NewFormQuery {
    #[serde(default, deserialize_with = "deserialize_blank_as_none")]
    kind: Option<Kind>,
    /// A maintenance the announcement follows: the form opens on what is
    /// new after it.
    after: Option<i64>,
}

pub async fn new_form(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Query(query): Query<NewFormQuery>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    let followed = match query.after {
        Some(id) => EventRepository::find_with_services(&state.pool, id)
            .await?
            .filter(|ews| ews.event.kind == Kind::Maintenance),
        None => None,
    };
    let form = match followed {
        Some(ews) => EventFormData::changelog_after(&ews, &i18n),
        None => EventFormData::blank(query.kind.unwrap_or(Kind::Incident)),
    };
    render_form(&state, &user, csrf_token.0, i18n, None, form, None).await
}

pub async fn create(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
    HtmlForm(input): HtmlForm<EventInput>,
) -> Result<Response, AppError> {
    let kind = input.kind();
    let planned = input.planned(kind);
    let create_input = CreateEventInput {
        kind,
        severity: input.severity(kind),
        planned,
        category: input.category(kind),
        title: input.title.trim().to_string(),
        description: input.description.trim().to_string(),
        planned_start: input.start().filter(|_| planned),
        planned_end: input.end().filter(|_| kind == Kind::Maintenance),
        started_at: input.began(kind),
        service_ids: input.service_ids.clone(),
        follows_event_id: input.follows_event_id,
        author_id: user.id,
    };
    match EventService::create(&state.pool, create_input).await {
        Ok(event) => {
            after_create(&state, &user, &input, kind).await;
            Ok(Redirect::to(&format!("/events/{}?published=1", event.id)).into_response())
        }
        Err(AppError::Validation(key)) => {
            let form = EventFormData::from_input(input, kind, planned);
            let error = Some(i18n.t(&key).to_string());
            render_form(&state, &user, csrf_token.0, i18n, None, form, error).await
        }
        Err(e) => Err(e),
    }
}

/// Template bookkeeping never blocks a publication: a failure is logged.
async fn after_create(state: &AppState, user: &User, input: &EventInput, kind: Kind) {
    if let Some(template_id) = input.template_id
        && let Err(e) = EventTemplateService::record_usage(&state.pool, template_id).await
    {
        tracing::warn!(error = %e, "Failed to record template usage");
    }
    if input.save_as_template.as_deref() != Some("on") {
        return;
    }
    let template = CreateTemplateInput {
        title: &input.title,
        description: &input.description,
        kind,
        severity: input.severity(kind),
        planned: input.planned(kind),
        category: input.category(kind),
        created_by: user.id,
    };
    if let Err(e) = EventTemplateService::create(&state.pool, template).await {
        tracing::warn!(error = %e, "Failed to save event template");
    }
}

pub async fn edit_form(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    let ews = EventService::find_with_services(&state.pool, id).await?;
    if !can_modify(&ews.event, user.role) {
        return Err(AppError::Validation(
            "validation.event_closed_admin_only".to_string(),
        ));
    }
    let service_ids = ews.services.iter().map(|s| s.id).collect();
    let form = EventFormData::from_event(&ews.event, service_ids);
    render_form(&state, &user, csrf_token.0, i18n, Some(id), form, None).await
}

pub async fn update(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
    HtmlForm(input): HtmlForm<EventInput>,
) -> Result<Response, AppError> {
    let event = EventService::find_with_services(&state.pool, id)
        .await?
        .event;
    let (kind, planned) = (event.kind, event.planned);
    let changes = UpdateEventInput {
        severity: input.severity(kind),
        planned,
        category: input.category(kind),
        title: input.title.trim().to_string(),
        description: input.description.trim().to_string(),
        planned_start: input.start().filter(|_| planned),
        planned_end: input.end().filter(|_| kind == Kind::Maintenance),
        service_ids: input.service_ids.clone(),
        follows_event_id: input.follows_event_id,
    };
    match EventService::update(&state.pool, id, changes, user.role).await {
        Ok(()) => Ok(Redirect::to(&format!("/events/{id}")).into_response()),
        Err(AppError::Validation(key)) => {
            let mut form = EventFormData::from_input(input, kind, planned);
            form.start_locked = kind == Kind::Maintenance && event.started_at.is_some();
            let error = Some(i18n.t(&key).to_string());
            render_form(&state, &user, csrf_token.0, i18n, Some(id), form, error).await
        }
        Err(e) => Err(e),
    }
}

// ---- Templates ----

#[derive(Template)]
#[template(path = "events/template_suggestions.html")]
struct TemplateSuggestionsTemplate {
    templates: Vec<crate::models::EventTemplate>,
    i18n: I18n,
}

#[derive(Deserialize)]
pub struct TemplateSearchQuery {
    #[serde(default, alias = "title")]
    q: String,
}

pub async fn template_search(
    _publisher: RequirePublisher,
    Locale(i18n): Locale,
    State(state): State<AppState>,
    Query(params): Query<TemplateSearchQuery>,
) -> Result<Response, AppError> {
    let query = params.q.trim();
    let templates = if query.chars().count() >= 2 {
        EventTemplateRepository::search_by_title(&state.pool, query, 5).await?
    } else {
        Vec::new()
    };
    render(&TemplateSuggestionsTemplate { templates, i18n })
}

pub async fn template_detail(
    _publisher: RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let template = EventTemplateRepository::find_by_id(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    let body = serde_json::json!({
        "id": template.id,
        "title": template.title,
        "description": template.description,
        "kind": template.kind.as_str(),
        "severity": template.severity.map(Severity::as_str),
        "planned": template.planned,
        "category": template.category.map(Category::as_str),
    });
    Ok(axum::Json(body).into_response())
}

/// An htmx caller removes the suggestion itself; a form post goes back to
/// the form.
pub async fn template_delete(
    _publisher: RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    EventTemplateService::delete(&state.pool, id).await?;
    if is_htmx(&headers) {
        return Ok(axum::http::StatusCode::OK.into_response());
    }
    Ok(Redirect::to("/events/new").into_response())
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

    #[test]
    fn dimensions_follow_the_kind() {
        let input: EventInput = serde_html_form::from_str(
            "title=x&description=&kind=publication&severity=critical&planned=on&category=",
        )
        .unwrap();
        let kind = input.kind();
        assert_eq!(input.severity(kind), None);
        assert_eq!(input.category(kind), Some(Category::Info));
        assert!(!input.planned(kind));
    }
}
