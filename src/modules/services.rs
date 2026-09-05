//! Services module.
//!
//! Renders the list of services with 30-day availability sparklines. Shown
//! as a left sidebar on desktop and a horizontal strip on mobile.

use std::collections::HashMap;

use askama::Template;
use async_trait::async_trait;
use chrono::{Duration, NaiveDate, Utc};

use crate::error::AppError;
use crate::i18n::I18n;
use crate::models::Service;
use crate::repositories::{EventRepository, ServiceRepository};

use super::{ColumnWidth, Module, ModuleContext, ModuleRenderContext};

const SPARKLINE_DAYS: u32 = 30;

pub struct ServicesModule;

pub struct SparklineDay {
    pub class: &'static str,
    /// Stacked on two lines in the tooltip: a single line was 152px wide in a
    /// 260px column, which left it no room to follow the day it describes.
    pub date: String,
    pub status: String,
}

#[derive(Template)]
#[template(path = "modules/services.html")]
struct ServicesTemplate {
    services: Vec<Service>,
    sparkline_map: HashMap<i64, Vec<u8>>,
    i18n: I18n,
}

impl ServicesTemplate {
    /// One entry per day in the window, oldest first. `None` marks a day before
    /// the service existed: nothing observed it, and painting it operational
    /// would claim uptime the instance never measured.
    fn day_levels(&self, service: &Service) -> Vec<(NaiveDate, Option<u8>)> {
        let empty = vec![0u8; SPARKLINE_DAYS as usize];
        let points = self.sparkline_map.get(&service.id).unwrap_or(&empty);
        let today = Utc::now().date_naive();
        let first_day = service.created_at.date_naive();
        let count = points.len();
        points
            .iter()
            .enumerate()
            .map(|(idx, &level)| {
                let offset =
                    i64::try_from(count.saturating_sub(1).saturating_sub(idx)).unwrap_or(0);
                let date = today - Duration::days(offset);
                let observed = if date < first_day { None } else { Some(level) };
                (date, observed)
            })
            .collect()
    }

    fn sparkline_days(&self, service: &Service) -> Vec<SparklineDay> {
        self.day_levels(service)
            .into_iter()
            .map(|(date, level)| {
                let (label_key, class) = match level {
                    None => ("dashboard.sparkline_legend_none", "bar bar-none"),
                    Some(0) => ("dashboard.sparkline_legend_ok", "bar"),
                    Some(1) => ("dashboard.sparkline_legend_minor", "bar bar-minor"),
                    Some(2) => ("dashboard.sparkline_legend_major", "bar bar-major"),
                    Some(_) => ("dashboard.sparkline_legend_critical", "bar bar-crit"),
                };
                SparklineDay {
                    class,
                    date: date.format("%Y-%m-%d").to_string(),
                    status: self.i18n.t(label_key).to_string(),
                }
            })
            .collect()
    }

    /// One label for the whole strip, replacing thirty individual ones.
    fn availability_label(&self, service: &Service) -> String {
        let levels = self.day_levels(service);
        let unknown = levels.iter().filter(|(_, l)| l.is_none()).count();
        let ok = levels.iter().filter(|(_, l)| *l == Some(0)).count();
        let incidents = levels.len() - unknown - ok;
        self.i18n.format_availability(ok, incidents, unknown)
    }

    /// Availability over the days the service actually existed, not over the
    /// whole window. A service with nothing observed yet prints a placeholder:
    /// a fresh instance cannot claim thirty perfect days on its first morning.
    /// One decimal, because thirty day-buckets only ever yield 31 values and a
    /// second decimal would advertise a precision the computation lacks.
    #[allow(clippy::naive_bytecount, clippy::cast_precision_loss)]
    fn uptime_pct(&self, service: &Service) -> String {
        let observed: Vec<u8> = self
            .day_levels(service)
            .into_iter()
            .filter_map(|(_, level)| level)
            .collect();
        // A single observed day is the partial day the service was created on,
        // which is not a measurement. The bars still show whatever happened.
        let total = observed.len();
        if total < 2 {
            return self.i18n.t("dashboard.availability_unknown").to_string();
        }
        let ok_days = observed.iter().filter(|&&level| level == 0).count();
        let pct = ok_days as f64 / total as f64 * 100.0;
        format!("{pct:.1}%")
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

    fn column_width(&self) -> ColumnWidth {
        ColumnWidth::Narrow
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
