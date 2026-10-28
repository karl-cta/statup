//! The forms that create and change an event.

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;

use crate::clock;
use crate::db::DbPool;
use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::{CsrfToken, HtmlForm, RequirePublisher};
use crate::models::{
    Category, CreateEventInput, Event, EventWithServices, JournalKind, Kind, Lifecycle, NextUpdate,
    Rhythm, SEARCH_FROM, SEPARATOR, Scope, Service, Severity, UpdateEventInput, User,
};
use crate::repositories::{
    CreateTemplateInput, EventRepository, EventTemplateRepository, FolderRepository,
    NewJournalEntry, OutageRepository, SeriesRepository, ServiceRepository,
};
use crate::routes::services::{ServiceGroup, grouped};
use crate::routes::{Frame, render};
use crate::services::{
    EventService, EventTemplateService, SeriesService, can_modify, read_next_update,
};
use crate::state::AppState;

/// How many recent maintenances the announcement form offers.
const MAINTENANCE_CHOICES: i64 = 20;

/// How many saved templates a new event offers to start from.
const TEMPLATE_CHOICES: i64 = 30;

#[derive(Deserialize)]
pub struct EventInput {
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default, deserialize_with = "super::deserialize_blank_as_none")]
    kind: Option<Kind>,
    #[serde(default, deserialize_with = "super::deserialize_blank_as_none")]
    severity: Option<Severity>,
    #[serde(default, deserialize_with = "super::deserialize_blank_as_none")]
    category: Option<Category>,
    #[serde(default)]
    service_ids: Vec<i64>,
    #[serde(default)]
    save_as_template: Option<String>,
    #[serde(default, deserialize_with = "super::deserialize_blank_as_none")]
    template_id: Option<i64>,
    #[serde(default)]
    planned_start: String,
    #[serde(default)]
    planned_end: String,
    #[serde(default)]
    started_at: String,
    #[serde(default, deserialize_with = "super::deserialize_blank_as_none")]
    opening_step: Option<Lifecycle>,
    #[serde(default)]
    keeps_services_up: Option<String>,
    #[serde(default, deserialize_with = "super::deserialize_blank_as_none")]
    follows_event_id: Option<i64>,
    #[serde(default)]
    next_update: String,
    #[serde(default)]
    repeats: Option<String>,
    #[serde(default)]
    repeat: String,
    #[serde(default)]
    also_following: Option<String>,
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

    /// Only a maintenance may leave its services up.
    fn keeps_services_up(&self, kind: Kind) -> bool {
        kind == Kind::Maintenance && self.keeps_services_up.is_some()
    }

    /// When an incident's first update is due; only an incident has one.
    fn next_update(&self, kind: Kind) -> Result<Option<NextUpdate>, AppError> {
        if kind != Kind::Incident {
            return Ok(None);
        }
        read_next_update(&self.next_update)
    }

    /// How an announced maintenance repeats, when it does. Every week
    /// unless the month is chosen.
    fn rhythm(&self, kind: Kind) -> Option<Rhythm> {
        (self.planned(kind) && self.repeats.is_some())
            .then(|| Rhythm::parse(&self.repeat).unwrap_or(Rhythm::Weekly))
    }

    /// The step an incident is declared at; only an incident has one.
    fn opening_step(&self, kind: Kind) -> Option<Lifecycle> {
        (kind == Kind::Incident)
            .then_some(self.opening_step)
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
    /// The step an incident being declared opens at.
    pub opening_step: Option<Lifecycle>,
    /// A maintenance being announced that leaves its services up.
    pub keeps_services_up: bool,
    pub follows_event_id: Option<i64>,
    /// When an incident being declared will have its first update.
    pub next_update: String,
    /// How a maintenance being announced repeats, when it does.
    pub repeat: Option<Rhythm>,
    /// Whether the edit of a session goes to the next ones too; `None`
    /// when the event is not a session of a maintenance that repeats.
    pub following: Option<bool>,
}

/// A maintenance an announcement may follow, dated so that two with the
/// same title tell apart.
pub struct MaintenanceChoice {
    pub id: i64,
    pub title: String,
    pub finished_on: String,
}

impl MaintenanceChoice {
    pub(super) fn new(
        id: i64,
        title: String,
        finished: &chrono::DateTime<chrono::Utc>,
        i18n: &I18n,
    ) -> Self {
        Self {
            id,
            title,
            finished_on: i18n.format_date_short(&clock::local_date(finished)),
        }
    }

