//! Repository for the `dashboard_layouts` table: which modules each
//! dashboard shows, and in what order.

use crate::db::DbPool;
use crate::modules::ModuleContext;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LayoutEntry {
    pub module_id: String,
    pub position: i64,
    pub enabled: bool,
    /// JSON settings of the module on this dashboard, `width` among them.
    pub config: String,
}

pub struct DashboardLayoutRepository;

impl DashboardLayoutRepository {
    pub async fn list(
        pool: &DbPool,
        context: ModuleContext,
    ) -> Result<Vec<LayoutEntry>, sqlx::Error> {
        sqlx::query_as::<_, LayoutEntry>(
            "SELECT module_id, position, enabled, config FROM dashboard_layouts \
             WHERE context = ? AND user_id IS NULL ORDER BY position ASC, id ASC",
        )
        .bind(context.as_str())
        .fetch_all(pool)
        .await
    }

    pub async fn insert_if_missing(
        pool: &DbPool,
        context: ModuleContext,
        module_id: &str,
        position: i64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO dashboard_layouts (context, user_id, module_id, position, enabled) \
             SELECT ?, NULL, ?, ?, 1 WHERE NOT EXISTS ( \
                 SELECT 1 FROM dashboard_layouts \
                 WHERE context = ? AND user_id IS NULL AND module_id = ?)",
        )
        .bind(context.as_str())
        .bind(module_id)
        .bind(position)
        .bind(context.as_str())
        .bind(module_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Rewrites the positions in the given order, in one transaction.
    pub async fn save_order(
        pool: &DbPool,
        context: ModuleContext,
        module_ids: &[String],
    ) -> Result<(), sqlx::Error> {
        let mut tx = pool.begin().await?;
        for (position, module_id) in (0_i64..).zip(module_ids) {
            sqlx::query(
                "UPDATE dashboard_layouts SET position = ?, updated_at = datetime('now') \
                 WHERE context = ? AND user_id IS NULL AND module_id = ?",
            )
            .bind(position)
            .bind(context.as_str())
            .bind(module_id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await
    }

    pub async fn set_enabled(
        pool: &DbPool,
        context: ModuleContext,
        module_id: &str,
        enabled: bool,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE dashboard_layouts SET enabled = ?, updated_at = datetime('now') \
             WHERE context = ? AND user_id IS NULL AND module_id = ?",
        )
        .bind(enabled)
        .bind(context.as_str())
        .bind(module_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Stores the width chosen for a module on a dashboard.
    pub async fn set_width(
        pool: &DbPool,
        context: ModuleContext,
        module_id: &str,
        width: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE dashboard_layouts \
             SET config = json_set(config, '$.width', ?), updated_at = datetime('now') \
             WHERE context = ? AND user_id IS NULL AND module_id = ?",
        )
        .bind(width)
        .bind(context.as_str())
        .bind(module_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Removes the rows of modules this binary does not ship.
    pub async fn prune_unknown(
        pool: &DbPool,
        context: ModuleContext,
        known_ids: &[&str],
    ) -> Result<(), sqlx::Error> {
        let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "DELETE FROM dashboard_layouts WHERE user_id IS NULL AND context = ",
        );
        qb.push_bind(context.as_str());
        if !known_ids.is_empty() {
            qb.push(" AND module_id NOT IN (");
            let mut ids = qb.separated(", ");
            for id in known_ids {
                ids.push_bind(*id);
            }
            qb.push(")");
        }
        qb.build().execute(pool).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::test_pool;

    #[tokio::test]
    async fn insert_is_idempotent() {
        let pool = test_pool().await;
        let context = ModuleContext::Public;
        DashboardLayoutRepository::insert_if_missing(&pool, context, "banner", 10)
            .await
            .unwrap();
        DashboardLayoutRepository::insert_if_missing(&pool, context, "banner", 99)
            .await
            .unwrap();
        let rows = DashboardLayoutRepository::list(&pool, context)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].position, 10);
        assert!(rows[0].enabled);
    }

    #[tokio::test]
    async fn save_order_rewrites_positions() {
        let pool = test_pool().await;
        let context = ModuleContext::Admin;
        for (position, id) in (0_i64..).zip(["a", "b", "c"]) {
            DashboardLayoutRepository::insert_if_missing(&pool, context, id, position)
                .await
                .unwrap();
        }
        let order = ["c", "a", "b"].map(String::from);
        DashboardLayoutRepository::save_order(&pool, context, &order)
            .await
            .unwrap();
        let rows = DashboardLayoutRepository::list(&pool, context)
            .await
            .unwrap();
        let ids: Vec<&str> = rows.iter().map(|r| r.module_id.as_str()).collect();
        assert_eq!(ids, vec!["c", "a", "b"]);
    }

    #[tokio::test]
    async fn prune_drops_removed_modules() {
        let pool = test_pool().await;
        let context = ModuleContext::Public;
        for id in ["kept", "gone"] {
            DashboardLayoutRepository::insert_if_missing(&pool, context, id, 0)
                .await
                .unwrap();
        }
        DashboardLayoutRepository::prune_unknown(&pool, context, &["kept"])
            .await
            .unwrap();
        let rows = DashboardLayoutRepository::list(&pool, context)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].module_id, "kept");
    }
}
