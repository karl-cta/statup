//! Maintenance commands the host runs on the server, next to the web app.

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::config::{
    ConfigError, DEFAULT_DATABASE_URL, DEFAULT_UPLOAD_DIR, backup_dir_for, database_file,
    load_dotenv,
};
use crate::db;
use crate::error::AppError;
use crate::services::AuthService;
use crate::services::backup::{
    BackupInfo, BackupKind, RestoreError, backup_file_name, create_backup, read_info,
    restore_backup,
};

const USAGE: &str = "Usage:
  statup                          Start the server
  statup reset-password <email>   Give an account a temporary password
  statup backup [--output <file>] Write a backup of the data
  statup restore <file> [--yes]   Replace the data with a backup

Put --env <file> first to read the settings from that file instead of .env.";

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Help,
    ResetPassword(String),
    Backup { output: Option<PathBuf> },
    Restore { file: PathBuf, yes: bool },
}

fn parse(args: &[String]) -> Option<Command> {
    match args {
        [flag] if flag == "--help" || flag == "-h" => Some(Command::Help),
        [name, email] if name == "reset-password" => Some(Command::ResetPassword(email.clone())),
        [name] if name == "backup" => Some(Command::Backup { output: None }),
        [name, flag, file] if name == "backup" && flag == "--output" => Some(Command::Backup {
            output: Some(PathBuf::from(file)),
        }),
        [name, file] if name == "restore" && !file.starts_with('-') => Some(Command::Restore {
            file: PathBuf::from(file),
            yes: false,
        }),
        [name, file, flag] if name == "restore" && !file.starts_with('-') && flag == "--yes" => {
            Some(Command::Restore {
                file: PathBuf::from(file),
                yes: true,
            })
        }
        _ => None,
    }
}

/// The command line was not understood.
#[derive(Debug, PartialEq, Eq)]
pub struct UsageError;

/// Splits a leading `--env <file>` from the arguments (the process
/// arguments without the binary name).
///
/// # Errors
///
/// [`UsageError`] when `--env` has no file after it.
pub fn take_env_file(args: Vec<String>) -> Result<(Option<PathBuf>, Vec<String>), UsageError> {
    if args.first().map(String::as_str) != Some("--env") {
        return Ok((None, args));
    }
    let mut rest = args.into_iter().skip(1);
    let file = rest.next().ok_or(UsageError)?;
    Ok((Some(PathBuf::from(file)), rest.collect()))
}

/// Prints the usage and returns the exit code for a command line that was
/// not understood.
pub fn report_usage() -> i32 {
    eprintln!("{USAGE}");
    2
}

/// Runs the command named by `args` (the process arguments without the
/// binary name and the `--env` option) and returns the process exit code.
pub async fn run(args: &[String]) -> i32 {
    match parse(args) {
        Some(Command::Help) => {
            println!("{USAGE}");
            0
        }
        Some(Command::ResetPassword(email)) => run_reset_password(&email).await,
        Some(Command::Backup { output }) => run_backup(output.as_deref()).await,
        Some(Command::Restore { file, yes }) => run_restore(&file, yes).await,
        None => report_usage(),
    }
}

async fn run_reset_password(email: &str) -> i32 {
    if let Err(e) = load_dotenv() {
        eprintln!("{e}");
        return 1;
    }
    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string());

    match reset_password(&database_url, email).await {
        Ok(password) => {
            println!("Temporary password for {email}: {password}");
            println!(
                "Hand it over within 7 days. They choose their own at next sign-in, and their open sessions are signed out."
            );
            0
        }
        Err(AppError::NotFound) => {
            eprintln!("No active account uses {email} in {database_url}.");
            1
        }
        Err(e) => {
            eprintln!("Password reset failed: {}", e.cause());
            1
        }
    }
}

/// Where the data lives, as the environment says.
struct Target {
    database_url: String,
    upload_dir: String,
    backup_dir: PathBuf,
}

impl Target {
    fn from_env() -> Result<Self, ConfigError> {
        load_dotenv()?;
        let database_url =
            std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string());
        Ok(Self {
            upload_dir: std::env::var("UPLOAD_DIR")
                .unwrap_or_else(|_| DEFAULT_UPLOAD_DIR.to_string()),
            backup_dir: backup_dir_for(&database_url),
            database_url,
        })
    }
}

