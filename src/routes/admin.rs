//! Admin routes - user management.

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use rand::Rng;
use rand::distributions::Alphanumeric;
use serde::Deserialize;
use validator::Validate;

use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::{CsrfToken, RequireAdmin, ValidatedForm};
use crate::models::{Role, User};
use crate::repositories::{EventRepository, IconRepository, SettingsRepository, UserRepository};
use crate::services::{AuthService, EventService};
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
    /// The value shown in the field: the saved name, or the refused one.
    instance_name: String,
    /// What the previous action changed, said at the top of the page.
    notice: Option<SettingsNotice>,
    /// A refused name, said in the page rather than on the bare error page.
    error: Option<String>,
    users_count: i64,
    admins_count: i64,
    icons_count: i64,
    i18n: I18n,
}

enum SettingsNotice {
    Renamed(String),
    NameReset,
    PublicOpened,
    PublicClosed,
}

#[derive(Deserialize, Default)]
pub struct SettingsQuery {
    renamed: Option<String>,
    public: Option<String>,
}

impl SettingsQuery {
    /// The name is read back from memory, not from the address bar, so the
    /// receipt cannot be made to say anything the instance is not called.
    fn notice(&self) -> Option<SettingsNotice> {
        if self.renamed.is_some() {
            let name = crate::instance_name();
            return Some(if name.is_empty() {
                SettingsNotice::NameReset
            } else {
                SettingsNotice::Renamed(name)
            });
        }
        match self.public.as_deref() {
            Some("on") => Some(SettingsNotice::PublicOpened),
            Some("off") => Some(SettingsNotice::PublicClosed),
            _ => None,
        }
    }
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
    /// The member the previous action touched, and what happened to them.
    notice: Option<Notice>,
    /// A newly created account, with the temporary password shown this once.
    created: Option<CreatedAccount>,
    /// A refused action, said in the page rather than on the bare error page.
    error: Option<String>,
    add_form: AddMemberForm,
    /// The add-member fold opens itself when its form came back refused.
    add_open: bool,
    i18n: I18n,
}

struct UserRow {
    id: i64,
    email: String,
    display_name: String,
    role: Role,
    is_active: bool,
    last_seen_at: Option<String>,
}

struct Notice {
    name: String,
    kind: NoticeKind,
}

enum NoticeKind {
    RoleUpdated(Role),
    Disabled,
    Enabled,
}

struct CreatedAccount {
    name: String,
    email: String,
    password: String,
}

#[derive(Default)]
struct AddMemberForm {
    display_name: String,
    email: String,
    role: String,
}

#[derive(Deserialize)]
pub struct UsersQuery {
    role: Option<i64>,
    toggled: Option<i64>,
}

#[derive(Deserialize, Validate)]
pub struct AddMemberInput {
    #[validate(length(min = 1, max = 100, message = "validation.display_name_required"))]
    display_name: String,
    #[validate(email(message = "validation.email_invalid"))]
    email: String,
    #[validate(length(min = 1, max = 20, message = "validation.invalid_role"))]
    role: String,
}

/// Long enough to resist guessing, short enough to read out over a call.
const TEMP_PASSWORD_LENGTH: usize = 16;

fn temporary_password() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(TEMP_PASSWORD_LENGTH)
        .map(char::from)
        .collect()
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
    }
}

