//! Maintenances card: work under way, work to come, and what recently
//! ended, as one list on the activity row model, worst first.

use askama::Template;
use async_trait::async_trait;

use crate::error::AppError;
use crate::i18n::I18n;
use crate::models::{EventSummary, Lifecycle, ServiceTag};
use crate::repositories::EventRepository;

use super::{ColumnWidth, Module, ModuleRenderContext, render_template};

const FINISHED_LIMIT: i64 = 5;

pub struct ScheduledMaintenancesModule;

pub struct MaintenanceRow {
    pub id: i64,
    pub title: String,
    /// The day the row is about: the start to come, the day work began,
    /// or the day it ended.
    pub day: String,
    /// The time, then the services, on one line.
    pub when: String,
    pub services: Vec<ServiceTag>,
    pub state: String,
    pub tone: &'static str,
    pub is_open: bool,
}

#[derive(Template)]
#[template(path = "modules/scheduled_maintenances.html")]
struct MaintenancesTemplate {
    rows: Vec<MaintenanceRow>,
    i18n: I18n,
}

#[async_trait]
impl Module for ScheduledMaintenancesModule {
    fn id(&self) -> &'static str {
        "scheduled_maintenances"
    }

    fn name_key(&self) -> &'static str {
        "modules.scheduled_maintenances.name"
    }

    fn default_position(&self) -> i64 {
        40
    }

    fn column_width(&self) -> ColumnWidth {
        ColumnWidth::Narrow
    }

    async fn render(&self, ctx: &ModuleRenderContext<'_>) -> Result<String, AppError> {
        let i18n = ctx.i18n;
        let open = EventRepository::list_open_maintenance(ctx.pool).await?;
        let (ongoing, upcoming): (Vec<_>, Vec<_>) = open
            .iter()
            .partition(|m| m.lifecycle == Some(Lifecycle::InProgress));
        let finished = EventRepository::list_finished_maintenance(ctx.pool, FINISHED_LIMIT).await?;
        let rows = ongoing
            .into_iter()
            .map(|m| ongoing_row(m, i18n))
            .chain(upcoming.into_iter().map(|m| upcoming_row(m, i18n)))
            .chain(finished.iter().map(|m| finished_row(m, i18n)))
            .collect();
        let template = MaintenancesTemplate {
            rows,
            i18n: i18n.clone(),
        };
        render_template(self.id(), &template)
    }
}

fn row(event: &EventSummary, i18n: &I18n, day: String, when: String) -> MaintenanceRow {
    MaintenanceRow {
        id: event.id,
        title: event.title.clone(),
        day,
        when,
        services: event.service_tags(i18n),
        state: event
            .lifecycle_key()
            .map(|key| i18n.t(key).to_string())
            .unwrap_or_default(),
        tone: event.tone().as_str(),
        is_open: event.lifecycle.is_some_and(Lifecycle::is_active),
    }
}

fn day_of(i18n: &I18n, at: Option<chrono::DateTime<chrono::Utc>>) -> String {
    at.map(|dt| i18n.format_date_short(&crate::clock::local_date(&dt)))
        .unwrap_or_default()
}

/// The day work began; the line says since when, and until when if an end
/// was announced.
fn ongoing_row(event: &EventSummary, i18n: &I18n) -> MaintenanceRow {
    let start = event.started_at.or(event.planned_start);
    let mut parts = Vec::new();
    if let Some(start) = start {
        parts.push(i18n.tf("maintenance.since", &[("when", &i18n.format_time(&start))]));
    }
    if let Some(end) = event.planned_end {
        let same_day =
            start.is_some_and(|s| crate::clock::local_date(&s) == crate::clock::local_date(&end));
        let when = if same_day {
            i18n.format_time(&end)
        } else {
            i18n.format_datetime(&end)
        };
        parts.push(i18n.tf("maintenance.until", &[("when", &when)]));
    }
    row(event, i18n, day_of(i18n, start), parts.join(", "))
}

/// The day it starts; the line says at what time.
fn upcoming_row(event: &EventSummary, i18n: &I18n) -> MaintenanceRow {
    let when = event
        .planned_start
        .map(|start| i18n.format_time(&start))
        .unwrap_or_default();
    row(event, i18n, day_of(i18n, event.planned_start), when)
}

/// The day and the time it ended.
fn finished_row(event: &EventSummary, i18n: &I18n) -> MaintenanceRow {
    let closed = event.closed_at();
    row(
        event,
        i18n,
        day_of(i18n, Some(closed)),
        i18n.format_time(&closed),
    )
}
