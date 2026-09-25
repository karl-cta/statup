//! Dashboard layouts: stored rows reconciled with the modules the binary
//! ships.

use crate::db::DbPool;
use crate::error::AppError;
use crate::i18n::I18n;
use crate::modules::{ColumnWidth, Module, ModuleRegistry};
use crate::repositories::DashboardLayoutRepository;

pub struct ResolvedModule {
    pub module: &'static dyn Module,
    pub enabled: bool,
    /// The width chosen for the page, or the module's own.
    pub width: ColumnWidth,
    /// What an administrator unticked in the module's settings, if they did.
    pub hidden: Option<Vec<String>>,
}

pub struct DashboardLayoutService;

impl DashboardLayoutService {
    /// Stores a row for every shipped module and drops rows of modules that
    /// are gone. Runs once at startup, so page views only read.
    pub async fn reconcile(pool: &DbPool) -> Result<(), AppError> {
        let modules = ModuleRegistry::global().all();
        let ids: Vec<&str> = modules.iter().map(|m| m.id()).collect();
        DashboardLayoutRepository::prune_unknown(pool, &ids).await?;
        for module in modules {
            DashboardLayoutRepository::insert_if_missing(
                pool,
                module.id(),
                module.default_position(),
            )
            .await?;
        }
        Ok(())
    }

    /// Saves the order the administrator left the modules in.
    pub async fn save_order(pool: &DbPool, module_ids: &[String]) -> Result<(), AppError> {
        DashboardLayoutRepository::save_order(pool, module_ids).await?;
        Ok(())
    }

    /// Shows or hides a module. The pinned banner cannot be hidden: a status
    /// page without its status is a blank page.
    pub async fn set_enabled(
        pool: &DbPool,
        module_id: &str,
        enabled: bool,
    ) -> Result<(), AppError> {
        if shipped(module_id)?.column_width() == ColumnWidth::Full {
            return Err(AppError::validation("validation.banner_always_shown"));
        }
        DashboardLayoutRepository::set_enabled(pool, module_id, enabled).await?;
        Ok(())
    }

    /// Gives a module a quarter of the row, half of it, or a row of its own.
    /// The pinned banner keeps its full row.
    pub async fn set_width(
        pool: &DbPool,
        module_id: &str,
        width: &str,
    ) -> Result<ColumnWidth, AppError> {
        let pinned = shipped(module_id)?.column_width() == ColumnWidth::Full;
        let width = ColumnWidth::parse(width)
            .filter(|_| !pinned)
            .ok_or_else(|| AppError::validation("error.invalid_data"))?;
        DashboardLayoutRepository::set_width(pool, module_id, width.as_str()).await?;
        Ok(width)
    }

    /// Saves what a module shows, among the options it offers, as what it
    /// leaves out, so an option that appears later shows. At least one stays
    /// ticked: a card that shows nothing is a blank card.
    pub async fn set_shown(
        pool: &DbPool,
        i18n: &I18n,
        module_id: &str,
        shown: &[String],
    ) -> Result<(), AppError> {
        let offered = shipped(module_id)?.options(pool, i18n, None).await?;
        let valid = !shown.is_empty()
            && shown
                .iter()
                .all(|value| offered.iter().any(|option| &option.value == value));
        if !valid {
            return Err(AppError::validation("error.invalid_data"));
        }
        let hidden: Vec<String> = offered
            .into_iter()
            .map(|option| option.value)
            .filter(|value| !shown.contains(value))
            .collect();
        DashboardLayoutRepository::set_hidden(pool, module_id, &hidden).await?;
        Ok(())
    }

    /// The modules of the page in saved order. A module without a stored row
    /// is shown at its default place; the pinned banner is always on.
    pub async fn resolve(pool: &DbPool) -> Result<Vec<ResolvedModule>, AppError> {
        let stored = DashboardLayoutRepository::list(pool).await?;
        let mut entries: Vec<(i64, ResolvedModule)> = ModuleRegistry::global()
            .all()
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
                let hidden = row.and_then(|r| stored_hidden(&r.config));
                (
                    position,
                    ResolvedModule {
                        module,
                        enabled,
                        width,
                        hidden,
                    },
                )
            })
            .collect();
        entries.sort_by_key(|(position, resolved)| (*position, resolved.module.id()));
        Ok(entries.into_iter().map(|(_, resolved)| resolved).collect())
    }
}

fn stored_hidden(config: &str) -> Option<Vec<String>> {
    serde_json::from_str::<serde_json::Value>(config)
        .ok()?
        .get("hide")?
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
}

fn stored_width(config: &str) -> Option<ColumnWidth> {
    serde_json::from_str::<serde_json::Value>(config)
        .ok()?
        .get("width")?
        .as_str()
        .and_then(ColumnWidth::parse)
}

/// A module this binary ships, by its id.
fn shipped(module_id: &str) -> Result<&'static dyn Module, AppError> {
    ModuleRegistry::global()
        .get(module_id)
        .ok_or(AppError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::test_pool;

    #[tokio::test]
    async fn resolve_works_before_any_row_exists() {
        let pool = test_pool().await;
        let resolved = DashboardLayoutService::resolve(&pool).await.unwrap();
        assert_eq!(resolved.len(), 4);
        assert_eq!(resolved[0].module.id(), "status_banner");
        assert!(resolved.iter().all(|r| r.enabled));
    }

    #[tokio::test]
    async fn saved_order_and_hidden_modules_are_honoured() {
        let pool = test_pool().await;
        DashboardLayoutService::reconcile(&pool).await.unwrap();
        DashboardLayoutService::reconcile(&pool).await.unwrap();
        let order = [
            "status_banner",
            "scheduled_maintenances",
            "services",
            "recent_activity",
        ]
        .map(String::from);
        DashboardLayoutRepository::save_order(&pool, &order)
            .await
            .unwrap();
        DashboardLayoutRepository::set_enabled(&pool, "services", false)
            .await
            .unwrap();
        DashboardLayoutRepository::set_enabled(&pool, "status_banner", false)
            .await
            .unwrap();

        let resolved = DashboardLayoutService::resolve(&pool).await.unwrap();
        let ids: Vec<&str> = resolved.iter().map(|r| r.module.id()).collect();
        assert_eq!(ids, order.iter().map(String::as_str).collect::<Vec<_>>());
        assert!(!resolved[2].enabled);
        assert!(resolved[0].enabled, "the banner cannot be hidden");
    }
}
