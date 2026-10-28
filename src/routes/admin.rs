//! Admin routes: instance settings and team management.

use askama::Template;
use axum::extract::{Multipart, Path, Query, State};
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;

use super::setup::{GeneralInput, apply_general, read_general, record_settings_saved};
use super::{Frame, render};
use crate::clock;
use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::headers::no_store;
use crate::middleware::{CsrfToken, HtmlForm, RequireAdmin};
use crate::models::{Accent, Ambiance, JournalKind, Role, User, check_display_name};
use crate::repositories::{
    ApiKeyRepository, GroupRepository, NewJournalEntry, NotificationRepository, UserRepository,
};
use crate::services::{AuthService, CURRENT_VERSION, LogoService, NewerRelease, RELEASES_PAGE};
use crate::state::AppState;

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
    /// The zone shown as chosen, and the ones it may switch to.
    time_zone: String,
    zones: Vec<&'static str>,
    users_count: i64,
    admins_count: i64,
    notifications_count: i64,
    api_keys_count: i64,
    /// What the journal captures and for how long.
    journal_label: String,
    /// A newer published version, once the daily check finds one.
    newer: Option<NewerRelease>,
    i18n: I18n,
}

enum SettingsNotice {
    Saved,
    LogoRemoved,
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

    fn newer_notice(&self, release: &NewerRelease) -> String {
        self.i18n
            .tf("admin.version_newer", &[("version", &release.version)])
    }

    fn notifications_label(&self) -> String {
        let count = usize::try_from(self.notifications_count).unwrap_or(0);
        self.i18n.plural("notifications.destinations", count)
    }

    fn api_keys_label(&self) -> String {
        let count = usize::try_from(self.api_keys_count).unwrap_or(0);
        self.i18n.plural("api_keys.count", count)
    }
}

/// What the general card shows: the saved values, or the ones a refused
/// save sent, with what to say on top.
struct SettingsPage {
    instance_name: String,
    time_zone: String,
    public_mode: bool,
    notice: Option<SettingsNotice>,
    error: Option<String>,
}

impl SettingsPage {
    fn saved(state: &AppState) -> Self {
        Self {
            instance_name: crate::instance_name(),
            time_zone: clock::zone().name().to_string(),
            public_mode: state.is_public_mode(),
            notice: None,
            error: None,
        }
    }

    fn typed(state: &AppState, input: &GeneralInput) -> Self {
        let saved = Self::saved(state);
        Self {
            instance_name: input.instance_name.trim().to_string(),
            time_zone: input.time_zone.clone().unwrap_or(saved.time_zone),
            public_mode: match input.access.as_str() {
                "everyone" => true,
                "members" => false,
                _ => saved.public_mode,
            },
            ..saved
        }
    }

    fn refused(self, i18n: &I18n, key: &str) -> Self {
        Self {
            error: Some(i18n.t(key).to_string()),
            ..self
        }
    }
}

#[derive(Deserialize, Default)]
pub struct SettingsQuery {
    saved: Option<String>,
    logo: Option<String>,
}

impl SettingsQuery {
    fn notice(&self) -> Option<SettingsNotice> {
        if self.saved.is_some() {
            return Some(SettingsNotice::Saved);
        }
        (self.logo.as_deref() == Some("removed")).then_some(SettingsNotice::LogoRemoved)
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
    groups: Vec<GroupRow>,
    new_group: NewGroupForm,
    i18n: I18n,
}

/// A group on the Team page: a link to its own page.
struct GroupRow {
    id: i64,
    name: String,
    members: String,
}

/// The "New group" panel, open again with the refused name and why.
#[derive(Default)]
struct NewGroupForm {
    value: String,
    error: Option<String>,
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
    GroupDeleted,
}

/// A new account, or a reset, with the temporary password to hand over.
struct CreatedAccount {
    name: String,
    email: String,
    password: String,
    reset: bool,
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
    group_deleted: Option<u8>,
}

impl UsersQuery {
    fn notice(&self, users: &[UserRow]) -> Option<Notice> {
        if self.group_deleted.is_some() {
            return Some(Notice {
                name: String::new(),
                kind: NoticeKind::GroupDeleted,
            });
        }
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
        notice: query.notice(),
        ..SettingsPage::saved(&state)
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
        public_mode: page.public_mode,
        instance_name: page.instance_name,
        notice: page.notice,
        error: page.error,
        time_zone: page.time_zone,
        zones: clock::zone_names(),
        users_count: UserRepository::count_all(&state.pool).await?,
        admins_count: UserRepository::count_admins(&state.pool).await?,
        notifications_count: NotificationRepository::count_channels(&state.pool).await?,
        api_keys_count: ApiKeyRepository::count_in_use(&state.pool).await?,
        journal_label: super::journal::tile_label(state, &i18n).await,
        newer: state.update.newer(),
        i18n,
    })
}

pub async fn users_list(
    RequireAdmin(user): RequireAdmin,
    State(state): State<AppState>,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
    Query(query): Query<UsersQuery>,
) -> Result<Response, AppError> {
    let users = load_rows(&state, &i18n).await?;
    let page = ListPage {
        notice: query.notice(&users),
        ..ListPage::default()
    };
    render_users(&state, &user, csrf_token, i18n, users, page).await
}

/// The temporary password is shown in the answer to the form, and kept
/// nowhere, not even in the session. The page then takes the address of the
/// list, so a reload does not send the form again.
async fn show_password(
    state: &AppState,
    admin: &User,
    csrf_token: String,
    i18n: I18n,
    created: CreatedAccount,
) -> Result<Response, AppError> {
    let users = load_rows(state, &i18n).await?;
    let page = ListPage {
        created: Some(created),
        ..ListPage::default()
    };
    let response = render_users(state, admin, csrf_token, i18n, users, page).await?;
    Ok(no_store(response))
}

