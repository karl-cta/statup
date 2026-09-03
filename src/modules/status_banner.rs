//! Status banner module.
//!
//! Displays the active incidents, the "all operational" chip, or, on an
//! instance with no service yet, an honest "nothing is watched" statement.
//! A fresh install must never claim that everything is fine. First (and
//! highest-priority) module in both public and admin contexts.

use askama::Template;
use async_trait::async_trait;

use crate::error::AppError;
use crate::i18n::I18n;
use crate::models::EventSummary;
use crate::repositories::{EventRepository, ServiceRepository};

use super::{ColumnWidth, Module, ModuleContext, ModuleRenderContext};

pub struct StatusBannerModule;

#[derive(Template)]
#[template(path = "modules/status_banner.html")]
struct StatusBannerTemplate {
    active_incidents: Vec<EventSummary>,
    has_services: bool,
    can_configure: bool,
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
        let has_services = ServiceRepository::count(ctx.pool).await? > 0;
        let tpl = StatusBannerTemplate {
            active_incidents,
            has_services,
            can_configure: ctx.user.is_some_and(|u| u.role.can_publish()),
            i18n: ctx.i18n.clone(),
        };
        tpl.render()
            .map_err(|e| AppError::Internal(anyhow::anyhow!("status_banner render: {e}")))
    }
}
