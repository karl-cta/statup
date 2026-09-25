//! Activity card: the latest events of the kinds an administrator ticked,
//! by day.

use askama::Template;
use async_trait::async_trait;

use crate::db::DbPool;
use crate::error::AppError;
use crate::i18n::I18n;
use crate::models::{ActivityKind, DayGroup, group_by_day};
use crate::repositories::EventRepository;

use super::{ColumnWidth, Module, ModuleOption, ModuleRenderContext, render_template};

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

    fn default_position(&self) -> i64 {
        30
    }

    fn column_width(&self) -> ColumnWidth {
        ColumnWidth::Wide
    }

    async fn render(&self, ctx: &ModuleRenderContext<'_>) -> Result<String, AppError> {
        let kinds = shown_kinds(ctx.hidden);
        let events = EventRepository::list_recent_activity(ctx.pool, LIMIT, &kinds).await?;
        let template = RecentActivityTemplate {
            groups: group_by_day(events, ctx.i18n),
            i18n: ctx.i18n.clone(),
        };
        render_template(self.id(), &template)
    }

    async fn options(
        &self,
        _pool: &DbPool,
        i18n: &I18n,
        hidden: Option<&[String]>,
    ) -> Result<Vec<ModuleOption>, AppError> {
        let kinds = shown_kinds(hidden);
        Ok(ActivityKind::ALL
            .into_iter()
            .map(|kind| ModuleOption {
                value: kind.as_str().to_string(),
                label: i18n.t(kind.label_key()).to_string(),
                shown: kinds.contains(&kind),
            })
            .collect())
    }
}

/// The kinds not unticked, or the default ones until an administrator
/// chooses.
fn shown_kinds(hidden: Option<&[String]>) -> Vec<ActivityKind> {
    ActivityKind::ALL
        .into_iter()
        .filter(|kind| match hidden {
            Some(values) => !values.iter().any(|v| v == kind.as_str()),
            None => kind.shown_by_default(),
        })
        .collect()
}
