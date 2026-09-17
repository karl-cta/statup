//! Dashboard layouts: stored rows reconciled with the modules the binary
//! ships.

use crate::db::DbPool;
use crate::error::AppError;
use crate::modules::{ColumnWidth, Module, ModuleContext, ModuleRegistry};
use crate::repositories::DashboardLayoutRepository;

pub struct ResolvedModule {
    pub module: &'static dyn Module,
    pub enabled: bool,
    /// The width chosen for this dashboard, or the module's own.
    pub width: ColumnWidth,
}

pub struct DashboardLayoutService;

impl DashboardLayoutService {
    /// Stores a row for every shipped module and drops rows of modules that
    /// are gone. Runs once at startup, so page views only read.
    pub async fn reconcile(pool: &DbPool) -> Result<(), AppError> {
        let registry = ModuleRegistry::global();
        for context in ModuleContext::ALL {
            let modules = registry.for_context(context);
            let ids: Vec<&str> = modules.iter().map(|m| m.id()).collect();
            DashboardLayoutRepository::prune_unknown(pool, context, &ids).await?;
            for module in modules {
                DashboardLayoutRepository::insert_if_missing(
                    pool,
                    context,
                    module.id(),
                    module.default_position(),
                )
                .await?;
            }
        }
        Ok(())
    }

    /// The modules of a dashboard in saved order. A module without a stored
    /// row is shown at its default place; the pinned banner is always on.
    pub async fn resolve(
        pool: &DbPool,
        context: ModuleContext,
    ) -> Result<Vec<ResolvedModule>, AppError> {
        let registry = ModuleRegistry::global();
        let stored = DashboardLayoutRepository::list(pool, context).await?;
        let mut entries: Vec<(i64, ResolvedModule)> = registry
            .for_context(context)
            .into_iter()
            .map(|module| {
                let row = stored.iter().find(|r| r.module_id == module.id());
                let position = row.map_or(module.default_position(), |r| r.position);
                let pinned = module.column_width() == ColumnWidth::Full;
                let enabled = pinned || row.is_none_or(|r| r.enabled);
                let width = if pinned {
                    ColumnWidth::Full
                } else {
                    row.and_then(|r| stored_width(&r.config))
                        .unwrap_or_else(|| module.column_width())
                };
                (
                    position,
                    ResolvedModule {
                        module,
                        enabled,
                        width,
                    },
                )
            })
            .collect();
        entries.sort_by_key(|(position, resolved)| (*position, resolved.module.id()));
        Ok(entries.into_iter().map(|(_, resolved)| resolved).collect())
    }
}

fn stored_width(config: &str) -> Option<ColumnWidth> {
    serde_json::from_str::<serde_json::Value>(config)
        .ok()?
        .get("width")?
        .as_str()
        .and_then(ColumnWidth::parse)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::test_pool;

    #[tokio::test]
    async fn resolve_works_before_any_row_exists() {
        let pool = test_pool().await;
        let resolved = DashboardLayoutService::resolve(&pool, ModuleContext::Public)
            .await
            .unwrap();
        assert_eq!(resolved.len(), 4);
        assert_eq!(resolved[0].module.id(), "status_banner");
        assert!(resolved.iter().all(|r| r.enabled));
    }

    #[tokio::test]
    async fn saved_order_and_hidden_modules_are_honoured() {
        let pool = test_pool().await;
        DashboardLayoutService::reconcile(&pool).await.unwrap();
        DashboardLayoutService::reconcile(&pool).await.unwrap();
        let context = ModuleContext::Admin;
        let order = [
            "status_banner",
            "scheduled_maintenances",
            "services",
            "recent_activity",
        ]
        .map(String::from);
        DashboardLayoutRepository::save_order(&pool, context, &order)
            .await
            .unwrap();
        DashboardLayoutRepository::set_enabled(&pool, context, "services", false)
            .await
            .unwrap();
        DashboardLayoutRepository::set_enabled(&pool, context, "status_banner", false)
            .await
            .unwrap();

        let resolved = DashboardLayoutService::resolve(&pool, context)
            .await
            .unwrap();
        let ids: Vec<&str> = resolved.iter().map(|r| r.module.id()).collect();
        assert_eq!(ids, order.iter().map(String::as_str).collect::<Vec<_>>());
        assert!(!resolved[2].enabled);
        assert!(resolved[0].enabled, "the banner cannot be hidden");
    }
}
