//! The status embed: the banner of the page, small, for an intranet, a wiki
//! or a support portal to show in a frame. The frame is on another site, so
//! the browser sends it no session: the private link stands for a member,
//! and without it the embed shows what a visitor sees.

use std::collections::HashSet;

use askama::Template;
use axum::extract::{Query, State};
use axum::http::header::{CACHE_CONTROL, VARY};
use axum::http::{HeaderMap, HeaderValue};
use axum::response::{Redirect, Response};
use serde::Deserialize;

use super::dashboard::origin;
use super::{Frame, render};
use crate::error::AppError;
use crate::i18n::{I18n, Locale, language};
use crate::middleware::headers::{DYNAMIC_CACHE_CONTROL, no_store};
use crate::middleware::{CsrfToken, RequireAdmin};
use crate::models::{JournalKind, Scope};
use crate::modules::status_banner::{BannerRow, PageState};
use crate::repositories::NewJournalEntry;
use crate::services::{EmbedService, member_scope, page_address, scope_of};
use crate::state::AppState;

/// The height of the frame, in pixels, in the code to paste: two lines.
const EMBED_HEIGHT: u32 = 68;

#[derive(Deserialize)]
pub struct EmbedQuery {
    key: Option<String>,
    theme: Option<String>,
    /// The language chosen when the address was copied: some tools frame
    /// the widget with a language of their own, not the reader's.
    lang: Option<String>,
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
    const ALL: [Self; 3] = [Self::Light, Self::Dark, Self::Device];

    fn parse(value: Option<&str>) -> Self {
        match value {
            Some("dark") => Self::Dark,
            Some("auto") => Self::Device,
            _ => Self::Light,
        }
    }

    /// The value in an address; light is the default and needs none.
    fn query(self) -> Option<&'static str> {
        match self {
            Self::Light => None,
            Self::Dark => Some("dark"),
            Self::Device => Some("auto"),
        }
    }

    fn is_dark(self) -> bool {
        self == Self::Dark
    }

    fn follows_device(self) -> bool {
        self == Self::Device
    }

    fn label_key(self) -> &'static str {
        match self {
            Self::Light => "nav.theme_light",
            Self::Dark => "nav.theme_dark",
            Self::Device => "embed.theme_device",
        }
    }
}

/// What the embed says: one line of state, one line of detail.
struct EmbedView {
    tone: &'static str,
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

impl EmbedView {
    /// The banner's tint, while a service is disrupted or under maintenance.
    fn alert(&self) -> bool {
        !matches!(self.tone, "ok" | "neutral")
    }
}

pub async fn show(
    State(state): State<AppState>,
    Locale(i18n): Locale,
    Query(query): Query<EmbedQuery>,
) -> Result<Response, AppError> {
    let i18n = query
        .lang
        .as_deref()
        .filter(|code| language(code).is_some())
        .map_or(i18n, I18n::new);
    let keyed = match query.key.as_deref() {
        Some(sent) => EmbedService::grants(&state.pool, sent).await?,
        None => false,
    };
    let view = if keyed {
        load_view(&state, &member_scope(&state.pool).await?, &i18n).await?
    } else if state.is_public_mode() {
        load_view(&state, &scope_of(&state.pool, None).await?, &i18n).await?
    } else {
        locked(&i18n)
    };
    let template = EmbedTemplate {
        view,
        theme: EmbedTheme::parse(query.theme.as_deref()),
        i18n,
    };
    Ok(with_cache_headers(render(&template)?, keyed))
}

#[derive(Template)]
#[template(path = "admin/embed.html")]
struct EmbedSettingsTemplate {
    frame: Frame,
    theme: EmbedTheme,
    /// The embed itself, framed in the page: the preview is what the
    /// intranet shows, colours and night variants included.
    preview: String,
    height: u32,
    address: String,
    code: String,
    changed: bool,
    i18n: I18n,
}

/// One segment of the theme choice: a link to the page with that theme.
struct ThemeChoice {
    label: String,
    href: String,
    current: bool,
}

impl EmbedSettingsTemplate {
    fn height_hint(&self) -> String {
        self.i18n
            .tf("embed.height", &[("px", &EMBED_HEIGHT.to_string())])
    }

    fn theme_choices(&self) -> Vec<ThemeChoice> {
        EmbedTheme::ALL
            .into_iter()
            .map(|theme| ThemeChoice {
                label: self.i18n.t(theme.label_key()).to_string(),
                href: theme.query().map_or_else(
                    || "/admin/embed".to_string(),
                    |q| format!("/admin/embed?theme={q}"),
                ),
                current: theme == self.theme,
            })
            .collect()
    }
}

#[derive(Deserialize)]
pub struct SettingsQuery {
    theme: Option<String>,
    changed: Option<String>,
}

/// The embed as the intranet will show it, and what to paste there. The
/// page carries the private link, so no cache keeps it.
pub async fn settings(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
    headers: HeaderMap,
    Query(query): Query<SettingsQuery>,
) -> Result<Response, AppError> {
    let theme = EmbedTheme::parse(query.theme.as_deref());
    let key = EmbedService::key(&state.pool).await?;
    let base = match page_address(&state.pool, state.public_url.as_deref()).await? {
        Some(address) => address,
        None => origin(&state, &headers),
    };
    let address = embed_address(&base, &key, i18n.locale(), theme);
    let code = format!(
        r#"<iframe src="{}" title="{}" width="100%" height="{EMBED_HEIGHT}" style="border:0"></iframe>"#,
        attribute(&address),
        attribute(i18n.t("dashboard.title")),
    );
    let frame = Frame::load(&state.pool, Some(&admin), csrf_token, &i18n).await?;
    Ok(no_store(render(&EmbedSettingsTemplate {
        frame,
        theme,
        preview: embed_address("", &key, i18n.locale(), theme),
        height: EMBED_HEIGHT,
        address,
        code,
        changed: query.changed.is_some(),
        i18n,
    })?))
}

/// A new private link: every embed pasted with the old one stops showing
/// the state at once.
pub async fn change_link(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
) -> Result<Redirect, AppError> {
    EmbedService::regenerate(&state.pool).await?;
    NewJournalEntry::new(JournalKind::EmbedLinkChanged)
        .by(&admin)
        .record(&state.pool)
        .await;
    Ok(Redirect::to("/admin/embed?changed=1"))
}

/// A value for an attribute of the code to paste, with the named entities
/// an administrator reads at a glance.
fn attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
}

fn embed_address(base: &str, key: &str, lang: &str, theme: EmbedTheme) -> String {
    let address = format!("{base}/embed?key={key}&lang={lang}");
    match theme.query() {
        Some(theme) => format!("{address}&theme={theme}"),
        None => address,
    }
}

async fn load_view(state: &AppState, scope: &Scope, i18n: &I18n) -> Result<EmbedView, AppError> {
    Ok(view_of(
        PageState::load(&state.pool, scope, i18n, false).await?,
        i18n,
    ))
}

/// Each refresh asks the server again; what the private link opens stays in
/// the reader's own cache. The words follow the reader's language.
fn with_cache_headers(mut response: Response, keyed: bool) -> Response {
    let cache = if keyed {
        DYNAMIC_CACHE_CONTROL
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
