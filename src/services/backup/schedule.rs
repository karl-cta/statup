//! Automatic backups: on the schedule an administrator sets, and before
//! the migrations of an upgrade.

use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Datelike, NaiveTime, TimeDelta, Utc, Weekday};
use chrono_tz::Tz;
use tokio::task::AbortHandle;

use super::{BackupKind, backup_file_name, create_backup, list_backups};
use crate::db::{DbPool, has_pending_migrations};
use crate::error::AppError;

use crate::models::JournalKind;
use crate::repositories::{NewJournalEntry, SettingsRepository};

const FREQUENCY_SETTING: &str = "backup_frequency";
const WEEKDAY_SETTING: &str = "backup_weekday";
const TIME_SETTING: &str = "backup_time";
const KEEP_SETTING: &str = "backup_keep";
const BEFORE_UPGRADE_SETTING: &str = "backup_before_upgrade";

/// When the last automatic backup was tried, whatever came of it: a failed
/// one waits for the next turn instead of retrying every minute.
const ATTEMPTED_SETTING: &str = "backup_attempted_at";

/// When the last automatic backup failed, cleared by the next success.
const FAILED_SETTING: &str = "backup_failed_at";

/// How many automatic backups an administrator may keep.
pub const KEEP_RANGE: RangeInclusive<u32> = 1..=90;

/// Backups taken before an upgrade that are kept.
const UPGRADE_KEEP: usize = 3;

/// How often the schedule is looked at.
const CHECK_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frequency {
    Daily,
    Weekly,
    Off,
}

impl Frequency {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::Off => "off",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "daily" => Some(Self::Daily),
            "weekly" => Some(Self::Weekly),
            "off" => Some(Self::Off),
            _ => None,
        }
    }
}

/// When automatic backups are taken and how many are kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupSchedule {
    pub frequency: Frequency,
    /// The day of a weekly backup.
    pub weekday: Weekday,
    /// The time of day, in the instance zone.
    pub time: NaiveTime,
    pub keep: u32,
    pub before_upgrade: bool,
}

impl Default for BackupSchedule {
    fn default() -> Self {
        Self {
            frequency: Frequency::Daily,
            weekday: Weekday::Sun,
            time: NaiveTime::from_hms_opt(3, 0, 0).unwrap_or(NaiveTime::MIN),
            keep: 7,
            before_upgrade: true,
        }
    }
}

impl BackupSchedule {
    /// The schedule in the settings; a missing or unreadable value keeps
    /// its default.
    pub async fn load(pool: &DbPool) -> Result<Self, sqlx::Error> {
        let defaults = Self::default();
        let get = |key| SettingsRepository::get(pool, key);
        Ok(Self {
            frequency: get(FREQUENCY_SETTING)
                .await?
                .and_then(|v| Frequency::parse(&v))
                .unwrap_or(defaults.frequency),
            weekday: get(WEEKDAY_SETTING)
                .await?
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.weekday),
            time: get(TIME_SETTING)
                .await?
                .and_then(|v| NaiveTime::parse_from_str(&v, "%H:%M").ok())
                .unwrap_or(defaults.time),
            keep: get(KEEP_SETTING)
                .await?
                .and_then(|v| v.parse().ok())
                .filter(|keep| KEEP_RANGE.contains(keep))
                .unwrap_or(defaults.keep),
            before_upgrade: get(BEFORE_UPGRADE_SETTING)
                .await?
                .map_or(defaults.before_upgrade, |v| v != "false"),
        })
    }

    /// Stores the schedule.
    pub async fn save(&self, pool: &DbPool) -> Result<(), sqlx::Error> {
        let weekday = self.weekday.to_string();
        let time = self.time.format("%H:%M").to_string();
        let keep = self.keep.to_string();
        let before_upgrade = if self.before_upgrade { "true" } else { "false" };
        for (key, value) in [
            (FREQUENCY_SETTING, self.frequency.as_str()),
            (WEEKDAY_SETTING, weekday.as_str()),
            (TIME_SETTING, time.as_str()),
            (KEEP_SETTING, keep.as_str()),
            (BEFORE_UPGRADE_SETTING, before_upgrade),
        ] {
            SettingsRepository::set(pool, key, value).await?;
        }
        Ok(())
    }

    /// The latest moment at or before `now` when a backup was due, `None`
    /// when automatic backups are off.
    pub fn last_due(&self, now: DateTime<Utc>, zone: Tz) -> Option<DateTime<Utc>> {
        if self.frequency == Frequency::Off {
            return None;
        }
        let today = now.with_timezone(&zone).date_naive();
        (0..=7).find_map(|days_back| {
            let day = today.checked_sub_signed(TimeDelta::days(days_back))?;
            if self.frequency == Frequency::Weekly && day.weekday() != self.weekday {
                return None;
            }
            crate::clock::to_utc_or_later(&zone, day.and_time(self.time)).filter(|at| *at <= now)
        })
    }
}

