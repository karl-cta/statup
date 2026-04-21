//! Admin routes for the dashboard module engine: layout editor and per-module
//! configuration.

use askama::Template;
use axum::Form;
use axum::extract::{Path, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;

use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::{CsrfToken, HtmlForm, RequireAdmin};
use crate::models::User;
use crate::modules::{Module, ModuleContext, ModuleRegistry};
use crate::repositories::{DashboardLayoutRepository, EventRepository};
use crate::services::{DashboardLayoutService, EventService};
use crate::state::AppState;

#[derive(Template)]
#[template(path = "admin/dashboard_layout.html")]
struct LayoutTemplate {
    csrf_token: String,
    user_display_name: String,
    is_admin: bool,
    is_authenticated: bool,
    unread_count: i64,
    last_admin_action: Option<String>,
    context: String,
    context_label_key: &'static str,
    rows: Vec<LayoutRow>,
    saved_flash: bool,
    i18n: I18n,
}

struct LayoutRow {
    module_id: String,
    name: String,
    description: String,
    enabled: bool,
}

#[derive(Template)]
#[template(path = "admin/module_config.html")]
struct ModuleConfigTemplate {
    csrf_token: String,
    user_display_name: String,
    is_admin: bool,
    is_authenticated: bool,
    unread_count: i64,
    last_admin_action: Option<String>,
    module_id: String,
    module_name: String,
    module_description: String,
    raw_config: String,
    saved_flash: bool,
    i18n: I18n,
}

fn render(tpl: &impl Template) -> Result<Response, AppError> {
    let html = tpl
        .render()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("template render error: {e}")))?;
    Ok(Html(html).into_response())
}

fn layout_fields(user: &User) -> (String, bool, bool) {
    (user.display_name.clone(), user.role.can_admin(), true)
}

fn parse_context(raw: &str) -> Result<ModuleContext, AppError> {
    ModuleContext::parse(raw)
        .ok_or_else(|| AppError::Validation("validation.unknown_dashboard_context".to_string()))
}

fn context_label_key(context: ModuleContext) -> &'static str {
    match context {
        ModuleContext::Public => "modules.layout_context_public",
        ModuleContext::Admin => "modules.layout_context_admin",
    }
}

#[derive(Deserialize)]
pub struct LayoutQuery {
    #[serde(default)]
    saved: bool,
}

pub async fn layout_editor(
    RequireAdmin(user): RequireAdmin,
    State(state): State<AppState>,
    Path(context_raw): Path<String>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
    axum::extract::Query(query): axum::extract::Query<LayoutQuery>,
) -> Result<Response, AppError> {
    let context = parse_context(&context_raw)?;
    let registry = ModuleRegistry::builtin();
    let resolved = DashboardLayoutService::resolve(&state.pool, &registry, context).await?;

    let rows: Vec<LayoutRow> = resolved
        .iter()
        .map(|r| LayoutRow {
            module_id: r.module.id().to_string(),
            name: i18n.t(r.module.name_key()).to_string(),
            description: i18n.t(r.module.description_key()).to_string(),
            enabled: r.enabled,
        })
        .collect();

    let unread_count = EventService::unread_count(&state.pool, user.last_seen_at).await?;
    let last_admin_action = EventRepository::last_admin_action(&state.pool)
        .await?
        .map(|dt| i18n.format_datetime_long(&dt));
    let (user_display_name, is_admin, is_authenticated) = layout_fields(&user);

    let tpl = LayoutTemplate {
        csrf_token: csrf_token.0,
        user_display_name,
        is_admin,
        is_authenticated,
        unread_count,
        last_admin_action,
        context: context.as_str().to_string(),
        context_label_key: context_label_key(context),
        rows,
        saved_flash: query.saved,
        i18n,
    };
    render(&tpl)
}

#[derive(Deserialize)]
pub struct OrderForm {
    #[serde(default, rename = "order")]
    order: Vec<String>,
}

pub async fn save_order(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Path(context_raw): Path<String>,
    HtmlForm(form): HtmlForm<OrderForm>,
) -> Result<Response, AppError> {
    let context = parse_context(&context_raw)?;
    DashboardLayoutRepository::save_default_order(&state.pool, context, &form.order).await?;

    tracing::info!(
        admin_id = admin.id,
        context = %context,
        count = form.order.len(),
        "Dashboard layout order saved"
    );

    Ok(Redirect::to(&format!("/admin/dashboard/{context}/layout?saved=1")).into_response())
}

