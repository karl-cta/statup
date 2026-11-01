//! The status embed: the banner of the page, small, for an intranet, a wiki
//! or a support portal to show in a frame. The frame is on another site, so
//! the browser sends it no session: the private link stands for a member,
//! and without it the embed shows what a visitor sees.

use std::collections::HashSet;

use askama::Template;
use axum::extract::{Query, State};
use axum::http::HeaderValue;
use axum::http::header::{CACHE_CONTROL, VARY};
use axum::response::Response;
use serde::Deserialize;

use super::page::render;
use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::modules::status_banner::{BannerRow, PageState};
use crate::services::{EmbedService, member_scope, scope_of};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct EmbedQuery {
    key: Option<String>,
    theme: Option<String>,
}

/// Light unless asked otherwise: most intranets are light, and the frame
/// cannot see the colours of the page around it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EmbedTheme {
    Light,
    Dark,
    Device,
}

impl EmbedTheme {
    fn parse(value: Option<&str>) -> Self {
        match value {
            Some("dark") => Self::Dark,
            Some("auto") => Self::Device,
            _ => Self::Light,
        }
    }
}

/// What the embed says: one line of state, one line of detail.
struct EmbedView {
    tone: &'static str,
    /// The banner's tint, while a service is disrupted or under maintenance.
    alert: bool,
    title: String,
    more: Option<String>,
    details: Vec<String>,
}

#[derive(Template)]
#[template(path = "embed.html")]
struct EmbedTemplate {
    view: EmbedView,
    theme: EmbedTheme,
    i18n: I18n,
}

impl EmbedTemplate {
    fn is_dark(&self) -> bool {
        self.theme == EmbedTheme::Dark
    }

    fn follows_device(&self) -> bool {
        self.theme == EmbedTheme::Device
    }
}

pub async fn show(
    State(state): State<AppState>,
    Locale(i18n): Locale,
    Query(query): Query<EmbedQuery>,
) -> Result<Response, AppError> {
    let keyed = match query.key.as_deref() {
        Some(sent) => EmbedService::grants(&state.pool, sent).await?,
        None => false,
    };
    let scope = if keyed {
        Some(member_scope(&state.pool).await?)
    } else if state.is_public_mode() {
        Some(scope_of(&state.pool, None).await?)
    } else {
        None
    };
    let view = match scope {
        Some(scope) => view_of(
            PageState::load(&state.pool, &scope, &i18n, false).await?,
            &i18n,
        ),
        None => locked(&i18n),
    };
    let template = EmbedTemplate {
        view,
        theme: EmbedTheme::parse(query.theme.as_deref()),
        i18n,
    };
    Ok(with_cache_headers(render(&template)?, keyed))
}

/// Each refresh asks the server again; what the private link opens stays in
/// the reader's own cache. The words follow the reader's language.
fn with_cache_headers(mut response: Response, keyed: bool) -> Response {
    let cache = if keyed {
        "private, no-cache"
    } else {
        "no-cache"
    };
    let headers = response.headers_mut();
    headers.insert(CACHE_CONTROL, HeaderValue::from_static(cache));
    headers.insert(VARY, HeaderValue::from_static("Accept-Language"));
    response
}

/// A members-only page asked without its private link: no state at all,
/// and a word for whoever pasted an old link.
fn locked(i18n: &I18n) -> EmbedView {
    EmbedView {
        tone: "neutral",
        alert: false,
        title: i18n.t("dashboard.title").to_string(),
        more: None,
        details: vec![i18n.t("embed.relink").to_string()],
    }
}

fn view_of(state: PageState, i18n: &I18n) -> EmbedView {
    if !state.has_services {
        let key = if state.nothing_visible {
            "banner.nothing_visible"
        } else {
            "banner.nothing_watched"
        };
        return EmbedView {
            tone: "neutral",
            alert: false,
            title: i18n.t(key).to_string(),
            more: None,
            details: Vec::new(),
        };
    }
    match state.rows.first() {
        Some(first) => troubled(&state, first, i18n),
        None => calm(state, i18n),
    }
}

/// Everything works: the page's name, or the maintenance announced.
fn calm(state: PageState, i18n: &I18n) -> EmbedView {
    let details = match state.next_maintenance {
        Some(next) => vec![next.when],
        None => vec![crate::brand_name(), i18n.t("dashboard.title").to_string()],
    };
    EmbedView {
        tone: "ok",
        alert: false,
        title: state.headline,
        more: None,
        details,
    }
}

/// The worst row leads: its event's title, or the service when nothing
/// explains it yet; the count of the other problems follows.
fn troubled(state: &PageState, first: &BannerRow, i18n: &I18n) -> EmbedView {
    let others = problems(&state.rows).saturating_sub(1);
    let mut details = if state.rows.len() > 1 {
        vec![state.headline.clone()]
    } else if first.cause.is_some() {
        vec![first.service.clone(), first.state.clone()]
    } else {
        vec![first.state.clone()]
    };
    details.extend(freshness(first, i18n));
    EmbedView {
        tone: state.tone.as_str(),
        alert: true,
        title: first
            .cause
            .as_ref()
            .map_or_else(|| first.service.clone(), |cause| cause.title.clone()),
        more: (others > 0).then(|| i18n.plural("embed.more", others)),
        details,
    }
}

/// How fresh the news is: until when a maintenance runs, when an incident
/// was last updated, or since when the checks find a service down.
fn freshness(row: &BannerRow, i18n: &I18n) -> Option<String> {
    if let Some(cause) = &row.cause {
        let latest = cause
            .latest
            .as_ref()
            .map(|latest| i18n.tf("events.last_news", &[("ago", &latest.when)]));
        return if row.tone == "info" {
            cause.since.clone().or(latest)
        } else {
            latest.or_else(|| cause.since.clone())
        };
    }
    row.detected
        .as_ref()
        .and_then(|detected| detected.since.clone())
}

/// Distinct problems: one per open event, and one per service nothing
/// explains, so an incident on three services counts once.
fn problems(rows: &[BannerRow]) -> usize {
    let events: HashSet<i64> = rows
        .iter()
        .filter_map(|row| row.cause.as_ref().map(|cause| cause.event_id))
        .collect();
    events.len() + rows.iter().filter(|row| row.cause.is_none()).count()
}
