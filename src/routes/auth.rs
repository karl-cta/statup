//! Sign-in, creation of the first account, and sign-out.
//!
//! Self-registration only exists on an empty instance: its first account is
//! the administrator, who then adds everyone else from the team page.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use askama::Template;
use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use axum::http::header::{LOCATION, SET_COOKIE};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;
use tower_sessions::Session;

use super::locale::locale_cookie;
use super::render;
use super::setup::{Preview, Progress};
use crate::clock;
use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::client_ip::client_ip;
use crate::middleware::csrf::{form_token, renew_token};
use crate::middleware::{FormCsrfToken, HtmlForm, OptionalUser};
use crate::models::{User, check_display_name};
use crate::repositories::{SettingsRepository, UserRepository};
use crate::services::AuthService;
use crate::session::{USER_ID_KEY, rotate_id, stamp_credential, start_signed_in, write_value};
use crate::state::AppState;

/// Messages placed under the field they concern; `form` is for anything
/// that is not about one field.
#[derive(Default)]
struct RegisterErrors {
    name: Option<String>,
    email: Option<String>,
    password: Option<String>,
    form: Option<String>,
}

impl RegisterErrors {
    fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.email.is_none()
            && self.password.is_none()
            && self.form.is_none()
    }

    /// The id of the first field with a message, where the cursor goes.
    fn focus(&self) -> &'static str {
        if self.name.is_some() {
            "display_name"
        } else if self.email.is_some() {
            "email"
        } else if self.password.is_some() {
            "password"
        } else {
            "display_name"
        }
    }

    fn from_service(key: &str, i18n: &I18n) -> Self {
        let message = Some(i18n.t(key).to_string());
        let mut errors = Self::default();
        match key {
            "validation.email_taken" | "validation.email_invalid" => errors.email = message,
            "validation.password_too_weak" => errors.password = message,
            _ => errors.form = message,
        }
        errors
    }
}

#[derive(Template)]
#[template(path = "auth/login.html")]
struct LoginTemplate {
    csrf_token: String,
    error: Option<String>,
    email: String,
    instance: String,
    powered_by: bool,
    i18n: I18n,
}

/// The first step of the first launch.
#[derive(Template)]
#[template(path = "auth/register.html")]
struct RegisterTemplate {
    csrf_token: String,
    progress: Progress,
    preview: Preview,
    errors: RegisterErrors,
    email: String,
    display_name: String,
    i18n: I18n,
}

#[derive(Deserialize)]
pub struct LoginInput {
    #[serde(default)]
    email: String,
    #[serde(default)]
    password: String,
    remember_me: Option<String>,
}

#[derive(Deserialize)]
pub struct RegisterInput {
    #[serde(default)]
    email: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    password: String,
    /// The zone the browser reports, filled in by script.
    #[serde(default)]
    time_zone: String,
}

/// The name the door is titled with, and whether it is the host's own name
/// (in which case the page also says who built the product).
pub(super) fn instance_title() -> (String, bool) {
    let powered_by = !crate::instance_name().is_empty();
    (crate::brand_name(), powered_by)
}

async fn instance_is_empty(state: &AppState) -> Result<bool, AppError> {
    Ok(UserRepository::count_all(&state.pool).await? == 0)
}

fn render_login(
    csrf_token: String,
    i18n: I18n,
    error: Option<String>,
    email: String,
) -> Result<Response, AppError> {
    let (instance, powered_by) = instance_title();
    render(&LoginTemplate {
        csrf_token,
        error,
        email,
        instance,
        powered_by,
        i18n,
    })
}

pub async fn login_form(
    State(state): State<AppState>,
    OptionalUser(user): OptionalUser,
    session: Session,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    if user.is_some() {
        return Ok(Redirect::to("/").into_response());
    }
    // Nobody can sign in yet: whoever installed the instance creates its
    // administrator first.
    if instance_is_empty(&state).await? {
        return Ok(Redirect::to("/register").into_response());
    }
    let csrf_token = form_token(&session).await?;
    render_login(csrf_token, i18n, None, String::new())
}

pub async fn login(
    State(state): State<AppState>,
    session: Session,
    FormCsrfToken(csrf_token): FormCsrfToken,
    headers: HeaderMap,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    Locale(i18n): Locale,
    HtmlForm(input): HtmlForm<LoginInput>,
) -> Result<Response, AppError> {
    let peer = connect_info.map(|info| info.0.ip());
    let ip = client_ip(&headers, peer, &state.client_ip_source)
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));

    let refusal = missing_credentials(&input).or_else(|| blocked(&state, &ip, &input.email));
    if let Some(key) = refusal {
        let message = Some(i18n.t(key).to_string());
        return render_login(csrf_token, i18n, message, input.email);
    }

    match AuthService::login(&state.pool, &input.email, &input.password).await {
        Ok(user) => {
            state.login_limiter.clear(&ip, &input.email);
            open_session(&session, &user, input.remember_me.is_some()).await?;
            Ok(signed_in_redirect(&user, state.serves_https()))
        }
        Err(AppError::Validation(key)) => {
            state.login_limiter.record_failure(&ip, &input.email);
            let message = Some(i18n.t(&key).to_string());
            render_login(csrf_token, i18n, message, input.email)
        }
        Err(e) => Err(e),
    }
}

fn blocked(state: &AppState, ip: &IpAddr, email: &str) -> Option<&'static str> {
    if !state.login_limiter.is_blocked(ip, email) {
        return None;
    }
    tracing::warn!(ip = %ip, "Login blocked by rate limiter");
    Some("validation.rate_limited")
}

