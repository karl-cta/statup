//! Dashboard route, powered by the module engine.

use askama::Template;
use axum::extract::State;
use axum::response::Redirect;
use axum::response::{Html, IntoResponse, Response};

use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::{CsrfToken, OptionalUser};
use crate::models::User;
use crate::modules::{ModuleContext, ModuleRegistry, ModuleRenderContext};
use crate::repositories::{EventRepository, UserRepository};
use crate::services::{DashboardLayoutService, EventService};
use crate::state::AppState;

#[derive(Template)]
#[template(path = "dashboard/index.html")]
struct DashboardTemplate {
    csrf_token: String,
    user_display_name: String,
    is_admin: bool,
    is_authenticated: bool,
    unread_count: i64,
    last_admin_action: Option<String>,
    context_label: &'static str,
    banner_html: Option<String>,
    services_html: Option<String>,
    activity_html: Option<String>,
    maintenances_html: Option<String>,
    extra_modules: Vec<RenderedModule>,
    i18n: I18n,
}

struct RenderedModule {
    id: String,
    html: String,
}

fn render(tpl: &impl Template) -> Result<Response, AppError> {
    let html = tpl
        .render()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("template render error: {e}")))?;
    Ok(Html(html).into_response())
}

fn layout_fields(user: Option<&User>) -> (String, bool, bool) {
    match user {
        Some(u) => (u.display_name.clone(), u.role.can_admin(), true),
        None => (String::new(), false, false),
    }
}

fn pick_context(user: Option<&User>) -> ModuleContext {
    match user {
        Some(_) => ModuleContext::Admin,
        None => ModuleContext::Public,
    }
}

pub async fn index(
    OptionalUser(user): OptionalUser,
    State(state): State<AppState>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    if user.is_none() && !state.is_public_mode() {
        return Ok(Redirect::to("/login").into_response());
    }

    let mut unread_count = 0;
    if let Some(ref u) = user {
        unread_count = EventService::unread_count(&state.pool, u.last_seen_at).await?;
        if let Err(e) = UserRepository::update_last_seen(&state.pool, u.id).await {
            tracing::warn!(user_id = u.id, error = %e, "Failed to update last_seen_at");
        }
    }

    let context = pick_context(user.as_ref());
    let registry = ModuleRegistry::builtin();
    let resolved = DashboardLayoutService::resolve(&state.pool, &registry, context).await?;

    let mut banner_html = None;
    let mut services_html = None;
    let mut activity_html = None;
    let mut maintenances_html = None;
    let mut extra_modules = Vec::new();

    for item in resolved {
        if !item.enabled {
            continue;
        }
        let render_ctx = ModuleRenderContext {
            pool: &state.pool,
            user: user.as_ref(),
            i18n: &i18n,
            context,
            config: &item.config,
        };
        let id = item.module.id();
        let html = item.module.render(&render_ctx).await?;
        match id {
            "status_banner" => banner_html = Some(html),
            "services" => services_html = Some(html),
            "recent_activity" => activity_html = Some(html),
            "scheduled_maintenances" => maintenances_html = Some(html),
            _ => extra_modules.push(RenderedModule {
                id: id.to_string(),
                html,
            }),
        }
    }

    let (user_display_name, is_admin, is_authenticated) = layout_fields(user.as_ref());
    let last_admin_action = EventRepository::last_admin_action(&state.pool)
        .await?
        .map(|dt| i18n.format_datetime_long(&dt));

    let tpl = DashboardTemplate {
        csrf_token: csrf_token.0,
        user_display_name,
        is_admin,
        is_authenticated,
        unread_count,
        last_admin_action,
        context_label: context.as_str(),
        banner_html,
        services_html,
        activity_html,
        maintenances_html,
        extra_modules,
        i18n,
    };
    render(&tpl)
}
