//! Event routes: listing, creation, update, detail, lifecycle transitions.

use askama::Template;
use axum::Form;
use axum::extract::{Path, Query, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::{Deserialize, Deserializer};
use validator::Validate;

use chrono::{DateTime, Utc};

use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::{CsrfToken, OptionalUser, RequirePublisher, ValidatedForm};
use crate::models::{
    Category, CreateEventInput, EventSummary, EventUpdateWithAuthor, EventWithServices, Kind,
    Lifecycle, Service, Severity, User,
};
use crate::repositories::{
    EventRepository, EventTemplateRepository, IconRepository, ServiceRepository,
};
use crate::services::{
    CreateTemplateParams, EventService, EventTemplateService, sanitize_markdown,
};
use crate::state::AppState;

#[derive(Template)]
#[template(path = "events/list.html")]
struct EventListTemplate {
    csrf_token: String,
    user_display_name: String,
    is_admin: bool,
    is_authenticated: bool,
    unread_count: i64,
    last_admin_action: Option<String>,
    events: Vec<EventSummary>,
    page: i64,
    has_next: bool,
    filter_kind: Option<String>,
    filter_service: Option<String>,
    filter_from: Option<String>,
    filter_to: Option<String>,
    services: Vec<Service>,
    base_url: String,
    i18n: I18n,
}

#[derive(Template)]
#[template(path = "events/detail.html")]
#[allow(clippy::struct_excessive_bools)]
struct EventDetailTemplate {
    csrf_token: String,
    user_display_name: String,
    is_admin: bool,
    is_authenticated: bool,
    unread_count: i64,
    last_admin_action: Option<String>,
    event: EventWithServices,
    description_html: String,
    updates: Vec<EventUpdateWithAuthor>,
    author_name: String,
    can_edit: bool,
    allowed_transitions: Vec<Lifecycle>,
    can_revert: bool,
    previous_lifecycle_label: Option<String>,
    i18n: I18n,
}

#[derive(Template)]
#[template(path = "events/detail_panel.html")]
#[allow(clippy::struct_excessive_bools)]
struct EventDetailPanelTemplate {
    csrf_token: String,
    event: EventWithServices,
    description_html: String,
    updates: Vec<EventUpdateWithAuthor>,
    author_name: String,
    can_edit: bool,
    allowed_transitions: Vec<Lifecycle>,
    can_revert: bool,
    previous_lifecycle_label: Option<String>,
    i18n: I18n,
}

#[derive(Template)]
#[template(path = "events/form.html")]
struct EventFormTemplate {
    csrf_token: String,
    user_display_name: String,
    is_admin: bool,
    is_authenticated: bool,
    unread_count: i64,
    last_admin_action: Option<String>,
    error: Option<String>,
    services: Vec<Service>,
    event: Option<EventFormData>,
    i18n: I18n,
}

struct EventFormData {
    id: i64,
    title: String,
    description: String,
    kind: Kind,
    severity: Option<Severity>,
    planned: bool,
    category: Option<Category>,
    service_ids: Vec<i64>,
    planned_start: Option<String>,
    planned_end: Option<String>,
}

impl EventFormData {
    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn has_service(&self, id: &i64) -> bool {
        self.service_ids.contains(id)
    }
}

#[derive(Deserialize, Validate)]
pub struct EventInput {
    #[validate(length(min = 1, max = 200, message = "validation.title_required"))]
    title: String,
    #[validate(length(min = 1, message = "validation.description_required"))]
    description: String,
    kind: Kind,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    severity: Option<Severity>,
    #[serde(default)]
    planned: Option<String>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    category: Option<Category>,
    #[serde(default)]
    service_ids: Vec<i64>,
    icon_id: Option<i64>,
    #[serde(default)]
    save_as_template: Option<String>,
    #[serde(default)]
    planned_start: Option<String>,
    #[serde(default)]
    planned_end: Option<String>,
}

impl EventInput {
    fn is_planned(&self) -> bool {
        self.planned.as_deref() == Some("on")
    }

    /// Normalize dimensions according to `kind` so SQL invariants hold:
    /// - publication: no severity, no `planned`, category required
    /// - incident: no category, planned = false
    /// - maintenance: no category, planned follows the checkbox
    fn normalized(&self) -> (Option<Severity>, bool, Option<Category>) {
        match self.kind {
            Kind::Publication => (None, false, self.category),
            Kind::Incident => (self.severity, false, None),
            Kind::Maintenance => (self.severity, self.is_planned(), None),
        }
    }
}

#[derive(Deserialize)]
pub struct LifecycleInput {
    lifecycle: Lifecycle,
    #[serde(default)]
    resolution_comment: Option<String>,
}

#[derive(Deserialize, Validate)]
pub struct UpdateInput {
    #[validate(length(min = 1, message = "validation.update_required"))]
    message: String,
}

fn empty_string_as_none<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let s = String::deserialize(deserializer)?;
    if s.is_empty() {
        Ok(None)
    } else {
        T::deserialize(serde::de::value::StringDeserializer::<D::Error>::new(s)).map(Some)
    }
}

