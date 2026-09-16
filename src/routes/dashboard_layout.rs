//! Layout editor: which modules each dashboard shows, and in what order.

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;

use super::{Frame, render};
use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::{CsrfToken, HtmlForm, RequireAdmin};
use crate::modules::{ColumnWidth, ModuleContext, ModuleRegistry};
use crate::repositories::DashboardLayoutRepository;
use crate::services::DashboardLayoutService;
use crate::state::AppState;

#[derive(Template)]
#[template(path = "admin/dashboard_layout.html")]
struct LayoutTemplate {
    frame: Frame,
    context: &'static str,
    pinned: Option<LayoutRow>,
    rows: Vec<LayoutRow>,
    saved: bool,
    i18n: I18n,
}

struct LayoutRow {
    module_id: &'static str,
    name: String,
    description: String,
    enabled: bool,
}

fn parse_context(raw: &str) -> Result<ModuleContext, AppError> {
    ModuleContext::parse(raw).ok_or(AppError::NotFound)
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
    Query(query): Query<LayoutQuery>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    let context = parse_context(&context_raw)?;
    let mut pinned = None;
    let mut rows = Vec::new();
    for item in DashboardLayoutService::resolve(&state.pool, context).await? {
        let row = LayoutRow {
            module_id: item.module.id(),
            name: i18n.t(item.module.name_key()).to_string(),
            description: i18n.t(item.module.description_key()).to_string(),
            enabled: item.enabled,
        };
        if item.module.column_width() == ColumnWidth::Full {
            pinned = Some(row);
        } else {
            rows.push(row);
        }
    }
    render(&LayoutTemplate {
        frame: Frame::load(&state.pool, Some(&user), csrf_token.0, &i18n).await?,
        context: context.as_str(),
        pinned,
        rows,
        saved: query.saved,
        i18n,
    })
}

#[derive(Deserialize)]
pub struct OrderForm {
    #[serde(default)]
    order: Vec<String>,
}

pub async fn save_order(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Path(context_raw): Path<String>,
    HtmlForm(form): HtmlForm<OrderForm>,
) -> Result<Response, AppError> {
    let context = parse_context(&context_raw)?;
    DashboardLayoutRepository::save_order(&state.pool, context, &form.order).await?;
    tracing::info!(admin_id = admin.id, %context, "Dashboard order saved");
    Ok(Redirect::to(&format!("/admin/dashboard/{context}/layout?saved=true")).into_response())
}

#[derive(Deserialize)]
pub struct ToggleForm {
    #[serde(default)]
    enabled: Option<String>,
}

/// Shows or hides a module. The pinned banner cannot be hidden: a status
/// page without its status is a blank page.
pub async fn toggle_module(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Path((context_raw, module_id)): Path<(String, String)>,
    HtmlForm(form): HtmlForm<ToggleForm>,
) -> Result<Response, AppError> {
    let context = parse_context(&context_raw)?;
    let module = ModuleRegistry::global()
        .get(&module_id)
        .ok_or(AppError::NotFound)?;
    if module.column_width() == ColumnWidth::Full {
        return Err(AppError::Validation(
            "validation.banner_always_shown".to_string(),
        ));
    }
    let enabled = matches!(form.enabled.as_deref(), Some("true" | "on" | "1"));
    DashboardLayoutRepository::set_enabled(&state.pool, context, &module_id, enabled).await?;
    tracing::info!(admin_id = admin.id, %context, %module_id, enabled, "Dashboard module toggled");
    Ok(Redirect::to(&format!("/admin/dashboard/{context}/layout?saved=true")).into_response())
}
