//! Service pages: the list with its status control, the form, deletion.

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;

use super::{Frame, render};
use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::{CsrfToken, HtmlForm, RequirePublisher};
use crate::models::{
    BUILTIN_ICONS, BuiltinIcon, Icon, Service, ServiceStatus, User, find_builtin_icon,
};
use crate::repositories::ServiceRepository;
use crate::services::{IconService, ServiceService};
use crate::state::AppState;

#[derive(Template)]
#[template(path = "services/list.html")]
struct ServiceListTemplate {
    frame: Frame,
    services: Vec<Service>,
    /// The list renders the status control at rest, never a receipt.
    previous: Option<ServiceStatus>,
    saved_id: Option<i64>,
    saved_name: Option<String>,
    deleted_name: Option<String>,
    error: Option<String>,
    i18n: I18n,
}

impl ServiceListTemplate {
    // Askama hands a field over by reference.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn is_saved(&self, id: &i64) -> bool {
        self.saved_id == Some(*id)
    }
}

#[derive(Deserialize)]
pub struct ListQuery {
    saved: Option<i64>,
    deleted: Option<String>,
}

async fn render_list(
    state: &AppState,
    user: &User,
    csrf_token: String,
    i18n: I18n,
    query: ListQuery,
    error: Option<String>,
) -> Result<Response, AppError> {
    let services = ServiceRepository::list_all(&state.pool).await?;
    let saved_name = query
        .saved
        .and_then(|id| services.iter().find(|s| s.id == id))
        .map(|s| s.name.clone());
    render(&ServiceListTemplate {
        frame: Frame::load(&state.pool, Some(user), csrf_token, &i18n).await?,
        services,
        previous: None,
        saved_id: query.saved,
        saved_name,
        deleted_name: query.deleted,
        error,
        i18n,
    })
}

pub async fn list(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    render_list(&state, &user, csrf_token.0, i18n, query, None).await
}

#[derive(Template)]
#[template(path = "services/form.html")]
struct ServiceFormTemplate {
    frame: Frame,
    error: Option<String>,
    name_error: Option<String>,
    edit_id: Option<i64>,
    form: ServiceFormData,
    selected_icon_id: Option<i64>,
    selected_icon_url: Option<String>,
    selected_icon_name: Option<String>,
    builtin_icons: &'static [BuiltinIcon],
    custom_icons: Vec<Icon>,
    upload_error: Option<String>,
    i18n: I18n,
}

impl ServiceFormTemplate {
    /// "Currently Operational." under the title of an edited service.
    fn status_line(&self) -> String {
        let status = self.i18n.t(self.form.status.i18n_key());
        self.i18n.tf("services.status_now", &[("status", status)])
    }

    fn is_builtin_selected(&self, name: &str) -> bool {
        self.selected_icon_name.as_deref() == Some(name)
    }

    /// The chosen icon, named for screen readers in the picker's summary.
    fn chosen_icon(&self) -> String {
        let builtin = self
            .selected_icon_name
            .as_deref()
            .and_then(find_builtin_icon)
            .map(|icon| self.i18n.t(icon.i18n_key()).to_string());
        let custom = || {
            self.selected_icon_id
                .and_then(|id| self.custom_icons.iter().find(|icon| icon.id == id))
                .map(|icon| icon.original_name.clone())
        };
        builtin
            .or_else(custom)
            .map(|name| self.i18n.tf("icons.current", &[("name", &name)]))
            .unwrap_or_default()
    }
}

pub struct ServiceFormData {
    pub name: String,
    pub description: String,
    pub status: ServiceStatus,
}

/// Everything a form page shows besides the frame.
struct FormPage {
    edit_id: Option<i64>,
    form: ServiceFormData,
    icon_id: Option<i64>,
    icon_name: Option<String>,
    error_key: Option<String>,
}

#[derive(Deserialize)]
pub struct ServiceInput {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    icon_id: Option<String>,
    #[serde(default)]
    icon_name: Option<String>,
}

impl ServiceInput {
    fn icon_id(&self) -> Option<i64> {
        self.icon_id
            .as_deref()
            .and_then(|v| v.parse().ok())
            .filter(|id| *id > 0)
    }

    fn icon_name(&self) -> Option<String> {
        self.icon_name.clone().filter(|name| !name.is_empty())
    }

    fn into_page(self, edit_id: Option<i64>, status: ServiceStatus, error_key: String) -> FormPage {
        FormPage {
            edit_id,
            icon_id: self.icon_id(),
            icon_name: self.icon_name(),
            form: ServiceFormData {
                name: self.name,
                description: self.description,
                status,
            },
            error_key: Some(error_key),
        }
    }
}