#[derive(Deserialize)]
pub struct ListQuery {
    page: Option<i64>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    kind: Option<Kind>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    service_id: Option<i64>,
    from: Option<String>,
    to: Option<String>,
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    page: Option<i64>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    kind: Option<Kind>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    service_id: Option<i64>,
    from: Option<String>,
    to: Option<String>,
}

#[derive(Deserialize)]
pub struct SearchQuery {
    q: Option<String>,
    page: Option<i64>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    kind: Option<Kind>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    service_id: Option<i64>,
}

/// Groups events sharing the same date label (e.g. "Today").
pub struct DateGroup {
    pub label: String,
    pub events: Vec<EventSummary>,
}

#[derive(Template)]
#[template(path = "events/history.html")]
struct HistoryTemplate {
    csrf_token: String,
    user_display_name: String,
    is_admin: bool,
    is_authenticated: bool,
    unread_count: i64,
    last_admin_action: Option<String>,
    groups: Vec<DateGroup>,
    page: i64,
    has_next: bool,
    filter_kind: Option<String>,
    filter_service: Option<String>,
    filter_from: Option<String>,
    filter_to: Option<String>,
    services: Vec<Service>,
    base_url: String,
    i18n: I18n,
}

/// Search result enriched with a title highlighted on the matching terms.
pub struct SearchResult {
    pub event: EventSummary,
    pub highlighted_title: String,
}

#[derive(Template)]
#[template(path = "events/search.html")]
struct SearchTemplate {
    csrf_token: String,
    user_display_name: String,
    is_admin: bool,
    is_authenticated: bool,
    unread_count: i64,
    last_admin_action: Option<String>,
    query: String,
    results: Vec<SearchResult>,
    page: i64,
    has_next: bool,
    filter_kind: Option<String>,
    filter_service: Option<String>,
    services: Vec<Service>,
    base_url: String,
    i18n: I18n,
}

fn render(tpl: &impl Template) -> Result<Response, AppError> {
    let html = tpl
        .render()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("template render error: {e}")))?;
    Ok(Html(html).into_response())
}

fn layout_fields(user: Option<&User>) -> (String, bool, bool) {
    match user {
        Some(u) => (u.display_name.clone(), u.role.can_admin(), true),
        None => (String::new(), false, false),
    }
}

fn layout_fields_auth(user: &User) -> (String, bool, bool) {
    (user.display_name.clone(), user.role.can_admin(), true)
}

async fn fetch_last_admin_action(
    pool: &crate::db::DbPool,
    i18n: &I18n,
) -> Result<Option<String>, AppError> {
    Ok(EventRepository::last_admin_action(pool)
        .await?
        .map(|dt| i18n.format_datetime_long(&dt)))
}

async fn unread(pool: &crate::db::DbPool, user: Option<&User>) -> Result<i64, AppError> {
    match user {
        Some(u) => EventService::unread_count(pool, u.last_seen_at).await,
        None => Ok(0),
    }
}

