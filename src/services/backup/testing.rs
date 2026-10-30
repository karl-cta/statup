//! A scratch folder and file databases for the backup tests.

use std::path::{Path, PathBuf};

use crate::db::DbPool;

/// A folder with `uploads/icons` and `uploads/brand`, removed on drop.
pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("statup-backup-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("uploads/icons")).unwrap();
        std::fs::create_dir_all(dir.join("uploads/brand")).unwrap();
        Self(dir)
    }

    pub fn uploads(&self) -> String {
        self.0.join("uploads").to_string_lossy().into_owned()
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A migrated file database, as in production: WAL, several connections.
pub async fn file_pool(path: &Path) -> DbPool {
    let url = format!("sqlite://{}", path.display());
    let pool = crate::db::create_pool(&url, 4).await.unwrap();
    crate::db::run_migrations(&pool).await.unwrap();
    pool
}
