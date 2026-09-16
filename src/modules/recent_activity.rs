//! Activity card: the latest incidents and announcements, by day.

use askama::Template;
use async_trait::async_trait;

use crate::error::AppError;
use crate::i18n::I18n;
use crate::models::{DayGroup, group_by_day};
use crate::repositories::EventRepository;

use super::{ColumnWidth, Module, ModuleContext, ModuleRenderContext, render_template};

const LIMIT: i64 = 10;

pub struct RecentActivityModule;

#[derive(Template)]
#[template(path = "modules/recent_activity.html")]
struct RecentActivityTemplate {
    groups: Vec<DayGroup>,
    i18n: I18n,
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

    fn default_position(&self) -> i64 {
        30
    }

    fn column_width(&self) -> ColumnWidth {
        ColumnWidth::Wide
    }

    async fn render(&self, ctx: &ModuleRenderContext<'_>) -> Result<String, AppError> {
        let events = EventRepository::list_recent_activity(ctx.pool, LIMIT).await?;
        let template = RecentActivityTemplate {
            groups: group_by_day(events, ctx.i18n),
            i18n: ctx.i18n.clone(),
        };
        render_template(self.id(), &template)
    }
}