async fn unread_auth(pool: &crate::db::DbPool, user: &User) -> Result<i64, AppError> {
    EventService::unread_count(pool, user.last_seen_at).await
}

async fn resolve_icon_url(
    pool: &crate::db::DbPool,
    icon_id: Option<i64>,
) -> Result<Option<String>, AppError> {
    let Some(id) = icon_id else {
        return Ok(None);
    };
    let icon = IconRepository::find_by_id(pool, id).await?;
    Ok(icon.map(|i| i.url()))
}

fn parse_icon_id(input: Option<i64>) -> Option<i64> {
    input.filter(|&id| id > 0)
}

fn parse_datetime_local(s: &str) -> Option<DateTime<Utc>> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
    chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M")
        .ok()
        .map(|dt| dt.and_utc())
}

fn format_datetime_local(dt: &DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M").to_string()
}

/// A lifecycle is a "normal closure" when it requires a resolution comment.
/// Cancelled does not trigger that prompt.
fn requires_resolution_comment(lifecycle: Lifecycle) -> bool {
    matches!(lifecycle, Lifecycle::Resolved | Lifecycle::Completed)
}

const PAGE_SIZE: i64 = 20;

pub async fn list(
    OptionalUser(user): OptionalUser,
    State(state): State<AppState>,
    Query(params): Query<ListQuery>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    if user.is_none() && !state.is_public_mode() {
        return Ok(Redirect::to("/login").into_response());
    }

    let page = params.page.unwrap_or(1).max(1);
    let offset = (page - 1) * PAGE_SIZE;

    let from = params.from.as_deref().and_then(parse_date);
    let to = params.to.as_deref().and_then(parse_date_end_of_day);

    let filters = crate::models::EventFilters {
        kind: params.kind,
        service_id: params.service_id,
        from,
        to,
        limit: PAGE_SIZE + 1,
        offset,
        ..Default::default()
    };

    let mut events = EventRepository::list_by_filters(&state.pool, filters).await?;
    #[allow(clippy::cast_possible_wrap)]
    let has_next = events.len() as i64 > PAGE_SIZE;
    #[allow(clippy::cast_possible_truncation)]
    events.truncate(PAGE_SIZE as usize);

    let services = ServiceRepository::list_all(&state.pool).await?;
    let (user_display_name, is_admin, is_authenticated) = layout_fields(user.as_ref());
    let unread_count = unread(&state.pool, user.as_ref()).await?;

    let filter_kind = params.kind.map(|k| k.as_str().to_string());
    let filter_service = params.service_id.map(|id| id.to_string());

    let mut base_url = String::from("/events?");
    {
        use std::fmt::Write;
        if let Some(ref t) = filter_kind {
            let _ = write!(base_url, "kind={t}&");
        }
        if let Some(ref s) = filter_service {
            let _ = write!(base_url, "service_id={s}&");
        }
        if let Some(ref f) = params.from {
            let _ = write!(base_url, "from={f}&");
        }
        if let Some(ref t) = params.to {
            let _ = write!(base_url, "to={t}&");
        }
    }

    let last_admin_action = fetch_last_admin_action(&state.pool, &i18n).await?;
    let tpl = EventListTemplate {
        csrf_token: csrf_token.0,
        user_display_name,
        is_admin,
        is_authenticated,
        unread_count,
        last_admin_action,
        events,
        page,
        has_next,
        filter_kind,
        filter_service,
        filter_from: params.from,
        filter_to: params.to,
        services,
        base_url,
        i18n,
    };
    render(&tpl)
}

