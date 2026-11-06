//! Service pages: the list with its status control, the form, deletion.

use std::collections::HashMap;

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use chrono::NaiveDate;

use crate::clock;
use serde::Deserialize;

use super::audience::{
    AccessBlock, Audience, VISIBILITY_CHOICE, VISIBILITY_NO_FOLDER, VISIBILITY_REQUIRED,
};
use super::checks::{CheckFields, CheckInput, OUTAGE_AFTER_INVALID};
use super::outages::{OutageLine, panel_outages};
use super::{Frame, members_only, render};
use crate::db::DbPool;
use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::headers::is_htmx;
use crate::middleware::{CsrfToken, HtmlForm, RequirePublisher, Viewer};
use crate::models::{
    BUILTIN_ICONS, BuiltinIcon, EventFilters, EventSummary, Icon, JournalKind, Service,
    ServiceFolder, ServiceStatus, User, find_builtin_icon,
};
use crate::modules::services::{DayCell, LONG_DAYS, ServiceRow, Strip, service_history, strips};
use crate::repositories::{EventRepository, FolderRepository, NewJournalEntry, ServiceRepository};
use crate::services::{
    DEFAULT_OUTAGE_AFTER, IconService, LastCheck, ServiceService, put_problems_first,
};
use crate::state::AppState;

#[derive(Template)]
#[template(path = "services/list.html")]
struct ServiceListTemplate {
    frame: Frame,
    /// The services under their folders, in the chosen order.
    groups: Vec<ServiceGroup>,
    service_count: usize,
    folder_count: usize,
    /// The event holding each service in its state, by service.
    drivers: HashMap<i64, (i64, String)>,
    /// The list renders the status control at rest, never a receipt.
    previous: Option<ServiceStatus>,
    saved_id: Option<i64>,
    saved_name: Option<String>,
    /// The delay of the saved service, when monitored: the notice says
    /// what that means.
    saved_monitored: Option<u8>,
    /// What the last check of each monitored service found.
    pulses: HashMap<i64, Pulse>,
    /// Each service's last ninety days, drawn small on its row.
    histories: HashMap<i64, Strip>,
    /// Services with a problem are listed first; the ordering mode puts
    /// back the chosen order, kept here as each service's place.
    problems_first: bool,
    chosen_places: HashMap<i64, usize>,
    deleted: bool,
    folder_deleted: bool,
    /// What an import from Uptime Kuma created.
    imported: Option<String>,
    new_folder: NewFolderForm,
    error: Option<String>,
    i18n: I18n,
}

impl ServiceListTemplate {
    // Askama hands a field over by reference.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn is_saved(&self, id: &i64) -> bool {
        self.saved_id == Some(*id)
    }

    /// What saving did, and for a monitored service, when it shows an outage.
    fn saved_notice(&self) -> String {
        match self.saved_monitored {
            Some(minutes) => self.i18n.tf(
                "services.saved_monitored_notice",
                &[("minutes", &minutes.to_string())],
            ),
            None => self.i18n.t("services.saved_notice").to_string(),
        }
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

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn chosen_place(&self, id: &i64) -> usize {
        self.chosen_places.get(id).copied().unwrap_or_default()
    }

    /// "Move Mail up": a button's name says which row it moves.
    fn move_label(&self, key: &str, name: &str) -> String {
        self.i18n.tf(key, &[("name", name)])
    }

    /// Arrows move services within their folder and folders among themselves.
    fn orderable(&self) -> bool {
        self.service_count > 1 || self.folder_count > 1
    }
}

/// A card of the list: the services of a folder, or of none, untitled.
pub(super) struct ServiceGroup {
    pub(super) folder: Option<ServiceFolder>,
    pub(super) services: Vec<Service>,
}

/// Those in no folder first, then each folder in its order, empty ones
/// included so that their page stays within reach.
pub(super) fn grouped(services: Vec<Service>, folders: Vec<ServiceFolder>) -> Vec<ServiceGroup> {
    let mut by_folder: HashMap<Option<i64>, Vec<Service>> = HashMap::new();
    for service in services {
        by_folder
            .entry(service.folder_id)
            .or_default()
            .push(service);
    }
    let loose = by_folder.remove(&None).unwrap_or_default();
    let first = (!loose.is_empty()).then_some(ServiceGroup {
        folder: None,
        services: loose,
    });
    let folders = folders.into_iter().map(|folder| ServiceGroup {
        services: by_folder.remove(&Some(folder.id)).unwrap_or_default(),
        folder: Some(folder),
    });
    first.into_iter().chain(folders).collect()
}

