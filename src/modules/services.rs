//! Services module.
//!
//! Renders the list of services with 30-day availability sparklines. Shown
//! as a left sidebar on desktop and a horizontal strip on mobile.

use std::collections::HashMap;

use askama::Template;
use async_trait::async_trait;

use crate::error::AppError;
use crate::i18n::I18n;
use crate::models::Service;
use crate::repositories::{EventRepository, ServiceRepository};

use super::{Module, ModuleContext, ModuleRenderContext};

const SPARKLINE_DAYS: u32 = 30;

pub struct ServicesModule;

#[derive(Template)]
#[template(path = "modules/services.html")]
struct ServicesTemplate {
    services: Vec<Service>,
    sparkline_map: HashMap<i64, Vec<u8>>,
    i18n: I18n,
}

impl ServicesTemplate {
    #[allow(clippy::trivially_copy_pass_by_ref)]
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

#[async_trait]
impl Module for ServicesModule {
    fn id(&self) -> &'static str {
        "services"
    }

    fn name_key(&self) -> &'static str {
        "modules.services.name"
    }

    fn description_key(&self) -> &'static str {
        "modules.services.description"
    }

    fn contexts(&self) -> &'static [ModuleContext] {
        &[ModuleContext::Public, ModuleContext::Admin]
    }

    fn default_position(&self, _context: ModuleContext) -> i64 {
        20
    }

    async fn render(&self, ctx: &ModuleRenderContext<'_>) -> Result<String, AppError> {
        let services = ServiceRepository::list_all_with_icons(ctx.pool).await?;
        let sparkline_map = EventRepository::sparkline_data(ctx.pool, SPARKLINE_DAYS).await?;
        let tpl = ServicesTemplate {
            services,
            sparkline_map,
            i18n: ctx.i18n.clone(),
        };
        tpl.render()
            .map_err(|e| AppError::Internal(anyhow::anyhow!("services render: {e}")))
    }
}
