//! Repository for the `dashboard_layouts` table: which modules the status
//! page shows, and in what order.

use crate::db::DbPool;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LayoutEntry {
    pub module_id: String,
    pub position: i64,
    pub enabled: bool,
    /// JSON settings of the module on the page, `width` among them.
    pub config: String,
}

pub struct DashboardLayoutRepository;

impl DashboardLayoutRepository {
    pub async fn list(pool: &DbPool) -> Result<Vec<LayoutEntry>, sqlx::Error> {
        sqlx::query_as::<_, LayoutEntry>(
            "SELECT module_id, position, enabled, config FROM dashboard_layouts \
             ORDER BY position ASC, id ASC",
        )
        .fetch_all(pool)
        .await
    }

    pub async fn insert_if_missing(
        pool: &DbPool,
        module_id: &str,
        position: i64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO dashboard_layouts (module_id, position) VALUES (?, ?) \
             ON CONFLICT (module_id) DO NOTHING",
        )
        .bind(module_id)
        .bind(position)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Rewrites the positions in the given order, in one transaction.
    pub async fn save_order(pool: &DbPool, module_ids: &[String]) -> Result<(), sqlx::Error> {
        let mut tx = pool.begin().await?;
        for (position, module_id) in (0_i64..).zip(module_ids) {
            sqlx::query(
                "UPDATE dashboard_layouts SET position = ?, updated_at = datetime('now') \
                 WHERE module_id = ?",
            )
            .bind(position)
            .bind(module_id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await
    }

    pub async fn set_enabled(
        pool: &DbPool,
        module_id: &str,
        enabled: bool,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE dashboard_layouts SET enabled = ?, updated_at = datetime('now') \
             WHERE module_id = ?",
        )
        .bind(enabled)
        .bind(module_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Stores the width chosen for a module.
    pub async fn set_width(pool: &DbPool, module_id: &str, width: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE dashboard_layouts \
             SET config = json_set(config, '$.width', ?), updated_at = datetime('now') \
             WHERE module_id = ?",
        )
        .bind(width)
        .bind(module_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Stores what an administrator unticked in a module's settings.
    pub async fn set_hidden(
        pool: &DbPool,
        module_id: &str,
        hidden: &[String],
    ) -> Result<(), sqlx::Error> {
        let values = serde_json::to_string(hidden).unwrap_or_else(|_| "[]".to_string());
        sqlx::query(
            "UPDATE dashboard_layouts \
             SET config = json_set(config, '$.hide', json(?)), updated_at = datetime('now') \
             WHERE module_id = ?",
        )
        .bind(values)
        .bind(module_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Removes the rows of modules this binary does not ship.
    pub async fn prune_unknown(pool: &DbPool, known_ids: &[&str]) -> Result<(), sqlx::Error> {
        let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new("DELETE FROM dashboard_layouts");
        if !known_ids.is_empty() {
            qb.push(" WHERE module_id NOT IN (");
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
        DashboardLayoutRepository::insert_if_missing(&pool, "banner", 10)
            .await
            .unwrap();
        DashboardLayoutRepository::insert_if_missing(&pool, "banner", 99)
            .await
            .unwrap();
        let rows = DashboardLayoutRepository::list(&pool).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].position, 10);
        assert!(rows[0].enabled);
    }

    #[tokio::test]
    async fn save_order_rewrites_positions() {
        let pool = test_pool().await;
        for (position, id) in (0_i64..).zip(["a", "b", "c"]) {
            DashboardLayoutRepository::insert_if_missing(&pool, id, position)
                .await
                .unwrap();
        }
        let order = ["c", "a", "b"].map(String::from);
        DashboardLayoutRepository::save_order(&pool, &order)
            .await
            .unwrap();
        let rows = DashboardLayoutRepository::list(&pool).await.unwrap();
        let ids: Vec<&str> = rows.iter().map(|r| r.module_id.as_str()).collect();
        assert_eq!(ids, vec!["c", "a", "b"]);
    }

    #[tokio::test]
    async fn prune_drops_removed_modules() {
        let pool = test_pool().await;
        for id in ["kept", "gone"] {
            DashboardLayoutRepository::insert_if_missing(&pool, id, 0)
                .await
                .unwrap();
        }
        DashboardLayoutRepository::prune_unknown(&pool, &["kept"])
            .await
            .unwrap();
        let rows = DashboardLayoutRepository::list(&pool).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].module_id, "kept");
    }
}
