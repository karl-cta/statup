//! Recent activity module.
//!
//! Central stream of the most recent events, grouped by day. Config blob
//! supports a `limit` integer overriding the default window.

use askama::Template;
use async_trait::async_trait;

use crate::error::AppError;
use crate::i18n::I18n;
use crate::models::EventSummary;
use crate::repositories::EventRepository;

use super::{Module, ModuleContext, ModuleRenderContext};

const DEFAULT_LIMIT: i64 = 10;

pub struct RecentActivityModule;

pub struct DayGroup {
    pub label: String,
    pub events: Vec<EventSummary>,
}

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

fn group_by_day(events: Vec<EventSummary>, i18n: &I18n) -> Vec<DayGroup> {
    let mut groups: Vec<DayGroup> = Vec::new();
    let mut current_date = None;
    for event in events {
        let date = event.created_at.date_naive();
        if Some(date) != current_date {
            groups.push(DayGroup {
                label: i18n.date_label(&date),
                events: Vec::new(),
            });
            current_date = Some(date);
        }
        if let Some(last) = groups.last_mut() {
            last.events.push(event);
        }
    }
    groups
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
