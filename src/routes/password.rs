//! Replacing a temporary password, the first thing a member added by an
//! administrator does after signing in.

use askama::Template;
use axum::extract::State;
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;
use tower_sessions::Session;

use super::auth::instance_title;
use super::render;
use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::csrf::renew_token;
use crate::middleware::headers::no_store;
use crate::middleware::{AuthUser, CsrfToken, HtmlForm};
use crate::models::User;
use crate::repositories::UserRepository;
use crate::services::AuthService;
use crate::session::{rotate_id, stamp_credential};
use crate::state::AppState;

#[derive(Default)]
struct NewPasswordErrors {
    password: Option<String>,
    confirm: Option<String>,
}

impl NewPasswordErrors {
    fn is_empty(&self) -> bool {
        self.password.is_none() && self.confirm.is_none()
    }
}

#[derive(Template)]
#[template(path = "auth/new_password.html")]
struct NewPasswordTemplate {
    csrf_token: String,
    email: String,
    errors: NewPasswordErrors,
    instance: String,
    powered_by: bool,
    i18n: I18n,
}

#[derive(Deserialize)]
pub struct NewPasswordInput {
    #[serde(default)]
    password: String,
    #[serde(default)]
    password_confirm: String,
}

/// Gives the session a new id, the new password stamp and a new form
/// token, so a copy of the old cookie is worth nothing. Returns the token
/// for a page rendered in the same response.
pub(super) async fn reopen_session(
    session: &Session,
    password_hash: &str,
) -> Result<String, AppError> {
    rotate_id(session).await?;
    stamp_credential(session, password_hash).await?;
    renew_token(session).await
}

fn render_page(
    user: &User,
    csrf_token: String,
    i18n: I18n,
    errors: NewPasswordErrors,
) -> Result<Response, AppError> {
    let (instance, powered_by) = instance_title();
    let page = render(&NewPasswordTemplate {
        csrf_token,
        email: user.email.clone(),
        errors,
        instance,
        powered_by,
        i18n,
    })?;
    Ok(no_store(page))
}

pub async fn form(
    AuthUser(user): AuthUser,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    if !user.must_change_password {
        return Ok(Redirect::to("/").into_response());
    }
    render_page(&user, csrf_token, i18n, NewPasswordErrors::default())
}

/// The temporary password itself is refused, otherwise typing it again
/// would pass the step.
async fn check_new_password(
    user: &User,
    input: &NewPasswordInput,
    i18n: &I18n,
) -> Result<NewPasswordErrors, AppError> {
    let mut errors = NewPasswordErrors::default();
    if let Err(AppError::Validation(key)) = AuthService::validate_password(&input.password) {
        errors.password = Some(i18n.t(&key).to_string());
    } else if AuthService::verify_password(&input.password, &user.password_hash).await? {
        errors.password = Some(i18n.t("validation.password_unchanged").to_string());
    } else if input.password != input.password_confirm {
        errors.confirm = Some(i18n.t("validation.passwords_mismatch").to_string());
    }
    Ok(errors)
}

pub async fn update(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    session: Session,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
    HtmlForm(input): HtmlForm<NewPasswordInput>,
) -> Result<Response, AppError> {
    if !user.must_change_password {
        return Ok(Redirect::to("/").into_response());
    }

    let errors = check_new_password(&user, &input, &i18n).await?;
    if !errors.is_empty() {
        return render_page(&user, csrf_token, i18n, errors);
    }

    let hash = AuthService::hash_password(&input.password).await?;
    UserRepository::update_password(&state.pool, user.id, &hash).await?;
    reopen_session(&session, &hash).await?;
    tracing::info!(user_id = user.id, "Temporary password replaced");

    Ok(Redirect::to("/").into_response())
}
