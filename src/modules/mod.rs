//! Dashboard module engine.
//!
//! A `Module` is a self-contained dashboard block (HTML fragment + optional
//! config page) rendered in a given context (public page or admin back-office).
//! Modules are registered at startup through the builtin registry. Third-party
//! (open-core premium) modules will implement the same trait in a separate
//! crate and register themselves via a feature flag.
//!
//! The trait is the only contract between the core and future extensions.

use std::collections::BTreeMap;
use std::fmt;

use async_trait::async_trait;
use serde_json::Value;

use crate::db::DbPool;
use crate::error::AppError;
use crate::i18n::I18n;
use crate::models::User;

pub mod recent_activity;
pub mod scheduled_maintenances;
pub mod services;
pub mod status_banner;

/// Where a module is allowed to appear.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModuleContext {
    /// Public status page (no auth required).
    Public,
    /// Admin back-office dashboard.
    Admin,
}

impl ModuleContext {
    /// String representation used in storage and URLs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Admin => "admin",
        }
    }

    /// Parse from the stored string. `None` for unknown values.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "public" => Some(Self::Public),
            "admin" => Some(Self::Admin),
            _ => None,
        }
    }
}

impl fmt::Display for ModuleContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Per-render context passed to a module's `render` call.
pub struct ModuleRenderContext<'a> {
    pub pool: &'a DbPool,
    pub user: Option<&'a User>,
    pub i18n: &'a I18n,
    pub context: ModuleContext,
    pub config: &'a Value,
}

/// Intrinsic width a module asks for when laid out in the dashboard flex row.
/// The dashboard renders modules in saved order; each keeps its own width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnWidth {
    /// Full width row, locked at the top of the dashboard (banner).
    Full,
    /// Flexible column, takes remaining space in the flex row.
    Wide,
    /// Fixed 260px sidebar column.
    Narrow,
}

impl ColumnWidth {
    pub fn is_pinned_top(self) -> bool {
        matches!(self, Self::Full)
    }
}

/// Async rendering contract implemented by every dashboard module.
///
/// The returned HTML fragment is trusted and assembled into the dashboard
/// template. Modules MUST escape any user-provided content themselves
/// (use Askama templates, do not concatenate raw strings).
#[async_trait]
pub trait Module: Send + Sync + 'static {
    /// Stable id, used for storage and URLs. Must be unique in the registry.
    fn id(&self) -> &'static str;

    /// i18n key for the module display name.
    fn name_key(&self) -> &'static str;

    /// i18n key for a short description shown in the layout editor.
    fn description_key(&self) -> &'static str;

    /// Contexts where this module is available.
    fn contexts(&self) -> &'static [ModuleContext];

    /// Render the module body. Return the raw HTML fragment.
    async fn render(&self, ctx: &ModuleRenderContext<'_>) -> Result<String, AppError>;

    /// Whether the module is enabled by default when seeding a new layout.
    fn default_enabled(&self, _context: ModuleContext) -> bool {
        true
    }

    /// Default ordering hint, used when seeding a new layout. Lower comes first.
    fn default_position(&self, _context: ModuleContext) -> i64 {
        100
    }

    /// Intrinsic width the module asks for in the dashboard flex row.
    /// Defaults to `Wide` (flex-1). Banner-like modules should return `Full`.
    fn column_width(&self) -> ColumnWidth {
        ColumnWidth::Wide
    }
}

/// Holds every module compiled into the binary. Built once at startup.
pub struct ModuleRegistry {
    modules: BTreeMap<&'static str, Box<dyn Module>>,
}

impl ModuleRegistry {
    /// Build the registry with every builtin module.
    pub fn builtin() -> Self {
        let mut registry = Self {
            modules: BTreeMap::new(),
        };
        registry.register(Box::new(status_banner::StatusBannerModule));
        registry.register(Box::new(services::ServicesModule));
        registry.register(Box::new(recent_activity::RecentActivityModule));
        registry.register(Box::new(
            scheduled_maintenances::ScheduledMaintenancesModule,
        ));
        registry
    }

    fn register(&mut self, module: Box<dyn Module>) {
        let id = module.id();
        assert!(!self.modules.contains_key(id), "duplicate module id: {id}");
        self.modules.insert(id, module);
    }

    /// Lookup a module by id.
    pub fn get(&self, id: &str) -> Option<&dyn Module> {
        self.modules.get(id).map(Box::as_ref)
    }

    /// Iterate over all modules available in the given context.
    pub fn for_context(&self, context: ModuleContext) -> Vec<&dyn Module> {
        self.modules
            .values()
            .filter(|m| m.contexts().contains(&context))
            .map(Box::as_ref)
            .collect()
    }

    /// All registered modules, sorted by id.
    pub fn all(&self) -> Vec<&dyn Module> {
        self.modules.values().map(Box::as_ref).collect()
    }
}

impl Default for ModuleRegistry {
    fn default() -> Self {
        Self::builtin()
    }
}
