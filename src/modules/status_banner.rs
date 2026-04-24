//! Status banner module.
//!
//! Displays either the "all operational" chip or the list of active incidents
//! across the top of the dashboard. First (and highest-priority) module in
//! both public and admin contexts.

use askama::Template;
use async_trait::async_trait;

use crate::error::AppError;
use crate::i18n::I18n;
use crate::models::EventSummary;
use crate::repositories::EventRepository;

use super::{ColumnWidth, Module, ModuleContext, ModuleRenderContext};

pub struct StatusBannerModule;

#[derive(Template)]
#[template(path = "modules/status_banner.html")]
struct StatusBannerTemplate {
    active_incidents: Vec<EventSummary>,
    i18n: I18n,
}

#[async_trait]
impl Module for StatusBannerModule {
    fn id(&self) -> &'static str {
        "status_banner"
    }

    fn name_key(&self) -> &'static str {
        "modules.status_banner.name"
    }

    fn description_key(&self) -> &'static str {
        "modules.status_banner.description"
    }

    fn contexts(&self) -> &'static [ModuleContext] {
        &[ModuleContext::Public, ModuleContext::Admin]
    }

    fn default_position(&self, _context: ModuleContext) -> i64 {
        10
    }

    fn column_width(&self) -> ColumnWidth {
        ColumnWidth::Full
    }

    async fn render(&self, ctx: &ModuleRenderContext<'_>) -> Result<String, AppError> {
        let active_incidents = EventRepository::list_active_incidents(ctx.pool).await?;
        let tpl = StatusBannerTemplate {
            active_incidents,
            i18n: ctx.i18n.clone(),
        };
        tpl.render()
            .map_err(|e| AppError::Internal(anyhow::anyhow!("status_banner render: {e}")))
    }
}
