//! `SQLite` connection pool configuration.

use sqlx::SqlitePool;
use sqlx::migrate::MigrateError;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use std::str::FromStr;
use std::time::Duration;

/// Create and configure a `SQLite` connection pool.
///
/// Configures:
/// - WAL journal mode for concurrent reads
/// - Foreign keys enabled
/// - Busy timeout of 5 seconds
/// - Synchronous NORMAL for WAL safety
/// - Configurable pool size with idle/lifetime management
///
/// # Errors
///
/// Returns `sqlx::Error` if the pool cannot be created or the database is unreachable.
pub async fn create_pool(
    database_url: &str,
    max_connections: u32,
) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::from_str(database_url)?
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5))
        .create_if_missing(true);

    SqlitePoolOptions::new()
        .max_connections(max_connections)
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(3))
        .idle_timeout(Duration::from_secs(600))
        .max_lifetime(Duration::from_secs(1800))
        .connect_with(options)
        .await
}

/// Run all pending database migrations.
///
/// # Errors
///
/// Returns `MigrateError` if a migration fails.
pub async fn run_migrations(pool: &SqlitePool) -> Result<(), MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}
