//! Replacing a temporary password, the first thing a member added by an
//! administrator does after signing in.

use askama::Template;
use axum::extract::State;
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;
use tower_sessions::Session;

use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::{AuthUser, CsrfToken, HtmlForm};
use crate::models::User;
use crate::repositories::UserRepository;
use crate::services::AuthService;
use crate::session::stamp_credential;
use crate::state::AppState;

use super::auth::instance_title;

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
    password: String,
    password_confirm: String,
}

fn render_page(
    user: &User,
    csrf_token: String,
    i18n: I18n,
    errors: NewPasswordErrors,
) -> Result<Response, AppError> {
    let (instance, powered_by) = instance_title();
    let tpl = NewPasswordTemplate {
        csrf_token,
        email: user.email.clone(),
        errors,
        instance,
        powered_by,
        i18n,
    };
    let html = tpl
        .render()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("template render error: {e}")))?;
    Ok(Html(html).into_response())
}

pub async fn form(
    AuthUser(user): AuthUser,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    if !user.must_change_password {
        return Ok(Redirect::to("/").into_response());
    }
    render_page(&user, csrf_token.0, i18n, NewPasswordErrors::default())
}

/// The temporary password itself is refused, otherwise typing it again
/// would pass the step.
fn check_new_password(
    user: &User,
    input: &NewPasswordInput,
    i18n: &I18n,
) -> Result<NewPasswordErrors, AppError> {
    let mut errors = NewPasswordErrors::default();
    if let Err(AppError::Validation(key)) = AuthService::validate_password(&input.password) {
        errors.password = Some(i18n.t(&key).to_string());
    } else if AuthService::verify_password(&input.password, &user.password_hash)? {
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
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
    HtmlForm(input): HtmlForm<NewPasswordInput>,
) -> Result<Response, AppError> {
    if !user.must_change_password {
        return Ok(Redirect::to("/").into_response());
    }

    let errors = check_new_password(&user, &input, &i18n)?;
    if !errors.is_empty() {
        return render_page(&user, csrf_token.0, i18n, errors);
    }

    let hash = AuthService::hash_password(&input.password)?;
    UserRepository::update_password(&state.pool, user.id, &hash).await?;
    stamp_credential(&session, &hash).await?;
    tracing::info!(user_id = user.id, "Temporary password replaced");

    Ok(Redirect::to("/").into_response())
}
