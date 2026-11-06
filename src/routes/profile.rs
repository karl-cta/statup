//! Profile routes: a person's own name, email and password.

use std::net::{IpAddr, SocketAddr};

use askama::Template;
use axum::Extension;
use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use axum::response::Response;
use serde::Deserialize;
use tower_sessions::Session;

use super::auth::request_ip;
use super::password::reopen_session;
use super::{Frame, render};
use crate::db::DbPool;
use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::headers::no_store;
use crate::middleware::{AuthUser, CsrfToken, HtmlForm};
use crate::models::{JournalKind, User, check_display_name};
use crate::repositories::{NewJournalEntry, UserRepository};
use crate::services::AuthService;
use crate::state::AppState;

#[derive(Template)]
#[template(path = "profile/edit.html")]
struct ProfileTemplate {
    frame: Frame,
    email: String,
    saved_email: String,
    display_name: String,
    /// The email typed differs from the saved one: its password is asked.
    ask_password: bool,
    role_label: String,
    error: Option<String>,
    success: Option<String>,
    password_error: Option<String>,
    password_success: Option<String>,
    i18n: I18n,
}

/// What the page says above each of its two forms.
#[derive(Default)]
struct ProfileMessages {
    error: Option<String>,
    success: Option<String>,
    password_error: Option<String>,
    password_success: Option<String>,
}

#[derive(Deserialize)]
pub struct ProfileInput {
    #[serde(default)]
    email: String,
    #[serde(default)]
    display_name: String,
    /// Asked only when the email changes.
    #[serde(default)]
    current_password: String,
}

#[derive(Deserialize)]
pub struct PasswordInput {
    #[serde(default)]
    current_password: String,
    #[serde(default)]
    new_password: String,
    #[serde(default)]
    new_password_confirm: String,
}

/// The page, with what was typed in the account form when it was refused.
async fn render_profile(
    state: &AppState,
    user: &User,
    csrf_token: String,
    i18n: I18n,
    messages: ProfileMessages,
    typed: Option<&ProfileInput>,
) -> Result<Response, AppError> {
    let frame = Frame::load(&state.pool, Some(user), csrf_token, &i18n).await?;
    let (email, display_name) = typed.map_or_else(
        || (user.email.clone(), user.display_name.clone()),
        |typed| (typed.email.clone(), typed.display_name.clone()),
    );
    let page = render(&ProfileTemplate {
        frame,
        ask_password: changes_email(user, &email),
        email,
        saved_email: user.email.clone(),
        display_name,
        role_label: i18n.t(user.role.i18n_key()).to_string(),
        error: messages.error,
        success: messages.success,
        password_error: messages.password_error,
        password_success: messages.password_success,
        i18n,
    })?;
    Ok(no_store(page))
}

pub async fn edit_form(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    render_profile(
        &state,
        &user,
        csrf_token,
        i18n,
        ProfileMessages::default(),
        None,
    )
    .await
}

pub async fn update_profile(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    CsrfToken(csrf_token): CsrfToken,
    headers: HeaderMap,
    connect_info: Option<Extension<ConnectInfo<SocketAddr>>>,
    Locale(i18n): Locale,
    HtmlForm(input): HtmlForm<ProfileInput>,
) -> Result<Response, AppError> {
    let ip = request_ip(&state, &headers, connect_info);
    match save_profile(&state, &user, &input, &ip).await {
        Ok(updated) => {
            let messages = ProfileMessages {
                success: Some(i18n.t("success.profile_updated").to_string()),
                ..ProfileMessages::default()
            };
            render_profile(&state, &updated, csrf_token, i18n, messages, None).await
        }
        Err(AppError::Validation(key)) => {
            let messages = ProfileMessages {
                error: Some(i18n.t(&key).to_string()),
                ..ProfileMessages::default()
            };
            render_profile(&state, &user, csrf_token, i18n, messages, Some(&input)).await
        }
        Err(e) => Err(e),
    }
}

/// Whether `email` is another address than the account's.
fn changes_email(user: &User, email: &str) -> bool {
    AuthService::normalize_email(email).map_or(true, |email| email != user.email)
}

