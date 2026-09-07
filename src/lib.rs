//! Statup - Internal IT/Ops status page.
//!
//! A self-hostable status page built with Rust, Axum, `SQLite`, HTMX and Tailwind CSS.

// Clippy strict lints
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::missing_errors_doc)]

use std::sync::{OnceLock, PoisonError, RwLock};

static CSS_VERSION: OnceLock<String> = OnceLock::new();

/// The name the host gave this instance, empty until an admin sets one. Kept
/// in memory like the CSS version so templates can read it through a function
/// instead of every page struct carrying one more field.
static INSTANCE_NAME: RwLock<String> = RwLock::new(String::new());

/// The instance's own name, or an empty string when none was set.
pub fn instance_name() -> String {
    INSTANCE_NAME
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// What the masthead and the tab titles display: the instance's name when the
/// host set one, otherwise the product's. A self-hosted page should carry the
/// host's identity to its visitors, not ours.
pub fn brand_name() -> String {
    let name = instance_name();
    if name.is_empty() {
        "Statup".to_string()
    } else {
        name
    }
}

/// Stores the instance name for every later render. Called once at startup
/// from the settings table, then whenever an admin saves it.
pub fn set_instance_name(name: &str) {
    *INSTANCE_NAME
        .write()
        .unwrap_or_else(PoisonError::into_inner) = name.trim().to_string();
}

/// Returns a short version tag for CSS cache-busting (file mtime as unix seconds).
pub fn css_version() -> &'static str {
    CSS_VERSION.get().map_or("0", |s| s.as_str())
}

/// Computes and stores the CSS file version. Call once at startup.
pub fn init_css_version() {
    let version = std::fs::metadata("static/css/style.css")
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or_else(|| "0".to_string(), |d| d.as_secs().to_string());
    let _ = CSS_VERSION.set(version);
}

pub mod config;
pub mod db;
pub mod error;
pub mod i18n;
pub mod middleware;
pub mod models;
pub mod modules;
pub mod repositories;
pub mod routes;
pub mod services;
pub mod session;
pub mod state;

#[cfg(test)]
pub mod test_helpers {
    use crate::db::DbPool;

    /// Create an in-memory `SQLite` pool with all migrations applied.
    ///
    /// # Panics
    /// Panics if pool creation or migration application fails. Test-only.
    pub async fn test_pool() -> DbPool {
        let pool = crate::db::create_pool("sqlite::memory:", 1)
            .await
            .expect("Failed to create test pool");
        crate::db::run_migrations(&pool)
            .await
            .expect("Failed to run migrations");
        pool
    }
}
