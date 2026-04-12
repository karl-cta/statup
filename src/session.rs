//! Session management, SQLite-backed session store with cleanup.

use time::Duration;
use tokio::task::AbortHandle;
use tower_sessions::cookie::SameSite;
use tower_sessions::{Expiry, SessionManagerLayer};
use tower_sessions_sqlx_store::SqliteStore;

use crate::config::Config;
use crate::db::DbPool;

/// Session cookie key for the authenticated user ID.
pub const USER_ID_KEY: &str = "user_id";

/// Create the session store backed by `SQLite`.
///
/// Runs the store migration (creates the `tower_sessions` table if absent)
/// and returns the store ready for use.
///
/// # Errors
///
/// Returns `sqlx::Error` if the migration fails.
pub async fn create_session_store(pool: &DbPool) -> Result<SqliteStore, sqlx::Error> {
    let store = SqliteStore::new(pool.clone());
    store.migrate().await?;
    Ok(store)
}

/// Build the `SessionManagerLayer` with cookie settings from config.
///
/// Cookie settings:
/// - `HttpOnly`: true (no JS access)
/// - `SameSite`: Lax (CSRF protection)
/// - `Secure`: false for dev, should be true behind HTTPS in prod
/// - Expiry: `OnInactivity` with the configured session lifetime
pub fn session_layer(store: SqliteStore, config: &Config) -> SessionManagerLayer<SqliteStore> {
    let expiry_secs = config.session_expiry.as_secs();

    #[allow(clippy::cast_possible_wrap)]
    let expiry = Expiry::OnInactivity(Duration::seconds(expiry_secs as i64));

    SessionManagerLayer::new(store)
        .with_secure(false) // Set to true when behind HTTPS reverse proxy
        .with_same_site(SameSite::Lax)
        .with_http_only(true)
        .with_expiry(expiry)
}

/// Spawn a background task that continuously deletes expired sessions.
///
/// Returns the `AbortHandle` so the caller can cancel the task on shutdown.
pub fn spawn_cleanup_task(store: SqliteStore) -> AbortHandle {
    use tower_sessions::session_store::ExpiredDeletion;

    let task =
        tokio::task::spawn(store.continuously_delete_expired(tokio::time::Duration::from_secs(60)));
    task.abort_handle()
}
