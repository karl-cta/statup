//! Dashboard route - main status page.

use std::collections::HashMap;

use askama::Template;
use axum::extract::State;
use axum::response::Redirect;
use axum::response::{Html, IntoResponse, Response};

use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::{CsrfToken, OptionalUser};
use crate::models::{EventSummary, Service, User};
use crate::repositories::{EventRepository, ServiceRepository, UserRepository};
use crate::services::EventService;
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
    services: Vec<Service>,
    recent_activity: Vec<EventSummary>,
    active_maintenance: Vec<EventSummary>,
    resolved_maintenance: Vec<EventSummary>,
    active_incidents: Vec<EventSummary>,
    sparkline_map: HashMap<i64, Vec<u8>>,
    i18n: I18n,
}

impl DashboardTemplate {
    /// Return Tailwind CSS color classes for the 30-day sparkline of a service.
    #[allow(clippy::trivially_copy_pass_by_ref)] // Askama passes field refs
    fn sparkline_classes(&self, service_id: &i64) -> Vec<&'static str> {
        let empty = vec![0u8; SPARKLINE_DAYS as usize];
        let points = self.sparkline_map.get(service_id).unwrap_or(&empty);
        points
            .iter()
            .map(|&level| match level {
                0 => "bg-emerald-400/70 dark:bg-emerald-500/50",
                1 => "bg-yellow-400 dark:bg-yellow-400/80",
                2 => "bg-orange-400 dark:bg-orange-400/80",
                _ => "bg-red-400 dark:bg-red-400/80",
            })
            .collect()
    }
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

const RECENT_EVENTS_LIMIT: i64 = 10;
const SPARKLINE_DAYS: u32 = 30;

/// and upcoming maintenance.
pub async fn index(
    OptionalUser(user): OptionalUser,
    State(state): State<AppState>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    if user.is_none() && !state.is_public_mode() {
        return Ok(Redirect::to("/login").into_response());
    }

    // Compute unread count and update last_seen_at only for authenticated users
    let mut unread_count = 0;
    if let Some(ref u) = user {
        unread_count = EventService::unread_count(&state.pool, u.last_seen_at).await?;
        if let Err(e) = UserRepository::update_last_seen(&state.pool, u.id).await {
            tracing::warn!(user_id = u.id, error = %e, "Failed to update last_seen_at");
        }
    }

    let services = ServiceRepository::list_all_with_icons(&state.pool).await?;
    let recent_activity =
        EventRepository::list_recent_activity(&state.pool, RECENT_EVENTS_LIMIT, 0).await?;
    let active_maintenance = EventRepository::list_active_maintenance(&state.pool).await?;
    let resolved_maintenance =
        EventRepository::list_recent_resolved_maintenance(&state.pool, 5).await?;
    let active_incidents = EventRepository::list_active_incidents(&state.pool).await?;
    let sparkline_map = EventRepository::sparkline_data(&state.pool, SPARKLINE_DAYS).await?;

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
        services,
        recent_activity,
        active_maintenance,
        resolved_maintenance,
        active_incidents,
        sparkline_map,
        i18n,
    };
    render(&tpl)
}
