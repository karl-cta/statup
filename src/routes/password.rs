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
use crate::services::AuthService;
use crate::session::{rotate_id, stamp_credential};
use crate::state::AppState;

#[derive(Default)]
struct NewPasswordErrors {
    password: Option<String>,
    confirm: Option<String>,
}

impl NewPasswordErrors {
    /// The message under the field it is about.
    fn from_key(key: &str, i18n: &I18n) -> Self {
        let message = Some(i18n.t(key).to_string());
        if key == "validation.passwords_mismatch" {
            Self {
                confirm: message,
                ..Self::default()
            }
        } else {
            Self {
                password: message,
                ..Self::default()
            }
        }
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

/// The temporary password itself is refused first, otherwise typing it
/// again would pass the step.
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
    if AuthService::verify_password(&input.password, &user.password_hash).await? {
        let errors = NewPasswordErrors::from_key("validation.password_unchanged", &i18n);
        return render_page(&user, csrf_token, i18n, errors);
    }
    let changed = AuthService::change_password(
        &state.pool,
        user.id,
        &input.password,
        &input.password_confirm,
    );
    match changed.await {
        Ok(hash) => {
            reopen_session(&session, &hash).await?;
            Ok(Redirect::to("/").into_response())
        }
        Err(AppError::Validation(key)) => {
            let errors = NewPasswordErrors::from_key(&key, &i18n);
            render_page(&user, csrf_token, i18n, errors)
        }
        Err(e) => Err(e),
    }
}
