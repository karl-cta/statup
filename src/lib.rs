//! Statup, a self-hosted status page for IT teams.
//!
//! Rust, Axum, `SQLite`, HTMX and Tailwind CSS, rendered on the server.

#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::missing_errors_doc)]

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hasher};
use std::path::Path;
use std::sync::{OnceLock, PoisonError, RwLock};

/// Content fingerprint of each file under `static/`, keyed by its path
/// relative to that directory.
static ASSET_VERSIONS: OnceLock<HashMap<String, String>> = OnceLock::new();

/// The name the host gave this instance, empty until an admin sets one.
static INSTANCE_NAME: RwLock<String> = RwLock::new(String::new());

/// File name of the host's logo, empty until an admin uploads one.
static INSTANCE_LOGO: RwLock<String> = RwLock::new(String::new());

/// Address of the host's logo, when there is one.
pub fn instance_logo_url() -> Option<String> {
    let name = INSTANCE_LOGO
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    (!name.is_empty()).then(|| format!("/uploads/brand/{name}"))
}

/// Stores the logo's file name for every later render.
pub fn set_instance_logo(filename: &str) {
    *INSTANCE_LOGO
        .write()
        .unwrap_or_else(PoisonError::into_inner) = filename.to_string();
}

/// The instance's own name, or an empty string when none was set.
pub fn instance_name() -> String {
    INSTANCE_NAME
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// The instance name when set, otherwise the product name.
pub fn brand_name() -> String {
    let name = instance_name();
    if name.is_empty() {
        "Statup".to_string()
    } else {
        name
    }
}

/// Stores the instance name for every later render.
pub fn set_instance_name(name: &str) {
    *INSTANCE_NAME
        .write()
        .unwrap_or_else(PoisonError::into_inner) = name.trim().to_string();
}

/// URL of a static file with its content fingerprint, so browsers may keep
/// it for a year and still fetch a changed file at once.
pub fn asset(path: &str) -> String {
    match ASSET_VERSIONS.get().and_then(|versions| versions.get(path)) {
        Some(version) => format!("/static/{path}?v={version}"),
        None => format!("/static/{path}"),
    }
}

/// Fingerprints every file under `static/`. Call once at startup, from the
/// directory the server serves.
pub fn init_asset_versions() {
    let mut versions = HashMap::new();
    collect_versions(Path::new("static"), Path::new("static"), &mut versions);
    let _ = ASSET_VERSIONS.set(versions);
}

fn collect_versions(root: &Path, dir: &Path, versions: &mut HashMap<String, String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for path in entries.flatten().map(|entry| entry.path()) {
        if path.is_dir() {
            collect_versions(root, &path, versions);
        } else if let (Ok(bytes), Ok(relative)) = (std::fs::read(&path), path.strip_prefix(root)) {
            let mut hasher = DefaultHasher::new();
            hasher.write(&bytes);
            let key = relative.to_string_lossy().replace('\\', "/");
            versions.insert(key, format!("{:x}", hasher.finish() & 0xffff_ffff));
        }
    }
}

pub mod cli;
pub mod clock;
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

    /// An in-memory database with every migration applied.
    ///
    /// # Panics
    /// When the pool or a migration fails. Test-only.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_assets_keep_their_plain_path() {
        assert_eq!(asset("nothing/here.css"), "/static/nothing/here.css");
    }

    #[test]
    fn versions_follow_the_content() {
        let dir = std::env::temp_dir().join(format!("statup-assets-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("js")).unwrap();
        std::fs::write(dir.join("js/a.js"), "one").unwrap();
        std::fs::write(dir.join("b.css"), "two").unwrap();
        let mut versions = HashMap::new();
        collect_versions(&dir, &dir, &mut versions);
        assert!(versions.contains_key("js/a.js"));
        assert_ne!(versions.get("js/a.js"), versions.get("b.css"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
