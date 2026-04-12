//! Profile routes - view and edit own user profile.

use askama::Template;
use axum::extract::State;
use axum::response::{Html, IntoResponse, Response};
use serde::Deserialize;
use validator::Validate;

use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::{AuthUser, CsrfToken, ValidatedForm};
use crate::models::User;
use crate::repositories::{EventRepository, UserRepository};
use crate::services::{AuthService, EventService};
use crate::state::AppState;

#[derive(Template)]
#[template(path = "profile/edit.html")]
struct ProfileTemplate {
    csrf_token: String,
    user_display_name: String,
    is_admin: bool,
    is_authenticated: bool,
    unread_count: i64,
    last_admin_action: Option<String>,
    email: String,
    display_name: String,
    role_label: String,
    error: Option<String>,
    success: Option<String>,
    password_error: Option<String>,
    password_success: Option<String>,
    i18n: I18n,
}

#[derive(Deserialize, Validate)]
pub struct ProfileInput {
    #[validate(email(message = "validation.email_invalid"))]
    email: String,
    #[validate(length(min = 1, max = 100, message = "validation.display_name_required"))]
    display_name: String,
}

#[derive(Deserialize, Validate)]
pub struct PasswordInput {
    #[validate(length(min = 1, message = "validation.current_password_required"))]
    current_password: String,
    #[validate(length(min = 12, message = "validation.new_password_min_length"))]
    new_password: String,
    new_password_confirm: String,
}

fn render(tpl: &impl Template) -> Result<Response, AppError> {
    let html = tpl
        .render()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("template render error: {e}")))?;
    Ok(Html(html).into_response())
}

fn layout_fields(user: &User) -> (String, bool, bool) {
    (user.display_name.clone(), user.role.can_admin(), true)
}

async fn unread(pool: &crate::db::DbPool, user: &User) -> Result<i64, AppError> {
    EventService::unread_count(pool, user.last_seen_at).await
}

fn role_label(user: &User, i18n: &I18n) -> String {
    match user.role {
        crate::models::Role::Reader => i18n.t("role.reader").to_string(),
        crate::models::Role::Publisher => i18n.t("role.publisher").to_string(),
        crate::models::Role::Admin => i18n.t("role.admin").to_string(),
    }
}

#[allow(clippy::too_many_arguments)]
async fn build_template(
    user: &User,
    state: &AppState,
    csrf_token: String,
    i18n: I18n,
    error: Option<String>,
    success: Option<String>,
    password_error: Option<String>,
    password_success: Option<String>,
) -> Result<ProfileTemplate, AppError> {
    let (user_display_name, is_admin, is_authenticated) = layout_fields(user);
    let unread_count = unread(&state.pool, user).await?;
    let last_admin_action = EventRepository::last_admin_action(&state.pool)
        .await?
        .map(|dt| i18n.format_datetime_long(&dt));

    Ok(ProfileTemplate {
        csrf_token,
        user_display_name,
        is_admin,
        is_authenticated,
        unread_count,
        last_admin_action,
        email: user.email.clone(),
        display_name: user.display_name.clone(),
        role_label: role_label(user, &i18n),
        error,
        success,
        password_error,
        password_success,
        i18n,
    })
}

pub async fn edit_form(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    let tpl = build_template(&user, &state, csrf_token.0, i18n, None, None, None, None).await?;
    render(&tpl)
}

pub async fn update_profile(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
    ValidatedForm(input): ValidatedForm<ProfileInput>,
) -> Result<Response, AppError> {
    let email = input.email.trim().to_lowercase();
    let display_name = input.display_name.trim().to_string();

    if email.is_empty() || !email.contains('@') || email.len() < 5 {
        let msg = i18n.t("validation.email_invalid").to_string();
        let tpl = build_template(
            &user,
            &state,
            csrf_token.0,
            i18n,
            Some(msg),
            None,
            None,
            None,
        )
        .await?;
        return render(&tpl);
    }

    if UserRepository::email_taken_by_other(&state.pool, &email, user.id).await? {
        let msg = i18n.t("validation.email_taken").to_string();
        let tpl = build_template(
            &user,
            &state,
            csrf_token.0,
            i18n,
            Some(msg),
            None,
            None,
            None,
        )
        .await?;
        return render(&tpl);
    }

    UserRepository::update_profile(&state.pool, user.id, &email, &display_name).await?;

    tracing::info!(user_id = user.id, "Profile updated");

    let updated_user = UserRepository::find_by_id(&state.pool, user.id)
        .await?
        .ok_or(AppError::NotFound)?;

    let msg = i18n.t("success.profile_updated").to_string();
    let tpl = build_template(
        &updated_user,
        &state,
        csrf_token.0,
        i18n,
        None,
        Some(msg),
        None,
        None,
    )
    .await?;
    render(&tpl)
}

pub async fn update_password(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
    ValidatedForm(input): ValidatedForm<PasswordInput>,
) -> Result<Response, AppError> {
    if !AuthService::verify_password(&input.current_password, &user.password_hash)? {
        let msg = i18n.t("validation.wrong_password").to_string();
        let tpl = build_template(
            &user,
            &state,
            csrf_token.0,
            i18n,
            None,
            None,
            Some(msg),
            None,
        )
        .await?;
        return render(&tpl);
    }

    if input.new_password != input.new_password_confirm {
        let msg = i18n.t("validation.new_passwords_mismatch").to_string();
        let tpl = build_template(
            &user,
            &state,
            csrf_token.0,
            i18n,
            None,
            None,
            Some(msg),
            None,
        )
        .await?;
        return render(&tpl);
    }

    AuthService::validate_password(&input.new_password)?;

    let hash = AuthService::hash_password(&input.new_password)?;
    UserRepository::update_password(&state.pool, user.id, &hash).await?;

    tracing::info!(user_id = user.id, "Password changed");

    let msg = i18n.t("success.password_changed").to_string();
    let tpl = build_template(
        &user,
        &state,
        csrf_token.0,
        i18n,
        None,
        None,
        None,
        Some(msg),
    )
    .await?;
    render(&tpl)
}
