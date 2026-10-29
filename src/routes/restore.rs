//! Restoring a backup from the first screen of an empty instance: a file
//! sent from this computer, or a backup already in the backup folder (the
//! disk of the old server mounted, a network share). An instance with an
//! account restores on the command line only.
//!
//! The file streams to disk, so this route stays outside the CSRF
//! middleware, which reads form bodies in memory; it checks the token
//! itself, from the first field of the form.

use std::path::{Path, PathBuf};

use askama::Template;
use axum::extract::multipart::{Field, MultipartError};
use axum::extract::{Multipart, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use tokio::io::AsyncWriteExt;
use tower_sessions::Session;

use super::auth::instance_is_empty;
use super::backups::BackupRow;
use super::render;
use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::csrf::{check_token, form_token};
use crate::models::JournalKind;
use crate::repositories::NewJournalEntry;
use crate::services::backup::{
    RestoreError, TempFolder, find_backup, list_backups, restore_backup,
};
use crate::services::{DashboardLayoutService, SettingsService};
use crate::state::AppState;

#[derive(Template)]
#[template(path = "auth/restore.html")]
struct RestoreTemplate {
    csrf_token: String,
    /// The backups of the backup folder, newest first.
    found: Vec<BackupRow>,
    /// A file of this computer was the choice of the refused form.
    sent_chosen: bool,
    error: Option<String>,
    powered_by: bool,
    i18n: I18n,
}

pub async fn page(
    State(state): State<AppState>,
    session: Session,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    if !instance_is_empty(&state).await? {
        return Ok(Redirect::to("/login").into_response());
    }
    render_page(&state, &session, i18n, None, false).await
}

/// The page, with why the form was refused and whether it had a file of
/// this computer chosen.
async fn render_page(
    state: &AppState,
    session: &Session,
    i18n: I18n,
    error: Option<String>,
    sent_chosen: bool,
) -> Result<Response, AppError> {
    let found = list_backups(&state.backup_dir)
        .await
        .into_iter()
        .map(|file| BackupRow::of(file, &i18n))
        .collect();
    let (_, powered_by) = super::auth::instance_title();
    render(&RestoreTemplate {
        csrf_token: form_token(session).await?,
        found,
        sent_chosen,
        error,
        powered_by,
        i18n,
    })
}

/// Where the backup to restore is.
enum Source {
    /// A backup of the backup folder, by its listed name.
    Listed(String),
    /// A file sent with the form, written to the staging folder, and the
    /// name it had on the computer.
    Sent(PathBuf, String),
    Nothing,
}

pub async fn restore(
    State(state): State<AppState>,
    session: Session,
    Locale(i18n): Locale,
    multipart: Multipart,
) -> Result<Response, AppError> {
    // Refused before the body is read: an instance with an account takes no
    // upload at all.
    if !instance_is_empty(&state).await? {
        let message = i18n.t("restore.not_empty").to_string();
        return render_page(&state, &session, i18n, Some(message), false).await;
    }
    let staging = TempFolder::new("upload").await?;
    let source = read_form(&session, multipart, staging.path()).await?;
    let sent_chosen = !matches!(source, Source::Listed(_));
    match restore_from(&state, source, &i18n).await? {
        Ok(()) => Ok(Redirect::to("/login?restored=1").into_response()),
        Err(message) => render_page(&state, &session, i18n, Some(message), sent_chosen).await,
    }
}

/// Restores the chosen backup. The inner error is a refusal said in the
/// page; the outer one, a failure of the server.
async fn restore_from(
    state: &AppState,
    source: Source,
    i18n: &I18n,
) -> Result<Result<(), String>, AppError> {
    // Checked again: an account may have been made while the file arrived.
    if !instance_is_empty(state).await? {
        return Ok(Err(i18n.t("restore.not_empty").to_string()));
    }
    let found = match source {
        Source::Listed(name) => find_backup(&state.backup_dir, &name)
            .await
            .map(|path| (path, name)),
        Source::Sent(path, name) => Some((path, name)),
        Source::Nothing => None,
    };
    let Some((path, name)) = found else {
        return Ok(Err(i18n.t("restore.no_file").to_string()));
    };
    match restore_backup(&state.pool, &state.upload_dir, &path, None).await {
        Ok(info) => {
            refresh_state(state).await?;
            NewJournalEntry::new(JournalKind::BackupRestored)
                .about(name)
                .with_detail(format!("made by Statup {}", info.app_version))
                .record(&state.pool)
                .await;
            Ok(Ok(()))
        }
        Err(RestoreError::Failed(e)) => Err(e),
        Err(refused) => Ok(Err(refusal_text(&refused, i18n))),
    }
}

fn refusal_text(refused: &RestoreError, i18n: &I18n) -> String {
    match refused {
        RestoreError::TooNew { app_version } => {
            i18n.tf("restore.too_new", &[("version", app_version)])
        }
        RestoreError::UnexpectedFile(_) => i18n.t("restore.unexpected_file").to_string(),
        RestoreError::NotABackup | RestoreError::Failed(_) => {
            i18n.t("restore.not_a_backup").to_string()
        }
    }
}

/// What the server holds in memory from the database, read again from the
/// restored one: the settings and the dashboard layout. An empty instance
/// has no service yet, so the monitoring holds nothing to forget.
async fn refresh_state(state: &AppState) -> Result<(), AppError> {
    let public_mode = SettingsService::load(&state.pool, state.is_public_mode()).await?;
    state.set_public_mode(public_mode);
    DashboardLayoutService::reconcile(&state.pool).await
}

/// Reads the form in its order: the token first, checked before anything
/// else is read, then the choice among the listed backups, then the file.
async fn read_form(
    session: &Session,
    mut multipart: Multipart,
    staging: &Path,
) -> Result<Source, AppError> {
    let token = match next_field(&mut multipart).await? {
        Some(field) if field.name() == Some("csrf_token") => field.text().await.ok(),
        _ => None,
    };
    check_token(session, token.as_deref()).await?;
    let mut source = Source::Nothing;
    while let Some(field) = next_field(&mut multipart).await? {
        match field.name() {
            Some("source") => {
                let name = field
                    .text()
                    .await
                    .map_err(|_| AppError::validation("restore.no_file"))?;
                if !name.is_empty() {
                    source = Source::Listed(name);
                }
            }
            Some("file") if matches!(source, Source::Nothing) => {
                source = save_upload(field, staging).await?;
            }
            _ => {}
        }
    }
    Ok(source)
}

async fn next_field(multipart: &mut Multipart) -> Result<Option<Field<'_>>, AppError> {
    multipart.next_field().await.map_err(unreadable)
}

