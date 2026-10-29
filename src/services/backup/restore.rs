//! Restoring a backup over the database and the uploads.

use std::path::{Path, PathBuf};

use sqlx::{Connection, SqliteConnection};

use super::{
    BackupInfo, FILES_TABLE, INFO_TABLE, UPLOAD_FOLDERS, create_backup, open_file, read_info,
};
use crate::db::{DbPool, copy_database, latest_migration, open_existing_file, run_migrations};
use crate::error::AppError;

/// Why a restore did not happen. Nothing was changed in any case.
#[derive(Debug, thiserror::Error)]
pub enum RestoreError {
    #[error("this file is not a Statup backup")]
    NotABackup,

    /// The backup's schema is newer than this binary's.
    #[error("this backup was made by Statup {app_version}: upgrade Statup first")]
    TooNew { app_version: String },

    /// The backup carries a file whose path leaves the upload folders.
    #[error("this backup carries an unexpected file: {0}")]
    UnexpectedFile(String),

    #[error(transparent)]
    Failed(#[from] AppError),
}

impl From<sqlx::Error> for RestoreError {
    fn from(error: sqlx::Error) -> Self {
        Self::Failed(AppError::Database(error))
    }
}

/// Replaces the database behind `pool` and the uploads under `upload_dir`
/// with the content of the backup at `backup`.
///
/// The backup is checked and brought to this binary's schema on a copy,
/// before anything changes. When `safety` is given, the current content is
/// backed up there first. The backup file itself is never modified.
///
/// # Errors
///
/// [`RestoreError`] says why; the database is untouched unless the error
/// comes from the copy into it or from writing the uploads.
pub async fn restore_backup(
    pool: &DbPool,
    upload_dir: &str,
    backup: &Path,
    safety: Option<&Path>,
) -> Result<BackupInfo, RestoreError> {
    let info = read_info(backup)
        .await
        .map_err(|_| RestoreError::NotABackup)?;
    if info.migration > latest_migration() {
        return Err(RestoreError::TooNew {
            app_version: info.app_version,
        });
    }
    let staging = Staging::new().await?;
    let staged = staging.0.join("restore.db");
    tokio::fs::copy(backup, &staged)
        .await
        .map_err(|e| internal(format!("cannot copy the backup: {e}")))?;
    let files = take_files(&staged).await?;
    bring_up_to_date(&staged).await?;
    if let Some(safety) = safety {
        create_backup(pool, upload_dir, safety).await?;
    }
    copy_into(pool, &staged).await?;
    replace_uploads(Path::new(upload_dir), files).await?;
    Ok(info)
}

/// Reads the carried files out of the staged copy, checks their paths, and
/// drops the two backup tables so that the copy is a plain database.
async fn take_files(staged: &Path) -> Result<Vec<(String, Vec<u8>)>, RestoreError> {
    let mut copy = open_file(staged, true).await?;
    let files: Vec<(String, Vec<u8>)> =
        sqlx::query_as(&format!("SELECT path, content FROM {FILES_TABLE}"))
            .fetch_all(&mut copy)
            .await?;
    if let Some((path, _)) = files.iter().find(|(path, _)| !is_upload_path(path)) {
        return Err(RestoreError::UnexpectedFile(path.clone()));
    }
    sqlx::query(&format!("DROP TABLE {FILES_TABLE}"))
        .execute(&mut copy)
        .await?;
    sqlx::query(&format!("DROP TABLE {INFO_TABLE}"))
        .execute(&mut copy)
        .await?;
    copy.close().await?;
    Ok(files)
}

/// A path the upload folders can hold: `icons/<name>` or `brand/<name>`,
/// with a plain file name.
fn is_upload_path(path: &str) -> bool {
    let Some((folder, name)) = path.split_once('/') else {
        return false;
    };
    UPLOAD_FOLDERS.contains(&folder)
        && !name.is_empty()
        && !name.starts_with('.')
        && !name.contains(['/', '\\', ':'])
}

/// Applies this binary's migrations to the staged copy, in WAL mode like
/// the live database, so that the copied pages carry the same journal mode.
async fn bring_up_to_date(staged: &Path) -> Result<(), RestoreError> {
    let copy = open_existing_file(staged).await?;
    run_migrations(&copy)
        .await
        .map_err(|e| internal(format!("cannot bring the backup up to date: {e}")))?;
    copy.close().await;
    Ok(())
}

async fn copy_into(pool: &DbPool, staged: &Path) -> Result<(), RestoreError> {
    let mut source =
        SqliteConnection::connect_with(&sqlx::sqlite::SqliteConnectOptions::new().filename(staged))
            .await?;
    let mut target = pool.acquire().await?;
    copy_database(&mut source, &mut target).await?;
    source.close().await?;
    Ok(())
}

/// Empties the carried folders, then writes the backup's files into them.
async fn replace_uploads(
    upload_dir: &Path,
    files: Vec<(String, Vec<u8>)>,
) -> Result<(), RestoreError> {
    for folder in UPLOAD_FOLDERS {
        let dir = upload_dir.join(folder);
        if let Err(e) = tokio::fs::remove_dir_all(&dir).await
            && e.kind() != std::io::ErrorKind::NotFound
        {
            return Err(internal(format!("cannot empty {folder}: {e}")));
        }
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|e| internal(format!("cannot create {folder}: {e}")))?;
    }
    for (path, content) in files {
        tokio::fs::write(upload_dir.join(&path), content)
            .await
            .map_err(|e| internal(format!("cannot write {path}: {e}")))?;
    }
    Ok(())
}