pub async fn detail(
    OptionalUser(user): OptionalUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    if user.is_none() && !state.is_public_mode() {
        return Ok(Redirect::to("/login").into_response());
    }

    let ews = EventService::find_with_services(&state.pool, id).await?;
    let updates = EventRepository::list_updates_with_author(&state.pool, id).await?;

    let author = crate::repositories::UserRepository::find_by_id(&state.pool, ews.event.author_id)
        .await?
        .map_or_else(
            || i18n.t("date.unknown_author").to_string(),
            |u| u.display_name,
        );

    let can_edit = user.as_ref().is_some_and(|u| u.role.can_publish());
    let allowed_transitions = ews
        .event
        .lifecycle
        .map(|l| ews.event.kind.allowed_transitions(l).to_vec())
        .unwrap_or_default();
    let can_revert = can_edit && ews.event.previous_lifecycle.is_some();
    let previous_lifecycle_label = ews
        .event
        .previous_lifecycle
        .map(|l| i18n.t(l.i18n_key()).to_string());
    let (user_display_name, is_admin, is_authenticated) = layout_fields(user.as_ref());
    let unread_count = unread(&state.pool, user.as_ref()).await?;

    let description_html = sanitize_markdown(&ews.event.description);
    let last_admin_action = fetch_last_admin_action(&state.pool, &i18n).await?;
    let tpl = EventDetailTemplate {
        csrf_token: csrf_token.0,
        user_display_name,
        is_admin,
        is_authenticated,
        unread_count,
        last_admin_action,
        event: ews,
        description_html,
        updates,
        author_name: author,
        can_edit,
        allowed_transitions,
        can_revert,
        previous_lifecycle_label,
        i18n,
    };
    render(&tpl)
}

pub async fn detail_panel(
    OptionalUser(user): OptionalUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    if user.is_none() && !state.is_public_mode() {
        return Ok(Redirect::to("/login").into_response());
    }

    let ews = EventService::find_with_services(&state.pool, id).await?;
    let updates = EventRepository::list_updates_with_author(&state.pool, id).await?;

    let author = crate::repositories::UserRepository::find_by_id(&state.pool, ews.event.author_id)
        .await?
        .map_or_else(
            || i18n.t("date.unknown_author").to_string(),
            |u| u.display_name,
        );

    let can_edit = user.as_ref().is_some_and(|u| u.role.can_publish());
    let allowed_transitions = ews
        .event
        .lifecycle
        .map(|l| ews.event.kind.allowed_transitions(l).to_vec())
        .unwrap_or_default();
    let can_revert = can_edit && ews.event.previous_lifecycle.is_some();
    let previous_lifecycle_label = ews
        .event
        .previous_lifecycle
        .map(|l| i18n.t(l.i18n_key()).to_string());
    let description_html = sanitize_markdown(&ews.event.description);
    let tpl = EventDetailPanelTemplate {
        csrf_token: csrf_token.0,
        event: ews,
        description_html,
        updates,
        author_name: author,
        can_edit,
        allowed_transitions,
        can_revert,
        previous_lifecycle_label,
        i18n,
    };
    render(&tpl)
}

pub async fn new_form(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    let services = ServiceRepository::list_all(&state.pool).await?;
    let (user_display_name, is_admin, is_authenticated) = layout_fields_auth(&user);
    let unread_count = unread_auth(&state.pool, &user).await?;
    let last_admin_action = fetch_last_admin_action(&state.pool, &i18n).await?;
    let tpl = EventFormTemplate {
        csrf_token: csrf_token.0,
        user_display_name,
        is_admin,
        is_authenticated,
        unread_count,
        last_admin_action,
        error: None,
        services,
        event: None,
        i18n,
    };
    render(&tpl)
}