// The error is read for its status only: `map_err` hands it over by value.
#[allow(clippy::needless_pass_by_value)]
fn unreadable(error: MultipartError) -> AppError {
    if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
        AppError::PayloadTooLarge
    } else {
        AppError::validation("restore.no_file")
    }
}

/// Writes the sent file to the staging folder as it arrives. A field
/// without a file, or an empty one, is no file.
async fn save_upload(mut field: Field<'_>, staging: &Path) -> Result<Source, AppError> {
    let Some(name) = field
        .file_name()
        .filter(|name| !name.is_empty())
        .map(str::to_string)
    else {
        return Ok(Source::Nothing);
    };
    let path = staging.join("sent-file.db");
    let mut file = tokio::fs::File::create(&path)
        .await
        .map_err(|e| write_failed(&e))?;
    let mut written = 0;
    while let Some(chunk) = field.chunk().await.map_err(unreadable)? {
        file.write_all(&chunk).await.map_err(|e| write_failed(&e))?;
        written += chunk.len();
    }
    file.flush().await.map_err(|e| write_failed(&e))?;
    Ok(if written == 0 {
        Source::Nothing
    } else {
        Source::Sent(path, name)
    })
}

fn write_failed(error: &std::io::Error) -> AppError {
    AppError::internal(format!("cannot write the sent backup: {error}"))
}
