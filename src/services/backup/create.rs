//! Writing a backup while Statup keeps running.

use std::path::{Path, PathBuf};

use chrono::Utc;
use sqlx::{Connection, SqliteConnection};

use super::{BackupInfo, FILES_TABLE, INFO_TABLE, UPLOAD_FOLDERS, open_file};
use crate::db::DbPool;
use crate::error::AppError;
use crate::services::CURRENT_VERSION;

/// Writes a backup of the database behind `pool` and of the uploads under
/// `upload_dir` to `destination`, which must not exist yet.
///
/// `VACUUM INTO` reads one snapshot of the database, so requests keep
/// writing while it runs. The file appears under its name only once it is
/// complete: a failure leaves nothing that looks like a backup.
///
/// # Errors
///
/// Fails when `destination` exists, its folder is missing or not writable,
/// or the database or an uploaded file cannot be read.
pub async fn create_backup(
    pool: &DbPool,
    upload_dir: &str,
    destination: &Path,
) -> Result<BackupInfo, AppError> {
    if tokio::fs::try_exists(destination).await.unwrap_or(true) {
        return Err(internal(format!(
            "{} already exists",
            destination.display()
        )));
    }
    let partial = partial_path(destination);
    let _ = tokio::fs::remove_file(&partial).await;
    let written = write_backup(pool, upload_dir, &partial).await;
    let info = match written {
        Ok(info) => info,
        Err(e) => {
            let _ = tokio::fs::remove_file(&partial).await;
            return Err(e);
        }
    };
    tokio::fs::rename(&partial, destination)
        .await
        .map_err(|e| internal(format!("cannot name the backup: {e}")))?;
    Ok(info)
}

async fn write_backup(
    pool: &DbPool,
    upload_dir: &str,
    path: &Path,
) -> Result<BackupInfo, AppError> {
    let target = path
        .to_str()
        .ok_or_else(|| internal("the backup path is not valid UTF-8".to_string()))?;
    sqlx::query("VACUUM INTO ?")
        .bind(target)
        .execute(pool)
        .await?;
    let mut copy = open_file(path, true).await?;
    let info = describe(&mut copy).await?;
    record_info(&mut copy, &info).await?;
    attach_uploads(&mut copy, Path::new(upload_dir)).await?;
    copy.close().await?;
    Ok(info)
}

/// What the copy holds: its last migration and the instance's name.
async fn describe(copy: &mut SqliteConnection) -> Result<BackupInfo, AppError> {
    let migration: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations WHERE success")
            .fetch_one(&mut *copy)
            .await?;
    let instance_name: Option<String> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'instance_name'")
            .fetch_optional(&mut *copy)
            .await?;
    Ok(BackupInfo {
        app_version: CURRENT_VERSION.to_string(),
        migration,
        created_at: Utc::now(),
        instance_name: instance_name.unwrap_or_default(),
    })
}

async fn record_info(copy: &mut SqliteConnection, info: &BackupInfo) -> Result<(), AppError> {
    sqlx::query(&format!(
        "CREATE TABLE {INFO_TABLE} (app_version TEXT NOT NULL, migration INTEGER NOT NULL, \
         created_at TEXT NOT NULL, instance_name TEXT NOT NULL)"
    ))
    .execute(&mut *copy)
    .await?;
    sqlx::query(&format!(
        "INSERT INTO {INFO_TABLE} (app_version, migration, created_at, instance_name) \
         VALUES (?, ?, ?, ?)"
    ))
    .bind(&info.app_version)
    .bind(info.migration)
    .bind(info.created_at)
    .bind(&info.instance_name)
    .execute(&mut *copy)
    .await?;
    Ok(())
}

/// Copies the logo and the icons into the backup, each under its path
/// relative to the upload directory.
async fn attach_uploads(copy: &mut SqliteConnection, upload_dir: &Path) -> Result<(), AppError> {
    sqlx::query(&format!(
        "CREATE TABLE {FILES_TABLE} (path TEXT PRIMARY KEY NOT NULL, content BLOB NOT NULL)"
    ))
    .execute(&mut *copy)
    .await?;
    let mut transaction = copy.begin().await?;
    for (path, content) in uploaded_files(upload_dir).await? {
        sqlx::query(&format!(
            "INSERT INTO {FILES_TABLE} (path, content) VALUES (?, ?)"
        ))
        .bind(path)
        .bind(content)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    Ok(())
}

/// The files of the carried folders, a missing folder holding none.
async fn uploaded_files(upload_dir: &Path) -> Result<Vec<(String, Vec<u8>)>, AppError> {
    let mut files = Vec::new();
    for folder in UPLOAD_FOLDERS {
        let Ok(mut entries) = tokio::fs::read_dir(upload_dir.join(folder)).await else {
            continue;
        };
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| internal(format!("cannot list {folder}: {e}")))?
        {
            let is_file = entry.file_type().await.is_ok_and(|kind| kind.is_file());
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if !is_file {
                continue;
            }
            let content = tokio::fs::read(entry.path())
                .await
                .map_err(|e| internal(format!("cannot read {folder}/{name}: {e}")))?;
            files.push((format!("{folder}/{name}"), content));
        }
    }
    Ok(files)
}

