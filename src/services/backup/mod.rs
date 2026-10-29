//! Backups: one `SQLite` file that holds a consistent copy of the database,
//! the uploaded logo and icons, and a row that says what made it. An expert
//! can open it with `sqlite3`; Statup restores it with `statup restore`.

mod create;
mod restore;
mod schedule;
#[cfg(test)]
mod testing;

use std::path::{Path, PathBuf};
use std::str::FromStr;

use chrono::{DateTime, NaiveDateTime, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode};
use sqlx::{Connection, SqliteConnection};

pub use create::create_backup;
pub use restore::{RestoreError, restore_backup};
pub use schedule::{
    BackupSchedule, Frequency, KEEP_RANGE, back_up_before_upgrade, last_failure, parse_time_input,
    spawn_backup_schedule,
};

use crate::error::AppError;
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
    let stamp = at.format(STAMP_FORMAT);
    match kind {
        BackupKind::Manual => format!("statup-backup-{stamp}.db"),
        BackupKind::Automatic => format!("statup-auto-{stamp}.db"),
        BackupKind::BeforeUpgrade => format!("statup-before-{CURRENT_VERSION}-{stamp}.db"),
        BackupKind::BeforeRestore => format!("statup-before-restore-{stamp}.db"),
    }
}

/// The part of a file name after its kind: `2026-10-29-163045`.
const STAMP_FORMAT: &str = "%Y-%m-%d-%H%M%S";

/// The length of a time stamp written with [`STAMP_FORMAT`].
const STAMP_LEN: usize = 17;

impl BackupKind {
    /// The kind a file name tells, `None` for a file Statup did not name.
    fn of_file_name(name: &str) -> Option<Self> {
        let stem = name.strip_suffix(".db")?;
        if stem.starts_with("statup-auto-") {
            Some(Self::Automatic)
        } else if stem.starts_with("statup-before-restore-") {
            Some(Self::BeforeRestore)
        } else if stem.starts_with("statup-before-") {
            Some(Self::BeforeUpgrade)
        } else if stem.starts_with("statup-backup-") {
            Some(Self::Manual)
        } else {
            None
        }
    }
}

/// A backup found in the backup folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupFile {
    pub name: String,
    pub kind: BackupKind,
    /// When it was taken, read from its name.
    pub taken_at: DateTime<Utc>,
    pub size: u64,
}

/// The backups in `dir` that Statup named, newest first. A missing folder
/// holds none.
pub async fn list_backups(dir: &Path) -> Vec<BackupFile> {
    let mut found = Vec::new();
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return found;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let Some(named) = entry
            .file_name()
            .to_str()
            .and_then(|name| backup_file(name.to_string(), 0))
        else {
            continue;
        };
        let Ok(meta) = entry.metadata().await else {
            continue;
        };
        if meta.is_file() {
            found.push(BackupFile {
                size: meta.len(),
                ..named
            });
        }
    }
    found.sort_by(|a, b| {
        b.taken_at
            .cmp(&a.taken_at)
            .then_with(|| b.name.cmp(&a.name))
    });
    found
}

fn backup_file(name: String, size: u64) -> Option<BackupFile> {
    let kind = BackupKind::of_file_name(&name)?;
    let stem = name.strip_suffix(".db")?;
    let stamp = stem.get(stem.len().checked_sub(STAMP_LEN)?..)?;
    let taken_at = NaiveDateTime::parse_from_str(stamp, STAMP_FORMAT)
        .ok()?
        .and_utc();
    Some(BackupFile {
        name,
        kind,
        taken_at,
        size,
    })
}

/// The path of the backup file named `name` in `dir`. A name sent by a
/// browser is accepted only as the plain name of a backup, never as a path.
pub async fn find_backup(dir: &Path, name: &str) -> Option<PathBuf> {
    if name.contains(['/', '\\']) || backup_file(name.to_string(), 0).is_none() {
        return None;
    }
    let path = dir.join(name);
    tokio::fs::metadata(&path)
        .await
        .ok()?
        .is_file()
        .then_some(path)
}

/// A private folder in the system temporary folder, removed with its
/// content when it goes out of scope.
pub(crate) struct TempFolder(PathBuf);

impl TempFolder {
    pub(crate) async fn new(prefix: &str) -> Result<Self, AppError> {
        let dir = std::env::temp_dir().join(format!("statup-{prefix}-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|e| AppError::internal(format!("cannot create a temporary folder: {e}")))?;
        Ok(Self(dir))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
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
    fn a_file_name_tells_its_kind_and_time() {
        let at = Utc.with_ymd_and_hms(2026, 10, 29, 16, 30, 45).unwrap();
        for kind in [
            BackupKind::Manual,
            BackupKind::Automatic,
            BackupKind::BeforeUpgrade,
            BackupKind::BeforeRestore,
        ] {
            let file = backup_file(backup_file_name(kind, at), 7).unwrap();
            assert_eq!((file.kind, file.taken_at, file.size), (kind, at, 7));
        }
        assert_eq!(backup_file("notes.db".to_string(), 0), None);
        assert_eq!(backup_file("statup-auto-garbage.db".to_string(), 0), None);
        assert_eq!(
            BackupKind::of_file_name("statup-auto-2026.db.partial"),
            None
        );
    }

    #[tokio::test]
    async fn the_folder_lists_its_backups_newest_first_and_nothing_else() {
        let scratch = super::testing::Scratch::new();
        let dir = scratch.path("backups");
        std::fs::create_dir_all(&dir).unwrap();
        for name in [
            "statup-auto-2026-10-27-030000.db",
            "statup-before-0.9.0-2026-10-28-120000.db",
            "statup-auto-2026-10-29-030000.db",
            "statup-auto-2026-10-30-030000.db.partial",
            "holiday.jpg",
        ] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }

        let names: Vec<String> = list_backups(&dir)
            .await
            .into_iter()
            .map(|f| f.name)
            .collect();

        assert_eq!(
            names,
            [
                "statup-auto-2026-10-29-030000.db",
                "statup-before-0.9.0-2026-10-28-120000.db",
                "statup-auto-2026-10-27-030000.db",
            ]
        );
        assert!(
            find_backup(&dir, "statup-auto-2026-10-29-030000.db")
                .await
                .is_some()
        );
        assert!(find_backup(&dir, "holiday.jpg").await.is_none());
        assert!(find_backup(&dir, "../statup.db").await.is_none());
        assert!(
            find_backup(&dir, "sub/statup-auto-2026-10-29-030000.db")
                .await
                .is_none()
        );
        assert!(
            find_backup(&dir, "sub\\statup-auto-2026-10-29-030000.db")
                .await
                .is_none()
        );
        assert!(
            find_backup(&dir, "statup-auto-2026-10-01-030000.db")
                .await
                .is_none()
        );
    }

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
