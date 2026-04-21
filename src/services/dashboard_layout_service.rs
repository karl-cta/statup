//! Dashboard layout resolution.
//!
//! Reads the persisted layout rows for a context, reconciles them with the
//! module registry (seeds new modules, prunes removed ones), and returns an
//! ordered list of modules along with their enabled flag and config blob.

use serde_json::Value;

use crate::db::DbPool;
use crate::error::AppError;
use crate::modules::{Module, ModuleContext, ModuleRegistry};
use crate::repositories::DashboardLayoutRepository;

pub struct ResolvedModule<'a> {
    pub module: &'a dyn Module,
    pub enabled: bool,
    pub config: Value,
    pub position: i64,
}

pub struct DashboardLayoutService;

impl DashboardLayoutService {
    /// Return the ordered layout for a context, seeding missing modules and
    /// pruning unknown ones in a single pass.
    pub async fn resolve<'a>(
        pool: &DbPool,
        registry: &'a ModuleRegistry,
        context: ModuleContext,
    ) -> Result<Vec<ResolvedModule<'a>>, AppError> {
        let available: Vec<&'a dyn Module> = registry.for_context(context);
        let known_ids: Vec<&'static str> = available.iter().map(|m| m.id()).collect();
        DashboardLayoutRepository::prune_unknown(pool, context, &known_ids).await?;

        for module in &available {
            DashboardLayoutRepository::insert_default_if_missing(
                pool,
                context,
                module.id(),
                module.default_position(context),
                module.default_enabled(context),
                "{}",
            )
            .await?;
        }

        let rows = DashboardLayoutRepository::list_default(pool, context).await?;
        let mut resolved = Vec::with_capacity(rows.len());
        for row in rows {
            let Some(module) = registry.get(&row.module_id) else {
                continue;
            };
            let config = serde_json::from_str::<Value>(&row.config).unwrap_or(Value::Null);
            resolved.push(ResolvedModule {
                module,
                enabled: row.enabled,
                config,
                position: row.position,
            });
        }
        Ok(resolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::test_pool;

    #[tokio::test]
    async fn resolve_seeds_all_registered_modules() {
        let pool = test_pool().await;
        let registry = ModuleRegistry::builtin();
        let resolved = DashboardLayoutService::resolve(&pool, &registry, ModuleContext::Admin)
            .await
            .expect("resolve");
        assert!(
            !resolved.is_empty(),
            "builtin registry should provide at least one admin module"
        );
    }

    #[tokio::test]
    async fn resolve_is_idempotent() {
        let pool = test_pool().await;
        let registry = ModuleRegistry::builtin();
        let first = DashboardLayoutService::resolve(&pool, &registry, ModuleContext::Public)
            .await
            .expect("first");
        let second = DashboardLayoutService::resolve(&pool, &registry, ModuleContext::Public)
            .await
            .expect("second");
        assert_eq!(first.len(), second.len());
    }
}
