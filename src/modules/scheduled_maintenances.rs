//! Maintenances card: work under way, work to come, and what recently ended.

use askama::Template;
use async_trait::async_trait;

use crate::error::AppError;
use crate::i18n::I18n;
use crate::models::{EventSummary, Lifecycle};
use crate::repositories::EventRepository;

use super::{ColumnWidth, Module, ModuleContext, ModuleRenderContext, render_template};

const FINISHED_LIMIT: i64 = 5;

pub struct ScheduledMaintenancesModule;

pub struct MaintenanceRow {
    pub id: i64,
    pub title: String,
    pub state: String,
    pub when: String,
    pub countdown: Option<String>,
    pub services: Vec<String>,
}

#[derive(Template)]
#[template(path = "modules/scheduled_maintenances.html")]
struct MaintenancesTemplate {
    ongoing: Vec<MaintenanceRow>,
    upcoming: Vec<MaintenanceRow>,
    finished: Vec<MaintenanceRow>,
    i18n: I18n,
}

impl MaintenancesTemplate {
    fn is_empty(&self) -> bool {
        self.ongoing.is_empty() && self.upcoming.is_empty() && self.finished.is_empty()
    }
}

#[async_trait]
impl Module for ScheduledMaintenancesModule {
    fn id(&self) -> &'static str {
        "scheduled_maintenances"
    }

    fn name_key(&self) -> &'static str {
        "modules.scheduled_maintenances.name"
    }

    fn description_key(&self) -> &'static str {
        "modules.scheduled_maintenances.description"
    }

    fn contexts(&self) -> &'static [ModuleContext] {
        &[ModuleContext::Public, ModuleContext::Admin]
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
        let template = MaintenancesTemplate {
            ongoing: ongoing.into_iter().map(|m| ongoing_row(m, i18n)).collect(),
            upcoming: upcoming
                .into_iter()
                .map(|m| upcoming_row(m, i18n))
                .collect(),
            finished: finished.iter().map(|m| finished_row(m, i18n)).collect(),
            i18n: i18n.clone(),
        };
        render_template(self.id(), &template)
    }
}

fn row(event: &EventSummary, i18n: &I18n, when: String) -> MaintenanceRow {
    MaintenanceRow {
        id: event.id,
        title: event.title.clone(),
        state: event
            .lifecycle_key()
            .map(|key| i18n.t(key).to_string())
            .unwrap_or_default(),
        when,
        countdown: None,
        services: event.services().into_iter().map(String::from).collect(),
    }
}

/// "Fin prévue le 16 sept. à 23:00" when an end is planned, otherwise the
/// start.
fn ongoing_row(event: &EventSummary, i18n: &I18n) -> MaintenanceRow {
    let when = match (event.planned_end, event.started_at) {
        (Some(end), _) => i18n.tf("maintenance.ends", &[("when", &i18n.format_datetime(&end))]),
        (None, Some(start)) => i18n.tf(
            "maintenance.since",
            &[("when", &i18n.format_datetime(&start))],
        ),
        (None, None) => String::new(),
    };
    row(event, i18n, when)
}

fn upcoming_row(event: &EventSummary, i18n: &I18n) -> MaintenanceRow {
    let when = event
        .planned_start
        .map(|start| i18n.format_datetime(&start))
        .unwrap_or_default();
    MaintenanceRow {
        countdown: i18n.format_countdown(event.countdown()),
        ..row(event, i18n, when)
    }
}

fn finished_row(event: &EventSummary, i18n: &I18n) -> MaintenanceRow {
    let when = i18n.format_date_short(&crate::clock::local_date(&event.closed_at()));
    row(event, i18n, when)
}
