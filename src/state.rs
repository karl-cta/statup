//! Shared application state passed to all handlers via Axum's `State` extractor.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::extract::FromRef;

use crate::db::DbPool;
use crate::services::LoginRateLimiter;

/// Application state shared across all request handlers.
#[derive(Clone)]
pub struct AppState {
    /// Database connection pool.
    pub pool: DbPool,
    /// Login rate limiter (shared across handlers).
    pub login_limiter: Arc<LoginRateLimiter>,
    /// Directory for user-uploaded files.
    pub upload_dir: String,
    /// Public mode: read-only pages accessible without login (REQ-16).
    /// Togglable at runtime from the admin panel.
    pub public_mode: Arc<AtomicBool>,
}

impl AppState {
    /// Returns the current public mode setting.
    pub fn is_public_mode(&self) -> bool {
        self.public_mode.load(Ordering::Relaxed)
    }

    /// Toggles the public mode setting and returns the new value.
    pub fn toggle_public_mode(&self) -> bool {
        let prev = self.public_mode.fetch_xor(true, Ordering::Relaxed);
        !prev
    }
}

impl FromRef<AppState> for DbPool {
    fn from_ref(state: &AppState) -> Self {
        state.pool.clone()
    }
}