/// Checks and stores the name and email, and returns the updated account.
/// A new email asks for the password first: the reset links go to it, so a
/// session left open must not be enough to take the account.
async fn save_profile(
    state: &AppState,
    user: &User,
    input: &ProfileInput,
    ip: &IpAddr,
) -> Result<User, AppError> {
    let name = check_display_name(&input.display_name).map_err(AppError::validation)?;
    let email = AuthService::normalize_email(&input.email)?;
    if email != user.email {
        check_own_password(state, user, &input.current_password, ip)
            .await?
            .map_err(|key| AppError::validation(&key))?;
    }
    if UserRepository::email_taken_by_other(&state.pool, &email, user.id).await? {
        return Err(AppError::validation("validation.email_taken"));
    }
    UserRepository::update_profile(&state.pool, user.id, &email, &name)
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(db) if db.is_unique_violation() => {
                AppError::validation("validation.email_taken")
            }
            _ => AppError::Database(e),
        })?;
    NewJournalEntry::new(JournalKind::ProfileUpdated)
        .by(user)
        .record(&state.pool)
        .await;

    UserRepository::find_by_id(&state.pool, user.id)
        .await?
        .ok_or(AppError::NotFound)
}

#[expect(
    clippy::too_many_arguments,
    reason = "each one is an extractor; the address needs two"
)]
pub async fn update_password(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    session: Session,
    CsrfToken(mut csrf_token): CsrfToken,
    headers: HeaderMap,
    connect_info: Option<Extension<ConnectInfo<SocketAddr>>>,
    Locale(i18n): Locale,
    HtmlForm(input): HtmlForm<PasswordInput>,
) -> Result<Response, AppError> {
    let ip = request_ip(&state, &headers, connect_info);
    let messages = match change_own_password(&state, &user, &input, &ip).await? {
        Ok(hash) => {
            csrf_token = reopen_session(&session, &hash).await?;
            ProfileMessages {
                password_success: Some(i18n.t("success.password_changed").to_string()),
                ..ProfileMessages::default()
            }
        }
        Err(key) => ProfileMessages {
            password_error: Some(i18n.t(&key).to_string()),
            ..ProfileMessages::default()
        },
    };
    render_profile(&state, &user, csrf_token, i18n, messages, None).await
}

/// The key of the message refusing the account's current password, if any.
/// Wrong ones are counted, so a session left open does not let anyone
/// guess it.
async fn check_own_password(
    state: &AppState,
    user: &User,
    password: &str,
    ip: &IpAddr,
) -> Result<Result<(), String>, AppError> {
    if password.is_empty() {
        return Ok(Err("validation.current_password_required".to_string()));
    }
    let limiter = &state.login_limiter;
    if limiter.is_password_check_blocked(user.id) {
        return Ok(Err("validation.rate_limited".to_string()));
    }
    if !AuthService::verify_password(password, &user.password_hash).await? {
        if limiter.record_password_check_failure(user.id) {
            record_password_check_blocked(&state.pool, user, ip).await;
        }
        return Ok(Err("validation.wrong_password".to_string()));
    }
    limiter.clear_password_checks(user.id);
    Ok(Ok(()))
}

/// The new password's hash, or the key of the message refusing it.
async fn change_own_password(
    state: &AppState,
    user: &User,
    input: &PasswordInput,
    ip: &IpAddr,
) -> Result<Result<String, String>, AppError> {
    if let Err(key) = check_own_password(state, user, &input.current_password, ip).await? {
        return Ok(Err(key));
    }
    let changed = AuthService::change_password(
        &state.pool,
        user.id,
        &input.new_password,
        &input.new_password_confirm,
    );
    match changed.await {
        Ok(hash) => {
            NewJournalEntry::new(JournalKind::OwnPasswordChanged)
                .by(user)
                .record(&state.pool)
                .await;
            Ok(Ok(hash))
        }
        Err(AppError::Validation(key)) => Ok(Err(key)),
        Err(e) => Err(e),
    }
}

async fn record_password_check_blocked(pool: &DbPool, user: &User, ip: &IpAddr) {
    NewJournalEntry::new(JournalKind::PasswordCheckBlocked)
        .on_member(user)
        .with_detail("too many attempts")
        .from_ip(ip)
        .record(pool)
        .await;
}