/// Looks at the schedule every minute for the life of the server.
pub fn spawn_backup_schedule(pool: DbPool, upload_dir: String, backup_dir: PathBuf) -> AbortHandle {
    let task = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(CHECK_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            if let Err(e) = run_due(&pool, &upload_dir, &backup_dir, Utc::now()).await {
                tracing::warn!(error = %e, "Backup schedule not read");
            }
        }
    });
    task.abort_handle()
}

/// Takes the automatic backup when one is due and was not tried since.
/// Returns the path of the backup written. A failed backup is recorded,
/// not returned: only reading the schedule fails this function.
///
/// # Errors
///
/// Returns `sqlx::Error` when the settings cannot be read or written.
pub async fn run_due(
    pool: &DbPool,
    upload_dir: &str,
    backup_dir: &Path,
    now: DateTime<Utc>,
) -> Result<Option<PathBuf>, sqlx::Error> {
    let schedule = BackupSchedule::load(pool).await?;
    let Some(due) = schedule.last_due(now, crate::clock::zone()) else {
        return Ok(None);
    };
    if stored_time(pool, ATTEMPTED_SETTING)
        .await?
        .is_some_and(|tried| tried >= due)
    {
        return Ok(None);
    }
    SettingsRepository::set(pool, ATTEMPTED_SETTING, &crate::clock::rfc3339(now)).await?;
    let written = write(pool, upload_dir, backup_dir, BackupKind::Automatic, now).await;
    match written {
        Ok(path) => {
            SettingsRepository::set(pool, FAILED_SETTING, "").await?;
            rotate(backup_dir, BackupKind::Automatic, schedule.keep as usize).await;
            journal(pool, JournalKind::BackupWritten, &path, None).await;
            Ok(Some(path))
        }
        Err(e) => {
            SettingsRepository::set(pool, FAILED_SETTING, &crate::clock::rfc3339(now)).await?;
            let detail = describe(&e);
            journal(pool, JournalKind::BackupFailed, backup_dir, Some(detail)).await;
            Ok(None)
        }
    }
}

/// When the last automatic backup failed, if the one after has not
/// succeeded yet.
///
/// # Errors
///
/// Returns `sqlx::Error` when the settings cannot be read.
pub async fn last_failure(pool: &DbPool) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
    stored_time(pool, FAILED_SETTING).await
}

/// Backs up the database before its migrations run, when it misses some
/// and the administrator kept this backup on. A failure is logged and the
/// upgrade goes on: a status page that does not start is worse during an
/// outage than an upgrade without a copy.
pub async fn back_up_before_upgrade(pool: &DbPool, upload_dir: &str, backup_dir: &Path) {
    match pending_and_wanted(pool).await {
        Ok(true) => {}
        Ok(false) => return,
        Err(e) => {
            tracing::error!(error = %e, "Cannot tell whether to back up before the upgrade");
            return;
        }
    }
    match write(
        pool,
        upload_dir,
        backup_dir,
        BackupKind::BeforeUpgrade,
        Utc::now(),
    )
    .await
    {
        Ok(path) => {
            rotate(backup_dir, BackupKind::BeforeUpgrade, UPGRADE_KEEP).await;
            tracing::info!(path = %path.display(), "Backup taken before the upgrade");
        }
        Err(e) => tracing::error!(error = %describe(&e), "No backup before the upgrade"),
    }
}

async fn pending_and_wanted(pool: &DbPool) -> Result<bool, sqlx::Error> {
    if !has_pending_migrations(pool).await? {
        return Ok(false);
    }
    // The settings table exists in every schema that has migrations.
    let wanted = SettingsRepository::get(pool, BEFORE_UPGRADE_SETTING).await?;
    Ok(wanted.as_deref() != Some("false"))
}

async fn write(
    pool: &DbPool,
    upload_dir: &str,
    backup_dir: &Path,
    kind: BackupKind,
    now: DateTime<Utc>,
) -> Result<PathBuf, AppError> {
    tokio::fs::create_dir_all(backup_dir).await.map_err(|e| {
        AppError::Internal(anyhow::anyhow!(
            "cannot create {}: {e}",
            backup_dir.display()
        ))
    })?;
    let path = backup_dir.join(backup_file_name(kind, now));
    create_backup(pool, upload_dir, &path).await?;
    Ok(path)
}

/// Removes the oldest backups of `kind` beyond the `keep` newest. The other
/// kinds are never touched: a backup taken by hand stays until removed by
/// hand.
async fn rotate(backup_dir: &Path, kind: BackupKind, keep: usize) {
    let of_kind = list_backups(backup_dir)
        .await
        .into_iter()
        .filter(|file| file.kind == kind);
    for file in of_kind.skip(keep) {
        if let Err(e) = tokio::fs::remove_file(backup_dir.join(&file.name)).await {
            tracing::warn!(error = %e, file = file.name, "Old backup not removed");
        }
    }
}