pub async fn create(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
    ValidatedForm(input): ValidatedForm<EventInput>,
) -> Result<Response, AppError> {
    let icon_id = parse_icon_id(input.icon_id);
    let planned_start = input
        .planned_start
        .as_deref()
        .and_then(parse_datetime_local);
    let planned_end = input.planned_end.as_deref().and_then(parse_datetime_local);
    let (severity, planned, category) = input.normalized();

    let create_input = CreateEventInput {
        kind: input.kind,
        severity,
        planned,
        category,
        title: input.title.clone(),
        description: input.description.clone(),
        planned_start,
        planned_end,
        service_ids: input.service_ids.clone(),
        icon_id,
        author_id: user.id,
    };

    let should_save_template = input.save_as_template.as_deref() == Some("on");

    match EventService::create(&state.pool, create_input).await {
        Ok(event) => {
            if should_save_template
                && let Err(e) = EventTemplateService::create(
                    &state.pool,
                    CreateTemplateParams {
                        title: &input.title,
                        description: &input.description,
                        kind: input.kind,
                        severity,
                        planned,
                        category,
                        icon_id,
                        created_by: user.id,
                    },
                )
                .await
            {
                tracing::warn!(error = %e, "Failed to save event template");
            }
            Ok(Redirect::to(&format!("/events/{}", event.id)).into_response())
        }
        Err(AppError::Validation(msg)) => {
            let services = ServiceRepository::list_all(&state.pool).await?;
            let (user_display_name, is_admin, is_authenticated) = layout_fields_auth(&user);
            let unread_count = unread_auth(&state.pool, &user).await?;
            let last_admin_action = fetch_last_admin_action(&state.pool, &i18n).await?;
            let tpl = EventFormTemplate {
                csrf_token: csrf_token.0,
                user_display_name,
                is_admin,
                is_authenticated,
                unread_count,
                last_admin_action,
                error: Some(i18n.t(&msg).to_string()),
                services,
                event: None,
                i18n,
            };
            render(&tpl)
        }
        Err(e) => Err(e),
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
    let services = ServiceRepository::list_all(&state.pool).await?;

    let service_ids: Vec<i64> = ews.services.iter().map(|s| s.id).collect();
    let (user_display_name, is_admin, is_authenticated) = layout_fields_auth(&user);
    let unread_count = unread_auth(&state.pool, &user).await?;
    let last_admin_action = fetch_last_admin_action(&state.pool, &i18n).await?;

    let tpl = EventFormTemplate {
        csrf_token: csrf_token.0,
        user_display_name,
        is_admin,
        is_authenticated,
        unread_count,
        last_admin_action,
        error: None,
        services,
        event: Some(EventFormData {
            id: ews.event.id,
            title: ews.event.title,
            description: ews.event.description,
            kind: ews.event.kind,
            severity: ews.event.severity,
            planned: ews.event.planned,
            category: ews.event.category,
            service_ids,
            planned_start: ews.event.planned_start.as_ref().map(format_datetime_local),
            planned_end: ews.event.planned_end.as_ref().map(format_datetime_local),
        }),
        i18n,
    };
    render(&tpl)
}

pub async fn update(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
    ValidatedForm(input): ValidatedForm<EventInput>,
) -> Result<Response, AppError> {
    let event = EventRepository::find_by_id(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;

    let is_active = event.lifecycle.is_none_or(Lifecycle::is_active);
    if !is_active && !user.role.can_admin() {
        return Err(AppError::Validation(
            i18n.t("validation.event_closed_admin_only").to_string(),
        ));
    }

    let icon_id = parse_icon_id(input.icon_id);
    let (severity, planned, category) = input.normalized();

    if input.title.trim().is_empty() {
        let services = ServiceRepository::list_all(&state.pool).await?;
        let (user_display_name, is_admin, is_authenticated) = layout_fields_auth(&user);
        let unread_count = unread_auth(&state.pool, &user).await?;
        let last_admin_action = fetch_last_admin_action(&state.pool, &i18n).await?;
        let tpl = EventFormTemplate {
            csrf_token: csrf_token.0,
            user_display_name,
            is_admin,
            is_authenticated,
            unread_count,
            last_admin_action,
            error: Some(i18n.t("validation.title_empty").to_string()),
            services,
            event: Some(EventFormData {
                id,
                title: input.title,
                description: input.description,
                kind: input.kind,
                severity,
                planned,
                category,
                service_ids: input.service_ids,
                planned_start: input.planned_start,
                planned_end: input.planned_end,
            }),
            i18n,
        };
        return render(&tpl);
    }

    let planned_start = input
        .planned_start
        .as_deref()
        .and_then(parse_datetime_local);
    let planned_end = input.planned_end.as_deref().and_then(parse_datetime_local);

    sqlx::query(
        "UPDATE events SET title = ?, description = ?, kind = ?, severity = ?, planned = ?, \
         category = ?, icon_id = ?, planned_start = ?, planned_end = ? WHERE id = ?",
    )
    .bind(&input.title)
    .bind(&input.description)
    .bind(input.kind)
    .bind(severity)
    .bind(planned)
    .bind(category)
    .bind(icon_id)
    .bind(planned_start)
    .bind(planned_end)
    .bind(id)
    .execute(&state.pool)
    .await?;

    let old_service_ids =
        EventRepository::update_services(&state.pool, id, &input.service_ids).await?;

    let mut all_service_ids = old_service_ids;
    for &sid in &input.service_ids {
        if !all_service_ids.contains(&sid) {
            all_service_ids.push(sid);
        }
    }
    for sid in all_service_ids {
        crate::services::ServiceService::recalculate_status(&state.pool, sid).await?;
    }

    Ok(Redirect::to(&format!("/events/{id}")).into_response())
}

pub async fn update_lifecycle(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Locale(i18n): Locale,
    Form(input): Form<LifecycleInput>,
) -> Result<Response, AppError> {
    if requires_resolution_comment(input.lifecycle) {
        let comment = input.resolution_comment.as_deref().unwrap_or("").trim();
        if comment.is_empty() {
            return Err(AppError::Validation(
                i18n.t("validation.resolution_required").to_string(),
            ));
        }
    }

    EventService::update_lifecycle(&state.pool, id, input.lifecycle, user.role).await?;

    if requires_resolution_comment(input.lifecycle)
        && let Some(ref comment) = input.resolution_comment
    {
        let trimmed = comment.trim();
        if !trimmed.is_empty() {
            let sanitized = sanitize_markdown(trimmed);
            EventRepository::add_update(&state.pool, id, &sanitized, user.id).await?;
        }
    }

    Ok(Redirect::to(&format!("/events/{id}")).into_response())
}

pub async fn delete(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Locale(_i18n): Locale,
) -> Result<Response, AppError> {
    EventService::delete(&state.pool, id, user.role).await?;
    Ok(Redirect::to("/events").into_response())
}

pub async fn revert_lifecycle(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Locale(_i18n): Locale,
) -> Result<Response, AppError> {
    EventService::revert_lifecycle(&state.pool, id, user.role).await?;
    Ok(Redirect::to(&format!("/events/{id}")).into_response())
}

pub async fn add_update(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Locale(_i18n): Locale,
    ValidatedForm(input): ValidatedForm<UpdateInput>,
) -> Result<Response, AppError> {
    EventService::add_update(&state.pool, id, &input.message, user.id, user.role).await?;
    Ok(Redirect::to(&format!("/events/{id}")).into_response())
}

fn parse_date(s: &str) -> Option<chrono::DateTime<Utc>> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| dt.and_utc())
}

