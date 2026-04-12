//! Database layer - `SQLite` connection pool and utilities.

mod pool;

pub use pool::{create_pool, run_migrations};

/// Type alias for the `SQLite` connection pool.
pub type DbPool = sqlx::SqlitePool;
