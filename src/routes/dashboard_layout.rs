//! Arranging the status page: order, width and visibility of its modules,
//! saved by the script of the page itself as the administrator changes
//! them.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::error::AppError;
use crate::i18n::Locale;
use crate::middleware::{HtmlForm, RequireAdmin};
use crate::services::DashboardLayoutService;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct OrderForm {
    #[serde(default)]
    order: Vec<String>,
}

pub async fn save_order(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    HtmlForm(form): HtmlForm<OrderForm>,
) -> Result<Response, AppError> {
    DashboardLayoutService::save_order(&state.pool, &form.order).await?;
    tracing::info!(admin_id = admin.id, "Dashboard order saved");
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Deserialize)]
pub struct ToggleForm {
    #[serde(default)]
    enabled: Option<String>,
}

pub async fn toggle_module(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Path(module_id): Path<String>,
    HtmlForm(form): HtmlForm<ToggleForm>,
) -> Result<Response, AppError> {
    let enabled = matches!(form.enabled.as_deref(), Some("true" | "on" | "1"));
    DashboardLayoutService::set_enabled(&state.pool, &module_id, enabled).await?;
    tracing::info!(admin_id = admin.id, %module_id, enabled, "Dashboard module toggled");
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Deserialize)]
pub struct WidthForm {
    #[serde(default)]
    width: String,
}

pub async fn set_width(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Path(module_id): Path<String>,
    HtmlForm(form): HtmlForm<WidthForm>,
) -> Result<Response, AppError> {
    let width = DashboardLayoutService::set_width(&state.pool, &module_id, &form.width).await?;
    tracing::info!(admin_id = admin.id, %module_id, width = width.as_str(), "Dashboard module width saved");
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Deserialize)]
pub struct ShownForm {
    #[serde(default)]
    show: Vec<String>,
}

pub async fn set_shown(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Locale(i18n): Locale,
    Path(module_id): Path<String>,
    HtmlForm(form): HtmlForm<ShownForm>,
) -> Result<Response, AppError> {
    DashboardLayoutService::set_shown(&state.pool, &i18n, &module_id, &form.show).await?;
    tracing::info!(admin_id = admin.id, %module_id, "Dashboard module settings saved");
    Ok(StatusCode::NO_CONTENT.into_response())
}
