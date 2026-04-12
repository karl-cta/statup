//! Service routes - list, create, update, delete.

use askama::Template;
use axum::extract::{Path, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;
use validator::Validate;

use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::{CsrfToken, RequirePublisher, ValidatedForm};
use crate::models::{BUILTIN_ICONS, BuiltinIcon, ServiceStatus, User};
use crate::repositories::{EventRepository, IconRepository, ServiceRepository};
use crate::services::{EventService, ServiceService};
use crate::state::AppState;

#[derive(Template)]
#[template(path = "services/list.html")]
struct ServiceListTemplate {
    csrf_token: String,
    user_display_name: String,
    is_admin: bool,
    is_authenticated: bool,
    unread_count: i64,
    last_admin_action: Option<String>,
    services: Vec<crate::models::Service>,
    i18n: I18n,
}

#[derive(Template)]
#[template(path = "services/form.html")]
struct ServiceFormTemplate {
    csrf_token: String,
    user_display_name: String,
    is_admin: bool,
    is_authenticated: bool,
    unread_count: i64,
    last_admin_action: Option<String>,
    error: Option<String>,
    service: Option<ServiceFormData>,
    selected_icon_id: Option<i64>,
    selected_icon_url: Option<String>,
    selected_icon_name: Option<String>,
    builtin_icons: &'static [BuiltinIcon],
    i18n: I18n,
}

struct ServiceFormData {
    id: i64,
    name: String,
    description: String,
}

#[derive(Deserialize, Validate)]
pub struct ServiceInput {
    #[validate(length(min = 1, max = 100, message = "validation.service_name_required"))]
    name: String,
    #[validate(length(max = 500, message = "validation.description_max_length"))]
    description: Option<String>,
    icon_id: Option<i64>,
    icon_name: Option<String>,
}

fn render(tpl: &impl Template) -> Result<Response, AppError> {
    let html = tpl
        .render()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("template render error: {e}")))?;
    Ok(Html(html).into_response())
}

fn layout_fields(user: &User) -> (String, bool, bool) {
    (user.display_name.clone(), user.role.can_admin(), true)
}

async fn unread(pool: &crate::db::DbPool, user: &User) -> Result<i64, AppError> {
    EventService::unread_count(pool, user.last_seen_at).await
}

/// Resolve an `icon_id` to its URL path.
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

/// Parse `icon_id` from form: empty string or 0 → None.
fn parse_icon_id(input: Option<i64>) -> Option<i64> {
    input.filter(|&id| id > 0)
}

/// Parse `icon_name` from form: empty string → None.
fn parse_icon_name(input: Option<String>) -> Option<String> {
    input.filter(|s| !s.is_empty())
}

async fn fetch_last_admin_action(
    pool: &crate::db::DbPool,
    i18n: &I18n,
) -> Result<Option<String>, AppError> {
    Ok(EventRepository::last_admin_action(pool)
        .await?
        .map(|dt| i18n.format_datetime_long(&dt)))
}

