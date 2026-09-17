//! Dashboard modules: self-contained blocks shown on the status page or on
//! the signed-in dashboard, in the order an admin saved.
//!
//! The `Module` trait is the only contract between the core and future
//! extensions, which would register their own modules.

use std::collections::BTreeMap;
use std::fmt;
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

/// Where a module may appear.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModuleContext {
    /// The status page anyone may read.
    Public,
    /// The dashboard every signed-in account sees.
    Admin,
}

impl ModuleContext {
    pub const ALL: [Self; 2] = [Self::Public, Self::Admin];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Admin => "admin",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == s)
    }
}

impl fmt::Display for ModuleContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub struct ModuleRenderContext<'a> {
    pub pool: &'a DbPool,
    pub user: Option<&'a User>,
    pub i18n: &'a I18n,
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
    /// Two shares of the row.
    Wide,
    /// One share of the row.
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

    fn contexts(&self) -> &'static [ModuleContext];

    async fn render(&self, ctx: &ModuleRenderContext<'_>) -> Result<String, AppError>;

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

    pub fn for_context(&self, context: ModuleContext) -> Vec<&dyn Module> {
        self.modules
            .values()
            .filter(|m| m.contexts().contains(&context))
            .map(Box::as_ref)
            .collect()
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
        assert_eq!(registry.for_context(ModuleContext::Public).len(), 4);
    }

    #[test]
    fn contexts_round_trip() {
        for context in ModuleContext::ALL {
            assert_eq!(ModuleContext::parse(context.as_str()), Some(context));
        }
        assert!(ModuleContext::parse("other").is_none());
    }
}