fn missing_credentials(input: &LoginInput) -> Option<&'static str> {
    if input.email.trim().is_empty() {
        Some("validation.email_required")
    } else if input.password.is_empty() {
        Some("validation.password_required")
    } else {
        None
    }
}

/// Signs the person in on a new session id, with a new form token, for 30
/// days of inactivity when they asked to stay signed in, 24 hours otherwise.
async fn open_session(session: &Session, user: &User, remember: bool) -> Result<(), AppError> {
    rotate_id(session).await?;
    write_value(session, USER_ID_KEY, user.id).await?;
    stamp_credential(session, &user.password_hash).await?;
    start_signed_in(session, remember).await?;
    renew_token(session).await?;
    Ok(())
}

/// Home, or the page that replaces a temporary password, with the saved
/// language so the first page is already in it.
fn signed_in_redirect(user: &User, secure: bool) -> Response {
    let target = if user.must_change_password {
        "/password/new"
    } else {
        "/"
    };
    let mut response = (StatusCode::SEE_OTHER, [(LOCATION, target)]).into_response();
    if let Some(cookie) = user
        .preferred_locale
        .as_deref()
        .and_then(|locale| HeaderValue::from_str(&locale_cookie(locale, secure)).ok())
    {
        response.headers_mut().insert(SET_COOKIE, cookie);
    }
    response
}

/// Where a visitor lands when the registration page is closed: the team
/// page creates accounts, so an admin goes there, everyone else goes home.
fn closed_door_redirect(user: Option<&User>) -> Response {
    let target = match user {
        Some(u) if u.role.can_admin() => "/admin/users",
        Some(_) => "/",
        None => "/login",
    };
    Redirect::to(target).into_response()
}

async fn render_register(
    state: &AppState,
    csrf_token: String,
    i18n: I18n,
    errors: RegisterErrors,
    input: RegisterInput,
) -> Result<Response, AppError> {
    render(&RegisterTemplate {
        csrf_token,
        progress: Progress { step: 1 },
        preview: Preview::load(state).await?,
        errors,
        email: input.email,
        display_name: input.display_name,
        i18n,
    })
}

/// The form that creates the administrator of an empty instance. Closed as
/// soon as one account exists; no session is opened for a closed door.
pub async fn register_form(
    State(state): State<AppState>,
    OptionalUser(user): OptionalUser,
    session: Session,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    if !instance_is_empty(&state).await? {
        return Ok(closed_door_redirect(user.as_ref()));
    }
    let csrf_token = form_token(&session).await?;
    let blank = RegisterInput {
        email: String::new(),
        display_name: String::new(),
        password: String::new(),
        time_zone: String::new(),
    };
    render_register(&state, csrf_token, i18n, RegisterErrors::default(), blank).await
}

/// Signed out, a reader of an open page lands back on it.
pub async fn logout(State(state): State<AppState>, session: Session) -> Result<Response, AppError> {
    AuthService::logout(&session).await;
    let next = if state.is_public_mode() {
        "/"
    } else {
        "/login"
    };
    Ok(Redirect::to(next).into_response())
}

/// The field rules, checked together so every message shows at once.
/// Returns the trimmed display name when everything passes.
fn check_register_input(input: &RegisterInput, i18n: &I18n) -> Result<String, RegisterErrors> {
    let message = |key: &str| Some(i18n.t(key).to_string());
    let mut errors = RegisterErrors::default();
    let name = check_display_name(&input.display_name).unwrap_or_else(|key| {
        errors.name = message(key);
        String::new()
    });
    if AuthService::normalize_email(&input.email).is_err() {
        errors.email = message("validation.email_invalid");
    }
    if AuthService::validate_password(&input.password).is_err() {
        errors.password = message("validation.password_too_weak");
    }
    if errors.is_empty() {
        Ok(name)
    } else {
        Err(errors)
    }
}

/// The first administrator's browser knows where the team is: its zone
/// becomes the instance's, until changed in the settings.
async fn adopt_browser_zone(state: &AppState, name: &str) -> Result<(), AppError> {
    let Some(zone) = clock::parse_zone(name) else {
        return Ok(());
    };
    SettingsRepository::set(&state.pool, clock::ZONE_SETTING, zone.name()).await?;
    clock::set_zone(zone);
    tracing::info!(
        time_zone = zone.name(),
        "Instance time zone taken from the browser"
    );
    Ok(())
}

pub async fn register(
    State(state): State<AppState>,
    OptionalUser(user): OptionalUser,
    session: Session,
    FormCsrfToken(csrf_token): FormCsrfToken,
    Locale(i18n): Locale,
    HtmlForm(input): HtmlForm<RegisterInput>,
) -> Result<Response, AppError> {
    if !instance_is_empty(&state).await? {
        return Ok(closed_door_redirect(user.as_ref()));
    }
    let name = match check_register_input(&input, &i18n) {
        Ok(name) => name,
        Err(errors) => return render_register(&state, csrf_token, i18n, errors, input).await,
    };
    let created =
        AuthService::create_first_admin(&state.pool, &input.email, &input.password, &name).await;
    match created {
        Ok(Some(admin)) => {
            adopt_browser_zone(&state, &input.time_zone).await?;
            open_session(&session, &admin, false).await?;
            Ok(Redirect::to("/setup/page").into_response())
        }
        Ok(None) => Ok(closed_door_redirect(user.as_ref())),
        Err(AppError::Validation(key)) => {
            let errors = RegisterErrors::from_service(&key, &i18n);
            render_register(&state, csrf_token, i18n, errors, input).await
        }
        Err(e) => Err(e),
    }
}
