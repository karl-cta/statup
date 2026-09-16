//! Profile routes: a person's own name, email and password.

use askama::Template;
use axum::extract::State;
use axum::response::Response;
use serde::Deserialize;
use tower_sessions::Session;

use super::password::reopen_session;
use super::{Frame, render};
use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::headers::no_store;
use crate::middleware::{AuthUser, CsrfToken, HtmlForm};
use crate::models::{User, check_display_name};
use crate::repositories::UserRepository;
use crate::services::AuthService;
use crate::state::AppState;

#[derive(Template)]
#[template(path = "profile/edit.html")]
struct ProfileTemplate {
    frame: Frame,
    email: String,
    display_name: String,
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

async fn render_profile(
    state: &AppState,
    user: &User,
    csrf_token: String,
    i18n: I18n,
    messages: ProfileMessages,
) -> Result<Response, AppError> {
    let frame = Frame::load(&state.pool, Some(user), csrf_token, &i18n).await?;
    let page = render(&ProfileTemplate {
        frame,
        email: user.email.clone(),
        display_name: user.display_name.clone(),
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
    render_profile(&state, &user, csrf_token, i18n, ProfileMessages::default()).await
}

pub async fn update_profile(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
    HtmlForm(input): HtmlForm<ProfileInput>,
) -> Result<Response, AppError> {
    match save_profile(&state, &user, &input).await {
        Ok(updated) => {
            let messages = ProfileMessages {
                success: Some(i18n.t("success.profile_updated").to_string()),
                ..ProfileMessages::default()
            };
            render_profile(&state, &updated, csrf_token, i18n, messages).await
        }
        Err(AppError::Validation(key)) => {
            let messages = ProfileMessages {
                error: Some(i18n.t(&key).to_string()),
                ..ProfileMessages::default()
            };
            render_profile(&state, &user, csrf_token, i18n, messages).await
        }
        Err(e) => Err(e),
    }
}

/// Checks and stores the name and email, and returns the updated account.
async fn save_profile(
    state: &AppState,
    user: &User,
    input: &ProfileInput,
) -> Result<User, AppError> {
    let name = check_display_name(&input.display_name)
        .map_err(|key| AppError::Validation(key.to_string()))?;
    let email = AuthService::normalize_email(&input.email)?;
    if UserRepository::email_taken_by_other(&state.pool, &email, user.id).await? {
        return Err(AppError::Validation("validation.email_taken".to_string()));
    }
    UserRepository::update_profile(&state.pool, user.id, &email, &name)
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(db) if db.is_unique_violation() => {
                AppError::Validation("validation.email_taken".to_string())
            }
            _ => AppError::Database(e),
        })?;
    tracing::info!(user_id = user.id, "Profile updated");

    UserRepository::find_by_id(&state.pool, user.id)
        .await?
        .ok_or(AppError::NotFound)
}

pub async fn update_password(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    session: Session,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
    HtmlForm(input): HtmlForm<PasswordInput>,
) -> Result<Response, AppError> {
    if let Some(key) = password_change_refusal(&user, &input).await? {
        let messages = ProfileMessages {
            password_error: Some(i18n.t(key).to_string()),
            ..ProfileMessages::default()
        };
        return render_profile(&state, &user, csrf_token, i18n, messages).await;
    }

    let hash = AuthService::hash_password(&input.new_password).await?;
    UserRepository::update_password(&state.pool, user.id, &hash).await?;
    let csrf_token = reopen_session(&session, &hash).await?;
    tracing::info!(user_id = user.id, "Password changed");

    let messages = ProfileMessages {
        password_success: Some(i18n.t("success.password_changed").to_string()),
        ..ProfileMessages::default()
    };
    render_profile(&state, &user, csrf_token, i18n, messages).await
}

/// The message key refusing the change, if any.
async fn password_change_refusal(
    user: &User,
    input: &PasswordInput,
) -> Result<Option<&'static str>, AppError> {
    if input.current_password.is_empty() {
        return Ok(Some("validation.current_password_required"));
    }
    if !AuthService::verify_password(&input.current_password, &user.password_hash).await? {
        return Ok(Some("validation.wrong_password"));
    }
    if AuthService::validate_password(&input.new_password).is_err() {
        return Ok(Some("validation.new_password_min_length"));
    }
    if input.new_password != input.new_password_confirm {
        return Ok(Some("validation.new_passwords_mismatch"));
    }
    Ok(None)
}