/// Parse a `YYYY-MM-DD` date and return the UTC `DateTime` at 23:59:59.
fn parse_date_end_of_day(s: &str) -> Option<chrono::DateTime<Utc>> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(23, 59, 59))
        .map(|dt| dt.and_utc())
}

fn date_label(date: chrono::NaiveDate, i18n: &I18n) -> String {
    i18n.date_label(&date)
}

fn group_by_date(events: Vec<EventSummary>, i18n: &I18n) -> Vec<DateGroup> {
    let mut groups: Vec<DateGroup> = Vec::new();

    for event in events {
        let date = event.created_at.date_naive();
        let label = date_label(date, i18n);

        if let Some(last) = groups.last_mut()
            && last.label == label
        {
            last.events.push(event);
            continue;
        }
        groups.push(DateGroup {
            label,
            events: vec![event],
        });
    }

    groups
}

pub async fn history(
    OptionalUser(user): OptionalUser,
    State(state): State<AppState>,
    Query(params): Query<HistoryQuery>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    if user.is_none() && !state.is_public_mode() {
        return Ok(Redirect::to("/login").into_response());
    }

    let page = params.page.unwrap_or(1).max(1);
    let offset = (page - 1) * PAGE_SIZE;

    let from = params.from.as_deref().and_then(parse_date);
    let to = params.to.as_deref().and_then(parse_date_end_of_day);

    let filters = crate::models::EventFilters {
        kind: params.kind,
        service_id: params.service_id,
        from,
        to,
        limit: PAGE_SIZE + 1,
        offset,
        ..Default::default()
    };

    let mut events = EventRepository::list_by_filters(&state.pool, filters).await?;
    #[allow(clippy::cast_possible_wrap)]
    let has_next = events.len() as i64 > PAGE_SIZE;
    #[allow(clippy::cast_possible_truncation)]
    events.truncate(PAGE_SIZE as usize);

    let groups = group_by_date(events, &i18n);
    let services = ServiceRepository::list_all(&state.pool).await?;
    let (user_display_name, is_admin, is_authenticated) = layout_fields(user.as_ref());
    let unread_count = unread(&state.pool, user.as_ref()).await?;

    let filter_kind = params.kind.map(|k| k.as_str().to_string());
    let filter_service = params.service_id.map(|id| id.to_string());

    let mut base_url = String::from("/history?");
    {
        use std::fmt::Write;
        if let Some(ref t) = filter_kind {
            let _ = write!(base_url, "kind={t}&");
        }
        if let Some(ref s) = filter_service {
            let _ = write!(base_url, "service_id={s}&");
        }
        if let Some(ref f) = params.from {
            let _ = write!(base_url, "from={f}&");
        }
        if let Some(ref t) = params.to {
            let _ = write!(base_url, "to={t}&");
        }
    }

    let last_admin_action = fetch_last_admin_action(&state.pool, &i18n).await?;
    let tpl = HistoryTemplate {
        csrf_token: csrf_token.0,
        user_display_name,
        is_admin,
        is_authenticated,
        unread_count,
        last_admin_action,
        groups,
        page,
        has_next,
        filter_kind,
        filter_service,
        filter_from: params.from,
        filter_to: params.to,
        services,
        base_url,
        i18n,
    };
    render(&tpl)
}

