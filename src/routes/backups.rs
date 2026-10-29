//! The Backups page of the administrators: the schedule of the automatic
//! backups, the backups kept in the backup folder, and a backup downloaded
//! on the spot.

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::response::{IntoResponse, Redirect, Response};
use chrono::{Utc, Weekday};
use serde::Deserialize;

use super::{Frame, render};
use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::headers::no_store;
use crate::middleware::{CsrfToken, HtmlForm, RequireAdmin};
use crate::models::{JournalKind, User};
use crate::repositories::NewJournalEntry;
use crate::services::backup::{
    BackupFile, BackupKind, BackupSchedule, Frequency, KEEP_RANGE, backup_file_name, create_backup,
    find_backup, last_failure, list_backups, parse_time_input,
};
use crate::state::AppState;

#[derive(Template)]
#[template(path = "admin/backups.html")]
struct BackupsTemplate {
    frame: Frame,
    schedule: ScheduleForm,
    rows: Vec<BackupRow>,
    folder: String,
    failed: Option<String>,
    saved: bool,
    i18n: I18n,
}

impl BackupsTemplate {
    fn weekdays(&self) -> Vec<(u32, &'static str)> {
        (1..=7)
            .filter_map(|n| {
                Weekday::try_from(u8::try_from(n - 1).ok()?)
                    .ok()
                    .map(|d| (n, d))
            })
            .map(|(n, day)| (n, self.i18n.weekday_name(day)))
            .collect()
    }
}

/// The schedule as the panel shows it: what is saved, or what was typed
/// when it was refused.
struct ScheduleForm {
    /// "Every day at 03:00", or that automatic backups are off.
    when: String,
    /// "7 kept", none while automatic backups are off.
    kept: Option<String>,
    frequency: Frequency,
    weekday: u32,
    time: String,
    keep: String,
    before_upgrade: bool,
    open: bool,
    error: Option<String>,
}

impl ScheduleForm {
    fn of(schedule: &BackupSchedule, i18n: &I18n) -> Self {
        Self {
            when: when(schedule, i18n),
            kept: (schedule.frequency != Frequency::Off)
                .then(|| i18n.plural("backups.kept", schedule.keep as usize)),
            frequency: schedule.frequency,
            weekday: schedule.weekday.number_from_monday(),
            time: schedule.time.format("%H:%M").to_string(),
            keep: schedule.keep.to_string(),
            before_upgrade: schedule.before_upgrade,
            open: false,
            error: None,
        }
    }

    fn is(&self, frequency: &str) -> bool {
        self.frequency.as_str() == frequency
    }
}

/// A backup as the list shows it.
struct BackupRow {
    name: String,
    taken: String,
    kind: &'static str,
    size: String,
}

impl BackupRow {
    fn of(file: BackupFile, i18n: &I18n) -> Self {
        Self {
            taken: i18n.format_datetime(&file.taken_at),
            kind: i18n.t(kind_key(file.kind)),
            size: size(file.size, i18n),
            name: file.name,
        }
    }
}

fn kind_key(kind: BackupKind) -> &'static str {
    match kind {
        BackupKind::Manual => "backups.kind_manual",
        BackupKind::Automatic => "backups.kind_automatic",
        BackupKind::BeforeUpgrade => "backups.kind_upgrade",
        BackupKind::BeforeRestore => "backups.kind_restore",
    }
}

/// "840 Ko" or "1,2 Mo", with the decimal comma where the language uses it.
fn size(bytes: u64, i18n: &I18n) -> String {
    const MB: u64 = 1024 * 1024;
    if bytes < MB {
        let kb = bytes.div_ceil(1024).max(1);
        return i18n.tf("backups.size_kb", &[("n", &kb.to_string())]);
    }
    let tenths = (bytes * 10 + MB / 2) / MB;
    let mut number = format!("{}.{}", tenths / 10, tenths % 10);
    if i18n.locale() != "en" {
        number = number.replace('.', ",");
    }
    i18n.tf("backups.size_mb", &[("n", &number)])
}