async fn journal(pool: &DbPool, kind: JournalKind, path: &Path, detail: Option<String>) {
    let mut entry = NewJournalEntry::new(kind).about(path.display().to_string());
    if let Some(detail) = detail {
        entry = entry.with_detail(detail);
    }
    entry.record(pool).await;
}

async fn stored_time(pool: &DbPool, key: &str) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
    Ok(SettingsRepository::get(pool, key)
        .await?
        .and_then(|value| DateTime::parse_from_rfc3339(&value).ok())
        .map(|at| at.with_timezone(&Utc)))
}

/// A time of day as an administrator types it: "03:00", "3:00", "3h",
/// "22h15" or "0300".
pub fn parse_time_input(text: &str) -> Option<NaiveTime> {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let compact = compact.to_ascii_lowercase();
    let (hours, minutes) = if let Some(parts) = compact.split_once([':', 'h']) {
        parts
    } else if compact.len() == 4 {
        compact.split_at(2)
    } else {
        (compact.as_str(), "")
    };
    let number = |part: &str| {
        (part.len() <= 2 && part.bytes().all(|b| b.is_ascii_digit()) && !part.is_empty())
            .then(|| part.parse::<u32>().ok())
            .flatten()
    };
    let minutes = if minutes.is_empty() {
        Some(0)
    } else {
        number(minutes).filter(|_| minutes.len() == 2)
    };
    NaiveTime::from_hms_opt(number(hours)?, minutes?, 0)
}