/// The target, or nothing after saying why it cannot be read.
fn load_target() -> Result<Target, ()> {
    Target::from_env().map_err(|e| eprintln!("{e}"))
}

async fn run_backup(output: Option<&Path>) -> i32 {
    let Ok(target) = load_target() else {
        return 1;
    };
    match take_backup(&target, output, Utc::now()).await {
        Ok(path) => {
            println!("Backup written to {}.", path.display());
            0
        }
        Err(e) => {
            eprintln!("Backup failed: {}", e.cause());
            1
        }
    }
}

/// Backs up the database as it is, without migrating it. Works on an
/// existing database only, and never overwrites a file.
async fn take_backup(
    target: &Target,
    output: Option<&Path>,
    now: DateTime<Utc>,
) -> Result<PathBuf, AppError> {
    let pool = db::open_existing_pool(&target.database_url)
        .await
        .map_err(|e| AppError::internal(format!("cannot open the database: {e}")))?;
    let result = write_backup(&pool, target, output, now).await;
    pool.close().await;
    result
}

async fn write_backup(
    pool: &db::DbPool,
    target: &Target,
    output: Option<&Path>,
    now: DateTime<Utc>,
) -> Result<PathBuf, AppError> {
    let destination = if let Some(path) = output {
        path.to_path_buf()
    } else {
        create_folder(&target.backup_dir)?;
        target
            .backup_dir
            .join(backup_file_name(BackupKind::Manual, now))
    };
    create_backup(pool, &target.upload_dir, &destination).await?;
    Ok(destination)
}

fn create_folder(dir: &Path) -> Result<(), AppError> {
    std::fs::create_dir_all(dir)
        .map_err(|e| AppError::internal(format!("cannot create {}: {e}", dir.display())))
}

async fn run_restore(file: &Path, yes: bool) -> i32 {
    let Ok(target) = load_target() else {
        return 1;
    };
    let Ok(info) = read_info(file).await else {
        eprintln!("{}", RestoreError::NotABackup);
        return 1;
    };
    describe_backup(&info);
    if let Some(code) = confirm(yes) {
        return code;
    }
    match restore_into(&target, file, Utc::now()).await {
        Ok(safety) => {
            println!("Restored.");
            if let Some(path) = safety {
                println!("The previous data is in {}.", path.display());
            }
            println!("Start Statup again.");
            0
        }
        Err(e) => {
            eprintln!("{}", describe_restore(&e));
            1
        }
    }
}

fn describe_backup(info: &BackupInfo) {
    // An instance that never got a name shows the Statup mark: no quotes.
    let of = if info.instance_name.is_empty() {
        String::new()
    } else {
        format!(" of \"{}\"", info.instance_name)
    };
    println!(
        "Backup{of}, made on {} by Statup {}.",
        info.created_at.format("%Y-%m-%d %H:%M UTC"),
        info.app_version
    );
    println!(
        "It replaces all of Statup's data: accounts, events, settings, logo and icons. Stop Statup before restoring."
    );
}

/// `None` to go on, or the exit code to stop with.
fn confirm(yes: bool) -> Option<i32> {
    if yes {
        return None;
    }
    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        eprintln!("Add --yes to restore without a question.");
        return Some(2);
    }
    print!("Restore it? [y/N] ");
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    if stdin.read_line(&mut answer).is_ok() && is_yes(&answer) {
        return None;
    }
    println!("Nothing was changed.");
    Some(1)
}