pub async fn list(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    let (user_display_name, is_admin, is_authenticated) = layout_fields(&user);
    let unread_count = unread(&state.pool, &user).await?;
    let services = ServiceRepository::list_all_with_icons(&state.pool).await?;
    let last_admin_action = fetch_last_admin_action(&state.pool, &i18n).await?;
    let tpl = ServiceListTemplate {
        csrf_token: csrf_token.0,
        user_display_name,
        is_admin,
        is_authenticated,
        unread_count,
        last_admin_action,
        services,
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
    let (user_display_name, is_admin, is_authenticated) = layout_fields(&user);
    let unread_count = unread(&state.pool, &user).await?;
    let last_admin_action = fetch_last_admin_action(&state.pool, &i18n).await?;
    let tpl = ServiceFormTemplate {
        csrf_token: csrf_token.0,
        user_display_name,
        is_admin,
        is_authenticated,
        unread_count,
        last_admin_action,
        error: None,
        service: None,
        selected_icon_id: None,
        selected_icon_url: None,
        selected_icon_name: None,
        builtin_icons: BUILTIN_ICONS,
        i18n,
    };
    render(&tpl)
}

pub async fn create(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
    ValidatedForm(input): ValidatedForm<ServiceInput>,
) -> Result<Response, AppError> {
    let icon_id = parse_icon_id(input.icon_id);
    let icon_name = parse_icon_name(input.icon_name);
    match ServiceService::create(
        &state.pool,
        &input.name,
        input.description.as_deref(),
        icon_id,
        icon_name.as_deref(),
    )
    .await
    {
        Ok(_) => Ok(Redirect::to("/services").into_response()),
        Err(AppError::Validation(msg)) => {
            let (user_display_name, is_admin, is_authenticated) = layout_fields(&user);
            let unread_count = unread(&state.pool, &user).await?;
            let last_admin_action = fetch_last_admin_action(&state.pool, &i18n).await?;
            let icon_url = resolve_icon_url(&state.pool, icon_id).await?;
            let tpl = ServiceFormTemplate {
                csrf_token: csrf_token.0,
                user_display_name,
                is_admin,
                is_authenticated,
                unread_count,
                last_admin_action,
                error: Some(i18n.t(&msg).to_string()),
                service: None,
                selected_icon_id: icon_id,
                selected_icon_url: icon_url,
                selected_icon_name: icon_name,
                builtin_icons: BUILTIN_ICONS,
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
    let service = ServiceRepository::find_by_id_with_icon(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;

    let icon_url = service.icon_url();
    let icon_name = service.icon_name.clone();
    let (user_display_name, is_admin, is_authenticated) = layout_fields(&user);
    let unread_count = unread(&state.pool, &user).await?;
    let last_admin_action = fetch_last_admin_action(&state.pool, &i18n).await?;
    let tpl = ServiceFormTemplate {
        csrf_token: csrf_token.0,
        user_display_name,
        is_admin,
        is_authenticated,
        unread_count,
        last_admin_action,
        error: None,
        selected_icon_id: service.icon_id,
        selected_icon_url: icon_url,
        selected_icon_name: icon_name,
        builtin_icons: BUILTIN_ICONS,
        i18n,
        service: Some(ServiceFormData {
            id: service.id,
            name: service.name,
            description: service.description.unwrap_or_default(),
        }),
    };
    render(&tpl)
}

pub async fn update(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
    ValidatedForm(input): ValidatedForm<ServiceInput>,
) -> Result<Response, AppError> {
    let icon_id = parse_icon_id(input.icon_id);
    let icon_name = parse_icon_name(input.icon_name);
    match ServiceService::update(
        &state.pool,
        id,
        &input.name,
        input.description.as_deref(),
        icon_id,
        icon_name.as_deref(),
    )
    .await
    {
        Ok(()) => Ok(Redirect::to("/services").into_response()),
        Err(AppError::Validation(msg)) => {
            let (user_display_name, is_admin, is_authenticated) = layout_fields(&user);
            let unread_count = unread(&state.pool, &user).await?;
            let last_admin_action = fetch_last_admin_action(&state.pool, &i18n).await?;
            let icon_url = resolve_icon_url(&state.pool, icon_id).await?;
            let tpl = ServiceFormTemplate {
                csrf_token: csrf_token.0,
                user_display_name,
                is_admin,
                is_authenticated,
                unread_count,
                last_admin_action,
                error: Some(i18n.t(&msg).to_string()),
                service: Some(ServiceFormData {
                    id,
                    name: input.name,
                    description: input.description.unwrap_or_default(),
                }),
                selected_icon_id: icon_id,
                selected_icon_url: icon_url,
                selected_icon_name: icon_name,
                builtin_icons: BUILTIN_ICONS,
                i18n,
            };
            render(&tpl)
        }
        Err(e) => Err(e),
    }
}

pub async fn delete(
    _publisher: RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Locale(_i18n): Locale,
) -> Result<Response, AppError> {
    ServiceService::delete(&state.pool, id).await?;
    Ok(Redirect::to("/services").into_response())
}

#[derive(Deserialize)]
pub struct StatusInput {
    status: String,
}

#[derive(Template)]
#[template(path = "components/status_selector.html")]
struct StatusSelectorFragment {
    csrf_token: String,
    service: crate::models::Service,
    i18n: I18n,
}

pub async fn update_status(
    _publisher: RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
    axum::extract::Form(input): axum::extract::Form<StatusInput>,
) -> Result<Response, AppError> {
    let status: ServiceStatus = input
        .status
        .parse()
        .map_err(|e: String| AppError::Validation(e))?;

    ServiceRepository::find_by_id(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;

    ServiceRepository::update_status(&state.pool, id, status).await?;

    let service = ServiceRepository::find_by_id_with_icon(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;

    let tpl = StatusSelectorFragment {
        csrf_token: csrf_token.0,
        service,
        i18n,
    };
    let html = tpl
        .render()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("template render error: {e}")))?;
    Ok(Html(html).into_response())
}
