//! Service pages: the list with its status control, the form, deletion.

use std::collections::HashMap;

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;

use super::{Frame, members_only, render};
use crate::db::DbPool;
use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::headers::is_htmx;
use crate::middleware::{CsrfToken, HtmlForm, OptionalUser, RequirePublisher};
use crate::models::{
    BUILTIN_ICONS, BuiltinIcon, CheckKind, CheckedService, EventFilters, EventSummary, Icon,
    Service, ServiceCheck, ServiceStatus, Tone, User, find_builtin_icon,
};
use crate::modules::services::{ServiceRow, service_history};
use crate::repositories::{EventRepository, ServiceRepository};
use crate::services::{
    CHECK_TIMEOUT, Finding, IconService, Probes, Report, ServiceService, target_allowed,
};
use crate::state::AppState;

#[derive(Template)]
#[template(path = "services/list.html")]
struct ServiceListTemplate {
    frame: Frame,
    services: Vec<Service>,
    /// The event holding each service in its state, by service.
    drivers: HashMap<i64, (i64, String)>,
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

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn driver_of(&self, id: &i64) -> Option<(i64, String)> {
        self.drivers.get(id).cloned()
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
        drivers: EventRepository::state_drivers(&state.pool, None).await?,
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
    check_error: Option<String>,
    edit_id: Option<i64>,
    form: ServiceFormData,
    selected_icon_id: Option<i64>,
    selected_icon_url: Option<String>,
    selected_icon_name: Option<String>,
    builtin_icons: &'static [BuiltinIcon],
    custom_icons: Vec<Icon>,
    upload_error: Option<String>,
    /// A service no event ever named can be deleted from its page.
    deletable: bool,
    i18n: I18n,
}

impl ServiceFormTemplate {
    /// "Currently Operational." under the title of an edited service.
    fn status_line(&self) -> String {
        let status = self.i18n.t(self.form.status.i18n_key());
        self.i18n.tf("services.status_now", &[("status", status)])
    }

    /// Whether the form shows this check, `none` for no check.
    fn check_is(&self, kind: &str) -> bool {
        self.form.check_kind.map_or("none", CheckKind::as_str) == kind
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
    pub check_kind: Option<CheckKind>,
    pub check_url: String,
    pub check_address: String,
    pub check_internal_cert: bool,
}

impl ServiceFormData {
    fn empty() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            status: ServiceStatus::Operational,
            check_kind: None,
            check_url: String::new(),
            check_address: String::new(),
            check_internal_cert: false,
        }
    }

    /// An edited service as saved; its target fills the field of its kind.
    fn from_service(service: Service) -> Self {
        let target = service.check_target.unwrap_or_default();
        let (check_url, check_address) = match service.check_kind {
            Some(CheckKind::Http) => (target, String::new()),
            Some(CheckKind::Tcp) => (String::new(), target),
            None => (String::new(), String::new()),
        };
        Self {
            name: service.name,
            description: service.description.unwrap_or_default(),
            status: service.status,
            check_kind: service.check_kind,
            check_url,
            check_address,
            check_internal_cert: service.check_internal_cert,
        }
    }
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
    #[serde(default)]
    check_kind: String,
    #[serde(default)]
    check_url: String,
    #[serde(default)]
    check_address: String,
    #[serde(default)]
    check_internal_cert: Option<String>,
}

impl ServiceInput {
    fn check_kind(&self) -> Option<CheckKind> {
        self.check_kind.parse().ok()
    }