/// The panel of "New folder", open again with the refused name and why.
#[derive(Default)]
pub struct NewFolderForm {
    value: String,
    error: Option<String>,
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

#[derive(Deserialize, Default)]
pub struct ListQuery {
    saved: Option<i64>,
    deleted: Option<String>,
    folder_deleted: Option<String>,
    imported: Option<usize>,
    folders: Option<usize>,
}

impl ListQuery {
    /// "5 services imported. 2 folders created."
    fn imported(&self, i18n: &I18n) -> Option<String> {
        let services = self.imported.filter(|n| *n > 0)?;
        let mut text = i18n.plural("services.imported", services);
        if let Some(folders) = self.folders.filter(|n| *n > 0) {
            text.push(' ');
            text.push_str(&i18n.plural("services.folders_created", folders));
        }
        Some(text)
    }
}

async fn render_list(
    state: &AppState,
    user: &User,
    csrf_token: String,
    i18n: I18n,
    query: ListQuery,
    (error, new_folder): (Option<String>, NewFolderForm),
) -> Result<Response, AppError> {
    let mut services = ServiceRepository::list_all(&state.pool).await?;
    let chosen_places = services
        .iter()
        .enumerate()
        .map(|(place, service)| (service.id, place))
        .collect();
    let problems_first = ServiceService::problems_first(&state.pool).await?;
    if problems_first {
        put_problems_first(&mut services);
    }
    let saved = query
        .saved
        .and_then(|id| services.iter().find(|s| s.id == id));
    let saved_name = saved.map(|s| s.name.clone());
    let saved_monitored = saved
        .filter(|s| s.check_kind.is_some())
        .map(|s| s.outage_after_minutes);
    let pulses = services
        .iter()
        .filter(|service| service.check_kind.is_some())
        .map(|service| (service.id, Pulse::new(state.checks.get(service.id), &i18n)))
        .collect();
    let histories = strips(&state.pool, &services, LONG_DAYS, &i18n).await?;
    let folders = FolderRepository::list(&state.pool).await?;
    render(&ServiceListTemplate {
        frame: Frame::load(&state.pool, Some(user), csrf_token, &i18n).await?,
        service_count: services.len(),
        folder_count: folders.len(),
        groups: grouped(services, folders),
        drivers: EventRepository::state_drivers(&state.pool, None).await?,
        previous: None,
        saved_id: query.saved,
        saved_name,
        saved_monitored,
        pulses,
        histories,
        problems_first,
        chosen_places,
        deleted: query.deleted.is_some(),
        folder_deleted: query.folder_deleted.is_some(),
        imported: query.imported(&i18n),
        new_folder,
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
    let page = (None, NewFolderForm::default());
    render_list(&state, &user, csrf_token.0, i18n, query, page).await
}

/// The list again, with the panel of "New folder" open on a refused name.
pub(super) async fn new_folder_refused(
    state: &AppState,
    user: &User,
    (csrf_token, i18n): (String, I18n),
    key: &str,
    name: String,
) -> Result<Response, AppError> {
    let new_folder = NewFolderForm {
        error: Some(i18n.t(key).to_string()),
        value: name,
    };
    let page = (None, new_folder);
    render_list(state, user, csrf_token, i18n, ListQuery::default(), page).await
}

#[derive(Template)]
#[template(path = "services/form.html")]
struct ServiceFormTemplate {
    frame: Frame,
    error: Option<String>,
    name_error: Option<String>,
    check_error: Option<String>,
    outage_error: Option<String>,
    edit_id: Option<i64>,
    form: ServiceFormData,
    /// Who sees the service.
    access: AccessBlock,
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
    pub visibility: Audience,
}

/// Records a change of audience; a save that changes nothing leaves no
/// line. A line that cannot be written never fails the save it follows.
async fn record_audience_change(
    pool: &DbPool,
    user: &User,
    (id, name): (i64, &str),
    before: &Audience,
) {
    let after = match Audience::of_service(pool, id).await {
        Ok(after) => after,
        Err(error) => {
            tracing::warn!(%error, "Audience of a saved service not read");
            return;
        }
    };
    if *before == after {
        return;
    }
    NewJournalEntry::new(JournalKind::ServiceVisibilityChanged)
        .by(user)
        .about(name)
        .with_detail(after.detail())
        .record(pool)
        .await;
}

impl ServiceFormData {
    fn empty() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            status: ServiceStatus::Operational,
            check: CheckFields {
                outage_after: Some(DEFAULT_OUTAGE_AFTER.to_string()),
                ..CheckFields::default()
            },
            visibility: Audience::EVERYONE,
        }
    }

    /// An edited service as saved.
    fn from_service(service: Service, visibility: Audience) -> Self {
        Self {
            visibility,
            check: CheckFields {
                outage_after: Some(service.outage_after_minutes.to_string()),
                ..CheckFields::from_saved(
                    service.check_kind,
                    service.check_target.as_deref(),
                    service.check_internal_cert,
                )
            },
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
    #[serde(default)]
    visibility: String,
    #[serde(default)]
    group_ids: Vec<i64>,
    #[serde(default)]
    member_ids: Vec<i64>,
    #[serde(default)]
    inherit: Option<String>,
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

    fn audience(&self) -> Result<Audience, AppError> {
        Audience::from_form(
            &self.visibility,
            &self.group_ids,
            &self.member_ids,
            self.inherit.is_some(),
        )
    }

    fn into_page(self, edit_id: Option<i64>, status: ServiceStatus, error_key: String) -> FormPage {
        FormPage {
            edit_id,
            icon_id: self.icon_id(),
            icon_name: self.icon_name(),
            form: ServiceFormData {
                // Shown again only; a choice that does not parse saved nothing.
                visibility: self.audience().unwrap_or(Audience::EVERYONE),
                check: self.check.fields_and_delay(),
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
    let service = match page.edit_id {
        Some(id) => ServiceRepository::find_by_id(&state.pool, id).await?,
        None => None,
    };
    let deletable = service.as_ref().is_some_and(|service| !service.has_history);
    let folder = match service.and_then(|service| service.folder_id) {
        Some(id) => FolderRepository::find(&state.pool, id).await?,
        None => None,
    };
    let placed = place_error(page.error_key.as_deref(), &i18n);
    let audience = shown_audience(&state.pool, &page.form.visibility, folder.as_ref()).await?;
    let access = AccessBlock::load(
        &state.pool,
        &i18n,
        audience,
        state.is_public_mode(),
        folder.as_ref().map(|folder| folder.name.as_str()),
        placed.visibility,
    )
    .await?
    .for_viewer(user);
    render(&ServiceFormTemplate {
        frame: Frame::load(&state.pool, Some(user), csrf_token, &i18n).await?,
        error: placed.form,
        name_error: placed.name,
        check_error: placed.check,
        outage_error: placed.outage,
        edit_id: page.edit_id,
        form: page.form,
        access,
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

/// What the Visibility block shows: a service that inherits shows its
/// folder's setting, read only, also when a refused form comes back with
/// nothing but the box ticked.
async fn shown_audience(
    pool: &DbPool,
    posted: &Audience,
    folder: Option<&ServiceFolder>,
) -> Result<Audience, AppError> {
    match folder {
        Some(folder) if posted.inherit => Ok(Audience {
            inherit: true,
            ..Audience::of_folder(pool, folder.id).await?
        }),
        _ => Ok(posted.clone()),
    }
}

/// One message per place a refusal can be said: above the form, under the
/// name, under the address to check, under the delay, under who sees it.
#[derive(Default)]
struct PlacedError {
    form: Option<String>,
    name: Option<String>,
    check: Option<String>,
    outage: Option<String>,
    visibility: Option<String>,
}

/// A refusal is said under its field when it has one: the name, the
/// address to check, the delay, who sees it; anything else above the form.
fn place_error(key: Option<&str>, i18n: &I18n) -> PlacedError {
    let Some(key) = key else {
        return PlacedError::default();
    };
    let message = Some(i18n.t(key).to_string());
    if key.starts_with("validation.service_name_") {
        PlacedError {
            name: message,
            ..PlacedError::default()
        }
    } else if key.starts_with("validation.check_") {
        PlacedError {
            check: message,
            ..PlacedError::default()
        }
    } else if key == OUTAGE_AFTER_INVALID {
        PlacedError {
            outage: message,
            ..PlacedError::default()
        }
    } else if [VISIBILITY_REQUIRED, VISIBILITY_CHOICE, VISIBILITY_NO_FOLDER].contains(&key) {
        PlacedError {
            visibility: message,
            ..PlacedError::default()
        }
    } else {
        PlacedError {
            form: message,
            ..PlacedError::default()
        }
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
        Ok(service) => {
            NewJournalEntry::new(JournalKind::ServiceCreated)
                .by(&user)
                .about(&service.name)
                .record(&state.pool)
                .await;
            let saved = (service.id, service.name.as_str());
            record_audience_change(&state.pool, &user, saved, &Audience::EVERYONE).await;
            Ok(Redirect::to(&format!("/services?saved={}", service.id)).into_response())
        }
        Err(AppError::Validation(key)) => {
            let page = input.into_page(None, ServiceStatus::Operational, key);
            render_form(&state, &user, csrf_token.0, i18n, page).await
        }
        Err(e) => Err(e),
    }
}

/// The check and who sees it are read first, so a refusal saves nothing.
async fn save_new(pool: &DbPool, input: &ServiceInput) -> Result<Service, AppError> {
    let monitoring = input.check.monitoring()?;
    let audience = input.audience()?;
    audience.check(pool, false).await?;
    let icon_name = input.icon_name();
    let service = ServiceService::create_for(
        pool,
        &input.name,
        Some(input.description.as_str()),
        (input.icon_id(), icon_name.as_deref()),
        (audience.mode, audience.grants()),
    )
    .await?;
    ServiceService::set_monitoring(pool, service.id, monitoring.as_ref()).await?;
    Ok(service)
}

async fn save_edit(pool: &DbPool, id: i64, input: &ServiceInput) -> Result<(), AppError> {
    let folder_id = ServiceRepository::find_by_id(pool, id)
        .await?
        .ok_or(AppError::NotFound)?
        .folder_id;
    let monitoring = input.check.monitoring()?;
    let audience = input.audience()?;
    audience.check(pool, folder_id.is_some()).await?;
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
    ServiceService::set_monitoring(pool, id, monitoring.as_ref()).await?;
    audience.save_for_service(pool, id, folder_id).await?;
    Ok(())
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
    let audience = Audience::of_service(&state.pool, id).await?;
    let page = FormPage {
        edit_id: Some(id),
        icon_id: service.icon_id,
        icon_name: service.icon_name.clone(),
        form: ServiceFormData::from_service(service, audience),
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
    let before = Audience::of_service(&state.pool, id).await?;
    match save_edit(&state.pool, id, &input).await {
        Ok(()) => {
            let name = input.name.trim();
            NewJournalEntry::new(JournalKind::ServiceUpdated)
                .by(&user)
                .about(name)
                .record(&state.pool)
                .await;
            record_audience_change(&state.pool, &user, (id, name), &before).await;
            Ok(Redirect::to(&format!("/services?saved={id}")).into_response())
        }
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
            NewJournalEntry::new(JournalKind::ServiceDeleted)
                .by(&user)
                .about(&service.name)
                .record(&state.pool)
                .await;
            Ok(Redirect::to("/services?deleted=1").into_response())
        }
        Err(AppError::Validation(key)) => {
            let page = (Some(i18n.t(&key).to_string()), NewFolderForm::default());
            render_list(
                &state,
                &user,
                csrf_token.0,
                i18n,
                ListQuery::default(),
                page,
            )
            .await
        }
        Err(e) => Err(e),
    }
}

#[derive(Deserialize)]
pub struct OrderForm {
    #[serde(default)]
    order: Vec<i64>,
}

/// Saves the order set by the page's script, which reloads the page on a
/// refusal: another tab or member changed the list meanwhile.
pub async fn save_order(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    HtmlForm(form): HtmlForm<OrderForm>,
) -> Result<Response, AppError> {
    if !ServiceRepository::reorder(&state.pool, &form.order).await? {
        return Err(AppError::validation("error.invalid_data"));
    }
    NewJournalEntry::new(JournalKind::ServicesReordered)
        .by(&user)
        .record(&state.pool)
        .await;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Deserialize)]
pub struct ProblemsFirstForm {
    #[serde(default)]
    enabled: bool,
}

/// Turns on or off listing services with a problem first, from the
/// ordering mode's checkbox.
pub async fn save_problems_first(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    HtmlForm(form): HtmlForm<ProblemsFirstForm>,
) -> Result<Response, AppError> {
    ServiceService::set_problems_first(&state.pool, form.enabled).await?;
    NewJournalEntry::new(JournalKind::ProblemsFirstChanged)
        .by(&user)
        .with_detail(if form.enabled { "on" } else { "off" })
        .record(&state.pool)
        .await;
    Ok(StatusCode::NO_CONTENT.into_response())
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

async fn record_status_set(
    pool: &DbPool,
    user: &User,
    name: &str,
    status: ServiceStatus,
    is_undo: bool,
) {
    let detail = if is_undo {
        format!("{}, undo", status.as_str())
    } else {
        status.as_str().to_string()
    };
    NewJournalEntry::new(JournalKind::ServiceStatusSet)
        .by(user)
        .about(name)
        .with_detail(detail)
        .record(pool)
        .await;
}

/// Answers htmx with the refreshed cell; a plain form post goes back to the
/// list.
pub async fn update_status(
    RequirePublisher(user): RequirePublisher,
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
    let current = ServiceRepository::find_by_id(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    let before = current.manual_status;
    ServiceService::set_manual_status(&state.pool, id, status).await?;
    let is_undo = input.undo.is_some();
    record_status_set(&state.pool, &user, &current.name, status, is_undo).await;
    if !is_htmx(&headers) {
        return Ok(Redirect::to(&format!("/services?saved={id}")).into_response());
    }
    let service = ServiceRepository::find_by_id(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    render(&StatusSelectorFragment {
        csrf_token: csrf_token.0,
        previous: (!is_undo && before != status).then_some(before),
        driver: EventRepository::state_drivers(&state.pool, Some(id))
            .await?
            .remove(&id),
        service,
        i18n,
    })
}

/// The day a service's panel opens on, from a click on its row.
#[derive(Deserialize)]
pub struct PanelQuery {
    day: Option<String>,
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
    outages: Vec<OutageLine>,
    /// The day the panel opened on.
    day: Option<NaiveDate>,
    can_edit: bool,
    csrf_token: String,
    i18n: I18n,
}

impl ServiceDrawerTemplate {
    fn is_selected(&self, day: &DayCell) -> bool {
        day.outage_day.is_some() && day.outage_day == self.day
    }

    fn shows_outages(&self) -> bool {
        self.outages.iter().any(|outage| outage.shown)
    }

    fn event_day(&self, event: &EventSummary) -> String {
        self.i18n
            .format_date_short(&crate::clock::local_date(&event.created_at))
    }
}

/// A service as the side panel shows it to anyone who may read the page:
/// what it is, how it fared this month, its last events.
pub async fn drawer_content(
    viewer: Viewer,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<PanelQuery>,
    csrf_token: CsrfToken,
    headers: HeaderMap,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    if let Some(redirect) = members_only(&state, viewer.user.as_ref(), &headers) {
        return Ok(redirect);
    }
    if !viewer.scope.sees_service(id) {
        return Err(AppError::NotFound);
    }
    let day = query.day.as_deref().and_then(clock::parse_day);
    service_panel(&state, &viewer, id, day, csrf_token.0, i18n).await
}

/// The side panel of a service, opened on `day` when one was clicked.
pub(super) async fn service_panel(
    state: &AppState,
    viewer: &Viewer,
    id: i64,
    day: Option<NaiveDate>,
    csrf_token: String,
    i18n: I18n,
) -> Result<Response, AppError> {
    let service = ServiceRepository::find_by_id(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    let filters = EventFilters {
        service_id: Some(id),
        limit: PANEL_EVENTS,
        ..EventFilters::default()
    };
    let events = EventRepository::list_page(&state.pool, &filters, &viewer.scope).await?;
    let detected = service.detected_status.is_some()
        && !EventRepository::state_drivers(&state.pool, Some(id))
            .await?
            .contains_key(&id);
    let outages = panel_outages(&state.pool, id, &service.name, viewer, day, &i18n).await?;
    let row = service_history(&state.pool, service, &i18n).await?;
    render(&ServiceDrawerTemplate {
        row,
        events,
        detected,
        outages,
        day,
        can_edit: viewer.user.as_ref().is_some_and(|u| u.role.can_publish()),
        csrf_token,
        i18n,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(body: &str) -> ServiceInput {
        serde_html_form::from_str(body).unwrap()
    }

    #[test]
    fn groups_ticked_beside_the_monitoring_fields_are_read() {
        let form = input(
            "name=Paie&check_kind=http&check_url=https%3A%2F%2Fpaie&visibility=groups&group_ids=3&group_ids=7",
        );
        assert_eq!(form.group_ids, vec![3, 7]);
        assert!(form.check.monitoring().is_ok());
    }

    #[test]
    fn members_and_inherit_are_read_beside_the_groups() {
        let form = input("visibility=groups&group_ids=3&member_ids=5&member_ids=6&inherit=on");
        assert_eq!(form.member_ids, vec![5, 6]);
        assert!(form.audience().unwrap().inherit);
    }
}
