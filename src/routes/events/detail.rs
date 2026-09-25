//! An event's page and its side panel.

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::Response;
use serde::Deserialize;

use super::form::MaintenanceChoice;
use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::{CsrfToken, OptionalUser};
use crate::models::{Event, Kind, Lifecycle, Service, ServiceTag, User};
use crate::repositories::EventRepository;
use crate::routes::timeline::{self, TimelineEntry};
use crate::routes::{Frame, members_only, render};
use crate::services::{EventService, can_modify, sanitize_markdown};
use crate::state::AppState;

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
pub(super) struct EventDetailTemplate {
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
pub(super) struct DrawerTemplate {
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
    /// An announcement reads as a text, its updates after it.
    pub article: Option<TimelineEntry>,
    pub timeline: Vec<TimelineEntry>,
}

impl EventView {
    pub fn service_tags(&self) -> Vec<ServiceTag> {
        self.services.iter().map(ServiceTag::from).collect()
    }

    /// The heading of an announcement's text: its category.
    pub fn article_title(&self, i18n: &I18n) -> String {
        self.event
            .qualifier(i18n)
            .unwrap_or_else(|| i18n.t(self.event.kind.i18n_key()).to_string())
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
            .map(|m| {
                MaintenanceChoice::new(id, m.title, &m.ended_at.unwrap_or(m.updated_at), i18n)
            }),
        None => None,
    };
    let mut timeline = timeline::build(
        &ews.event,
        sanitize_markdown(&ews.event.description),
        author,
        updates,
        user,
        i18n,
    );
    let article = (ews.event.kind == Kind::Publication)
        .then(|| timeline.iter().position(|entry| entry.update_id.is_none()))
        .flatten()
        .map(|opening| timeline.remove(opening));
    Ok(EventView {
        follows,
        article,
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

pub(super) async fn detail_page(
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

pub(super) async fn drawer_page(
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