#[derive(Deserialize)]
pub struct ToggleForm {
    #[serde(default)]
    enabled: Option<String>,
}

pub async fn toggle_module(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Path((context_raw, module_id)): Path<(String, String)>,
    Form(form): Form<ToggleForm>,
) -> Result<Response, AppError> {
    let context = parse_context(&context_raw)?;
    let registry = ModuleRegistry::builtin();
    if registry.get(&module_id).is_none() {
        return Err(AppError::NotFound);
    }

    let enabled = matches!(form.enabled.as_deref(), Some("true" | "on" | "1"));
    DashboardLayoutRepository::set_default_enabled(&state.pool, context, &module_id, enabled)
        .await?;

    tracing::info!(
        admin_id = admin.id,
        context = %context,
        module_id = %module_id,
        enabled,
        "Dashboard module toggled"
    );

    Ok(Redirect::to(&format!("/admin/dashboard/{context}/layout?saved=1")).into_response())
}

pub async fn module_config_form(
    RequireAdmin(user): RequireAdmin,
    State(state): State<AppState>,
    Path(module_id): Path<String>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
    axum::extract::Query(query): axum::extract::Query<LayoutQuery>,
) -> Result<Response, AppError> {
    let registry = ModuleRegistry::builtin();
    let module = registry.get(&module_id).ok_or(AppError::NotFound)?;

    let raw = read_admin_config(&state.pool, module).await?;
    let pretty = pretty_json(&raw);
    let unread_count = EventService::unread_count(&state.pool, user.last_seen_at).await?;
    let last_admin_action = EventRepository::last_admin_action(&state.pool)
        .await?
        .map(|dt| i18n.format_datetime_long(&dt));
    let (user_display_name, is_admin, is_authenticated) = layout_fields(&user);

    let tpl = ModuleConfigTemplate {
        csrf_token: csrf_token.0,
        user_display_name,
        is_admin,
        is_authenticated,
        unread_count,
        last_admin_action,
        module_id: module.id().to_string(),
        module_name: i18n.t(module.name_key()).to_string(),
        module_description: i18n.t(module.description_key()).to_string(),
        raw_config: pretty,
        saved_flash: query.saved,
        i18n,
    };
    render(&tpl)
}

#[derive(Deserialize)]
pub struct ConfigForm {
    config: String,
}

pub async fn module_config_save(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Path(module_id): Path<String>,
    Form(form): Form<ConfigForm>,
) -> Result<Response, AppError> {
    let registry = ModuleRegistry::builtin();
    let module = registry.get(&module_id).ok_or(AppError::NotFound)?;

    let normalized = normalize_config(&form.config)?;
    for context in module.contexts() {
        DashboardLayoutRepository::set_default_config(
            &state.pool,
            *context,
            module.id(),
            &normalized,
        )
        .await?;
    }

    tracing::info!(
        admin_id = admin.id,
        module_id = module.id(),
        "Dashboard module config saved"
    );

    Ok(Redirect::to(&format!("/admin/modules/{module_id}/config?saved=1")).into_response())
}

async fn read_admin_config(
    pool: &crate::db::DbPool,
    module: &dyn Module,
) -> Result<String, AppError> {
    let context = module
        .contexts()
        .iter()
        .copied()
        .next()
        .unwrap_or(ModuleContext::Admin);
    let rows = DashboardLayoutRepository::list_default(pool, context).await?;
    Ok(rows
        .into_iter()
        .find(|r| r.module_id == module.id())
        .map_or_else(|| "{}".to_string(), |r| r.config))
}

fn pretty_json(raw: &str) -> String {
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
        .unwrap_or_else(|| raw.to_string())
}

fn normalize_config(input: &str) -> Result<String, AppError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok("{}".to_string());
    }
    let value: serde_json::Value = serde_json::from_str(trimmed)
        .map_err(|_| AppError::Validation("validation.invalid_json".to_string()))?;
    serde_json::to_string(&value)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("config serialize: {e}")))
}
