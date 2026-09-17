//! The status page and the signed-in dashboard, built from modules, and the
//! page that explains how to follow updates.

use std::hash::{DefaultHasher, Hash, Hasher};

use askama::Template;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderName, HeaderValue, header};
use axum::response::Response;
use chrono::Utc;

use super::{Frame, members_only, render};
use crate::clock;
use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::{CsrfToken, OptionalUser};
use crate::models::User;
use crate::modules::{ColumnWidth, ModuleContext, ModuleRenderContext};
use crate::repositories::{ServiceRepository, UserRepository};
use crate::services::DashboardLayoutService;
use crate::state::AppState;

/// Banner and module row, the part the page refreshes on its own.
pub struct LiveModules {
    pub banner_html: Option<String>,
    pub row: Vec<RenderedModule>,
    /// Changes when what the modules show changes, the time stamp aside.
    pub version: String,
}

pub struct RenderedModule {
    pub html: String,
    pub is_wide: bool,
}

#[derive(Template)]
#[template(path = "dashboard/index.html")]
struct DashboardTemplate {
    frame: Frame,
    live: LiveModules,
    has_services: bool,
    i18n: I18n,
}

#[derive(Template)]
#[template(path = "dashboard/_live.html")]
struct LiveTemplate {
    live: LiveModules,
}

fn context_for(user: Option<&User>) -> ModuleContext {
    if user.is_some() {
        ModuleContext::Admin
    } else {
        ModuleContext::Public
    }
}

async fn render_modules(
    state: &AppState,
    user: Option<&User>,
    i18n: &I18n,
) -> Result<LiveModules, AppError> {
    let ctx = ModuleRenderContext {
        pool: &state.pool,
        user,
        i18n,
    };
    let mut live = LiveModules {
        banner_html: None,
        row: Vec::new(),
        version: String::new(),
    };
    for item in DashboardLayoutService::resolve(&state.pool, context_for(user)).await? {
        if !item.enabled {
            continue;
        }
        let html = item.module.render(&ctx).await?;
        match item.module.column_width() {
            ColumnWidth::Full => live.banner_html = Some(html),
            width => live.row.push(RenderedModule {
                html,
                is_wide: width == ColumnWidth::Wide,
            }),
        }
    }
    live.version = live_version(&live);
    Ok(live)
}

/// A refresh that brings nothing new leaves the page as the reader left it:
/// the script compares this value before replacing anything.
fn live_version(live: &LiveModules) -> String {
    let mut hasher = DefaultHasher::new();
    let modules = live
        .banner_html
        .iter()
        .chain(live.row.iter().map(|m| &m.html));
    for html in modules {
        without_stamp(html).hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

/// The markup around the banner's "updated at" text, which changes every
/// minute.
fn without_stamp(html: &str) -> (&str, &str) {
    const MARK: &str = "data-stamp>";
    let Some(start) = html.find(MARK).map(|i| i + MARK.len()) else {
        return (html, "");
    };
    let end = html[start..].find('<').map_or(html.len(), |i| start + i);
    (&html[..start], &html[end..])
}

pub async fn index(
    OptionalUser(user): OptionalUser,
    State(state): State<AppState>,
    headers: HeaderMap,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    if let Some(redirect) = members_only(&state, user.as_ref(), &headers) {
        return Ok(redirect);
    }
    let live = render_modules(&state, user.as_ref(), &i18n).await?;
    if headers.contains_key("hx-request") {
        return live_response(live);
    }
    let frame = Frame::load(&state.pool, user.as_ref(), csrf_token.0, &i18n).await?;
    if let Some(u) = &user {
        UserRepository::mark_seen(&state.pool, u.id).await;
    }
    let has_services = ServiceRepository::any(&state.pool).await?;
    render(&DashboardTemplate {
        frame,
        live,
        has_services,
        i18n,
    })
}

/// The refreshed fragment, with what the script needs to decide: whether
/// the content changed, and whether the instance clock moved (daylight
/// saving).
fn live_response(live: LiveModules) -> Result<Response, AppError> {
    let version = HeaderValue::from_str(&live.version).map_err(|e| AppError::Internal(e.into()))?;
    let offset = HeaderValue::from(clock::offset_minutes(&Utc::now()));
    let mut response = render(&LiveTemplate { live })?;
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(HeaderName::from_static("x-live-version"), version);
    headers.insert(HeaderName::from_static("x-clock-offset"), offset);
    Ok(response)
}

#[derive(Template)]
#[template(path = "subscribe.html")]
struct SubscribeTemplate {
    frame: Frame,
    feed_url: String,
    feed_readable: bool,
    i18n: I18n,
}

/// How to follow updates without opening the page: the feed address and
/// where to paste it.
pub async fn subscribe(
    OptionalUser(user): OptionalUser,
    State(state): State<AppState>,
    headers: HeaderMap,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    if let Some(redirect) = members_only(&state, user.as_ref(), &headers) {
        return Ok(redirect);
    }
    render(&SubscribeTemplate {
        frame: Frame::load(&state.pool, user.as_ref(), csrf_token.0, &i18n).await?,
        feed_url: format!("{}/feed", origin(&state, &headers)),
        feed_readable: state.is_public_mode(),
        i18n,
    })
}

/// Address visitors use: `PUBLIC_URL`, or the host they asked for.
fn origin(state: &AppState, headers: &HeaderMap) -> String {
    if let Some(url) = &state.public_url {
        return url.clone();
    }
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");
    let https = state.trust_proxy_headers
        && headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|proto| proto.eq_ignore_ascii_case("https"));
    format!("{}://{host}", if https { "https" } else { "http" })
}