fn is_yes(answer: &str) -> bool {
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// Restores into the database of `target`, creating it when it is missing.
/// An existing database is backed up first; the path of that backup is
/// returned.
async fn restore_into(
    target: &Target,
    backup: &Path,
    now: DateTime<Utc>,
) -> Result<Option<PathBuf>, RestoreError> {
    let file = database_file(&target.database_url)
        .ok_or_else(|| AppError::internal("DATABASE_URL does not name a database file"))?;
    let existed = file.exists();
    let safety = if existed {
        create_folder(&target.backup_dir)?;
        Some(
            target
                .backup_dir
                .join(backup_file_name(BackupKind::BeforeRestore, now)),
        )
    } else {
        None
    };
    let pool = db::create_pool(&target.database_url, 1).await?;
    let result = restore_backup(&pool, &target.upload_dir, backup, safety.as_deref()).await;
    pool.close().await;
    if result.is_err() && !existed {
        remove_database_files(&file);
    }
    result.map(|_| safety)
}

/// A restore that failed in a database it had to create leaves nothing
/// behind: Statup would start on an empty database and ask to set up.
fn remove_database_files(file: &Path) {
    for suffix in ["", "-wal", "-shm"] {
        let mut name = file.as_os_str().to_os_string();
        name.push(suffix);
        let _ = std::fs::remove_file(name);
    }
}

fn describe_restore(error: &RestoreError) -> String {
    match error {
        RestoreError::Failed(inner) => format!("Restore failed: {}", inner.cause()),
        other => other.to_string(),
    }
}

/// Works on an existing database only: a mistyped path must not leave an
/// empty database behind.
async fn reset_password(database_url: &str, email: &str) -> Result<String, AppError> {
    let pool = db::open_existing_pool(database_url)
        .await
        .map_err(|e| AppError::internal(format!("cannot open the database: {e}")))?;
    db::run_migrations(&pool)
        .await
        .map_err(|e| AppError::internal(format!("cannot migrate the database: {e}")))?;
    let result = AuthService::reset_password(&pool, email).await;
    pool.close().await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn reset_password_takes_one_email() {
        assert_eq!(
            parse(&args(&["reset-password", "a@b.com"])),
            Some(Command::ResetPassword("a@b.com".to_string()))
        );
        assert_eq!(parse(&args(&["reset-password"])), None);
    }

    #[test]
    fn unknown_commands_are_refused() {
        assert_eq!(parse(&args(&["serve"])), None);
        assert_eq!(parse(&args(&["--help"])), Some(Command::Help));
    }

    #[test]
    fn backup_takes_an_optional_output_file() {
        assert_eq!(
            parse(&args(&["backup"])),
            Some(Command::Backup { output: None })
        );
        assert_eq!(
            parse(&args(&["backup", "--output", "/tmp/a.db"])),
            Some(Command::Backup {
                output: Some(PathBuf::from("/tmp/a.db"))
            })
        );
        assert_eq!(parse(&args(&["backup", "--output"])), None);
        assert_eq!(parse(&args(&["backup", "--force"])), None);
        assert_eq!(parse(&args(&["backup", "/tmp/a.db"])), None);
    }

    #[test]
    fn restore_takes_a_file_and_an_optional_yes() {
        let restore = |file: &str, yes| {
            Some(Command::Restore {
                file: PathBuf::from(file),
                yes,
            })
        };
        assert_eq!(parse(&args(&["restore", "a.db"])), restore("a.db", false));
        assert_eq!(
            parse(&args(&["restore", "a.db", "--yes"])),
            restore("a.db", true)
        );
        assert_eq!(parse(&args(&["restore"])), None);
        assert_eq!(parse(&args(&["restore", "--yes"])), None);
        assert_eq!(parse(&args(&["restore", "--force"])), None);
        assert_eq!(parse(&args(&["restore", "a.db", "--force"])), None);
        assert_eq!(parse(&args(&["restore", "--yes", "a.db"])), None);
    }

    #[test]
    fn the_env_option_is_taken_from_the_front_only() {
        assert_eq!(
            take_env_file(args(&["--env", "/etc/statup.env", "backup"])),
            Ok((Some(PathBuf::from("/etc/statup.env")), args(&["backup"])))
        );
        assert_eq!(
            take_env_file(args(&["--env", "/etc/statup.env"])),
            Ok((Some(PathBuf::from("/etc/statup.env")), Vec::new()))
        );
        assert_eq!(
            take_env_file(args(&["backup", "--env", "x"])),
            Ok((None, args(&["backup", "--env", "x"])))
        );
        assert_eq!(take_env_file(Vec::new()), Ok((None, Vec::new())));
        assert_eq!(take_env_file(args(&["--env"])), Err(UsageError));
    }

    #[test]
    fn only_yes_confirms() {
        assert!(is_yes("y\n"));
        assert!(is_yes(" YES "));
        assert!(!is_yes("n"));
        assert!(!is_yes(""));
    }

    /// A folder removed on drop, and databases inside it.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("statup-cli-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn target(&self, name: &str) -> Target {
            Target {
                database_url: self
                    .0
                    .join(format!("{name}.db"))
                    .to_string_lossy()
                    .into_owned(),
                upload_dir: self
                    .0
                    .join(format!("{name}-uploads"))
                    .to_string_lossy()
                    .into_owned(),
                backup_dir: self.0.join(format!("{name}-backups")),
            }
        }

        /// A migrated database named `instance`, with an icon.
        async fn seeded(&self, name: &str, instance: &str) -> Target {
            let target = self.target(name);
            let pool = db::create_pool(&target.database_url, 1).await.unwrap();
            db::run_migrations(&pool).await.unwrap();
            crate::repositories::SettingsRepository::set(&pool, "instance_name", instance)
                .await
                .unwrap();
            pool.close().await;
            let icons = Path::new(&target.upload_dir).join("icons");
            std::fs::create_dir_all(&icons).unwrap();
            std::fs::write(icons.join("db.png"), instance.as_bytes()).unwrap();
            target
        }

        async fn instance_name(&self, target: &Target) -> String {
            let pool = db::open_existing_pool(&target.database_url).await.unwrap();
            let name: String =
                sqlx::query_scalar("SELECT value FROM settings WHERE key = 'instance_name'")
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            pool.close().await;
            name
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn noon() -> DateTime<Utc> {
        DateTime::from_timestamp(1_793_275_200, 0).unwrap()
    }

    #[tokio::test]
    async fn a_backup_restores_into_a_missing_database_without_a_safety_copy() {
        let scratch = Scratch::new();
        let source = scratch.seeded("source", "Acme").await;

        let file = take_backup(&source, None, noon()).await.unwrap();
        assert_eq!(
            file,
            source
                .backup_dir
                .join(backup_file_name(BackupKind::Manual, noon()))
        );
        assert_eq!(read_info(&file).await.unwrap().instance_name, "Acme");

        let fresh = scratch.target("fresh");
        let safety = restore_into(&fresh, &file, noon()).await.unwrap();

        assert_eq!(safety, None);
        assert!(!fresh.backup_dir.exists());
        assert_eq!(scratch.instance_name(&fresh).await, "Acme");
        let icon = Path::new(&fresh.upload_dir).join("icons/db.png");
        assert_eq!(std::fs::read(icon).unwrap(), b"Acme");
    }

    #[tokio::test]
    async fn a_restore_over_a_database_leaves_a_backup_of_what_it_replaced() {
        let scratch = Scratch::new();
        let source = scratch.seeded("source", "Acme").await;
        let file = scratch.0.join("chosen.db");
        take_backup(&source, Some(&file), noon()).await.unwrap();
        let live = scratch.seeded("live", "Other").await;

        let safety = restore_into(&live, &file, noon())
            .await
            .unwrap()
            .expect("a safety copy of the existing database");

        assert_eq!(
            safety,
            live.backup_dir
                .join(backup_file_name(BackupKind::BeforeRestore, noon()))
        );
        assert_eq!(read_info(&safety).await.unwrap().instance_name, "Other");
        assert_eq!(scratch.instance_name(&live).await, "Acme");
    }

    #[tokio::test]
    async fn a_backup_of_a_missing_database_creates_nothing() {
        let scratch = Scratch::new();
        let target = scratch.target("missing");

        assert!(take_backup(&target, None, noon()).await.is_err());

        assert!(!Path::new(&target.database_url).exists());
        assert!(!target.backup_dir.exists());
    }

    #[tokio::test]
    async fn a_restore_that_fails_does_not_leave_a_database_it_created() {
        let scratch = Scratch::new();
        let not_a_backup = scratch.0.join("plain.txt");
        std::fs::write(&not_a_backup, b"hello").unwrap();
        let target = scratch.target("fresh");

        let error = restore_into(&target, &not_a_backup, noon())
            .await
            .unwrap_err();

        assert!(matches!(error, RestoreError::NotABackup));
        assert!(!Path::new(&target.database_url).exists());
    }

    #[tokio::test]
    async fn a_mistyped_database_path_is_not_created() {
        let path = std::env::temp_dir().join(format!("statup-cli-{}.db", uuid::Uuid::new_v4()));
        let url = path.to_string_lossy().to_string();

        assert!(reset_password(&url, "a@b.com").await.is_err());
        assert!(!path.exists());
    }
}
