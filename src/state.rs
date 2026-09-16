//! Shared application state passed to all handlers via Axum's `State` extractor.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::extract::FromRef;

use crate::config::serves_https;
use crate::db::DbPool;
use crate::services::LoginRateLimiter;

/// Application state shared across all request handlers.
#[derive(Clone)]
pub struct AppState {
    /// Database connection pool.
    pub pool: DbPool,
    /// Failed sign-in attempts per client address.
    pub login_limiter: Arc<LoginRateLimiter>,
    /// Directory for user-uploaded files.
    pub upload_dir: String,
    /// Whether visitors without an account can read the pages. An admin
    /// changes it at runtime.
    pub public_mode: Arc<AtomicBool>,
    /// Read the client address from the headers of a trusted reverse proxy.
    pub trust_proxy_headers: bool,
    /// Absolute address of the instance, for feed links and secure cookies.
    pub public_url: Option<String>,
}

impl AppState {
    /// Returns the current public mode setting.
    pub fn is_public_mode(&self) -> bool {
        self.public_mode.load(Ordering::Relaxed)
    }

    pub fn set_public_mode(&self, enabled: bool) {
        self.public_mode.store(enabled, Ordering::Relaxed);
    }

    /// Whether visitors reach the instance over HTTPS, which decides the
    /// `Secure` cookie flag and the HSTS header.
    pub fn serves_https(&self) -> bool {
        serves_https(self.public_url.as_deref())
    }
}

impl FromRef<AppState> for DbPool {
    fn from_ref(state: &AppState) -> Self {
        state.pool.clone()
    }
}
