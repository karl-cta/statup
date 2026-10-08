//! The way back in for a member who forgot their password: a link by email,
//! when the instance can send one, and what to do instead when it cannot.
//!
//! The answer to a request never tells whether the address has an account:
//! the account is looked up and the email sent after the page is answered.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use askama::Template;
use axum::Extension;
use axum::extract::{ConnectInfo, Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;
use tower_sessions::Session;

use super::auth::{instance_title, open_session, request_ip, signed_in_redirect};
use super::password::NewPasswordErrors;
use super::render;
use crate::db::DbPool;
use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::csrf::form_token;
use crate::middleware::headers::no_store;
use crate::middleware::{FormCsrfToken, HtmlForm, OptionalUser};
use crate::models::User;
use crate::services::{Notifier, PasswordResetService, ResetEmail, page_address};
use crate::state::AppState;

const INVALID_LINK: &str = "validation.reset_link_invalid";

#[derive(Template)]
#[template(path = "auth/forgot_password.html")]
struct ForgotTemplate {
    csrf_token: String,
    /// Whether the instance can send a link; when not, the page says what
    /// to do instead.
    mail_ready: bool,
    sent: bool,
    instance: String,
    powered_by: bool,
    i18n: I18n,
}

#[derive(Template)]
#[template(path = "auth/reset_password.html")]
struct ResetTemplate {
    csrf_token: String,
    token: String,
    /// The account the link opens, `None` for a link that no longer works.
    email: Option<String>,
    errors: NewPasswordErrors,
    instance: String,
    powered_by: bool,
    i18n: I18n,
}

#[derive(Deserialize)]
pub struct ForgotInput {
    #[serde(default)]
    email: String,
}

#[derive(Deserialize)]
pub struct ResetQuery {
    #[serde(default)]
    token: String,
}

#[derive(Deserialize)]
pub struct ResetInput {
    #[serde(default)]
    token: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    password_confirm: String,
}

/// The address the links lead to, when the instance can send them. Never
/// taken from the request, which anyone can word.
async fn link_base(state: &AppState) -> Result<Option<String>, AppError> {
    if !state.notifier.mail_ready() {
        return Ok(None);
    }
    page_address(&state.pool, state.public_url.as_deref()).await
}

fn render_forgot(
    csrf_token: String,
    i18n: I18n,
    mail_ready: bool,
    sent: bool,
) -> Result<Response, AppError> {
    let (instance, powered_by) = instance_title();
    render(&ForgotTemplate {
        csrf_token,
        mail_ready,
        sent,
        instance,
        powered_by,
        i18n,
    })
}

pub async fn forgot_form(
    State(state): State<AppState>,
    OptionalUser(user): OptionalUser,
    session: Session,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    if user.is_some() {
        return Ok(Redirect::to("/").into_response());
    }
    let csrf_token = form_token(&session).await?;
    let mail_ready = link_base(&state).await?.is_some();
    render_forgot(csrf_token, i18n, mail_ready, false)
}

/// The same page whatever the address: a request past the limit of this
/// address, or of the account, is dropped without a word.
pub async fn forgot(
    State(state): State<AppState>,
    FormCsrfToken(csrf_token): FormCsrfToken,
    headers: HeaderMap,
    connect_info: Option<Extension<ConnectInfo<SocketAddr>>>,
    Locale(i18n): Locale,
    HtmlForm(input): HtmlForm<ForgotInput>,
) -> Result<Response, AppError> {
    let Some(base) = link_base(&state).await? else {
        return render_forgot(csrf_token, i18n, false, false);
    };
    let email = input.email.trim();
    if email.is_empty() {
        return render_forgot(csrf_token, i18n, true, false);
    }
    let ip = request_ip(&state, &headers, connect_info);
    if state.login_limiter.allow_reset_request(&ip) {
        send_link_later(&state, email.to_string(), base, i18n.locale());
    } else {
        tracing::warn!(ip = %ip, "Password reset request blocked by rate limiter");
    }
    render_forgot(csrf_token, i18n, true, true)
}

fn send_link_later(state: &AppState, email: String, base: String, page_locale: &'static str) {
    let pool = state.pool.clone();
    let notifier = Arc::clone(&state.notifier);
    tokio::spawn(async move {
        if let Err(e) = send_link(&pool, &notifier, &email, &base, page_locale).await {
            tracing::error!("Password reset link not issued: {e}");
        }
    });
}

/// Issues a link for the account, if there is one, and emails it in the
/// member's language, else the language the page was read in.
async fn send_link(
    pool: &DbPool,
    notifier: &Notifier,
    email: &str,
    base: &str,
    page_locale: &str,
) -> Result<(), AppError> {
    let Some(link) = PasswordResetService::request(pool, email).await? else {
        return Ok(());
    };
    let i18n = I18n::new(link.user.preferred_locale.as_deref().unwrap_or(page_locale));
    let url = format!("{base}/password/reset?token={}", link.token);
    let instance = crate::brand_name();
    let mail = ResetEmail {
        to: &link.user.email,
        url: &url,
        instance: &instance,
        i18n: &i18n,
    };
    if let Err(failure) = notifier.send_password_reset(&mail).await {
        tracing::warn!(user_id = link.user.id, failure = %failure.code(), "Password reset email not sent");
    }
    Ok(())
}

fn render_reset(page: &ResetTemplate) -> Result<Response, AppError> {
    Ok(no_store(render(page)?))
}

fn reset_page(
    csrf_token: String,
    i18n: I18n,
    token: String,
    email: Option<String>,
    errors: NewPasswordErrors,
) -> ResetTemplate {
    let (instance, powered_by) = instance_title();
    // A link that no longer works is not written back into the page.
    let token = if email.is_some() {
        token
    } else {
        String::new()
    };
    ResetTemplate {
        csrf_token,
        token,
        email,
        errors,
        instance,
        powered_by,
        i18n,
    }
}

/// The token travels in the query, which the request log leaves out.
pub async fn reset_form(
    State(state): State<AppState>,
    session: Session,
    Locale(i18n): Locale,
    Query(query): Query<ResetQuery>,
) -> Result<Response, AppError> {
    let csrf_token = form_token(&session).await?;
    let account = PasswordResetService::account_for(&state.pool, &query.token).await?;
    let email = account.map(|user| user.email);
    let page = reset_page(
        csrf_token,
        i18n,
        query.token,
        email,
        NewPasswordErrors::default(),
    );
    render_reset(&page)
}

/// The link proved who the member is, like a sign-in: their sign-in
/// failures from this address are forgotten.
async fn sign_in(
    state: &AppState,
    session: &Session,
    ip: &IpAddr,
    user: &User,
) -> Result<Response, AppError> {
    state.login_limiter.clear(ip, &user.email);
    open_session(session, user, false).await?;
    Ok(signed_in_redirect(user, state.serves_https()))
}

/// Signs the member in with the new password; their other sessions no
/// longer match it.
pub async fn reset(
    State(state): State<AppState>,
    session: Session,
    FormCsrfToken(csrf_token): FormCsrfToken,
    headers: HeaderMap,
    connect_info: Option<Extension<ConnectInfo<SocketAddr>>>,
    Locale(i18n): Locale,
    HtmlForm(input): HtmlForm<ResetInput>,
) -> Result<Response, AppError> {
    let redeemed = PasswordResetService::redeem(
        &state.pool,
        &input.token,
        &input.password,
        &input.password_confirm,
    );
    match redeemed.await {
        Ok(user) => {
            let ip = request_ip(&state, &headers, connect_info);
            sign_in(&state, &session, &ip, &user).await
        }
        Err(AppError::Validation(key)) if key == INVALID_LINK => {
            let page = reset_page(
                csrf_token,
                i18n,
                input.token,
                None,
                NewPasswordErrors::default(),
            );
            render_reset(&page)
        }
        Err(AppError::Validation(key)) => {
            let account = PasswordResetService::account_for(&state.pool, &input.token).await?;
            let errors = NewPasswordErrors::from_key(&key, &i18n);
            let email = account.map(|user| user.email);
            let page = reset_page(csrf_token, i18n, input.token, email, errors);
            render_reset(&page)
        }
        Err(e) => Err(e),
    }
}
