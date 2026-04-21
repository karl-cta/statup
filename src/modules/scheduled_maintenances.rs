//! Scheduled maintenances module.
//!
//! Right-column list of upcoming and recently resolved maintenances.

use askama::Template;
use async_trait::async_trait;

use crate::error::AppError;
use crate::i18n::I18n;
use crate::models::EventSummary;
use crate::repositories::EventRepository;

use super::{Module, ModuleContext, ModuleRenderContext};

const RESOLVED_LIMIT: i64 = 5;

pub struct ScheduledMaintenancesModule;

#[derive(Template)]
#[template(path = "modules/scheduled_maintenances.html")]
struct ScheduledMaintenancesTemplate {
    active_maintenance: Vec<EventSummary>,
    resolved_maintenance: Vec<EventSummary>,
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

    fn description_key(&self) -> &'static str {
        "modules.scheduled_maintenances.description"
    }

    fn contexts(&self) -> &'static [ModuleContext] {
        &[ModuleContext::Public, ModuleContext::Admin]
    }

    fn default_position(&self, _context: ModuleContext) -> i64 {
        40
    }

    async fn render(&self, ctx: &ModuleRenderContext<'_>) -> Result<String, AppError> {
        let active_maintenance = EventRepository::list_active_maintenance(ctx.pool).await?;
        let resolved_maintenance =
            EventRepository::list_recent_resolved_maintenance(ctx.pool, RESOLVED_LIMIT).await?;
        let tpl = ScheduledMaintenancesTemplate {
            active_maintenance,
            resolved_maintenance,
            i18n: ctx.i18n.clone(),
        };
        tpl.render()
            .map_err(|e| AppError::Internal(anyhow::anyhow!("scheduled_maintenances render: {e}")))
    }
}
