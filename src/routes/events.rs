//! Event pages: the list and its search, the event page and its side panel,
//! the forms, updates, state changes and templates.

use std::fmt::Write as _;

use askama::Template;
use axum::extract::{Path, Query, RawQuery, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use serde::{Deserialize, Deserializer};

use super::page::{TitlePart, title_parts};
use super::{Frame, members_only, render};
use crate::clock;
use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::{CsrfToken, HtmlForm, OptionalUser, RequirePublisher};
use crate::models::{
    Category, CreateEventInput, DayGroup, Event, EventFilters, EventUpdateWithAuthor, Kind,
    Lifecycle, LifecycleGroup, Service, Severity, UpdateEventInput, User, group_by_day,
};
use crate::repositories::{
    CreateTemplateInput, EventRepository, EventTemplateRepository, ServiceRepository,
};
use crate::services::{
    EventService, EventTemplateService, can_delete_update, can_modify, sanitize_markdown,
};
use crate::state::AppState;

const PAGE_SIZE: i64 = 20;
const MAX_PAGE: i64 = 10_000;

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
    render(&EventListTemplate {
        frame: Frame::load(&state.pool, user.as_ref(), csrf_token.0, &i18n).await?,
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

pub struct UpdateView {
    pub id: i64,
    pub html: String,
    pub author: Option<String>,
    pub time: String,
    pub iso: String,
    pub can_delete: bool,
}

pub struct TransitionOption {
    pub value: &'static str,
    pub label: String,
    pub closing: bool,
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
    i18n: I18n,
}

/// An event as its page and its side panel show it.
pub struct EventView {
    pub event: Event,
    pub services: Vec<Service>,
    pub description_html: String,
    pub updates: Vec<UpdateView>,
    /// "published on Sep 5 at 2:05 PM (UTC+2) by Jane", the name for members only.
    pub published: String,
    pub state: Option<StateLine>,
    /// The announced window of a planned maintenance.
    pub schedule: Option<String>,
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
    let updates = updates
        .into_iter()
        .map(|u| update_view(u, &ews.event, user, i18n))
        .collect();
    Ok(EventView {
        description_html: sanitize_markdown(&ews.event.description),
        published: published_line(&ews.event, author.as_deref(), i18n),
        state: state_line(&ews.event, i18n),
        schedule: schedule_line(&ews.event, i18n),
        event: ews.event,
        services: ews.services,
        updates,
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

fn published_line(event: &Event, author: Option<&str>, i18n: &I18n) -> String {
    let when = i18n.format_datetime_long(&event.created_at);
    match author {
        Some(name) => i18n.tf("events.published_by", &[("when", &when), ("author", name)]),
        None => i18n.tf("events.published_on", &[("when", &when)]),
    }
}

fn state_line(event: &Event, i18n: &I18n) -> Option<StateLine> {
    let lifecycle = event.lifecycle?;
    let detail = match lifecycle {
        Lifecycle::Scheduled => i18n.format_countdown(event.countdown()),
        Lifecycle::Cancelled => None,
        open if open.is_active() => event.elapsed_text(i18n),
        _ => event.ended_at.map(|end| ended_detail(event, end, i18n)),
    };
    Some(StateLine {
        tone: event.tone().as_str(),
        label: i18n.t(lifecycle.label_key(event.kind)).to_string(),
        detail,
    })
}

fn ended_detail(event: &Event, end: chrono::DateTime<chrono::Utc>, i18n: &I18n) -> String {
    let when = i18n.format_datetime(&end);
    match event.duration() {
        Some(parts) => i18n.tf(
            "events.ended_after",
            &[("when", &when), ("duration", &i18n.format_duration(&parts))],
        ),
        None => i18n.tf("events.ended_on", &[("when", &when)]),
    }
}

fn schedule_line(event: &Event, i18n: &I18n) -> Option<String> {
    let start = i18n.format_datetime(&event.planned_start?);
    Some(match event.planned_end {
        Some(end) => i18n.tf(
            "events.planned_window",
            &[("start", &start), ("end", &i18n.format_datetime(&end))],
        ),
        None => i18n.tf("events.planned_from", &[("start", &start)]),
    })
}

/// Staff names are shown to members only.
fn update_view(
    update: EventUpdateWithAuthor,
    event: &Event,
    user: Option<&User>,
    i18n: &I18n,
) -> UpdateView {
    UpdateView {
        id: update.id,
        can_delete: user.is_some_and(|u| can_delete_update(event, update.author_id, u)),
        author: user.map(|_| update.author_name),
        time: i18n.format_datetime(&update.created_at),
        iso: update.created_at.to_rfc3339(),
        html: update.message,
    }
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
            closing: next.needs_closing_message(),
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
    Ok(EventDetailTemplate {
        frame: Frame::load(&state.pool, user, csrf_token, &i18n).await?,
        transitions: transition_options(&view.event, composer.lifecycle, &i18n),
        revert_label: view
            .event
            .previous_lifecycle_key()
            .map(|key| i18n.tf("events.revert_to", &[("state", i18n.t(key))])),
        published_notice: false,
        can_edit,
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
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    if let Some(redirect) = members_only(&state, user.as_ref(), &headers) {
        return Ok(redirect);
    }
    let view = load_view(&state, id, user.as_ref(), &i18n).await?;
    let can_edit = user
        .as_ref()
        .is_some_and(|u| can_modify(&view.event, u.role));
    render(&DrawerTemplate {
        view,
        can_edit,
        i18n,
    })
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
    #[serde(default)]
    planned: Option<String>,
    #[serde(default, deserialize_with = "deserialize_blank_as_none")]
    category: Option<Category>,
    #[serde(default)]
    service_ids: Vec<i64>,
    #[serde(default)]
    save_as_template: Option<String>,
    #[serde(default, deserialize_with = "deserialize_blank_as_none")]
    template_id: Option<i64>,
    #[serde(default)]
    planned_start: String,
    #[serde(default)]
    planned_end: String,
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

    fn planned(&self, kind: Kind) -> bool {
        kind == Kind::Maintenance && self.planned.as_deref() == Some("on")
    }

    fn start(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        clock::parse_input(&self.planned_start)
    }

    fn end(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        clock::parse_input(&self.planned_end)
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
        }
    }

    fn from_event(event: &Event, service_ids: Vec<i64>) -> Self {
        Self {
            title: event.title.clone(),
            description: event.description.clone(),
            kind: event.kind,
            severity: event.severity,
            planned: event.planned,
            category: event.category,
            service_ids,
            planned_start: event
                .planned_start
                .as_ref()
                .map(clock::format_input)
                .unwrap_or_default(),
            planned_end: event
                .planned_end
                .as_ref()
                .map(clock::format_input)
                .unwrap_or_default(),
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
        }
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
    edit_id: Option<i64>,
    form: EventFormData,
    /// "Times are in the instance zone (UTC+2)."
    zone_hint: String,
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
        zone_hint: i18n.tf(
            "events.zone_hint",
            &[("zone", &clock::offset_label(&chrono::Utc::now()))],
        ),
        error,
        edit_id,
        form,
        i18n,
    })
}

#[derive(Deserialize)]
pub struct NewFormQuery {
    #[serde(default, deserialize_with = "deserialize_blank_as_none")]
    kind: Option<Kind>,
}

pub async fn new_form(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Query(query): Query<NewFormQuery>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    let form = EventFormData::blank(query.kind.unwrap_or(Kind::Incident));
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
        planned_end: input.end().filter(|_| planned),
        service_ids: input.service_ids.clone(),
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
        planned_end: input.end().filter(|_| planned),
        service_ids: input.service_ids.clone(),
    };
    match EventService::update(&state.pool, id, changes, user.role).await {
        Ok(()) => Ok(Redirect::to(&format!("/events/{id}")).into_response()),
        Err(AppError::Validation(key)) => {
            let form = EventFormData::from_input(input, kind, planned);
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
