//! Recent activity module.
//!
//! Central stream of the most recent events, grouped by day. Config blob
//! supports a `limit` integer overriding the default window.

use askama::Template;
use async_trait::async_trait;

use crate::error::AppError;
use crate::i18n::I18n;
use crate::models::{DayGroup, group_by_day};
use crate::repositories::EventRepository;

use super::{ColumnWidth, Module, ModuleContext, ModuleRenderContext};

const DEFAULT_LIMIT: i64 = 10;

pub struct RecentActivityModule;

#[derive(Template)]
#[template(path = "modules/recent_activity.html")]
struct RecentActivityTemplate {
    groups: Vec<DayGroup>,
    total: usize,
    i18n: I18n,
}

fn read_limit(config: &serde_json::Value) -> i64 {
    config
        .get("limit")
        .and_then(serde_json::Value::as_i64)
        .filter(|l| *l > 0 && *l <= 100)
        .unwrap_or(DEFAULT_LIMIT)
}

#[async_trait]
impl Module for RecentActivityModule {
    fn id(&self) -> &'static str {
        "recent_activity"
    }

    fn name_key(&self) -> &'static str {
        "modules.recent_activity.name"
    }

    fn description_key(&self) -> &'static str {
        "modules.recent_activity.description"
    }

    fn contexts(&self) -> &'static [ModuleContext] {
        &[ModuleContext::Public, ModuleContext::Admin]
    }

    fn default_position(&self, _context: ModuleContext) -> i64 {
        30
    }

    fn column_width(&self) -> ColumnWidth {
        ColumnWidth::Wide
    }

    async fn render(&self, ctx: &ModuleRenderContext<'_>) -> Result<String, AppError> {
        let limit = read_limit(ctx.config);
        let events = EventRepository::list_recent_activity(ctx.pool, limit, 0).await?;
        let total = events.len();
        let groups = group_by_day(events, ctx.i18n);
        let tpl = RecentActivityTemplate {
            groups,
            total,
            i18n: ctx.i18n.clone(),
        };
        tpl.render()
            .map_err(|e| AppError::Internal(anyhow::anyhow!("recent_activity render: {e}")))
    }
}
