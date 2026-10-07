//! Service pages: the list with its status control, the form, deletion.

use std::collections::HashMap;

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;

use super::checks::{CheckFields, CheckInput};
use super::{Frame, members_only, render};
use crate::db::DbPool;
use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::headers::is_htmx;
use crate::middleware::{CsrfToken, HtmlForm, OptionalUser, RequirePublisher};
use crate::models::{
    BUILTIN_ICONS, BuiltinIcon, EventFilters, EventSummary, Icon, Service, ServiceStatus, User,
    find_builtin_icon,
};
use crate::modules::services::{LONG_DAYS, ServiceRow, Strip, service_history, strips};
use crate::repositories::{EventRepository, ServiceRepository};
use crate::services::{IconService, LastCheck, ServiceService};
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
    /// The saved service is monitored: the notice says what that means.
    saved_monitored: bool,
    /// What the last check of each monitored service found.
    pulses: HashMap<i64, Pulse>,
    /// Each service's last ninety days, drawn small on its row.
    histories: HashMap<i64, Strip>,
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

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn pulse_of(&self, id: &i64) -> Option<Pulse> {
        self.pulses.get(id).cloned()
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn history_of(&self, id: &i64) -> Option<&Strip> {
        self.histories.get(id)
    }
}

/// The last check of a monitored service, as its row says it: the latency,
/// "no answer", or nothing before the first round; and the words a screen
/// reader says instead.
#[derive(Clone)]
struct Pulse {
    text: Option<String>,
    label: String,
}

impl Pulse {
    fn new(last: Option<LastCheck>, i18n: &I18n) -> Self {
        match last {
            Some(LastCheck::Answered(latency)) => {
                let ms = latency.as_millis().to_string();
                Self {
                    text: Some(i18n.tf("services.check_latency", &[("ms", &ms)])),
                    label: i18n.tf("services.check_aria_latency", &[("ms", &ms)]),
                }
            }
            Some(LastCheck::Failed) => Self {
                text: Some(i18n.t("services.check_no_answer").to_string()),
                label: i18n.t("services.check_aria_no_answer").to_string(),
            },
            None => Self {
                text: None,
                label: i18n.t("services.check_aria_pending").to_string(),
            },
        }
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
    let saved = query
        .saved
        .and_then(|id| services.iter().find(|s| s.id == id));
    let saved_name = saved.map(|s| s.name.clone());
    let saved_monitored = saved.is_some_and(|s| s.check_kind.is_some());
    let pulses = services
        .iter()
        .filter(|service| service.check_kind.is_some())
        .map(|service| (service.id, Pulse::new(state.checks.get(service.id), &i18n)))
        .collect();
    let histories = strips(&state.pool, &services, LONG_DAYS, &i18n).await?;
    render(&ServiceListTemplate {
        frame: Frame::load(&state.pool, Some(user), csrf_token, &i18n).await?,
        services,
        drivers: EventRepository::state_drivers(&state.pool, None).await?,
        previous: None,
        saved_id: query.saved,
        saved_name,
        saved_monitored,
        pulses,
        histories,
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

    /// The monitoring fields, as the shared block reads them.
    fn check(&self) -> &CheckFields {
        &self.form.check
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
    pub check: CheckFields,
}

impl ServiceFormData {
    fn empty() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            status: ServiceStatus::Operational,
            check: CheckFields::default(),
        }
    }

    /// An edited service as saved.
    fn from_service(service: Service) -> Self {
        Self {
            check: CheckFields::from_saved(
                service.check_kind,
                service.check_target.as_deref(),
                service.check_internal_cert,
            ),
            name: service.name,
            description: service.description.unwrap_or_default(),
            status: service.status,
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
    #[serde(flatten)]
    check: CheckInput,
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
                check: self.check.fields(),
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
    let check = input.check.check()?;
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
    let check = input.check.check()?;
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
    can_edit: bool,
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
    let detected = service.detected_status.is_some()
        && !EventRepository::state_drivers(&state.pool, Some(id))
            .await?
            .contains_key(&id);
    let row = service_history(&state.pool, service, &i18n).await?;
    render(&ServiceDrawerTemplate {
        row,
        events,
        detected,
        can_edit: user.is_some_and(|u| u.role.can_publish()),
        i18n,
    })
}
