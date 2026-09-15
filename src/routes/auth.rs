//! Authentication routes - login, register, logout.

use std::net::SocketAddr;

use askama::Template;
use axum::extract::{ConnectInfo, State};
use axum::http::header::{HeaderValue, LOCATION, SET_COOKIE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;
use time::Duration;
use tower_sessions::{Expiry, Session};
use validator::{Validate, ValidationErrors};

use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::{CsrfToken, HtmlForm, OptionalUser, ValidatedForm};
use crate::models::{Role, User};
use crate::repositories::UserRepository;
use crate::services::AuthService;
use crate::session::USER_ID_KEY;
use crate::state::AppState;

/// Who the registration page is for. Decided from the database and the
/// public mode, never from the visitor.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RegisterDoor {
    /// No account exists yet: the form creates the first administrator.
    FirstAccount,
    /// Members-only instance: anyone may create a reader account.
    Members,
    /// Public instance with accounts: the team page creates accounts.
    Closed,
}

impl RegisterDoor {
    async fn resolve(state: &AppState) -> Result<Self, AppError> {
        if UserRepository::count_all(&state.pool).await? == 0 {
            return Ok(Self::FirstAccount);
        }
        if state.is_public_mode() {
            Ok(Self::Closed)
        } else {
            Ok(Self::Members)
        }
    }

    fn is_first_account(self) -> bool {
        matches!(self, Self::FirstAccount)
    }

    fn is_members(self) -> bool {
        matches!(self, Self::Members)
    }
}

/// Messages placed under the field they concern; `form` is for anything
/// that is not about one field.
#[derive(Default)]
struct RegisterErrors {
    name: Option<String>,
    email: Option<String>,
    password: Option<String>,
    confirm: Option<String>,
    form: Option<String>,
}

impl RegisterErrors {
    fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.email.is_none()
            && self.password.is_none()
            && self.confirm.is_none()
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
        } else if self.confirm.is_some() {
            "password_confirm"
        } else {
            "display_name"
        }
    }

    fn absorb(&mut self, errors: &ValidationErrors, i18n: &I18n) {
        for (field, errs) in errors.field_errors() {
            let Some(message) = errs.first().and_then(|e| e.message.as_deref()) else {
                continue;
            };
            let message = i18n.t(message).to_string();
            match field {
                "email" => self.email = Some(message),
                "password" => self.password = Some(message),
                _ => self.form = Some(message),
            }
        }
    }

    fn from_service(key: &str, i18n: &I18n) -> Self {
        let message = Some(i18n.t(key).to_string());
        let mut errors = Self::default();
        match key {
            "validation.email_taken" | "validation.email_invalid" => errors.email = message,
            "validation.password_min_length" => errors.password = message,
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
    door: RegisterDoor,
    i18n: I18n,
}

#[derive(Template)]
#[template(path = "auth/register.html")]
struct RegisterTemplate {
    csrf_token: String,
    errors: RegisterErrors,
    email: String,
    display_name: String,
    instance: String,
    powered_by: bool,
    door: RegisterDoor,
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

/// Validated in the handler, not by the extractor: a short password must
/// come back into the form with the other fields kept, not as a bare 400.
#[derive(Deserialize, Validate)]
pub struct RegisterInput {
    #[validate(email(message = "validation.email_invalid"))]
    email: String,
    display_name: String,
    #[validate(length(min = 12, message = "validation.password_min_length"))]
    password: String,
    password_confirm: String,
}

const DISPLAY_NAME_MAX: usize = 100;

/// 303 redirect with an optional `lang` cookie sync. Used after login so the
/// authenticated user lands on a page already rendered in their saved locale.
fn redirect_with_locale(target: &str, locale: Option<&str>) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        LOCATION,
        HeaderValue::from_str(target).unwrap_or_else(|_| HeaderValue::from_static("/")),
    );
    if let Some(loc) = locale {
        let cookie = format!("lang={loc}; Path=/; Max-Age=31536000; SameSite=Lax");
        if let Ok(value) = HeaderValue::from_str(&cookie) {
            headers.insert(SET_COOKIE, value);
        }
    }
    (StatusCode::SEE_OTHER, headers).into_response()
}

/// Render an Askama template into an HTML response.
fn render(tpl: &impl Template) -> Result<Response, AppError> {
    let html = tpl
        .render()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("template render error: {e}")))?;
    Ok(Html(html).into_response())
}

/// The name the door is titled with, and whether it is the host's own name
/// (in which case the page also says who built the product).
fn instance_title() -> (String, bool) {
    let powered_by = !crate::instance_name().is_empty();
    (crate::brand_name(), powered_by)
}

async fn render_login(
    state: &AppState,
    csrf_token: String,
    i18n: I18n,
    error: Option<String>,
    email: String,
) -> Result<Response, AppError> {
    let (instance, powered_by) = instance_title();
    let tpl = LoginTemplate {
        csrf_token,
        error,
        email,
        instance,
        powered_by,
        door: RegisterDoor::resolve(state).await?,
        i18n,
    };
    render(&tpl)
}

