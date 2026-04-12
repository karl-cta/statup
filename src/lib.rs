//! Statup - Internal IT/Ops status page.
//!
//! A self-hostable status page built with Rust, Axum, `SQLite`, HTMX and Tailwind CSS.

// Clippy strict lints
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::missing_errors_doc)]

use std::sync::OnceLock;

static CSS_VERSION: OnceLock<String> = OnceLock::new();

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
pub mod repositories;
pub mod routes;
pub mod services;
pub mod session;
pub mod state;

#[cfg(test)]
pub mod test_helpers {
    use crate::db::DbPool;

    /// Create an in-memory SQLite pool with all migrations applied.
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