fn partial_path(destination: &Path) -> PathBuf {
    let mut name = destination.as_os_str().to_owned();
    name.push(".partial");
    PathBuf::from(name)
}

fn internal(message: String) -> AppError {
    AppError::Internal(anyhow::anyhow!(message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::backup::read_info;
    use crate::services::backup::testing::{Scratch, file_pool};

    async fn open(path: &Path) -> SqliteConnection {
        open_file(path, false).await.unwrap()
    }

    #[tokio::test]
    async fn a_backup_holds_the_data_the_uploads_and_what_made_it() {
        let scratch = Scratch::new();
        let pool = file_pool(&scratch.path("statup.db")).await;
        crate::repositories::SettingsRepository::set(&pool, "instance_name", "Acme")
            .await
            .unwrap();
        std::fs::write(scratch.path("uploads/icons/db.png"), b"icon").unwrap();
        std::fs::write(scratch.path("uploads/brand/logo-1.png"), b"logo").unwrap();
        let destination = scratch.path("backup.db");

        let info = create_backup(&pool, &scratch.uploads(), &destination)
            .await
            .unwrap();

        assert_eq!(info.app_version, CURRENT_VERSION);
        assert_eq!(info.instance_name, "Acme");
        assert!(info.migration > 0);
        assert_eq!(read_info(&destination).await.unwrap(), info);
        let mut copy = open(&destination).await;
        let name: String =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = 'instance_name'")
                .fetch_one(&mut copy)
                .await
                .unwrap();
        assert_eq!(name, "Acme");
        let files: Vec<(String, Vec<u8>)> =
            sqlx::query_as("SELECT path, content FROM statup_backup_files ORDER BY path")
                .fetch_all(&mut copy)
                .await
                .unwrap();
        assert_eq!(
            files,
            vec![
                ("brand/logo-1.png".to_string(), b"logo".to_vec()),
                ("icons/db.png".to_string(), b"icon".to_vec()),
            ]
        );
    }

    #[tokio::test]
    async fn a_backup_taken_while_requests_write_is_whole() {
        let scratch = Scratch::new();
        let pool = file_pool(&scratch.path("statup.db")).await;
        let writer = {
            let pool = pool.clone();
            tokio::spawn(async move {
                for i in 0..200 {
                    crate::repositories::SettingsRepository::set(&pool, &format!("k{i}"), "v")
                        .await
                        .unwrap();
                }
            })
        };
        let destination = scratch.path("backup.db");

        create_backup(&pool, &scratch.uploads(), &destination)
            .await
            .unwrap();
        writer.await.unwrap();

        let mut copy = open(&destination).await;
        let check: String = sqlx::query_scalar("PRAGMA integrity_check")
            .fetch_one(&mut copy)
            .await
            .unwrap();
        assert_eq!(check, "ok");
        let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&mut copy)
            .await
            .unwrap();
        assert_eq!(mode, "delete");
        assert!(!scratch.path("backup.db-wal").exists());
    }

    #[tokio::test]
    async fn an_existing_file_is_never_overwritten() {
        let scratch = Scratch::new();
        let pool = file_pool(&scratch.path("statup.db")).await;
        let destination = scratch.path("backup.db");
        std::fs::write(&destination, b"keep me").unwrap();

        assert!(
            create_backup(&pool, &scratch.uploads(), &destination)
                .await
                .is_err()
        );
        assert_eq!(std::fs::read(&destination).unwrap(), b"keep me");
    }

    #[tokio::test]
    async fn a_failed_backup_leaves_nothing_behind() {
        let scratch = Scratch::new();
        let pool = file_pool(&scratch.path("statup.db")).await;
        let destination = scratch.path("missing-folder/backup.db");

        assert!(
            create_backup(&pool, &scratch.uploads(), &destination)
                .await
                .is_err()
        );
        assert!(!destination.exists());
        assert!(!partial_path(&destination).exists());
    }

    #[tokio::test]
    async fn a_database_that_is_not_a_backup_is_told_apart() {
        let scratch = Scratch::new();
        let _pool = file_pool(&scratch.path("statup.db")).await;

        assert!(read_info(&scratch.path("statup.db")).await.is_err());
        std::fs::write(scratch.path("notes.txt"), b"not a database").unwrap();
        assert!(read_info(&scratch.path("notes.txt")).await.is_err());
    }
}