pub async fn login_form(
    State(state): State<AppState>,
    OptionalUser(user): OptionalUser,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    if user.is_some() {
        return Ok(Redirect::to("/").into_response());
    }
    render_login(&state, csrf_token.0, i18n, None, String::new()).await
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

    if state.login_limiter.is_blocked(&ip) {
        tracing::warn!(ip = %ip, "Login blocked by rate limiter");
        let message = i18n.t("validation.rate_limited").to_string();
        return render_login(&state, csrf_token.0, i18n, Some(message), input.email).await;
    }

    match AuthService::login(&state.pool, &input.email, &input.password).await {
        Ok(user) => {
            state.login_limiter.clear(&ip);

            let expiry = if input.remember_me.is_some() {
                Expiry::OnInactivity(Duration::days(30))
            } else {
                Expiry::OnInactivity(Duration::hours(24))
            };
            open_session(&session, &user, expiry).await?;

            Ok(redirect_with_locale("/", user.preferred_locale.as_deref()))
        }
        Err(e) => {
            state.login_limiter.record_failure(&ip);

            let message = match &e {
                AppError::Validation(msg) => i18n.t(msg).to_string(),
                _ => i18n.t("validation.generic_error").to_string(),
            };
            render_login(&state, csrf_token.0, i18n, Some(message), input.email).await
        }
    }
}

async fn open_session(session: &Session, user: &User, expiry: Expiry) -> Result<(), AppError> {
    session.set_expiry(Some(expiry));
    session
        .insert(USER_ID_KEY, user.id)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("session insert failed: {e}")))
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

fn render_register(
    csrf_token: String,
    i18n: I18n,
    door: RegisterDoor,
    errors: RegisterErrors,
    email: String,
    display_name: String,
) -> Result<Response, AppError> {
    let (instance, powered_by) = instance_title();
    let tpl = RegisterTemplate {
        csrf_token,
        errors,
        email,
        display_name,
        instance,
        powered_by,
        door,
        i18n,
    };
    render(&tpl)
}

/// The first account of an empty instance, or a reader account on a
/// members-only instance. Closed on a public instance that has accounts.
pub async fn register_form(
    State(state): State<AppState>,
    OptionalUser(user): OptionalUser,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    let door = RegisterDoor::resolve(&state).await?;
    if door == RegisterDoor::Closed {
        return Ok(closed_door_redirect(user.as_ref()));
    }
    render_register(
        csrf_token.0,
        i18n,
        door,
        RegisterErrors::default(),
        String::new(),
        String::new(),
    )
}

pub async fn logout(session: Session) -> Result<Response, AppError> {
    AuthService::logout(&session).await;
    Ok(Redirect::to("/login").into_response())
}

fn check_register_input(input: &RegisterInput, i18n: &I18n) -> RegisterErrors {
    let mut errors = RegisterErrors::default();
    let name = input.display_name.trim();
    if name.is_empty() {
        errors.name = Some(i18n.t("validation.display_name_required").to_string());
    } else if name.chars().count() > DISPLAY_NAME_MAX {
        errors.name = Some(i18n.t("validation.display_name_too_long").to_string());
    }
    if let Err(e) = input.validate() {
        errors.absorb(&e, i18n);
    }
    if errors.password.is_none() && input.password != input.password_confirm {
        errors.confirm = Some(i18n.t("validation.passwords_mismatch").to_string());
    }
    errors
}

pub async fn register(
    State(state): State<AppState>,
    OptionalUser(user): OptionalUser,
    session: Session,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
    HtmlForm(input): HtmlForm<RegisterInput>,
) -> Result<Response, AppError> {
    let door = RegisterDoor::resolve(&state).await?;
    if door == RegisterDoor::Closed {
        return Ok(closed_door_redirect(user.as_ref()));
    }

    let errors = check_register_input(&input, &i18n);
    if !errors.is_empty() {
        return render_register(
            csrf_token.0,
            i18n,
            door,
            errors,
            input.email,
            input.display_name,
        );
    }

    let role = if door.is_first_account() {
        Role::Admin
    } else {
        Role::Reader
    };
    let name = input.display_name.trim();

    match AuthService::register(&state.pool, &input.email, &input.password, name, role).await {
        Ok(created) => {
            open_session(
                &session,
                &created,
                Expiry::OnInactivity(Duration::hours(24)),
            )
            .await?;
            Ok(Redirect::to("/").into_response())
        }
        Err(AppError::Validation(key)) => {
            let errors = RegisterErrors::from_service(&key, &i18n);
            render_register(
                csrf_token.0,
                i18n,
                door,
                errors,
                input.email,
                input.display_name,
            )
        }
        Err(e) => Err(e),
    }
}
