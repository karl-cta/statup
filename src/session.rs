//! Session management, SQLite-backed session store with cleanup.

use time::Duration;
use tokio::task::AbortHandle;
use tower_sessions::cookie::SameSite;
use tower_sessions::{Expiry, Session, SessionManagerLayer};
use tower_sessions_sqlx_store::SqliteStore;

use crate::config::Config;
use crate::db::DbPool;
use crate::error::AppError;

/// Session cookie key for the authenticated user ID.
pub const USER_ID_KEY: &str = "user_id";

/// Session key for the stamp of the password the session was opened with.
const CREDENTIAL_KEY: &str = "credential";

/// The tail of the Argon2 hash output. It changes with every new password
/// (fresh salt), and is too short to be worth anything outside the database.
fn credential_stamp(password_hash: &str) -> &str {
    let start = password_hash.len().saturating_sub(16);
    password_hash.get(start..).unwrap_or(password_hash)
}

/// Ties the session to the current password, so that a password set
/// elsewhere (a reset by the host, a change on another device) signs it out.
pub async fn stamp_credential(session: &Session, password_hash: &str) -> Result<(), AppError> {
    session
        .insert(CREDENTIAL_KEY, credential_stamp(password_hash))
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("session insert failed: {e}")))
}

/// Whether the session was opened with the password the account has now.
pub async fn credential_matches(session: &Session, password_hash: &str) -> bool {
    session
        .get::<String>(CREDENTIAL_KEY)
        .await
        .ok()
        .flatten()
        .is_some_and(|stamp| stamp == credential_stamp(password_hash))
}

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
/// - `Secure`: when `PUBLIC_URL` is an https address
/// - Expiry: `OnInactivity` with the configured session lifetime
pub fn session_layer(store: SqliteStore, config: &Config) -> SessionManagerLayer<SqliteStore> {
    let expiry_secs = config.session_expiry.as_secs();

    #[allow(clippy::cast_possible_wrap)]
    let expiry = Expiry::OnInactivity(Duration::seconds(expiry_secs as i64));

    SessionManagerLayer::new(store)
        .with_secure(config.secure_cookies())
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