/// "Every Sunday at 03:00", or that automatic backups are off.
fn when(schedule: &BackupSchedule, i18n: &I18n) -> String {
    let time = schedule.time.format("%H:%M").to_string();
    match schedule.frequency {
        Frequency::Off => i18n.t("backups.off").to_string(),
        Frequency::Daily => i18n.tf("backups.daily", &[("time", &time)]),
        Frequency::Weekly => i18n.tf(
            "backups.weekly",
            &[
                ("weekday", i18n.weekday_name(schedule.weekday)),
                ("time", &time),
            ],
        ),
    }
}

/// The schedule in English, for the Journal.
fn journal_detail(schedule: &BackupSchedule) -> String {
    let english = I18n::new("en");
    format!(
        "{}, keep={}, before_upgrade={}",
        when(schedule, &english),
        schedule.keep,
        schedule.before_upgrade
    )
}

#[derive(Deserialize)]
pub struct BackupsQuery {
    saved: Option<String>,
}

pub async fn page(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Query(query): Query<BackupsQuery>,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    let schedule = BackupSchedule::load(&state.pool).await?;
    let form = ScheduleForm::of(&schedule, &i18n);
    render_page(
        &state,
        &admin,
        csrf_token,
        i18n,
        form,
        query.saved.is_some(),
    )
    .await
}

async fn render_page(
    state: &AppState,
    admin: &User,
    csrf_token: String,
    i18n: I18n,
    schedule: ScheduleForm,
    saved: bool,
) -> Result<Response, AppError> {
    let frame = Frame::load(&state.pool, Some(admin), csrf_token, &i18n).await?;
    let rows = list_backups(&state.backup_dir)
        .await
        .into_iter()
        .map(|file| BackupRow::of(file, &i18n))
        .collect();
    let failed = last_failure(&state.pool)
        .await?
        .map(|at| i18n.tf("backups.failed", &[("when", &i18n.format_datetime(&at))]));
    render(&BackupsTemplate {
        frame,
        schedule,
        rows,
        folder: i18n.tf(
            "backups.folder",
            &[("path", &state.backup_dir.display().to_string())],
        ),
        failed,
        saved,
        i18n,
    })
}

#[derive(Deserialize)]
pub struct ScheduleInput {
    frequency: String,
    weekday: Option<String>,
    time: String,
    keep: String,
    before_upgrade: Option<String>,
}

/// Saves the schedule of the automatic backups. A refused time or count
/// shows the page again with what was typed.
pub async fn save_schedule(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
    HtmlForm(input): HtmlForm<ScheduleInput>,
) -> Result<Response, AppError> {
    match read_schedule(&input) {
        Ok(schedule) => {
            schedule.save(&state.pool).await?;
            NewJournalEntry::new(JournalKind::BackupSettingsSaved)
                .by(&admin)
                .with_detail(journal_detail(&schedule))
                .record(&state.pool)
                .await;
            Ok(Redirect::to("/admin/backups?saved=1").into_response())
        }
        Err(key) => {
            let saved = BackupSchedule::load(&state.pool).await?;
            let form = ScheduleForm {
                frequency: Frequency::parse(&input.frequency).unwrap_or(saved.frequency),
                time: input.time.trim().to_string(),
                keep: input.keep.trim().to_string(),
                before_upgrade: input.before_upgrade.is_some(),
                open: true,
                error: Some(i18n.t(key).to_string()),
                ..ScheduleForm::of(&saved, &i18n)
            };
            render_page(&state, &admin, csrf_token, i18n, form, false).await
        }
    }
}

/// The schedule the form describes, or the translation key of what is
/// wrong with it.
fn read_schedule(input: &ScheduleInput) -> Result<BackupSchedule, &'static str> {
    let defaults = BackupSchedule::default();
    let frequency = Frequency::parse(&input.frequency).ok_or("error.invalid_data")?;
    let time = parse_time_input(&input.time).ok_or("backups.time_invalid")?;
    let keep = input
        .keep
        .trim()
        .parse()
        .ok()
        .filter(|keep| KEEP_RANGE.contains(keep))
        .ok_or("backups.keep_invalid")?;
    let weekday = input
        .weekday
        .as_deref()
        .and_then(|n| n.parse::<u8>().ok())
        .and_then(|n| Weekday::try_from(n.checked_sub(1)?).ok())
        .unwrap_or(defaults.weekday);
    Ok(BackupSchedule {
        frequency,
        weekday,
        time,
        keep,
        before_upgrade: input.before_upgrade.is_some(),
    })
}

