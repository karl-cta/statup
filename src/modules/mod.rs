//! Dashboard modules: self-contained blocks of the status page, in the order
//! an admin saved. Visitors and members see the same page; a module may add
//! what only a member needs.
//!
//! The `Module` trait is the only contract between the core and future
//! extensions, which would register their own modules.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use async_trait::async_trait;

use crate::db::DbPool;
use crate::error::AppError;
use crate::i18n::I18n;
use crate::models::User;

pub mod recent_activity;
pub mod scheduled_maintenances;
pub mod services;
pub mod status_banner;

static REGISTRY: LazyLock<ModuleRegistry> = LazyLock::new(ModuleRegistry::builtin);

pub struct ModuleRenderContext<'a> {
    pub pool: &'a DbPool,
    pub user: Option<&'a User>,
    pub i18n: &'a I18n,
    /// Where readers find the page.
    pub page_address: &'a str,
    /// What an administrator unticked in the module's settings; `None`
    /// until they choose. Kept as what is left out, so that something new,
    /// a service added later, shows.
    pub hidden: Option<&'a [String]>,
}

/// One thing an administrator may show in a module or leave out.
pub struct ModuleOption {
    pub value: String,
    pub label: String,
    pub shown: bool,
}

impl ModuleRenderContext<'_> {
    pub fn can_publish(&self) -> bool {
        self.user.is_some_and(|u| u.role.can_publish())
    }
}

/// Width a module takes in the dashboard row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnWidth {
    /// Full width: the pinned banner, or a module given its own row.
    Full,
    /// Half the row.
    Wide,
    /// A quarter of the row.
    Narrow,
}

impl ColumnWidth {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Wide => "wide",
            Self::Narrow => "narrow",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Full, Self::Wide, Self::Narrow]
            .into_iter()
            .find(|w| w.as_str() == s)
    }
}

/// Rendering contract of a dashboard module. The returned fragment is
/// inserted as is, so modules render through Askama templates, which escape
/// every value.
#[async_trait]
pub trait Module: Send + Sync + 'static {
    /// Stable id, used in storage and URLs.
    fn id(&self) -> &'static str;

    fn name_key(&self) -> &'static str;

    fn description_key(&self) -> &'static str;

    async fn render(&self, ctx: &ModuleRenderContext<'_>) -> Result<String, AppError>;

    /// What an administrator may show or leave out, with what is shown now.
    /// Empty for a module without such a choice.
    async fn options(
        &self,
        _pool: &DbPool,
        _i18n: &I18n,
        _hidden: Option<&[String]>,
    ) -> Result<Vec<ModuleOption>, AppError> {
        Ok(Vec::new())
    }

    /// Position given to the module in a new layout, lower first.
    fn default_position(&self) -> i64;

    fn column_width(&self) -> ColumnWidth {
        ColumnWidth::Wide
    }
}

/// Every module compiled into the binary.
pub struct ModuleRegistry {
    modules: BTreeMap<&'static str, Box<dyn Module>>,
}

impl ModuleRegistry {
    /// The registry shared by every request.
    pub fn global() -> &'static Self {
        &REGISTRY
    }

    fn builtin() -> Self {
        let modules: [Box<dyn Module>; 4] = [
            Box::new(status_banner::StatusBannerModule),
            Box::new(services::ServicesModule),
            Box::new(recent_activity::RecentActivityModule),
            Box::new(scheduled_maintenances::ScheduledMaintenancesModule),
        ];
        Self {
            modules: modules.into_iter().map(|m| (m.id(), m)).collect(),
        }
    }

    pub fn get(&self, id: &str) -> Option<&dyn Module> {
        self.modules.get(id).map(Box::as_ref)
    }

    pub fn all(&self) -> Vec<&dyn Module> {
        self.modules.values().map(Box::as_ref).collect()
    }
}

/// Renders a module template, naming the module when it fails.
pub(crate) fn render_template(
    id: &str,
    template: &impl askama::Template,
) -> Result<String, AppError> {
    template
        .render()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("{id} render: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_module_is_registered_once() {
        let registry = ModuleRegistry::global();
        for id in [
            "status_banner",
            "services",
            "recent_activity",
            "scheduled_maintenances",
        ] {
            assert!(registry.get(id).is_some(), "{id}");
        }
        assert_eq!(registry.all().len(), 4);
    }
}