    /// The title and the day, as a plain-text option reads them.
    pub fn label(&self) -> String {
        format!("{}{SEPARATOR}{}", self.title, self.finished_on)
    }
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
            opening_step: None,
            keeps_services_up: false,
            follows_event_id: None,
            next_update: String::new(),
            repeat: None,
            following: None,
        }
    }

    /// A critical incident on one service, like the outage the page shows,
    /// begun when the checks first failed.
    fn outage_on(service_id: i64, started: Option<chrono::DateTime<chrono::Utc>>) -> Self {
        Self {
            severity: Some(Severity::Critical),
            service_ids: vec![service_id],
            started_at: started
                .as_ref()
                .map(clock::format_input)
                .unwrap_or_default(),
            ..Self::blank(Kind::Incident)
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
            opening_step: None,
            keeps_services_up: false,
            follows_event_id: Some(ews.event.id),
            next_update: String::new(),
            repeat: None,
            following: None,
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
            opening_step: None,
            keeps_services_up: false,
            follows_event_id: event.follows_event_id,
            next_update: String::new(),
            repeat: None,
            following: None,
        }
    }

    fn from_input(input: EventInput, kind: Kind, planned: bool) -> Self {
        Self {
            repeat: input.rhythm(kind),
            severity: input.severity(kind),
            category: input.category(kind),
            opening_step: input.opening_step(kind),
            keeps_services_up: input.keeps_services_up(kind),
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
            next_update: input.next_update,
            following: None,
        }
    }

    // The template hands a field over by reference.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn follows(&self, id: &i64) -> bool {
        self.follows_event_id == Some(*id)
    }

    /// Opens the extra options when they already hold a choice.
    fn options_in_use(&self) -> bool {
        self.follows_event_id.is_some()
            || !self.started_at.is_empty()
            || !self.next_update.is_empty()
            || self
                .opening_step
                .is_some_and(|step| step != Lifecycle::Investigating)
    }

    /// An incident is declared under investigation unless chosen otherwise.
    fn opening_step_is(&self, step: &str) -> bool {
        self.opening_step
            .unwrap_or(Lifecycle::Investigating)
            .as_str()
            == step
    }

    fn offers_following(&self) -> bool {
        self.following.is_some()
    }

    fn also_following(&self) -> bool {
        self.following == Some(true)
    }

    fn repeat_is(&self, rhythm: &str) -> bool {
        self.repeat.is_some_and(|r| r.as_str() == rhythm)
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
    /// The services under their folders, those in none first.
    sections: Vec<ServiceGroup>,
    service_count: usize,
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

impl EventFormTemplate {
    /// A folder's box starts checked when the event names all its services.
    fn folder_checked(&self, section: &ServiceGroup) -> bool {
        section
            .services
            .iter()
            .all(|service| self.form.service_ids.contains(&service.id))
    }

    fn searchable(&self) -> bool {
        self.service_count > SEARCH_FROM
    }
}

/// The services under their folders; a folder with none is left out. The
/// folders are only read when a service is in one.
async fn service_sections(
    pool: &DbPool,
    services: Vec<Service>,
) -> Result<Vec<ServiceGroup>, AppError> {
    let folders = if services.iter().any(|service| service.folder_id.is_some()) {
        FolderRepository::list(pool).await?
    } else {
        Vec::new()
    };
    Ok(grouped(services, folders)
        .into_iter()
        .filter(|section| !section.services.is_empty())
        .collect())
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
    let services = ServiceRepository::list_all(&state.pool).await?;
    render(&EventFormTemplate {
        frame: Frame::load(&state.pool, Some(user), csrf_token, &i18n).await?,
        service_count: services.len(),
        sections: service_sections(&state.pool, services).await?,
        maintenances: maintenance_choices(state, form.follows_event_id, &i18n).await?,
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
    i18n: &I18n,
) -> Result<Vec<MaintenanceChoice>, AppError> {
    let mut choices: Vec<MaintenanceChoice> =
        EventRepository::list_finished_maintenance(&state.pool, MAINTENANCE_CHOICES, &Scope::All)
            .await?
            .into_iter()
            .map(|m| {
                MaintenanceChoice::new(m.id, m.title, &m.ended_at.unwrap_or(m.updated_at), i18n)
            })
            .collect();
    if let Some(id) = followed
        && !choices.iter().any(|c| c.id == id)
        && let Some(event) = EventRepository::find_by_id(&state.pool, id).await?
    {
        let finished = event.ended_at.unwrap_or(event.updated_at);
        choices.insert(0, MaintenanceChoice::new(id, event.title, &finished, i18n));
    }
    Ok(choices)
}

#[derive(Deserialize)]
pub struct NewFormQuery {
    #[serde(default, deserialize_with = "super::deserialize_blank_as_none")]
    kind: Option<Kind>,
    /// A maintenance the announcement follows: the form opens on what is
    /// new after it.
    after: Option<i64>,
    /// A service the checks found down: the form opens on its incident.
    service: Option<i64>,
}

pub async fn new_form(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Query(query): Query<NewFormQuery>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    let followed = match query.after {
        Some(id) => EventRepository::find_with_services(&state.pool, id, &Scope::All)
            .await?
            .filter(|ews| ews.event.kind == Kind::Maintenance),
        None => None,
    };
    let outage = match query.service {
        Some(id) => detected_outage(&state, id).await?,
        None => None,
    };
    let form = match (followed, outage) {
        (Some(ews), _) => EventFormData::changelog_after(&ews, &i18n),
        (None, Some(form)) => form,
        (None, None) => EventFormData::blank(query.kind.unwrap_or(Kind::Incident)),
    };
    render_form(&state, &user, csrf_token.0, i18n, None, form, None).await
}

/// The incident on a service the checks found down, dated from the start of
/// the outage; nothing for a service that does not exist.
async fn detected_outage(
    state: &AppState,
    service_id: i64,
) -> Result<Option<EventFormData>, AppError> {
    if ServiceRepository::find_by_id(&state.pool, service_id)
        .await?
        .is_none()
    {
        return Ok(None);
    }
    let started = OutageRepository::open_starts(&state.pool)
        .await?
        .remove(&service_id);
    Ok(Some(EventFormData::outage_on(service_id, started)))
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
    let created = match input.next_update(kind) {
        Ok(next_update) => {
            let event = CreateEventInput {
                kind,
                severity: input.severity(kind),
                planned,
                category: input.category(kind),
                title: input.title.trim().to_string(),
                description: input.description.trim().to_string(),
                planned_start: input.start().filter(|_| planned),
                planned_end: input.end().filter(|_| kind == Kind::Maintenance),
                started_at: input.began(kind),
                opening_step: input.opening_step(kind),
                keeps_services_up: input.keeps_services_up(kind),
                service_ids: input.service_ids.clone(),
                follows_event_id: input.follows_event_id,
                author_id: user.id,
                api_key_id: None,
                next_update,
            };
            match input.rhythm(kind) {
                Some(rhythm) => EventService::create_series(&state.pool, event, rhythm).await,
                None => EventService::create(&state.pool, event).await,
            }
        }
        Err(e) => Err(e),
    };
    match created {
        Ok(event) => {
            NewJournalEntry::new(JournalKind::EventCreated)
                .by(&user)
                .about(&event.title)
                .record(&state.pool)
                .await;
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
    match EventTemplateService::create(&state.pool, template).await {
        Ok(saved) => {
            NewJournalEntry::new(JournalKind::TemplateSaved)
                .by(user)
                .about(&saved.title)
                .record(&state.pool)
                .await;
        }
        Err(e) => tracing::warn!(error = %e, "Failed to save event template"),
    }
}

pub async fn edit_form(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    let ews = EventService::find_with_services(&state.pool, id, &Scope::All).await?;
    if !can_modify(&ews.event, user.role) {
        return Err(AppError::refused("validation.event_closed_admin_only"));
    }
    let service_ids = ews.services.iter().map(|s| s.id).collect();
    let mut form = EventFormData::from_event(&ews.event, service_ids);
    form.following = repeats(&state, &ews.event).await?.then_some(false);
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
    let event = EventService::find_with_services(&state.pool, id, &Scope::All)
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
    let title = changes.title.clone();
    let offers_following = repeats(&state, &event).await?;
    let also_following = offers_following && input.also_following.is_some();
    let saved = if also_following {
        SeriesService::update_with_following(&state.pool, id, changes, user.role).await
    } else {
        EventService::update(&state.pool, id, changes, user.role).await
    };
    match saved {
        Ok(()) => {
            let kind = if also_following {
                JournalKind::RepeatChanged
            } else {
                JournalKind::EventEdited
            };
            NewJournalEntry::new(kind)
                .by(&user)
                .about(&title)
                .record(&state.pool)
                .await;
            Ok(Redirect::to(&format!("/events/{id}")).into_response())
        }
        Err(AppError::Validation(key)) => {
            let mut form = EventFormData::from_input(input, kind, planned);
            form.start_locked = kind == Kind::Maintenance && event.started_at.is_some();
            form.following = offers_following.then_some(also_following);
            let error = Some(i18n.t(&key).to_string());
            render_form(&state, &user, csrf_token.0, i18n, Some(id), form, error).await
        }
        Err(e) => Err(e),
    }
}

/// Whether the event is a session of a maintenance that still repeats.
async fn repeats(state: &AppState, event: &Event) -> Result<bool, AppError> {
    let Some(series_id) = event.series_id else {
        return Ok(false);
    };
    let series = SeriesRepository::find(&state.pool, series_id).await?;
    Ok(series.is_some_and(|series| series.stopped_at.is_none()))
}

#[cfg(test)]
mod tests {
    use super::*;

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