async fn load_groups(state: &AppState, i18n: &I18n) -> Result<Vec<GroupRow>, AppError> {
    Ok(GroupRepository::list(&state.pool)
        .await?
        .into_iter()
        .map(|group| GroupRow {
            members: i18n.plural(
                "admin.group_members",
                usize::try_from(group.member_count).unwrap_or(0),
            ),
            id: group.id,
            name: group.name,
        })
        .collect())
}

/// The Team page again, with the "New group" panel open on the refused
/// name and why.
pub(super) async fn refused_page(
    state: &AppState,
    admin: &User,
    csrf_token: String,
    i18n: I18n,
    key: &str,
    name: String,
) -> Result<Response, AppError> {
    let users = load_rows(state, &i18n).await?;
    let page = ListPage {
        new_group: NewGroupForm {
            error: Some(i18n.t(key).to_string()),
            value: name,
        },
        ..ListPage::default()
    };
    render_users(state, admin, csrf_token, i18n, users, page).await
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
    new_group: NewGroupForm,
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
        groups: load_groups(state, &i18n).await?,
        new_group: page.new_group,
        i18n,
    })
}

/// The admin creates the account and reads a temporary password off the
/// page, once: there is no outgoing mail to carry an invitation.
pub async fn add_member(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
    HtmlForm(input): HtmlForm<AddMemberInput>,
) -> Result<Response, AppError> {
    match create_member(&state, &input).await {
        Ok(created) => {
            NewJournalEntry::new(JournalKind::MemberAdded)
                .by(&admin)
                .about(&created.name)
                .with_detail(input.role.as_str())
                .record(&state.pool)
                .await;
            show_password(&state, &admin, csrf_token, i18n, created).await
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
    let name = check_display_name(&input.display_name).map_err(AppError::validation)?;
    let role: Role = input
        .role
        .parse()
        .map_err(|_| AppError::validation("validation.invalid_role"))?;
    let (user, password) = AuthService::add_member(&state.pool, &input.email, &name, role).await?;
    Ok(CreatedAccount {
        name: user.display_name,
        email: user.email,
        password,
        reset: false,
    })
}

pub async fn reset_member_password(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    match issue_reset(&state, &admin, user_id).await {
        Ok(reset) => show_password(&state, &admin, csrf_token, i18n, reset).await,
        Err(AppError::Validation(key)) => {
            let users = load_rows(&state, &i18n).await?;
            let page = ListPage::refused(i18n.t(&key).to_string());
            render_users(&state, &admin, csrf_token, i18n, users, page).await
        }
        Err(e) => Err(e),
    }
}

/// An admin changes their own password from their profile, not here.
async fn issue_reset(
    state: &AppState,
    admin: &User,
    user_id: i64,
) -> Result<CreatedAccount, AppError> {
    if user_id == admin.id {
        return Err(AppError::validation("validation.cannot_reset_self"));
    }
    let (user, password) = AuthService::reset_member_password(&state.pool, user_id).await?;
    NewJournalEntry::new(JournalKind::MemberPasswordReset)
        .by(admin)
        .about(&user.display_name)
        .record(&state.pool)
        .await;
    Ok(CreatedAccount {
        name: user.display_name,
        email: user.email,
        password,
        reset: true,
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
        .map_err(|_| AppError::validation("validation.invalid_role"))?;
    if user_id == admin.id {
        return Err(AppError::validation("validation.cannot_change_own_role"));
    }
    let target = UserRepository::find_by_id(&state.pool, user_id)
        .await?
        .ok_or(AppError::NotFound)?;
    // The panel sends the role with every save, changed or not.
    if target.role == new_role {
        return Ok(());
    }
    if !UserRepository::update_role(&state.pool, user_id, new_role).await? {
        return Err(AppError::validation("validation.last_admin"));
    }

    NewJournalEntry::new(JournalKind::MemberRoleChanged)
        .by(admin)
        .about(&target.display_name)
        .with_detail(new_role.as_str())
        .record(&state.pool)
        .await;
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
        return Err(AppError::validation("validation.cannot_disable_self"));
    }
    let target = UserRepository::find_by_id(&state.pool, user_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let new_active = !target.is_active;
    if !UserRepository::set_active(&state.pool, user_id, new_active).await? {
        return Err(AppError::validation("validation.last_active_admin"));
    }

    let kind = if new_active {
        JournalKind::MemberEnabled
    } else {
        JournalKind::MemberDisabled
    };
    NewJournalEntry::new(kind)
        .by(admin)
        .about(&target.display_name)
        .record(&state.pool)
        .await;
    Ok(())
}

/// The general card is saved at once: name, logo, zone and audience. A
/// refusal shows the page again with what was chosen.
pub async fn save_settings(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
    mut multipart: Multipart,
) -> Result<Response, AppError> {
    let input = match read_general(&mut multipart).await {
        Ok(input) => input,
        Err(AppError::Validation(key)) => {
            let page = SettingsPage::saved(&state).refused(&i18n, &key);
            return render_settings(&state, &admin, csrf_token, i18n, page).await;
        }
        Err(e) => return Err(e),
    };
    let changed = input.changed_fields(&state);
    match apply_general(&state, &input).await {
        Ok(()) => {
            record_settings_saved(&state, &admin, &changed).await;
            Ok(Redirect::to("/admin/settings?saved=1").into_response())
        }
        Err(AppError::Validation(key)) => {
            let page = SettingsPage::typed(&state, &input).refused(&i18n, &key);
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
    NewJournalEntry::new(JournalKind::LogoRemoved)
        .by(&admin)
        .record(&state.pool)
        .await;
    Ok(Redirect::to("/admin/settings?logo=removed").into_response())
}