/// A backup made on the spot, sent and not kept on the server.
pub async fn download_new(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
) -> Result<Response, AppError> {
    let name = backup_file_name(BackupKind::Manual, Utc::now());
    let folder = std::env::temp_dir().join(format!("statup-download-{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&folder)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("cannot create a download folder: {e}")))?;
    let path = folder.join(&name);
    let written = create_backup(&state.pool, &state.upload_dir, &path).await;
    let content = match written {
        Ok(_) => tokio::fs::read(&path).await.map_err(|e| read_failed(&e)),
        Err(e) => Err(e),
    };
    let _ = tokio::fs::remove_dir_all(&folder).await;
    let content = content?;
    record_download(&state, &admin, &name).await;
    Ok(attachment(&name, content))
}

/// A backup of the backup folder, by its name in the list.
pub async fn download(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Response, AppError> {
    let path = find_backup(&state.backup_dir, &name)
        .await
        .ok_or(AppError::NotFound)?;
    let content = tokio::fs::read(&path).await.map_err(|e| read_failed(&e))?;
    record_download(&state, &admin, &name).await;
    Ok(attachment(&name, content))
}

async fn record_download(state: &AppState, admin: &User, name: &str) {
    NewJournalEntry::new(JournalKind::BackupDownloaded)
        .by(admin)
        .about(name)
        .record(&state.pool)
        .await;
}

fn attachment(name: &str, content: Vec<u8>) -> Response {
    let response = (
        [
            (CONTENT_TYPE, "application/vnd.sqlite3".to_string()),
            (
                CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}\""),
            ),
        ],
        content,
    )
        .into_response();
    no_store(response)
}

fn read_failed(error: &std::io::Error) -> AppError {
    AppError::Internal(anyhow::anyhow!("cannot read the backup: {error}"))
}

/// What the Backups tile of the settings says: the failure of the last
/// automatic backup, else when the newest backup was taken.
pub async fn tile_label(state: &AppState, i18n: &I18n) -> Result<String, AppError> {
    if let Some(at) = last_failure(&state.pool).await? {
        return Ok(i18n.tf(
            "backups.tile_failed",
            &[("when", &i18n.format_datetime(&at))],
        ));
    }
    let newest = list_backups(&state.backup_dir).await.into_iter().next();
    Ok(match newest {
        Some(file) => i18n.tf(
            "backups.tile_last",
            &[("when", &i18n.format_datetime(&file.taken_at))],
        ),
        None => i18n.t("backups.tile_none").to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(frequency: &str, time: &str, keep: &str) -> ScheduleInput {
        ScheduleInput {
            frequency: frequency.to_string(),
            weekday: Some("5".to_string()),
            time: time.to_string(),
            keep: keep.to_string(),
            before_upgrade: None,
        }
    }

    #[test]
    fn the_form_reads_into_a_schedule() {
        let schedule = read_schedule(&input("weekly", "22h15", "30")).unwrap();

        assert_eq!(schedule.frequency, Frequency::Weekly);
        assert_eq!(schedule.weekday, Weekday::Fri);
        assert_eq!(schedule.time.format("%H:%M").to_string(), "22:15");
        assert_eq!(schedule.keep, 30);
        assert!(!schedule.before_upgrade);
    }

    #[test]
    fn a_wrong_time_or_count_is_refused() {
        assert_eq!(
            read_schedule(&input("daily", "25:00", "7")).err(),
            Some("backups.time_invalid")
        );
        assert_eq!(
            read_schedule(&input("daily", "03:00", "0")).err(),
            Some("backups.keep_invalid")
        );
        assert_eq!(
            read_schedule(&input("daily", "03:00", "91")).err(),
            Some("backups.keep_invalid")
        );
        assert_eq!(
            read_schedule(&input("hourly", "03:00", "7")).err(),
            Some("error.invalid_data")
        );
    }

    #[test]
    fn sizes_read_in_the_language_of_the_page() {
        let fr = I18n::new("fr");
        assert_eq!(size(500, &fr), "1\u{a0}Ko");
        assert_eq!(size(840 * 1024, &fr), "840\u{a0}Ko");
        assert_eq!(size(1_258_291, &fr), "1,2\u{a0}Mo");
        assert_eq!(size(1_258_291, &I18n::new("en")), "1.2 MB");
    }
}