    /// What to check, read from the field of the chosen kind; the other one
    /// is ignored, so a page without script that sends both still works.
    fn check(&self) -> Result<Option<ServiceCheck>, AppError> {
        let Some(kind) = self.check_kind() else {
            return match self.check_kind.as_str() {
                "" | "none" => Ok(None),
                _ => Err(AppError::validation("error.invalid_data")),
            };
        };
        let target = match kind {
            CheckKind::Http => self.check_url.trim(),
            CheckKind::Tcp => self.check_address.trim(),
        };
        target_allowed(kind, target).map_err(AppError::validation)?;
        Ok(Some(ServiceCheck {
            kind,
            target: target.to_string(),
            internal_cert: kind == CheckKind::Http && self.check_internal_cert.is_some(),
        }))
    }

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
                check_kind: self.check_kind(),
                check_internal_cert: self.check_internal_cert.is_some(),
                name: self.name,
                description: self.description,
                status,
                check_url: self.check_url,
                check_address: self.check_address,
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
    let deletable = match page.edit_id {
        Some(id) => ServiceRepository::find_by_id(&state.pool, id)
            .await?
            .is_some_and(|service| !service.has_history),
        None => false,
    };
    let (error, name_error, check_error) = place_error(page.error_key.as_deref(), &i18n);
    render(&ServiceFormTemplate {
        frame: Frame::load(&state.pool, Some(user), csrf_token, &i18n).await?,
        error,
        name_error,
        check_error,
        edit_id: page.edit_id,
        form: page.form,
        selected_icon_id: page.icon_id.filter(|_| selected_icon_url.is_some()),
        selected_icon_url,
        selected_icon_name: page.icon_name,
        deletable,
        builtin_icons: BUILTIN_ICONS,
        custom_icons,
        upload_error: None,
        i18n,
    })
}

/// A refusal is said under its field when it has one: the name, the
/// address to check; anything else above the form.
fn place_error(key: Option<&str>, i18n: &I18n) -> (Option<String>, Option<String>, Option<String>) {
    let Some(key) = key else {
        return (None, None, None);
    };
    let message = Some(i18n.t(key).to_string());
    if key.starts_with("validation.service_name_") {
        (None, message, None)
    } else if key.starts_with("validation.check_") {
        (None, None, message)
    } else {
        (message, None, None)
    }
}

