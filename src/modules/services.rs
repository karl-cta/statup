//! Services module.
//!
//! Renders the list of services with 30-day availability sparklines. Shown
//! as a left sidebar on desktop and a horizontal strip on mobile.

use std::collections::HashMap;

use askama::Template;
use async_trait::async_trait;
use chrono::{Duration, Utc};

use crate::error::AppError;
use crate::i18n::I18n;
use crate::models::Service;
use crate::repositories::{EventRepository, ServiceRepository};

use super::{Module, ModuleContext, ModuleRenderContext};

const SPARKLINE_DAYS: u32 = 30;

pub struct ServicesModule;

pub struct SparklineDay {
    pub class: &'static str,
    pub tooltip: String,
}

#[derive(Template)]
#[template(path = "modules/services.html")]
struct ServicesTemplate {
    services: Vec<Service>,
    sparkline_map: HashMap<i64, Vec<u8>>,
    i18n: I18n,
}

impl ServicesTemplate {
    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn sparkline_days(&self, service_id: &i64) -> Vec<SparklineDay> {
        let empty = vec![0u8; SPARKLINE_DAYS as usize];
        let points = self.sparkline_map.get(service_id).unwrap_or(&empty);
        let today = Utc::now().date_naive();
        let count = points.len();
        points
            .iter()
            .enumerate()
            .map(|(idx, &level)| {
                let offset =
                    i64::try_from(count.saturating_sub(1).saturating_sub(idx)).unwrap_or(0);
                let date = today - Duration::days(offset);
                let label_key = match level {
                    0 => "dashboard.sparkline_legend_ok",
                    1 => "dashboard.sparkline_legend_minor",
                    2 => "dashboard.sparkline_legend_major",
                    _ => "dashboard.sparkline_legend_critical",
                };
                let class = match level {
                    0 => "bar",
                    1 => "bar bar-minor",
                    2 => "bar bar-major",
                    _ => "bar bar-crit",
                };
                let tooltip = format!("{} · {}", date.format("%Y-%m-%d"), self.i18n.t(label_key));
                SparklineDay { class, tooltip }
            })
            .collect()
    }

    #[allow(clippy::trivially_copy_pass_by_ref, clippy::naive_bytecount)]
    fn uptime_pct(&self, service_id: &i64) -> String {
        let points = match self.sparkline_map.get(service_id) {
            Some(p) if !p.is_empty() => p,
            _ => return "100.00%".to_string(),
        };
        let total = u32::try_from(points.len()).unwrap_or(u32::MAX);
        let ok_days = u32::try_from(points.iter().filter(|&&level| level == 0).count())
            .unwrap_or(u32::MAX);
        let pct = if total > 0 {
            f64::from(ok_days) / f64::from(total) * 100.0
        } else {
            100.0
        };
        format!("{pct:.2}%")
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
