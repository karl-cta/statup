//! Admin routes - user management.

use askama::Template;
use axum::extract::{Path, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;
use validator::Validate;

use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::{CsrfToken, RequireAdmin, ValidatedForm};
use crate::models::{Role, User};
use crate::repositories::{EventRepository, SettingsRepository, UserRepository};
use crate::services::EventService;
use crate::state::AppState;

#[derive(Template)]
#[template(path = "admin/settings.html")]
struct SettingsPageTemplate {
    csrf_token: String,
    user_display_name: String,
    is_admin: bool,
    is_authenticated: bool,
    unread_count: i64,
    last_admin_action: Option<String>,
    public_mode: bool,
    users_count: i64,
    admins_count: i64,
    i18n: I18n,
}

#[derive(Template)]
#[template(path = "admin/users.html")]
struct UsersListTemplate {
    csrf_token: String,
    user_display_name: String,
    is_admin: bool,
    is_authenticated: bool,
    unread_count: i64,
    last_admin_action: Option<String>,
    current_user_id: i64,
    users: Vec<UserRow>,
    i18n: I18n,
}

#[allow(dead_code)]
struct UserRow {
    id: i64,
    email: String,
    display_name: String,
    role: Role,
    is_active: bool,
    last_seen_at: Option<String>,
    created_at: String,
}

#[derive(Deserialize, Validate)]
pub struct RoleInput {
    #[validate(length(min = 1, max = 20, message = "validation.invalid_role"))]
    role: String,
}

fn parse_role(s: &str) -> Result<Role, AppError> {
    match s {
        "reader" => Ok(Role::Reader),
        "publisher" => Ok(Role::Publisher),
        "admin" => Ok(Role::Admin),
        _ => Err(AppError::Validation("validation.invalid_role".to_string())),
    }
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

fn format_datetime(dt: chrono::DateTime<chrono::Utc>, i18n: &I18n) -> String {
    i18n.format_datetime(&dt)
}

fn to_user_row(u: User, i18n: &I18n) -> UserRow {
    UserRow {
        id: u.id,
        email: u.email,
        display_name: u.display_name,
        role: u.role,
        is_active: u.is_active,
        last_seen_at: u.last_seen_at.map(|dt| format_datetime(dt, i18n)),
        created_at: format_datetime(u.created_at, i18n),
    }
}

pub async fn settings_page(
    RequireAdmin(user): RequireAdmin,
    State(state): State<AppState>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    let (user_display_name, is_admin, is_authenticated) = layout_fields(&user);
    let unread_count = unread(&state.pool, &user).await?;
    let users_count = UserRepository::count_all(&state.pool).await?;
    let admins_count = UserRepository::count_admins(&state.pool).await?;
    let last_admin_action = EventRepository::last_admin_action(&state.pool)
        .await?
        .map(|dt| i18n.format_datetime_long(&dt));

    let tpl = SettingsPageTemplate {
        csrf_token: csrf_token.0,
        user_display_name,
        is_admin,
        is_authenticated,
        unread_count,
        last_admin_action,
        public_mode: state.is_public_mode(),
        users_count,
        admins_count,
        i18n,
    };
    render(&tpl)
}

pub async fn users_list(
    RequireAdmin(user): RequireAdmin,
    State(state): State<AppState>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    let (user_display_name, is_admin, is_authenticated) = layout_fields(&user);
    let unread_count = unread(&state.pool, &user).await?;
    let all_users = UserRepository::list_all(&state.pool).await?;
    let users: Vec<UserRow> = all_users
        .into_iter()
        .map(|u| to_user_row(u, &i18n))
        .collect();
    let last_admin_action = EventRepository::last_admin_action(&state.pool)
        .await?
        .map(|dt| i18n.format_datetime_long(&dt));

    let tpl = UsersListTemplate {
        csrf_token: csrf_token.0,
        user_display_name,
        is_admin,
        is_authenticated,
        unread_count,
        last_admin_action,
        current_user_id: user.id,
        users,
        i18n,
    };
    render(&tpl)
}

pub async fn update_role(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
    Locale(i18n): Locale,
    ValidatedForm(input): ValidatedForm<RoleInput>,
) -> Result<Response, AppError> {
    let new_role = parse_role(&input.role)?;

    // Cannot change own role
    if user_id == admin.id {
        return Err(AppError::Validation(
            i18n.t("validation.cannot_change_own_role").to_string(),
        ));
    }

    // Verify target user exists
    let target = UserRepository::find_by_id(&state.pool, user_id)
        .await?
        .ok_or(AppError::NotFound)?;

    // If demoting an admin, ensure at least one admin remains
    if target.role == Role::Admin && new_role != Role::Admin {
        let admin_count = UserRepository::count_admins(&state.pool).await?;
        if admin_count <= 1 {
            return Err(AppError::Validation(
                i18n.t("validation.last_admin").to_string(),
            ));
        }
    }

    UserRepository::update_role(&state.pool, user_id, new_role).await?;

    tracing::info!(
        admin_id = admin.id,
        target_user_id = user_id,
        new_role = input.role,
        "Role updated"
    );

    Ok(Redirect::to("/admin/users").into_response())
}

pub async fn toggle_active(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    // Cannot disable yourself
    if user_id == admin.id {
        return Err(AppError::Validation(
            i18n.t("validation.cannot_disable_self").to_string(),
        ));
    }

    let target = UserRepository::find_by_id(&state.pool, user_id)
        .await?
        .ok_or(AppError::NotFound)?;

    // If disabling an admin, ensure at least one active admin remains
    if target.is_active && target.role == Role::Admin {
        let admin_count = UserRepository::count_admins(&state.pool).await?;
        if admin_count <= 1 {
            return Err(AppError::Validation(
                i18n.t("validation.last_active_admin").to_string(),
            ));
        }
    }

    let new_active = !target.is_active;
    UserRepository::set_active(&state.pool, user_id, new_active).await?;

    let action = if new_active { "enabled" } else { "disabled" };
    tracing::info!(
        admin_id = admin.id,
        target_user_id = user_id,
        action,
        "User active status changed"
    );

    Ok(Redirect::to("/admin/users").into_response())
}

pub async fn toggle_public_mode(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Locale(_i18n): Locale,
) -> Result<Response, AppError> {
    let new_value = state.toggle_public_mode();
    let value_str = if new_value { "true" } else { "false" };
    SettingsRepository::set(&state.pool, "public_mode", value_str).await?;

    tracing::info!(
        admin_id = admin.id,
        public_mode = new_value,
        "Public mode toggled"
    );

    Ok(Redirect::to("/admin/settings").into_response())
}
