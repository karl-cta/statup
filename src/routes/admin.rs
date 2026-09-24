//! Admin routes: instance settings and team management.

use askama::Template;
use axum::extract::{Multipart, Path, Query, State};
use axum::response::{IntoResponse, Redirect, Response};
use serde::{Deserialize, Serialize};
use tower_sessions::Session;

use super::{Frame, render};
use crate::clock;
use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::headers::no_store;
use crate::middleware::{CsrfToken, HtmlForm, RequireAdmin};
use crate::models::{Role, User, check_display_name};
use crate::repositories::{IconRepository, SettingsRepository, UserRepository};
use crate::services::{AuthService, LogoService};
use crate::session::{read_value, write_value};
use crate::state::AppState;

/// Session key of the account just created, shown once on the team page.
const CREATED_ACCOUNT_KEY: &str = "created_account";

/// Long enough for a company and a purpose, short enough to stay on one line
/// of the masthead beside the mark.
const INSTANCE_NAME_MAX_CHARS: usize = 40;

#[derive(Template)]
#[template(path = "admin/settings.html")]
struct SettingsPageTemplate {
    frame: Frame,
    public_mode: bool,
    /// The value shown in the field: the saved name, or the refused one.
    instance_name: String,
    /// What the previous action changed, said at the top of the page.
    notice: Option<SettingsNotice>,
    /// A refused name, said in the page rather than on the bare error page.
    error: Option<String>,
    /// The instance zone, and the ones it may switch to.
    time_zone: &'static str,
    zones: Vec<&'static str>,
    users_count: i64,
    admins_count: i64,
    icons_count: i64,
    i18n: I18n,
}

enum SettingsNotice {
    Renamed(String),
    NameReset,
    ZoneSet(&'static str),
    LogoSet,
    LogoRemoved,
    PublicOpened,
    PublicClosed,
}

impl SettingsPageTemplate {
    /// "4 members, 1 administrator".
    fn team_count(&self) -> String {
        let count = |n: i64| usize::try_from(n).unwrap_or(0);
        format!(
            "{}, {}",
            self.i18n.plural("admin.members", count(self.users_count)),
            self.i18n.plural("admin.admins", count(self.admins_count))
        )
    }

    fn is_current_zone(&self, zone: &str) -> bool {
        zone == self.time_zone
    }

    /// `America/New_York` as people read it.
    fn zone_label(zone: &str) -> String {
        zone.replace('_', " ")
    }

    fn zone_notice(&self, zone: &str) -> String {
        self.i18n
            .tf("admin.zone_notice", &[("zone", &Self::zone_label(zone))])
    }