/// A private folder for the staged copy, removed when the restore ends.
struct Staging(PathBuf);

impl Staging {
    async fn new() -> Result<Self, RestoreError> {
        let dir = std::env::temp_dir().join(format!("statup-restore-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|e| internal(format!("cannot create a staging folder: {e}")))?;
        Ok(Self(dir))
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn internal(message: String) -> RestoreError {
    RestoreError::Failed(AppError::Internal(anyhow::anyhow!(message)))
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;
    use crate::services::backup::testing::{Scratch, file_pool};

    async fn seed(pool: &DbPool, name: &str) {
        sqlx::query(
            "INSERT INTO users (email, password_hash, display_name, role) \
             VALUES (?, 'hash', 'Ada', 'admin')",
        )
        .bind(format!("{name}@example.com"))
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO events (kind, lifecycle, title, description, author_id) \
             VALUES ('incident', 'investigating', ?, 'Mail is down', 1)",
        )
        .bind(format!("{name} outage"))
        .execute(pool)
        .await
        .unwrap();
        crate::repositories::SettingsRepository::set(pool, "instance_name", name)
            .await
            .unwrap();
    }

    async fn scalar<T>(pool: &DbPool, sql: &str) -> T
    where
        T: Send + Unpin + for<'r> sqlx::Decode<'r, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite>,
    {
        sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
    }

    /// A backup of a database holding `name`'s data, with one icon.
    async fn backup_of(scratch: &Scratch, name: &str) -> PathBuf {
        let source = file_pool(&scratch.path(&format!("{name}.db"))).await;
        seed(&source, name).await;
        let uploads = scratch.path(&format!("{name}-uploads"));
        std::fs::create_dir_all(uploads.join("icons")).unwrap();
        std::fs::write(uploads.join("icons/db.png"), name.as_bytes()).unwrap();
        let backup = scratch.path(&format!("{name}-backup.db"));
        create_backup(&source, &uploads.to_string_lossy(), &backup)
            .await
            .unwrap();
        source.close().await;
        backup
    }

    #[tokio::test]
    async fn a_restore_brings_back_the_data_the_index_and_the_uploads() {
        let scratch = Scratch::new();
        let backup = backup_of(&scratch, "acme").await;
        let live = file_pool(&scratch.path("statup.db")).await;
        seed(&live, "other").await;
        std::fs::write(scratch.path("uploads/brand/logo-old.png"), b"old").unwrap();

        let info = restore_backup(&live, &scratch.uploads(), &backup, None)
            .await
            .unwrap();

        assert_eq!(info.instance_name, "acme");
        let email: String = scalar(&live, "SELECT email FROM users").await;
        assert_eq!(email, "acme@example.com");
        let found: i64 = scalar(
            &live,
            "SELECT COUNT(*) FROM events_fts WHERE events_fts MATCH 'acme'",
        )
        .await;
        assert_eq!(found, 1);
        let tables: i64 = scalar(
            &live,
            "SELECT COUNT(*) FROM sqlite_schema WHERE name LIKE 'statup_backup%'",
        )
        .await;
        assert_eq!(tables, 0);
        let mode: String = scalar(&live, "PRAGMA journal_mode").await;
        assert_eq!(mode, "wal");
        let check: String = scalar(&live, "PRAGMA integrity_check").await;
        assert_eq!(check, "ok");
        assert_eq!(
            std::fs::read(scratch.path("uploads/icons/db.png")).unwrap(),
            b"acme"
        );
        assert!(!scratch.path("uploads/brand/logo-old.png").exists());
    }

    #[tokio::test]
    async fn the_restored_database_keeps_working() {
        let scratch = Scratch::new();
        let backup = backup_of(&scratch, "acme").await;
        let live = file_pool(&scratch.path("statup.db")).await;

        restore_backup(&live, &scratch.uploads(), &backup, None)
            .await
            .unwrap();
        seed(&live, "later").await;

        let users: i64 = scalar(&live, "SELECT COUNT(*) FROM users").await;
        assert_eq!(users, 2);
        let next_id: i64 = scalar(&live, "SELECT MAX(id) FROM events").await;
        assert_eq!(next_id, 2);
    }

    #[tokio::test]
    async fn the_current_content_is_kept_aside_first() {
        let scratch = Scratch::new();
        let backup = backup_of(&scratch, "acme").await;
        let live = file_pool(&scratch.path("statup.db")).await;
        seed(&live, "before").await;
        let safety = scratch.path("safety.db");

        restore_backup(&live, &scratch.uploads(), &backup, Some(&safety))
            .await
            .unwrap();

        assert_eq!(read_info(&safety).await.unwrap().instance_name, "before");
    }

    #[tokio::test]
    async fn a_backup_from_an_older_schema_is_brought_up_to_date() {
        let scratch = Scratch::new();
        let old =
            crate::db::create_pool(&format!("sqlite://{}", scratch.path("old.db").display()), 1)
                .await
                .unwrap();
        let full = sqlx::migrate!("./migrations");
        let first_only = sqlx::migrate::Migrator {
            migrations: Cow::Owned(full.iter().take(1).cloned().collect()),
            ..sqlx::migrate!("./migrations")
        };
        first_only.run(&old).await.unwrap();
        seed(&old, "acme").await;
        let backup = scratch.path("old-backup.db");
        let info = create_backup(&old, &scratch.uploads(), &backup)
            .await
            .unwrap();
        assert!(info.migration < latest_migration());
        let live = file_pool(&scratch.path("statup.db")).await;

        restore_backup(&live, &scratch.uploads(), &backup, None)
            .await
            .unwrap();

        let applied: i64 = scalar(&live, "SELECT MAX(version) FROM _sqlx_migrations").await;
        assert_eq!(applied, latest_migration());
        let email: String = scalar(&live, "SELECT email FROM users").await;
        assert_eq!(email, "acme@example.com");
    }

    #[tokio::test]
    async fn a_backup_from_a_newer_statup_is_refused() {
        let scratch = Scratch::new();
        let backup = backup_of(&scratch, "acme").await;
        let mut file = open_file(&backup, true).await.unwrap();
        sqlx::query("UPDATE statup_backup SET migration = migration + 1, app_version = '9.0.0'")
            .execute(&mut file)
            .await
            .unwrap();
        file.close().await.unwrap();
        let live = file_pool(&scratch.path("statup.db")).await;
        seed(&live, "kept").await;

        let refused = restore_backup(&live, &scratch.uploads(), &backup, None).await;

        assert!(
            matches!(refused, Err(RestoreError::TooNew { app_version }) if app_version == "9.0.0")
        );
        let email: String = scalar(&live, "SELECT email FROM users").await;
        assert_eq!(email, "kept@example.com");
    }

    #[tokio::test]
    async fn a_file_that_is_not_a_backup_is_refused() {
        let scratch = Scratch::new();
        let live = file_pool(&scratch.path("statup.db")).await;
        std::fs::write(scratch.path("notes.txt"), b"not a database").unwrap();

        let text =
            restore_backup(&live, &scratch.uploads(), &scratch.path("notes.txt"), None).await;
        let plain =
            restore_backup(&live, &scratch.uploads(), &scratch.path("statup.db"), None).await;

        assert!(matches!(text, Err(RestoreError::NotABackup)));
        assert!(matches!(plain, Err(RestoreError::NotABackup)));
    }

    #[tokio::test]
    async fn a_backup_carrying_a_path_outside_the_uploads_is_refused() {
        let scratch = Scratch::new();
        let backup = backup_of(&scratch, "acme").await;
        let mut file = open_file(&backup, true).await.unwrap();
        sqlx::query(
            "INSERT INTO statup_backup_files (path, content) VALUES ('icons/../../evil', x'00')",
        )
        .execute(&mut file)
        .await
        .unwrap();
        file.close().await.unwrap();
        let live = file_pool(&scratch.path("statup.db")).await;
        seed(&live, "kept").await;

        let refused = restore_backup(&live, &scratch.uploads(), &backup, None).await;

        assert!(matches!(refused, Err(RestoreError::UnexpectedFile(_))));
        let email: String = scalar(&live, "SELECT email FROM users").await;
        assert_eq!(email, "kept@example.com");
    }

    #[test]
    fn only_plain_names_in_the_upload_folders_are_accepted() {
        assert!(is_upload_path("icons/db.png"));
        assert!(is_upload_path("brand/logo-1.webp"));
        for path in [
            "db.png",
            "data/db.png",
            "icons/",
            "icons/.env",
            "icons/a/b",
            "icons/a\\b",
            "icons/C:x",
        ] {
            assert!(!is_upload_path(path), "{path}");
        }
    }

    #[tokio::test]
    async fn the_page_size_of_the_live_database_is_unchanged() {
        let scratch = Scratch::new();
        let backup = backup_of(&scratch, "acme").await;
        let live = file_pool(&scratch.path("statup.db")).await;
        let before: i64 = scalar(&live, "PRAGMA page_size").await;

        restore_backup(&live, &scratch.uploads(), &backup, None)
            .await
            .unwrap();

        let after: i64 = scalar(&live, "PRAGMA page_size").await;
        assert_eq!(before, after);
    }
}
