//! Repository for the `dashboard_layouts` table.
//!
//! Stores one row per (context, `user_id`, `module_id`). `user_id` is NULL for
//! the admin-defined default layout. Per-user overrides (non-null `user_id`)
//! are supported at the schema level but not yet used by the engine.

use sqlx::FromRow;

use crate::db::DbPool;
use crate::error::AppError;

use super::super::modules::ModuleContext;

#[derive(Debug, Clone, FromRow)]
pub struct DashboardLayoutRow {
    pub id: i64,
    pub context: String,
    pub user_id: Option<i64>,
    pub module_id: String,
    pub position: i64,
    pub enabled: bool,
    pub config: String,
}

pub struct DashboardLayoutRepository;

impl DashboardLayoutRepository {
    /// List the default layout rows for a context, ordered by position.
    pub async fn list_default(
        pool: &DbPool,
        context: ModuleContext,
    ) -> Result<Vec<DashboardLayoutRow>, AppError> {
        let rows = sqlx::query_as::<_, DashboardLayoutRow>(
            "SELECT id, context, user_id, module_id, position, enabled, config
             FROM dashboard_layouts
             WHERE context = ? AND user_id IS NULL
             ORDER BY position ASC, id ASC",
        )
        .bind(context.as_str())
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    /// Insert a layout row for a module if none exists yet.
    pub async fn insert_default_if_missing(
        pool: &DbPool,
        context: ModuleContext,
        module_id: &str,
        position: i64,
        enabled: bool,
        config: &str,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO dashboard_layouts (context, user_id, module_id, position, enabled, config)
             SELECT ?, NULL, ?, ?, ?, ?
             WHERE NOT EXISTS (
                 SELECT 1 FROM dashboard_layouts
                 WHERE context = ? AND user_id IS NULL AND module_id = ?
             )",
        )
        .bind(context.as_str())
        .bind(module_id)
        .bind(position)
        .bind(i64::from(enabled))
        .bind(config)
        .bind(context.as_str())
        .bind(module_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Replace the ordering of default-layout rows in bulk. Missing modules
    /// are left untouched. Transactional.
    pub async fn save_default_order(
        pool: &DbPool,
        context: ModuleContext,
        ordered_module_ids: &[String],
    ) -> Result<(), AppError> {
        let mut tx = pool.begin().await?;
        for (index, module_id) in ordered_module_ids.iter().enumerate() {
            let position = i64::try_from(index).unwrap_or(i64::MAX);
            sqlx::query(
                "UPDATE dashboard_layouts
                 SET position = ?, updated_at = datetime('now')
                 WHERE context = ? AND user_id IS NULL AND module_id = ?",
            )
            .bind(position)
            .bind(context.as_str())
            .bind(module_id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Toggle the enabled flag for a default-layout row.
    pub async fn set_default_enabled(
        pool: &DbPool,
        context: ModuleContext,
        module_id: &str,
        enabled: bool,
    ) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE dashboard_layouts
             SET enabled = ?, updated_at = datetime('now')
             WHERE context = ? AND user_id IS NULL AND module_id = ?",
        )
        .bind(i64::from(enabled))
        .bind(context.as_str())
        .bind(module_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Remove layout rows whose module is no longer registered.
    pub async fn prune_unknown(
        pool: &DbPool,
        context: ModuleContext,
        known_module_ids: &[&'static str],
    ) -> Result<(), AppError> {
        if known_module_ids.is_empty() {
            sqlx::query("DELETE FROM dashboard_layouts WHERE context = ? AND user_id IS NULL")
                .bind(context.as_str())
                .execute(pool)
                .await?;
            return Ok(());
        }
        let placeholders = known_module_ids
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "DELETE FROM dashboard_layouts
             WHERE context = ? AND user_id IS NULL
               AND module_id NOT IN ({placeholders})"
        );
        let mut query = sqlx::query(&sql).bind(context.as_str());
        for id in known_module_ids {
            query = query.bind(*id);
        }
        query.execute(pool).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::test_pool;

    #[tokio::test]
    async fn insert_default_is_idempotent() {
        let pool = test_pool().await;
        DashboardLayoutRepository::insert_default_if_missing(
            &pool,
            ModuleContext::Public,
            "status_banner",
            10,
            true,
            "{}",
        )
        .await
        .expect("insert");
        DashboardLayoutRepository::insert_default_if_missing(
            &pool,
            ModuleContext::Public,
            "status_banner",
            99,
            false,
            "{\"ignored\":true}",
        )
        .await
        .expect("re-insert");
        let rows = DashboardLayoutRepository::list_default(&pool, ModuleContext::Public)
            .await
            .expect("list");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].position, 10);
        assert!(rows[0].enabled);
    }

    #[tokio::test]
    async fn save_default_order_rewrites_positions() {
        let pool = test_pool().await;
        for (index, id) in ["a", "b", "c"].iter().enumerate() {
            DashboardLayoutRepository::insert_default_if_missing(
                &pool,
                ModuleContext::Admin,
                id,
                i64::try_from(index).unwrap(),
                true,
                "{}",
            )
            .await
            .expect("seed");
        }
        DashboardLayoutRepository::save_default_order(
            &pool,
            ModuleContext::Admin,
            &["c".to_string(), "a".to_string(), "b".to_string()],
        )
        .await
        .expect("save");
        let rows = DashboardLayoutRepository::list_default(&pool, ModuleContext::Admin)
            .await
            .expect("list");
        let order: Vec<&str> = rows.iter().map(|r| r.module_id.as_str()).collect();
        assert_eq!(order, vec!["c", "a", "b"]);
    }

    #[tokio::test]
    async fn prune_unknown_drops_removed_modules() {
        let pool = test_pool().await;
        for id in ["kept", "gone"] {
            DashboardLayoutRepository::insert_default_if_missing(
                &pool,
                ModuleContext::Public,
                id,
                0,
                true,
                "{}",
            )
            .await
            .expect("seed");
        }
        DashboardLayoutRepository::prune_unknown(&pool, ModuleContext::Public, &["kept"])
            .await
            .expect("prune");
        let rows = DashboardLayoutRepository::list_default(&pool, ModuleContext::Public)
            .await
            .expect("list");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].module_id, "kept");
    }
}