fn describe(error: &AppError) -> String {
    match error {
        AppError::Internal(inner) => format!("{inner:#}"),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use chrono::TimeZone;

    use super::*;
    use crate::services::backup::testing::{Scratch, file_pool};

    const PARIS: Tz = chrono_tz::Europe::Paris;

    fn utc(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, min, 0).unwrap()
    }

    fn at(hour: u32, minute: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(hour, minute, 0).unwrap()
    }

    #[test]
    fn a_daily_backup_is_due_at_its_local_time_today_or_else_yesterday() {
        let schedule = BackupSchedule::default();

        assert_eq!(
            schedule.last_due(utc(2026, 10, 29, 10, 0), PARIS),
            Some(utc(2026, 10, 29, 2, 0))
        );
        assert_eq!(
            schedule.last_due(utc(2026, 10, 29, 1, 0), PARIS),
            Some(utc(2026, 10, 28, 2, 0))
        );
    }

    #[test]
    fn a_weekly_backup_is_due_on_its_day() {
        let schedule = BackupSchedule {
            frequency: Frequency::Weekly,
            weekday: Weekday::Mon,
            ..BackupSchedule::default()
        };

        // Thursday 29 October: the Monday before, 26 October, at 03:00 CET.
        assert_eq!(
            schedule.last_due(utc(2026, 10, 29, 10, 0), PARIS),
            Some(utc(2026, 10, 26, 2, 0))
        );
    }

    #[test]
    fn a_time_skipped_by_daylight_saving_moves_to_the_next_valid_minute() {
        let schedule = BackupSchedule {
            time: at(2, 30),
            ..BackupSchedule::default()
        };

        // 29 March 2026: Paris jumps from 02:00 to 03:00 (01:00 UTC).
        assert_eq!(
            schedule.last_due(utc(2026, 3, 29, 12, 0), PARIS),
            Some(utc(2026, 3, 29, 1, 0))
        );
    }

    #[test]
    fn a_time_is_read_as_people_type_it() {
        for (typed, expected) in [
            ("03:00", at(3, 0)),
            ("3:00", at(3, 0)),
            ("3h", at(3, 0)),
            ("22h15", at(22, 15)),
            ("22 h 15", at(22, 15)),
            ("0300", at(3, 0)),
            ("7", at(7, 0)),
        ] {
            assert_eq!(parse_time_input(typed), Some(expected), "{typed}");
        }
        for typed in ["", "25:00", "3:5", "3:60", "abc", "12345", "-1"] {
            assert_eq!(parse_time_input(typed), None, "{typed}");
        }
    }

    #[test]
    fn nothing_is_due_when_automatic_backups_are_off() {
        let schedule = BackupSchedule {
            frequency: Frequency::Off,
            ..BackupSchedule::default()
        };

        assert_eq!(schedule.last_due(utc(2026, 10, 29, 10, 0), PARIS), None);
    }

    #[tokio::test]
    async fn the_schedule_is_stored_and_falls_back_to_its_defaults() {
        let pool = crate::test_helpers::test_pool().await;
        assert_eq!(
            BackupSchedule::load(&pool).await.unwrap(),
            BackupSchedule::default()
        );
        let chosen = BackupSchedule {
            frequency: Frequency::Weekly,
            weekday: Weekday::Fri,
            time: at(22, 15),
            keep: 30,
            before_upgrade: false,
        };

        chosen.save(&pool).await.unwrap();
        assert_eq!(BackupSchedule::load(&pool).await.unwrap(), chosen);

        SettingsRepository::set(&pool, KEEP_SETTING, "500")
            .await
            .unwrap();
        SettingsRepository::set(&pool, TIME_SETTING, "25:00")
            .await
            .unwrap();
        let loaded = BackupSchedule::load(&pool).await.unwrap();
        assert_eq!((loaded.keep, loaded.time), (7, at(3, 0)));
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[tokio::test]
    async fn one_backup_per_turn_and_only_the_newest_are_kept() {
        let scratch = Scratch::new();
        let pool = file_pool(&scratch.path("statup.db")).await;
        let dir = scratch.path("backups");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("statup-backup-2026-01-01-120000.db"), b"by hand").unwrap();
        BackupSchedule {
            keep: 2,
            ..BackupSchedule::default()
        }
        .save(&pool)
        .await
        .unwrap();
        let uploads = scratch.uploads();

        for day in 27..=29 {
            let noon = utc(2026, 10, day, 12, 0);
            assert!(
                run_due(&pool, &uploads, &dir, noon)
                    .await
                    .unwrap()
                    .is_some()
            );
            assert!(
                run_due(&pool, &uploads, &dir, noon)
                    .await
                    .unwrap()
                    .is_none()
            );
        }

        assert_eq!(
            names(&dir),
            [
                "statup-auto-2026-10-28-120000.db",
                "statup-auto-2026-10-29-120000.db",
                "statup-backup-2026-01-01-120000.db",
            ]
        );
    }

    #[tokio::test]
    async fn a_failed_backup_is_recorded_until_the_next_one_succeeds() {
        let scratch = Scratch::new();
        let pool = file_pool(&scratch.path("statup.db")).await;
        let dir = scratch.path("backups");
        std::fs::write(&dir, b"a file where the folder should be").unwrap();

        let first = run_due(&pool, &scratch.uploads(), &dir, utc(2026, 10, 28, 12, 0)).await;
        assert!(first.unwrap().is_none());
        assert_eq!(
            last_failure(&pool).await.unwrap(),
            Some(utc(2026, 10, 28, 12, 0))
        );
        let retried = run_due(&pool, &scratch.uploads(), &dir, utc(2026, 10, 28, 12, 1)).await;
        assert!(retried.unwrap().is_none());

        std::fs::remove_file(&dir).unwrap();
        let next = run_due(&pool, &scratch.uploads(), &dir, utc(2026, 10, 29, 12, 0)).await;
        assert!(next.unwrap().is_some());
        assert_eq!(last_failure(&pool).await.unwrap(), None);
    }

    async fn old_database(scratch: &Scratch) -> DbPool {
        let url = format!("sqlite://{}", scratch.path("statup.db").display());
        let pool = crate::db::create_pool(&url, 1).await.unwrap();
        let first_only = sqlx::migrate::Migrator {
            migrations: Cow::Owned(
                sqlx::migrate!("./migrations")
                    .iter()
                    .take(1)
                    .cloned()
                    .collect(),
            ),
            ..sqlx::migrate!("./migrations")
        };
        first_only.run(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn an_upgrade_is_backed_up_before_its_migrations() {
        let scratch = Scratch::new();
        let pool = old_database(&scratch).await;
        let dir = scratch.path("backups");

        back_up_before_upgrade(&pool, &scratch.uploads(), &dir).await;

        let files = list_backups(&dir).await;
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].kind, BackupKind::BeforeUpgrade);
    }

    #[tokio::test]
    async fn no_upgrade_backup_when_nothing_is_pending_or_it_is_turned_off() {
        let scratch = Scratch::new();
        let dir = scratch.path("backups");
        let fresh = crate::db::create_pool(
            &format!("sqlite://{}", scratch.path("fresh.db").display()),
            1,
        )
        .await
        .unwrap();
        back_up_before_upgrade(&fresh, &scratch.uploads(), &dir).await;
        let current = file_pool(&scratch.path("current.db")).await;
        back_up_before_upgrade(&current, &scratch.uploads(), &dir).await;
        let old = old_database(&scratch).await;
        SettingsRepository::set(&old, BEFORE_UPGRADE_SETTING, "false")
            .await
            .unwrap();
        back_up_before_upgrade(&old, &scratch.uploads(), &dir).await;

        assert_eq!(list_backups(&dir).await, Vec::new());
    }
}