pub async fn new_form(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    let page = FormPage {
        edit_id: None,
        form: ServiceFormData::empty(),
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
    match save_new(&state.pool, &input).await {
        Ok(id) => Ok(Redirect::to(&format!("/services?saved={id}")).into_response()),
        Err(AppError::Validation(key)) => {
            let page = input.into_page(None, ServiceStatus::Operational, key);
            render_form(&state, &user, csrf_token.0, i18n, page).await
        }
        Err(e) => Err(e),
    }
}

/// The check is read first, so a refused address saves nothing.
async fn save_new(pool: &DbPool, input: &ServiceInput) -> Result<i64, AppError> {
    let check = input.check()?;
    let icon_name = input.icon_name();
    let service = ServiceService::create(
        pool,
        &input.name,
        Some(input.description.as_str()),
        input.icon_id(),
        icon_name.as_deref(),
    )
    .await?;
    ServiceService::set_check(pool, service.id, check.as_ref()).await?;
    Ok(service.id)
}

async fn save_edit(pool: &DbPool, id: i64, input: &ServiceInput) -> Result<(), AppError> {
    let check = input.check()?;
    let icon_name = input.icon_name();
    ServiceService::update(
        pool,
        id,
        &input.name,
        Some(input.description.as_str()),
        input.icon_id(),
        icon_name.as_deref(),
    )
    .await?;
    ServiceService::set_check(pool, id, check.as_ref()).await
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
        icon_name: service.icon_name.clone(),
        form: ServiceFormData::from_service(service),
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
    match save_edit(&state.pool, id, &input).await {
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

#[derive(Template)]
#[template(path = "services/_check_result.html")]
struct CheckResultFragment {
    /// The tone of the verdict; none when the address itself was refused.
    tone: Option<&'static str>,
    message: String,
    detail: Option<String>,
    i18n: I18n,
}

/// Runs the check the form describes, without saving anything, and says
/// what the automatic checks would make of it.
pub async fn test_check(
    _publisher: RequirePublisher,
    Locale(i18n): Locale,
    HtmlForm(input): HtmlForm<ServiceInput>,
) -> Result<Response, AppError> {
    let check = match input.check() {
        Ok(Some(check)) => check,
        Ok(None) => return Err(AppError::validation("error.invalid_data")),
        Err(AppError::Validation(key)) => {
            let message = i18n.t(&key).to_string();
            return render(&CheckResultFragment {
                tone: None,
                message,
                detail: None,
                i18n,
            });
        }
        Err(e) => return Err(e),
    };
    let probes = Probes::new(CHECK_TIMEOUT).map_err(anyhow::Error::from)?;
    let service = CheckedService {
        id: 0,
        kind: check.kind,
        target: check.target,
        internal_cert: check.internal_cert,
        detected_status: None,
    };
    let report = probes.examine(&service).await;
    let (tone, message) = verdict(&report, &i18n);
    render(&CheckResultFragment {
        tone: Some(tone.as_str()),
        message,
        detail: report.detail,
        i18n,
    })
}

/// What a test found, in the words of the form: green for what the checks
/// count as an answer, red for a failure.
fn verdict(report: &Report, i18n: &I18n) -> (Tone, String) {
    let ms = report.elapsed.as_millis().to_string();
    let failure = |key: &str| (Tone::Crit, i18n.t(key).to_string());
    match report.finding {
        Finding::Answered {
            status: Some(status),
        } => (
            Tone::Ok,
            i18n.tf(
                "services.check_result_web_ok",
                &[("status", &status.to_string()), ("ms", &ms)],
            ),
        ),
        Finding::Answered { status: None } => (
            Tone::Ok,
            i18n.tf("services.check_result_port_ok", &[("ms", &ms)]),
        ),
        Finding::ServerError(status) => (
            Tone::Crit,
            i18n.tf(
                "services.check_result_server_error",
                &[("status", &status.to_string())],
            ),
        ),
        Finding::Refused => failure("services.check_result_refused"),
        Finding::TimedOut => failure("services.check_result_timeout"),
        Finding::NameNotFound => failure("services.check_result_name"),
        Finding::Certificate => failure("services.check_result_certificate"),
        Finding::TooManyRedirects => failure("services.check_result_redirects"),
        Finding::Unreachable => failure("services.check_result_unreachable"),
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
    csrf_token: String,
    service: Service,
    /// The event holding the service in its state, if one does.
    driver: Option<(i64, String)>,
    /// The status held a moment ago: turns the fragment into a receipt with
    /// an undo.
    previous: Option<ServiceStatus>,
    i18n: I18n,
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
        .map_err(|()| AppError::validation("error.invalid_data"))?;
    let before = ServiceRepository::find_by_id(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?
        .manual_status;
    ServiceService::set_manual_status(&state.pool, id, status).await?;
    if !is_htmx(&headers) {
        return Ok(Redirect::to(&format!("/services?saved={id}")).into_response());
    }
    let service = ServiceRepository::find_by_id(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    render(&StatusSelectorFragment {
        csrf_token: csrf_token.0,
        previous: (input.undo.is_none() && before != status).then_some(before),
        driver: EventRepository::state_drivers(&state.pool, Some(id))
            .await?
            .remove(&id),
        service,
        i18n,
    })
}

/// How many of its events a service's side panel lists.
const PANEL_EVENTS: i64 = 3;

#[derive(Template)]
#[template(path = "services/drawer_content.html")]
struct ServiceDrawerTemplate {
    row: ServiceRow,
    events: Vec<EventSummary>,
    /// The checks found the service down and no open event explains it.
    detected: bool,
    i18n: I18n,
}

impl ServiceDrawerTemplate {
    fn event_day(&self, event: &EventSummary) -> String {
        self.i18n
            .format_date_short(&crate::clock::local_date(&event.created_at))
    }
}

/// A service as the side panel shows it to anyone who may read the page:
/// what it is, how it fared this month, its last events.
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
    let service = ServiceRepository::find_by_id(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    let filters = EventFilters {
        service_id: Some(id),
        limit: PANEL_EVENTS,
        ..EventFilters::default()
    };
    let events = EventRepository::list_page(&state.pool, &filters).await?;
    let explained = EventRepository::state_drivers(&state.pool, Some(id))
        .await?
        .contains_key(&id);
    let detected = service.detected_status.is_some() && !explained;
    let row = service_history(&state.pool, service, &i18n).await?;
    render(&ServiceDrawerTemplate {
        row,
        events,
        detected,
        i18n,
    })
}