    fn icons_label(&self) -> String {
        let count = usize::try_from(self.icons_count).unwrap_or(0);
        self.i18n.plural("admin.icons", count)
    }
}

/// What the settings page says on top of the saved values.
struct SettingsPage {
    instance_name: String,
    notice: Option<SettingsNotice>,
    error: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct SettingsQuery {
    renamed: Option<String>,
    public: Option<String>,
    zone: Option<String>,
    logo: Option<String>,
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
        if self.zone.is_some() {
            return Some(SettingsNotice::ZoneSet(clock::zone().name()));
        }
        match self.logo.as_deref() {
            Some("set") => return Some(SettingsNotice::LogoSet),
            Some("removed") => return Some(SettingsNotice::LogoRemoved),
            _ => {}
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
    frame: Frame,
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

/// Kept in the session between the creation and the page that shows it.
#[derive(Serialize, Deserialize)]
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

impl UsersQuery {
    fn notice(&self, users: &[UserRow]) -> Option<Notice> {
        let find = |id: Option<i64>| id.and_then(|id| users.iter().find(|u| u.id == id));
        if let Some(user) = find(self.role) {
            return Some(Notice {
                name: user.display_name.clone(),
                kind: NoticeKind::RoleUpdated(user.role),
            });
        }
        find(self.toggled).map(|user| Notice {
            name: user.display_name.clone(),
            kind: if user.is_active {
                NoticeKind::Enabled
            } else {
                NoticeKind::Disabled
            },
        })
    }
}

#[derive(Deserialize)]
pub struct AddMemberInput {
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    role: String,
}

#[derive(Deserialize)]
pub struct RoleInput {
    #[serde(default)]
    role: String,
}

fn validation(key: &str) -> AppError {
    AppError::Validation(key.to_string())
}

fn to_user_row(u: User, i18n: &I18n) -> UserRow {
    UserRow {
        id: u.id,
        email: u.email,
        display_name: u.display_name,
        role: u.role,
        is_active: u.is_active,
        last_seen_at: u.last_seen_at.map(|dt| i18n.format_datetime(&dt)),
    }
}

pub async fn settings_page(
    RequireAdmin(user): RequireAdmin,
    State(state): State<AppState>,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
    Query(query): Query<SettingsQuery>,
) -> Result<Response, AppError> {
    let page = SettingsPage {
        instance_name: crate::instance_name(),
        notice: query.notice(),
        error: None,
    };
    render_settings(&state, &user, csrf_token, i18n, page).await
}

async fn render_settings(
    state: &AppState,
    user: &User,
    csrf_token: String,
    i18n: I18n,
    page: SettingsPage,
) -> Result<Response, AppError> {
    let frame = Frame::load(&state.pool, Some(user), csrf_token, &i18n).await?;
    render(&SettingsPageTemplate {
        frame,
        public_mode: state.is_public_mode(),
        instance_name: page.instance_name,
        notice: page.notice,
        error: page.error,
        time_zone: clock::zone().name(),
        zones: clock::zone_names(),
        users_count: UserRepository::count_all(&state.pool).await?,
        admins_count: UserRepository::count_admins(&state.pool).await?,
        icons_count: IconRepository::count(&state.pool).await?,
        i18n,
    })
}

pub async fn users_list(
    RequireAdmin(user): RequireAdmin,
    State(state): State<AppState>,
    session: Session,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
    Query(query): Query<UsersQuery>,
) -> Result<Response, AppError> {
    let users = load_rows(&state, &i18n).await?;
    let created = take_created_account(&session).await?;
    let shows_password = created.is_some();
    let page = ListPage {
        notice: query.notice(&users),
        created,
        ..ListPage::default()
    };
    let response = render_users(&state, &user, csrf_token, i18n, users, page).await?;
    Ok(if shows_password {
        no_store(response)
    } else {
        response
    })
}

/// The account created by the previous request, removed from the session
/// so the password is shown once. Reading alone does not write the session.
async fn take_created_account(session: &Session) -> Result<Option<CreatedAccount>, AppError> {
    let created: Option<CreatedAccount> = read_value(session, CREATED_ACCOUNT_KEY).await?;
    if created.is_some() {
        session
            .remove_value(CREATED_ACCOUNT_KEY)
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!("session write failed: {e}")))?;
    }
    Ok(created)
}

async fn load_rows(state: &AppState, i18n: &I18n) -> Result<Vec<UserRow>, AppError> {
    Ok(UserRepository::list_all(&state.pool)
        .await?
        .into_iter()
        .map(|u| to_user_row(u, i18n))
        .collect())
}

/// What the list says on top of the rows themselves.
#[derive(Default)]
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
            error: Some(error),
            ..Self::default()
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
    let frame = Frame::load(&state.pool, Some(user), csrf_token, &i18n).await?;
    render(&UsersListTemplate {
        frame,
        current_user_id: user.id,
        users,
        notice: page.notice,
        created: page.created,
        error: page.error,
        add_form: page.add_form,
        add_open: page.add_open,
        i18n,
    })
}

/// The admin creates the account and reads a temporary password off the
/// page, once: there is no outgoing mail to carry an invitation. The
/// password travels to the next page in the session, so a reload of that
/// page does not post the form again.
pub async fn add_member(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    session: Session,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
    HtmlForm(input): HtmlForm<AddMemberInput>,
) -> Result<Response, AppError> {
    match create_member(&state, &input).await {
        Ok(created) => {
            tracing::info!(admin_id = admin.id, "Member added");
            write_value(&session, CREATED_ACCOUNT_KEY, &created).await?;
            Ok(Redirect::to("/admin/users").into_response())
        }
        Err(AppError::Validation(key)) => {
            let page = ListPage {
                error: Some(i18n.t(&key).to_string()),
                add_form: AddMemberForm {
                    display_name: input.display_name,
                    email: input.email,
                    role: input.role,
                },
                add_open: true,
                ..ListPage::default()
            };
            let users = load_rows(&state, &i18n).await?;
            render_users(&state, &admin, csrf_token, i18n, users, page).await
        }
        Err(e) => Err(e),
    }
}

async fn create_member(
    state: &AppState,
    input: &AddMemberInput,
) -> Result<CreatedAccount, AppError> {
    let name = check_display_name(&input.display_name).map_err(validation)?;
    let role: Role = input
        .role
        .parse()
        .map_err(|_| validation("validation.invalid_role"))?;
    let (user, password) = AuthService::add_member(&state.pool, &input.email, &name, role).await?;
    tracing::info!(new_user_id = user.id, "Temporary password issued");
    Ok(CreatedAccount {
        name: user.display_name,
        email: user.email,
        password,
    })
}

pub async fn update_role(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
    HtmlForm(input): HtmlForm<RoleInput>,
) -> Result<Response, AppError> {
    match apply_role(&state, &admin, user_id, &input.role).await {
        Ok(()) => Ok(Redirect::to(&format!("/admin/users?role={user_id}")).into_response()),
        Err(AppError::Validation(key)) => {
            let users = load_rows(&state, &i18n).await?;
            let page = ListPage::refused(i18n.t(&key).to_string());
            render_users(&state, &admin, csrf_token, i18n, users, page).await
        }
        Err(e) => Err(e),
    }
}

async fn apply_role(
    state: &AppState,
    admin: &User,
    user_id: i64,
    role: &str,
) -> Result<(), AppError> {
    let new_role: Role = role
        .parse()
        .map_err(|_| validation("validation.invalid_role"))?;
    if user_id == admin.id {
        return Err(validation("validation.cannot_change_own_role"));
    }
    UserRepository::find_by_id(&state.pool, user_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if !UserRepository::update_role(&state.pool, user_id, new_role).await? {
        return Err(validation("validation.last_admin"));
    }

    tracing::info!(
        admin_id = admin.id,
        target_user_id = user_id,
        new_role = new_role.as_str(),
        "Role updated"
    );
    Ok(())
}

pub async fn toggle_active(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    match apply_toggle(&state, &admin, user_id).await {
        Ok(()) => Ok(Redirect::to(&format!("/admin/users?toggled={user_id}")).into_response()),
        Err(AppError::Validation(key)) => {
            let users = load_rows(&state, &i18n).await?;
            let page = ListPage::refused(i18n.t(&key).to_string());
            render_users(&state, &admin, csrf_token, i18n, users, page).await
        }
        Err(e) => Err(e),
    }
}

async fn apply_toggle(state: &AppState, admin: &User, user_id: i64) -> Result<(), AppError> {
    if user_id == admin.id {
        return Err(validation("validation.cannot_disable_self"));
    }
    let target = UserRepository::find_by_id(&state.pool, user_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let new_active = !target.is_active;
    if !UserRepository::set_active(&state.pool, user_id, new_active).await? {
        return Err(validation("validation.last_active_admin"));
    }

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

/// The message key refusing a name, if any.
fn instance_name_refusal(name: &str) -> Option<&'static str> {
    if name.chars().count() > INSTANCE_NAME_MAX_CHARS {
        Some("validation.instance_name_too_long")
    } else if name.chars().any(char::is_control) {
        Some("validation.display_name_invalid")
    } else {
        None
    }
}

pub async fn update_instance_name(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
    HtmlForm(input): HtmlForm<InstanceNameInput>,
) -> Result<Response, AppError> {
    let name = input.instance_name.trim();
    if let Some(key) = instance_name_refusal(name) {
        let page = SettingsPage {
            instance_name: name.to_string(),
            notice: None,
            error: Some(i18n.t(key).to_string()),
        };
        return render_settings(&state, &admin, csrf_token, i18n, page).await;
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

/// A logo refused for its file is said on the settings page.
pub async fn update_logo(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
    mut multipart: Multipart,
) -> Result<Response, AppError> {
    let stored = match super::icons::extract_upload(&mut multipart).await {
        Ok((_, data)) => LogoService::replace(&state.pool, &state.upload_dir, data).await,
        Err(e) => Err(e),
    };
    match stored {
        Ok(()) => {
            tracing::info!(admin_id = admin.id, "Logo updated");
            Ok(Redirect::to("/admin/settings?logo=set").into_response())
        }
        Err(AppError::Validation(key)) => {
            let page = SettingsPage {
                instance_name: crate::instance_name(),
                notice: None,
                error: Some(i18n.t(&key).to_string()),
            };
            render_settings(&state, &admin, csrf_token, i18n, page).await
        }
        Err(e) => Err(e),
    }
}

pub async fn remove_logo(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
) -> Result<Response, AppError> {
    LogoService::remove(&state.pool, &state.upload_dir).await?;
    tracing::info!(admin_id = admin.id, "Logo removed");
    Ok(Redirect::to("/admin/settings?logo=removed").into_response())
}

#[derive(Deserialize)]
pub struct TimeZoneInput {
    #[serde(default)]
    time_zone: String,
}

pub async fn update_time_zone(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    HtmlForm(input): HtmlForm<TimeZoneInput>,
) -> Result<Response, AppError> {
    let zone =
        clock::parse_zone(&input.time_zone).ok_or_else(|| validation("validation.unknown_zone"))?;
    SettingsRepository::set(&state.pool, clock::ZONE_SETTING, zone.name()).await?;
    clock::set_zone(zone);
    tracing::info!(
        admin_id = admin.id,
        time_zone = zone.name(),
        "Time zone updated"
    );
    Ok(Redirect::to("/admin/settings?zone=1").into_response())
}

#[derive(Deserialize)]
pub struct AccessInput {
    #[serde(default)]
    access: String,
}

/// Who can read the page: the two choices are the state and the action at
/// once, so the handler sets rather than toggles. The database is written
/// first: memory never holds a choice a restart would lose.
pub async fn set_public_access(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    HtmlForm(input): HtmlForm<AccessInput>,
) -> Result<Response, AppError> {
    let public = match input.access.as_str() {
        "everyone" => true,
        "members" => false,
        _ => return Err(validation("validation.unknown_access")),
    };
    let stored = if public { "true" } else { "false" };
    SettingsRepository::set(&state.pool, "public_mode", stored).await?;
    state.set_public_mode(public);

    tracing::info!(
        admin_id = admin.id,
        public_mode = public,
        "Public access updated"
    );

    let receipt = if public { "on" } else { "off" };
    Ok(Redirect::to(&format!("/admin/settings?public={receipt}")).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_names_are_bounded_and_printable() {
        assert_eq!(instance_name_refusal("Acme Status"), None);
        assert_eq!(instance_name_refusal(&"é".repeat(40)), None);
        assert_eq!(
            instance_name_refusal(&"a".repeat(41)),
            Some("validation.instance_name_too_long")
        );
        assert_eq!(
            instance_name_refusal("Acme\u{7}"),
            Some("validation.display_name_invalid")
        );
    }
}
