//! Database layer: `SQLite` pool, migrations and planner statistics.

mod pool;

pub use pool::{create_pool, open_existing_pool, optimize, run_migrations};

/// Type alias for the `SQLite` connection pool.
pub type DbPool = sqlx::SqlitePool;
