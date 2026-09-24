//! The status page and the signed-in dashboard, built from modules, and the
//! page that explains how to follow updates.

use std::hash::{DefaultHasher, Hash, Hasher};

use askama::Template;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, header};
use axum::response::Response;
use chrono::Utc;
use serde::Deserialize;

use super::{Frame, members_only, render};
use crate::clock;
use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::{CsrfToken, OptionalUser};
use crate::models::User;
use crate::modules::{ColumnWidth, ModuleRenderContext};
use crate::repositories::{ServiceRepository, UserRepository};
use crate::services::DashboardLayoutService;
use crate::state::AppState;

/// Banner and module row, the part the page refreshes on its own.
pub struct LiveModules {
    pub banner_html: Option<String>,
    pub row: Vec<RenderedModule>,
    /// Modules an administrator may add back, empty for everyone else.
    pub hidden: Vec<ModuleChoice>,
    /// Whether the reader may arrange this dashboard.
    pub arrangeable: bool,
    /// Changes when what the modules show changes, the time stamp aside.
    pub version: String,
}

pub struct RenderedModule {
    pub html: String,
    pub module_id: &'static str,
    pub name: String,
    pub width: &'static str,
}

pub struct ModuleChoice {
    pub module_id: &'static str,
    pub name: String,
}

#[derive(Template)]
#[template(path = "dashboard/index.html")]
struct DashboardTemplate {
    frame: Frame,
    live: LiveModules,
    has_services: bool,
    /// Opened in arrange mode.
    arranging: bool,
    i18n: I18n,
}

#[derive(Template)]
#[template(path = "dashboard/_live.html")]
struct LiveTemplate {
    live: LiveModules,
    i18n: I18n,
}

#[derive(Deserialize)]
pub struct DashboardQuery {
    /// `1` opens the arrange mode.
    #[serde(default)]
    arrange: String,
}

async fn render_modules(
    state: &AppState,
    user: Option<&User>,
    page_address: &str,
    i18n: &I18n,
) -> Result<LiveModules, AppError> {
    let ctx = ModuleRenderContext {
        pool: &state.pool,
        user,
        i18n,
        page_address,
    };
    let mut live = LiveModules {
        banner_html: None,
        row: Vec::new(),
        hidden: Vec::new(),
        arrangeable: user.is_some_and(|u| u.role.can_admin()),
        version: String::new(),
    };
    for item in DashboardLayoutService::resolve(&state.pool).await? {
        let name = i18n.t(item.module.name_key()).to_string();
        if !item.enabled {
            if live.arrangeable {
                live.hidden.push(ModuleChoice {
                    module_id: item.module.id(),
                    name,
                });
            }
            continue;
        }
        let html = item.module.render(&ctx).await?;
        if item.module.column_width() == ColumnWidth::Full {
            live.banner_html = Some(html);
        } else {
            live.row.push(RenderedModule {
                html,
                module_id: item.module.id(),
                name,
                width: item.width.as_str(),
            });
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
        html.hash(&mut hasher);
    }
    for module in &live.row {
        module.width.hash(&mut hasher);
    }
    for choice in &live.hidden {
        choice.module_id.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

pub async fn index(
    OptionalUser(user): OptionalUser,
    State(state): State<AppState>,
    Query(query): Query<DashboardQuery>,
    headers: HeaderMap,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    if let Some(redirect) = members_only(&state, user.as_ref(), &headers) {
        return Ok(redirect);
    }
    let page_address = origin(&state, &headers);
    let live = render_modules(&state, user.as_ref(), &page_address, &i18n).await?;
    if headers.contains_key("hx-request") {
        return live_response(live, i18n);
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
        arranging: matches!(query.arrange.as_str(), "1" | "true"),
        i18n,
    })
}

/// The refreshed fragment, with what the script needs to decide: whether
/// the content changed, and whether the instance clock moved (daylight
/// saving).
fn live_response(live: LiveModules, i18n: I18n) -> Result<Response, AppError> {
    let version = HeaderValue::from_str(&live.version).map_err(|e| AppError::Internal(e.into()))?;
    let offset = HeaderValue::from(clock::offset_minutes(&Utc::now()));
    let mut response = render(&LiveTemplate { live, i18n })?;
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
