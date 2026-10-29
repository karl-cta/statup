//! Database layer: `SQLite` pool, migrations, planner statistics and whole
//! database copies.

mod online_copy;
mod pool;

pub use online_copy::copy_database;
pub use pool::{
    create_pool, latest_migration, open_existing_file, open_existing_pool, optimize, run_migrations,
};

/// Type alias for the `SQLite` connection pool.
pub type DbPool = sqlx::SqlitePool;