pub async fn settings_page(
    RequireAdmin(user): RequireAdmin,
    State(state): State<AppState>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
    Query(query): Query<SettingsQuery>,
) -> Result<Response, AppError> {
    let notice = query.notice();
    render_settings(
        &state,
        &user,
        csrf_token.0,
        i18n,
        crate::instance_name(),
        notice,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn render_settings(
    state: &AppState,
    user: &User,
    csrf_token: String,
    i18n: I18n,
    instance_name: String,
    notice: Option<SettingsNotice>,
    error: Option<String>,
) -> Result<Response, AppError> {
    let (user_display_name, is_admin, is_authenticated) = layout_fields(user);
    let unread_count = unread(&state.pool, user).await?;
    let users_count = UserRepository::count_all(&state.pool).await?;
    let admins_count = UserRepository::count_admins(&state.pool).await?;
    let icons_count = IconRepository::count(&state.pool).await?;
    let last_admin_action = EventRepository::last_admin_action(&state.pool)
        .await?
        .map(|dt| i18n.format_datetime_long(&dt));

    let tpl = SettingsPageTemplate {
        csrf_token,
        user_display_name,
        is_admin,
        is_authenticated,
        unread_count,
        last_admin_action,
        public_mode: state.is_public_mode(),
        instance_name,
        notice,
        error,
        users_count,
        admins_count,
        icons_count,
        i18n,
    };
    render(&tpl)
}

pub async fn users_list(
    RequireAdmin(user): RequireAdmin,
    State(state): State<AppState>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
    Query(query): Query<UsersQuery>,
) -> Result<Response, AppError> {
    let users = load_rows(&state, &i18n).await?;
    let notice = query
        .role
        .and_then(|id| users.iter().find(|u| u.id == id))
        .map(|u| Notice {
            name: u.display_name.clone(),
            kind: NoticeKind::RoleUpdated(u.role),
        })
        .or_else(|| {
            query
                .toggled
                .and_then(|id| users.iter().find(|u| u.id == id))
                .map(|u| Notice {
                    name: u.display_name.clone(),
                    kind: if u.is_active {
                        NoticeKind::Enabled
                    } else {
                        NoticeKind::Disabled
                    },
                })
        });
    let page = ListPage {
        notice,
        created: None,
        error: None,
        add_form: AddMemberForm::default(),
        add_open: false,
    };
    render_users(&state, &user, csrf_token.0, i18n, users, page).await
}

async fn load_rows(state: &AppState, i18n: &I18n) -> Result<Vec<UserRow>, AppError> {
    Ok(UserRepository::list_all(&state.pool)
        .await?
        .into_iter()
        .map(|u| to_user_row(u, i18n))
        .collect())
}

/// What the list says on top of the rows themselves.
struct ListPage {
    notice: Option<Notice>,
    created: Option<CreatedAccount>,
    error: Option<String>,
    add_form: AddMemberForm,
    add_open: bool,
}

impl ListPage {
    fn refused(error: String) -> Self {
        Self {
            notice: None,
            created: None,
            error: Some(error),
            add_form: AddMemberForm::default(),
            add_open: false,
        }
    }
}

async fn render_users(
    state: &AppState,
    user: &User,
    csrf_token: String,
    i18n: I18n,
    users: Vec<UserRow>,
    page: ListPage,
) -> Result<Response, AppError> {
    let (user_display_name, is_admin, is_authenticated) = layout_fields(user);
    let unread_count = unread(&state.pool, user).await?;
    let last_admin_action = EventRepository::last_admin_action(&state.pool)
        .await?
        .map(|dt| i18n.format_datetime_long(&dt));
    let tpl = UsersListTemplate {
        csrf_token,
        user_display_name,
        is_admin,
        is_authenticated,
        unread_count,
        last_admin_action,
        current_user_id: user.id,
        users,
        notice: page.notice,
        created: page.created,
        error: page.error,
        add_form: page.add_form,
        add_open: page.add_open,
        i18n,
    };
    render(&tpl)
}

/// The admin creates the account and reads a temporary password off the
/// page, once: there is no outgoing mail to carry an invitation.
pub async fn add_member(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
    ValidatedForm(input): ValidatedForm<AddMemberInput>,
) -> Result<Response, AppError> {
    let created = async {
        let role = parse_role(&input.role)?;
        let password = temporary_password();
        let user = AuthService::register(
            &state.pool,
            &input.email,
            &password,
            &input.display_name,
            role,
        )
        .await?;
        Ok::<_, AppError>((user, password))
    }
    .await;

    let page = match created {
        Ok((user, password)) => {
            tracing::info!(admin_id = admin.id, new_user_id = user.id, "Member added");
            ListPage {
                notice: None,
                created: Some(CreatedAccount {
                    name: user.display_name,
                    email: user.email,
                    password,
                }),
                error: None,
                add_form: AddMemberForm::default(),
                add_open: false,
            }
        }
        Err(AppError::Validation(msg)) => ListPage {
            notice: None,
            created: None,
            error: Some(i18n.t(&msg).to_string()),
            add_form: AddMemberForm {
                display_name: input.display_name,
                email: input.email,
                role: input.role,
            },
            add_open: true,
        },
        Err(e) => return Err(e),
    };
    let users = load_rows(&state, &i18n).await?;
    render_users(&state, &admin, csrf_token.0, i18n, users, page).await
}

pub async fn update_role(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
    ValidatedForm(input): ValidatedForm<RoleInput>,
) -> Result<Response, AppError> {
    match apply_role(&state, &admin, user_id, &input.role, &i18n).await {
        Ok(()) => Ok(Redirect::to(&format!("/admin/users?role={user_id}")).into_response()),
        Err(AppError::Validation(msg)) => {
            let users = load_rows(&state, &i18n).await?;
            render_users(
                &state,
                &admin,
                csrf_token.0,
                i18n,
                users,
                ListPage::refused(msg),
            )
            .await
        }
        Err(e) => Err(e),
    }
}

async fn apply_role(
    state: &AppState,
    admin: &User,
    user_id: i64,
    role: &str,
    i18n: &I18n,
) -> Result<(), AppError> {
    let new_role = parse_role(role)?;

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
        new_role = role,
        "Role updated"
    );
    Ok(())
}

pub async fn toggle_active(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    match apply_toggle(&state, &admin, user_id, &i18n).await {
        Ok(()) => Ok(Redirect::to(&format!("/admin/users?toggled={user_id}")).into_response()),
        Err(AppError::Validation(msg)) => {
            let users = load_rows(&state, &i18n).await?;
            render_users(
                &state,
                &admin,
                csrf_token.0,
                i18n,
                users,
                ListPage::refused(msg),
            )
            .await
        }
        Err(e) => Err(e),
    }
}

async fn apply_toggle(
    state: &AppState,
    admin: &User,
    user_id: i64,
    i18n: &I18n,
) -> Result<(), AppError> {
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
    Ok(())
}

#[derive(Deserialize)]
pub struct InstanceNameInput {
    #[serde(default)]
    instance_name: String,
}

/// Long enough for a company and a purpose, short enough to stay on one line
/// of the masthead beside the mark.
const INSTANCE_NAME_MAX_CHARS: usize = 40;

pub async fn update_instance_name(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
    axum::extract::Form(input): axum::extract::Form<InstanceNameInput>,
) -> Result<Response, AppError> {
    let name = input.instance_name.trim();
    if name.chars().count() > INSTANCE_NAME_MAX_CHARS {
        let error = i18n.t("validation.instance_name_too_long").to_string();
        return render_settings(
            &state,
            &admin,
            csrf_token.0,
            i18n,
            name.to_string(),
            None,
            Some(error),
        )
        .await;
    }

    SettingsRepository::set(&state.pool, "instance_name", name).await?;
    crate::set_instance_name(name);

    tracing::info!(
        admin_id = admin.id,
        instance_name = name,
        "Instance name updated"
    );

    Ok(Redirect::to("/admin/settings?renamed=1").into_response())
}

#[derive(Deserialize)]
pub struct AccessInput {
    #[serde(default)]
    access: String,
}

/// Who can read the page: the two choices are the state and the action at
/// once, so the handler sets rather than toggles.
pub async fn set_public_access(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    axum::extract::Form(input): axum::extract::Form<AccessInput>,
) -> Result<Response, AppError> {
    let public = match input.access.as_str() {
        "everyone" => true,
        "members" => false,
        _ => {
            return Err(AppError::Validation(
                "validation.unknown_access".to_string(),
            ));
        }
    };
    state.set_public_mode(public);
    let value_str = if public { "true" } else { "false" };
    SettingsRepository::set(&state.pool, "public_mode", value_str).await?;

    tracing::info!(
        admin_id = admin.id,
        public_mode = public,
        "Public access updated"
    );

    let receipt = if public { "on" } else { "off" };
    Ok(Redirect::to(&format!("/admin/settings?public={receipt}")).into_response())
}
