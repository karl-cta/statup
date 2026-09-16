//! `SQLite` connection pool configuration.

use std::str::FromStr;
use std::time::Duration;

use sqlx::SqlitePool;
use sqlx::migrate::MigrateError;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};

/// How long a statement waits for the write lock held by another connection.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Longer than the busy timeout: a request queued behind connections that
/// wait on the write lock must not give up before they do.
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(10);

/// Rows `ANALYZE` samples per index, the value the `SQLite` manual recommends.
const ANALYSIS_LIMIT: u32 = 400;

/// Creates the application pool, and the database file when it is missing.
///
/// Connections use WAL, `synchronous = NORMAL`, foreign keys, a busy timeout,
/// and run `PRAGMA optimize` when they close.
///
/// # Errors
///
/// Returns `sqlx::Error` if the pool cannot be created or the database is unreachable.
pub async fn create_pool(
    database_url: &str,
    max_connections: u32,
) -> Result<SqlitePool, sqlx::Error> {
    let options = connect_options(database_url)?.create_if_missing(true);
    pool_options(max_connections).connect_with(options).await
}

/// Opens a database that must already exist, for maintenance commands: a
/// mistyped path fails instead of creating an empty database.
///
/// # Errors
///
/// Returns `sqlx::Error` if the file does not exist or cannot be opened.
pub async fn open_existing_pool(database_url: &str) -> Result<SqlitePool, sqlx::Error> {
    let options = connect_options(database_url)?.create_if_missing(false);
    pool_options(1).connect_with(options).await
}

fn connect_options(database_url: &str) -> Result<SqliteConnectOptions, sqlx::Error> {
    Ok(SqliteConnectOptions::from_str(database_url)?
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(BUSY_TIMEOUT)
        .analysis_limit(ANALYSIS_LIMIT)
        .optimize_on_close(true, None))
}

fn pool_options(max_connections: u32) -> SqlitePoolOptions {
    SqlitePoolOptions::new()
        .max_connections(max_connections)
        .min_connections(1)
        .acquire_timeout(ACQUIRE_TIMEOUT)
        .idle_timeout(Duration::from_secs(600))
        .max_lifetime(Duration::from_secs(1800))
}

/// Run all pending database migrations.
///
/// # Errors
///
/// Returns `MigrateError` if a migration fails.
pub async fn run_migrations(pool: &SqlitePool) -> Result<(), MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}

/// Gives the query planner statistics. `PRAGMA optimize` only analyzes the
/// tables its own connection has queried, so a database that was never
/// analyzed gets one `ANALYZE`, bounded by the analysis limit.
///
/// # Errors
///
/// Returns `sqlx::Error` if a statement fails.
pub async fn optimize(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let analyzed: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM sqlite_schema WHERE name = 'sqlite_stat1')",
    )
    .fetch_one(pool)
    .await?;
    let statement = if analyzed {
        "PRAGMA optimize"
    } else {
        "ANALYZE"
    };
    sqlx::query(statement).execute(pool).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn maintenance_commands_never_create_a_database() {
        let path = std::env::temp_dir().join(format!("statup-missing-{}.db", uuid::Uuid::new_v4()));
        let url = path.to_string_lossy().to_string();

        assert!(open_existing_pool(&url).await.is_err());
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn optimize_analyzes_a_fresh_database_then_optimizes() {
        let pool = create_pool("sqlite::memory:", 1).await.unwrap();
        run_migrations(&pool).await.unwrap();

        optimize(&pool).await.unwrap();
        optimize(&pool).await.unwrap();

        let analyzed: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM sqlite_schema WHERE name = 'sqlite_stat1')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(analyzed);
    }
}
