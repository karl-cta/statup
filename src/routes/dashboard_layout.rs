//! Arranging the status page: order, width and visibility of its modules,
//! saved by the script of the page itself as the administrator changes
//! them.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::error::AppError;
use crate::middleware::{HtmlForm, RequireAdmin};
use crate::modules::{ColumnWidth, ModuleRegistry};
use crate::repositories::DashboardLayoutRepository;
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
    DashboardLayoutRepository::save_order(&state.pool, &form.order).await?;
    tracing::info!(admin_id = admin.id, "Dashboard order saved");
    Ok(StatusCode::NO_CONTENT.into_response())
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
    Path(module_id): Path<String>,
    HtmlForm(form): HtmlForm<ToggleForm>,
) -> Result<Response, AppError> {
    let module = ModuleRegistry::global()
        .get(&module_id)
        .ok_or(AppError::NotFound)?;
    if module.column_width() == ColumnWidth::Full {
        return Err(AppError::Validation(
            "validation.banner_always_shown".to_string(),
        ));
    }
    let enabled = matches!(form.enabled.as_deref(), Some("true" | "on" | "1"));
    DashboardLayoutRepository::insert_if_missing(
        &state.pool,
        &module_id,
        module.default_position(),
    )
    .await?;
    DashboardLayoutRepository::set_enabled(&state.pool, &module_id, enabled).await?;
    tracing::info!(admin_id = admin.id, %module_id, enabled, "Dashboard module toggled");
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Deserialize)]
pub struct WidthForm {
    #[serde(default)]
    width: String,
}

/// Gives a module one share, two shares, or a row of its own.
pub async fn set_width(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Path(module_id): Path<String>,
    HtmlForm(form): HtmlForm<WidthForm>,
) -> Result<Response, AppError> {
    let module = ModuleRegistry::global()
        .get(&module_id)
        .ok_or(AppError::NotFound)?;
    let width = ColumnWidth::parse(&form.width)
        .filter(|_| module.column_width() != ColumnWidth::Full)
        .ok_or_else(|| AppError::Validation("error.invalid_data".to_string()))?;
    DashboardLayoutRepository::insert_if_missing(
        &state.pool,
        &module_id,
        module.default_position(),
    )
    .await?;
    DashboardLayoutRepository::set_width(&state.pool, &module_id, width.as_str()).await?;
    tracing::info!(admin_id = admin.id, %module_id, width = width.as_str(), "Dashboard module width saved");
    Ok(StatusCode::NO_CONTENT.into_response())
}