fn html_escape_char(ch: char, out: &mut String) {
    match ch {
        '&' => out.push_str("&amp;"),
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        '"' => out.push_str("&quot;"),
        '\'' => out.push_str("&#x27;"),
        _ => out.push(ch),
    }
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        html_escape_char(ch, &mut out);
    }
    out
}

/// Wrap each occurrence of a search term in `<mark>`. The text is
/// HTML-escaped in the process; the template renders with `|safe`.
fn highlight_terms(title: &str, query: &str) -> String {
    let title_lower = title.to_lowercase();
    let terms: Vec<String> = query
        .split_whitespace()
        .map(str::to_lowercase)
        .filter(|t| !t.is_empty())
        .collect();

    if terms.is_empty() {
        return html_escape(title);
    }

    let mut result = String::with_capacity(title.len() * 2);
    let mut i = 0;
    let chars: Vec<char> = title.chars().collect();
    let chars_lower: Vec<char> = title_lower.chars().collect();

    while i < chars.len() {
        let mut matched = false;
        for term in &terms {
            let term_chars: Vec<char> = term.chars().collect();
            if i + term_chars.len() <= chars_lower.len()
                && chars_lower[i..i + term_chars.len()] == term_chars[..]
            {
                result.push_str(
                    "<mark class=\"bg-yellow-200 dark:bg-yellow-900/50 rounded px-0.5\">",
                );
                for ch in &chars[i..i + term_chars.len()] {
                    html_escape_char(*ch, &mut result);
                }
                result.push_str("</mark>");
                i += term_chars.len();
                matched = true;
                break;
            }
        }
        if !matched {
            html_escape_char(chars[i], &mut result);
            i += 1;
        }
    }

    result
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push('+'),
            _ => {
                use std::fmt::Write;
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

pub async fn search(
    OptionalUser(user): OptionalUser,
    State(state): State<AppState>,
    Query(params): Query<SearchQuery>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    if user.is_none() && !state.is_public_mode() {
        return Ok(Redirect::to("/login").into_response());
    }

    let query = params.q.unwrap_or_default();
    let page = params.page.unwrap_or(1).max(1);
    let offset = (page - 1) * PAGE_SIZE;

    let filters = crate::models::EventFilters {
        kind: params.kind,
        service_id: params.service_id,
        limit: PAGE_SIZE + 1,
        offset,
        ..Default::default()
    };

    let mut events = if query.trim().is_empty() {
        Vec::new()
    } else {
        EventRepository::search(&state.pool, &query, &filters).await?
    };

    #[allow(clippy::cast_possible_wrap)]
    let has_next = events.len() as i64 > PAGE_SIZE;
    #[allow(clippy::cast_possible_truncation)]
    events.truncate(PAGE_SIZE as usize);

    let results: Vec<SearchResult> = events
        .into_iter()
        .map(|e| {
            let highlighted_title = highlight_terms(&e.title, &query);
            SearchResult {
                event: e,
                highlighted_title,
            }
        })
        .collect();

    let services = ServiceRepository::list_all(&state.pool).await?;
    let (user_display_name, is_admin, is_authenticated) = layout_fields(user.as_ref());
    let unread_count = unread(&state.pool, user.as_ref()).await?;

    let filter_kind = params.kind.map(|k| k.as_str().to_string());
    let filter_service = params.service_id.map(|id| id.to_string());

    let mut base_url = String::from("/search?");
    {
        use std::fmt::Write;
        if !query.is_empty() {
            let encoded = url_encode(&query);
            let _ = write!(base_url, "q={encoded}&");
        }
        if let Some(ref t) = filter_kind {
            let _ = write!(base_url, "kind={t}&");
        }
        if let Some(ref s) = filter_service {
            let _ = write!(base_url, "service_id={s}&");
        }
    }

    let last_admin_action = fetch_last_admin_action(&state.pool, &i18n).await?;
    let tpl = SearchTemplate {
        csrf_token: csrf_token.0,
        user_display_name,
        is_admin,
        is_authenticated,
        unread_count,
        last_admin_action,
        query,
        results,
        page,
        has_next,
        filter_kind,
        filter_service,
        services,
        base_url,
        i18n,
    };
    render(&tpl)
}

#[derive(Template)]
#[template(path = "events/template_suggestions.html")]
struct TemplateSuggestionsTemplate {
    templates: Vec<crate::models::EventTemplate>,
    i18n: I18n,
}

#[derive(Deserialize)]
pub struct TemplateSearchQuery {
    q: Option<String>,
}

pub async fn template_search(
    _publisher: RequirePublisher,
    Locale(i18n): Locale,
    State(state): State<AppState>,
    Query(params): Query<TemplateSearchQuery>,
) -> Result<Response, AppError> {
    let query = params.q.unwrap_or_default();
    let templates = if query.trim().len() >= 2 {
        EventTemplateRepository::search_by_title(&state.pool, query.trim(), 5).await?
    } else {
        Vec::new()
    };

    let tpl = TemplateSuggestionsTemplate { templates, i18n };
    let html = tpl
        .render()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("template render error: {e}")))?;
    Ok(Html(html).into_response())
}

pub async fn template_detail(
    _publisher: RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let tpl = EventTemplateRepository::find_by_id(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;

    if let Err(e) = EventTemplateService::record_usage(&state.pool, id).await {
        tracing::warn!(error = %e, "Failed to increment template usage");
    }

    let icon_url = resolve_icon_url(&state.pool, tpl.icon_id).await?;

    let body = serde_json::json!({
        "title": tpl.title,
        "description": tpl.description,
        "kind": tpl.kind.as_str(),
        "severity": tpl.severity.map(Severity::as_str),
        "planned": tpl.planned,
        "category": tpl.category.map(Category::as_str),
        "icon_id": tpl.icon_id,
        "icon_url": icon_url,
    });

    Ok(axum::Json(body).into_response())
}

pub async fn template_delete(
    _publisher: RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    EventTemplateService::delete(&state.pool, id).await?;
    Ok(Redirect::to("/events/new").into_response())
}
