//! Backups: one `SQLite` file that holds a consistent copy of the database,
//! the uploaded logo and icons, and a row that says what made it. An expert
//! can open it with `sqlite3`; Statup restores it with `statup restore`.

mod create;
mod restore;
#[cfg(test)]
mod testing;

use std::path::Path;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode};
use sqlx::{Connection, SqliteConnection};

pub use create::create_backup;
pub use restore::{RestoreError, restore_backup};

use crate::services::CURRENT_VERSION;

/// What triggered a backup, which its file name tells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupKind {
    Manual,
    Automatic,
    BeforeUpgrade,
    BeforeRestore,
}

/// The file name of a backup taken at `at` (UTC), such as
/// `statup-backup-2026-10-29-163045.db`.
pub fn backup_file_name(kind: BackupKind, at: DateTime<Utc>) -> String {
    let stamp = at.format("%Y-%m-%d-%H%M%S");
    match kind {
        BackupKind::Manual => format!("statup-backup-{stamp}.db"),
        BackupKind::Automatic => format!("statup-auto-{stamp}.db"),
        BackupKind::BeforeUpgrade => format!("statup-before-{CURRENT_VERSION}-{stamp}.db"),
        BackupKind::BeforeRestore => format!("statup-before-restore-{stamp}.db"),
    }
}

/// What made a backup, read from its `statup_backup` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupInfo {
    pub app_version: String,
    /// The last migration applied to the copied database: a restore
    /// refuses a backup whose schema is newer than the binary's.
    pub migration: i64,
    pub created_at: DateTime<Utc>,
    pub instance_name: String,
}

/// The table that marks a file as a Statup backup.
const INFO_TABLE: &str = "statup_backup";

/// The table that carries the uploaded files.
const FILES_TABLE: &str = "statup_backup_files";

/// The folders of the upload directory that a backup carries.
const UPLOAD_FOLDERS: [&str; 2] = ["icons", "brand"];

/// Reads what made the backup at `path`.
///
/// # Errors
///
/// Fails when the file cannot be opened as a database, or holds no
/// `statup_backup` row: it is not a Statup backup.
pub async fn read_info(path: &Path) -> Result<BackupInfo, sqlx::Error> {
    let mut connection = open_file(path, false).await?;
    let row: (String, i64, DateTime<Utc>, String) = sqlx::query_as(&format!(
        "SELECT app_version, migration, created_at, instance_name FROM {INFO_TABLE}"
    ))
    .fetch_one(&mut connection)
    .await?;
    connection.close().await?;
    Ok(BackupInfo {
        app_version: row.0,
        migration: row.1,
        created_at: row.2,
        instance_name: row.3,
    })
}

/// One connection to a backup file. Written in rollback journal mode, so
/// that the file never grows `-wal` and `-shm` companions.
async fn open_file(path: &Path, writable: bool) -> Result<SqliteConnection, sqlx::Error> {
    let options = SqliteConnectOptions::from_str("sqlite:")?
        .filename(path)
        .read_only(!writable)
        .foreign_keys(false);
    let options = if writable {
        options.journal_mode(SqliteJournalMode::Delete)
    } else {
        options
    };
    SqliteConnection::connect_with(&options).await
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn each_kind_of_backup_has_its_own_file_name() {
        let at = Utc.with_ymd_and_hms(2026, 10, 29, 16, 30, 45).unwrap();
        assert_eq!(
            backup_file_name(BackupKind::Manual, at),
            "statup-backup-2026-10-29-163045.db"
        );
        assert_eq!(
            backup_file_name(BackupKind::Automatic, at),
            "statup-auto-2026-10-29-163045.db"
        );
        assert_eq!(
            backup_file_name(BackupKind::BeforeUpgrade, at),
            format!("statup-before-{CURRENT_VERSION}-2026-10-29-163045.db")
        );
        assert_eq!(
            backup_file_name(BackupKind::BeforeRestore, at),
            "statup-before-restore-2026-10-29-163045.db"
        );
    }
}