/// A form shown again after a refusal keeps what the author typed; a name
/// problem is said under the name field.
async fn render_form(
    state: &AppState,
    user: &User,
    csrf_token: String,
    i18n: I18n,
    page: FormPage,
) -> Result<Response, AppError> {
    let custom_icons = IconService::choosable(&state.pool, &state.upload_dir).await?;
    let selected_icon_url = page
        .icon_id
        .and_then(|id| custom_icons.iter().find(|icon| icon.id == id))
        .map(Icon::url);
    let message = page.error_key.as_deref().map(|key| i18n.t(key).to_string());
    let on_name = page
        .error_key
        .as_deref()
        .is_some_and(|key| key.starts_with("validation.service_name_"));
    let (error, name_error) = if on_name {
        (None, message)
    } else {
        (message, None)
    };
    render(&ServiceFormTemplate {
        frame: Frame::load(&state.pool, Some(user), csrf_token, &i18n).await?,
        error,
        name_error,
        edit_id: page.edit_id,
        form: page.form,
        selected_icon_id: page.icon_id.filter(|_| selected_icon_url.is_some()),
        selected_icon_url,
        selected_icon_name: page.icon_name,
        builtin_icons: BUILTIN_ICONS,
        custom_icons,
        upload_error: None,
        i18n,
    })
}

pub async fn new_form(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    let page = FormPage {
        edit_id: None,
        form: ServiceFormData {
            name: String::new(),
            description: String::new(),
            status: ServiceStatus::Operational,
        },
        icon_id: None,
        icon_name: None,
        error_key: None,
    };
    render_form(&state, &user, csrf_token.0, i18n, page).await
}

pub async fn create(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
    HtmlForm(input): HtmlForm<ServiceInput>,
) -> Result<Response, AppError> {
    let icon_name = input.icon_name();
    let result = ServiceService::create(
        &state.pool,
        &input.name,
        Some(input.description.as_str()),
        input.icon_id(),
        icon_name.as_deref(),
    )
    .await;
    match result {
        Ok(service) => Ok(Redirect::to(&format!("/services?saved={}", service.id)).into_response()),
        Err(AppError::Validation(key)) => {
            let page = input.into_page(None, ServiceStatus::Operational, key);
            render_form(&state, &user, csrf_token.0, i18n, page).await
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
    let service = ServiceRepository::find_by_id(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    let page = FormPage {
        edit_id: Some(id),
        icon_id: service.icon_id,
        icon_name: service.icon_name,
        form: ServiceFormData {
            name: service.name,
            description: service.description.unwrap_or_default(),
            status: service.status,
        },
        error_key: None,
    };
    render_form(&state, &user, csrf_token.0, i18n, page).await
}

pub async fn update(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
    HtmlForm(input): HtmlForm<ServiceInput>,
) -> Result<Response, AppError> {
    let icon_name = input.icon_name();
    let result = ServiceService::update(
        &state.pool,
        id,
        &input.name,
        Some(input.description.as_str()),
        input.icon_id(),
        icon_name.as_deref(),
    )
    .await;
    match result {
        Ok(()) => Ok(Redirect::to(&format!("/services?saved={id}")).into_response()),
        Err(AppError::Validation(key)) => {
            let status = ServiceRepository::find_by_id(&state.pool, id)
                .await?
                .ok_or(AppError::NotFound)?
                .status;
            let page = input.into_page(Some(id), status, key);
            render_form(&state, &user, csrf_token.0, i18n, page).await
        }
        Err(e) => Err(e),
    }
}

/// Deletes a service without history. A refusal is said in the list.
pub async fn delete(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    let service = ServiceRepository::find_by_id(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    match ServiceService::delete(&state.pool, id).await {
        Ok(()) => {
            let name = percent_encoding::utf8_percent_encode(
                &service.name,
                percent_encoding::NON_ALPHANUMERIC,
            );
            Ok(Redirect::to(&format!("/services?deleted={name}")).into_response())
        }
        Err(AppError::Validation(key)) => {
            let error = Some(i18n.t(&key).to_string());
            let query = ListQuery {
                saved: None,
                deleted: None,
            };
            render_list(&state, &user, csrf_token.0, i18n, query, error).await
        }
        Err(e) => Err(e),
    }
}

#[derive(Deserialize)]
pub struct StatusInput {
    status: String,
    /// Sent by the receipt's undo button, so putting a status back does not
    /// offer to put it back again.
    #[serde(default)]
    undo: Option<String>,
}

#[derive(Template)]
#[template(path = "components/status_selector.html")]
struct StatusSelectorFragment {
    frame: StatusFrame,
    service: Service,
    /// The status held a moment ago: turns the fragment into a receipt with
    /// an undo.
    previous: Option<ServiceStatus>,
    i18n: I18n,
}

/// The one field of the page frame the fragment reads.
struct StatusFrame {
    csrf_token: String,
}

/// Answers htmx with the refreshed cell; a plain form post goes back to the
/// list.
pub async fn update_status(
    _publisher: RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
    HtmlForm(input): HtmlForm<StatusInput>,
) -> Result<Response, AppError> {
    let status: ServiceStatus = input
        .status
        .parse()
        .map_err(|()| AppError::Validation("error.invalid_data".to_string()))?;
    let before = ServiceRepository::find_by_id(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?
        .manual_status;
    ServiceService::set_manual_status(&state.pool, id, status).await?;
    if !headers.contains_key("hx-request") {
        return Ok(Redirect::to(&format!("/services?saved={id}")).into_response());
    }
    let service = ServiceRepository::find_by_id(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    render(&StatusSelectorFragment {
        frame: StatusFrame {
            csrf_token: csrf_token.0,
        },
        previous: (input.undo.is_none() && before != status).then_some(before),
        service,
        i18n,
    })
}
