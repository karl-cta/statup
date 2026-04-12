//! Authentication routes - login, register, logout.

use std::net::SocketAddr;

use askama::Template;
use axum::extract::{ConnectInfo, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;
use time::Duration;
use tower_sessions::{Expiry, Session};
use validator::Validate;

use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::{CsrfToken, OptionalUser, ValidatedForm};
use crate::services::AuthService;
use crate::session::USER_ID_KEY;
use crate::state::AppState;

#[derive(Template)]
#[template(path = "auth/login.html")]
struct LoginTemplate {
    csrf_token: String,
    error: Option<String>,
    email: String,
    i18n: I18n,
}

#[derive(Template)]
#[template(path = "auth/register.html")]
struct RegisterTemplate {
    csrf_token: String,
    error: Option<String>,
    email: String,
    display_name: String,
    i18n: I18n,
}

#[derive(Deserialize, Validate)]
pub struct LoginInput {
    #[validate(length(min = 1, message = "validation.email_required"))]
    email: String,
    #[validate(length(min = 1, message = "validation.password_required"))]
    password: String,
    remember_me: Option<String>,
}

#[derive(Deserialize, Validate)]
pub struct RegisterInput {
    #[validate(email(message = "validation.email_invalid"))]
    email: String,
    #[validate(length(min = 1, max = 100, message = "validation.display_name_required"))]
    display_name: String,
    #[validate(length(min = 12, message = "validation.password_min_length"))]
    password: String,
    password_confirm: String,
}

/// Render an Askama template into an HTML response.
fn render(tpl: &impl Template) -> Result<Response, AppError> {
    let html = tpl
        .render()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("template render error: {e}")))?;
    Ok(Html(html).into_response())
}

pub async fn login_form(csrf_token: CsrfToken, Locale(i18n): Locale) -> Result<Response, AppError> {
    let tpl = LoginTemplate {
        csrf_token: csrf_token.0,
        error: None,
        email: String::new(),
        i18n,
    };
    render(&tpl)
}

pub async fn login(
    State(state): State<AppState>,
    session: Session,
    csrf_token: CsrfToken,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    Locale(i18n): Locale,
    ValidatedForm(input): ValidatedForm<LoginInput>,
) -> Result<Response, AppError> {
    let ip = connect_info.map_or_else(|| [127, 0, 0, 1].into(), |ci| ci.0.ip());

    // Check rate limit
    if state.login_limiter.is_blocked(&ip) {
        tracing::warn!(ip = %ip, "Login blocked by rate limiter");
        let tpl = LoginTemplate {
            csrf_token: csrf_token.0,
            error: Some(i18n.t("validation.rate_limited").to_string()),
            email: input.email,
            i18n,
        };
        return render(&tpl);
    }

    // Attempt login
    match AuthService::login(&state.pool, &input.email, &input.password).await {
        Ok(user) => {
            state.login_limiter.clear(&ip);

            let expiry = if input.remember_me.is_some() {
                Expiry::OnInactivity(Duration::days(30))
            } else {
                Expiry::OnInactivity(Duration::hours(24))
            };
            session.set_expiry(Some(expiry));

            session
                .insert(USER_ID_KEY, user.id)
                .await
                .map_err(|e| AppError::Internal(anyhow::anyhow!("session insert failed: {e}")))?;

            Ok(Redirect::to("/").into_response())
        }
        Err(e) => {
            state.login_limiter.record_failure(&ip);

            let message = match &e {
                AppError::Validation(msg) => i18n.t(msg).to_string(),
                _ => i18n.t("validation.generic_error").to_string(),
            };

            let tpl = LoginTemplate {
                csrf_token: csrf_token.0,
                error: Some(message),
                email: input.email,
                i18n,
            };
            render(&tpl)
        }
    }
}

///
/// When `PUBLIC_MODE=true`, only admins can access this page (REQ-16.3).
/// When `PUBLIC_MODE=false`, the form is publicly accessible (REQ-16.4).
pub async fn register_form(
    State(state): State<AppState>,
    OptionalUser(user): OptionalUser,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    if state.is_public_mode() {
        match &user {
            Some(u) if u.role.can_admin() => {}
            Some(_) => return Err(AppError::Forbidden),
            None => return Err(AppError::Unauthorized),
        }
    }

    let tpl = RegisterTemplate {
        csrf_token: csrf_token.0,
        error: None,
        email: String::new(),
        display_name: String::new(),
        i18n,
    };
    render(&tpl)
}

pub async fn logout(session: Session) -> Result<Response, AppError> {
    AuthService::logout(&session).await;
    Ok(Redirect::to("/login").into_response())
}

///
/// When `PUBLIC_MODE=true`, only admins can register new users (REQ-16.3).
pub async fn register(
    State(state): State<AppState>,
    OptionalUser(user): OptionalUser,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
    ValidatedForm(input): ValidatedForm<RegisterInput>,
) -> Result<Response, AppError> {
    if state.is_public_mode() {
        match &user {
            Some(u) if u.role.can_admin() => {}
            Some(_) => return Err(AppError::Forbidden),
            None => return Err(AppError::Unauthorized),
        }
    }
    let render_error = |msg: String, i18n: I18n| {
        let tpl = RegisterTemplate {
            csrf_token: csrf_token.0.clone(),
            error: Some(msg),
            email: input.email.clone(),
            display_name: input.display_name.clone(),
            i18n,
        };
        render(&tpl)
    };

    if input.password != input.password_confirm {
        return render_error(i18n.t("validation.passwords_mismatch").to_string(), i18n);
    }

    match AuthService::register(
        &state.pool,
        &input.email,
        &input.password,
        &input.display_name,
    )
    .await
    {
        Ok(_) => Ok(Redirect::to("/login").into_response()),
        Err(AppError::Validation(msg)) => render_error(i18n.t(&msg).to_string(), i18n),
        Err(e) => Err(e),
    }
}
