//! The notification destinations, in the settings: the list, a destination
//! added, changed, deleted or tested, and the page address the messages
//! link to.

use std::collections::HashMap;

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use chrono::Utc;
use serde::Deserialize;

use super::dashboard::origin;
use super::{Frame, render};
use crate::error::AppError;
use crate::i18n::{I18n, LANGUAGES, Locale, language};
use crate::middleware::{CsrfToken, HtmlForm, RequireAdmin};
use crate::models::{
    Channel, ChannelInput, ChannelKind, Delivery, DeliveryStatus, Happening, SEPARATOR, User,
};
use crate::repositories::{NotificationRepository, SettingsRepository};
use crate::services::{
    Facts, Failure, Finding, Notice, Origin, PAGE_ADDRESS_SETTING, Sender, Subject,
    destination_allowed, destination_name_refusal, email_addresses, page_address,
};
use crate::state::AppState;

/// A destination as its form sends it, and as the form shows it again.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DestinationForm {
    #[serde(default)]
    name: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    target: String,
    #[serde(default)]
    locale: String,
    on_incidents: Option<String>,
    on_maintenances: Option<String>,
    on_publications: Option<String>,
    on_detected: Option<String>,
}

/// The field a refusal is said under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Kind,
    Name,
    Target,
}

struct FieldError {
    field: Field,
    message: String,
}

impl DestinationForm {
    /// A new destination receives everything but the detected outages, in
    /// the instance language.
    fn fresh() -> Self {
        let on = || Some("on".to_string());
        Self {
            kind: ChannelKind::ALL[0].as_str().to_string(),
            locale: I18n::default().locale().to_string(),
            on_incidents: on(),
            on_maintenances: on(),
            on_publications: on(),
            ..Self::default()
        }
    }

    fn of(channel: &Channel) -> Self {
        let on = |ticked: bool| ticked.then(|| "on".to_string());
        Self {
            name: channel.name.clone(),
            kind: channel.kind.as_str().to_string(),
            target: channel.target.clone(),
            locale: channel.locale.clone(),
            on_incidents: on(channel.on_incidents),
            on_maintenances: on(channel.on_maintenances),
            on_publications: on(channel.on_publications),
            on_detected: on(channel.on_detected),
        }
    }

    /// The tool and its address, checked: all a test needs. Email needs a
    /// mail server set on the instance.
    fn destination(
        &self,
        email_ready: bool,
    ) -> Result<(ChannelKind, String), (Field, &'static str)> {
        let kind = self
            .kind
            .parse::<ChannelKind>()
            .map_err(|()| (Field::Kind, "error.invalid_data"))?;
        if kind == ChannelKind::Email && !email_ready {
            return Err((Field::Kind, "notifications.email_unavailable"));
        }
        let target = self.target.trim();
        let target = if kind == ChannelKind::Email {
            email_addresses(target)
        } else {
            destination_allowed(kind, target).map(|()| target.to_string())
        };
        Ok((kind, target.map_err(|key| (Field::Target, key))?))
    }

    /// What to save, or the field to fix and why.
    fn check(&self, email_ready: bool) -> Result<ChannelInput, (Field, &'static str)> {
        if let Some(key) = destination_name_refusal(&self.name) {
            return Err((Field::Name, key));
        }
        let (kind, target) = self.destination(email_ready)?;
        Ok(ChannelInput {
            name: self.name.trim().to_string(),
            kind,
            target,
            locale: self.language().to_string(),
            on_incidents: self.on_incidents.is_some(),
            on_maintenances: self.on_maintenances.is_some(),
            on_publications: self.on_publications.is_some(),
            on_detected: self.on_detected.is_some(),
        })
    }

    /// The language chosen, or the instance one when the form names none
    /// that ships.
    fn language(&self) -> &'static str {
        language(&self.locale).map_or_else(|| I18n::default().locale(), |l| l.code)
    }
}

fn field_error((field, key): (Field, &'static str), i18n: &I18n) -> FieldError {
    FieldError {
        field,
        message: i18n.t(key).to_string(),
    }
}

/// One destination of the list.
struct DestinationRow {
    id: i64,
    name: String,
    /// "Slack · Français".
    tool: String,
    /// "Incidents, maintenances, annonces", or that it is paused.
    receives: String,
    last: LastSend,
}

/// How the last message went, in the words of the list.
enum LastSend {
    Never,
    Sent(String),
    /// A tone and the sentence: a failure being tried again, or given up.
    Trouble(&'static str, String),
}

/// What the previous action did, said at the top of the list.
enum ListNotice {
    Added(String),
    Saved(String),
    Deleted,
    AddressSaved,
}

/// The address the messages link to, as the foot of the list shows it.
struct PageAddress {
    value: String,
    /// Set on the server by `PUBLIC_URL`: shown, not edited.
    fixed: bool,
    error: Option<String>,
}

#[derive(Template)]
#[template(path = "admin/notifications.html")]
struct NotificationsTemplate {
    frame: Frame,
    rows: Vec<DestinationRow>,
    notice: Option<ListNotice>,
    form: DestinationForm,
    form_error: Option<FieldError>,
    add_open: bool,
    email_ready: bool,
    page: PageAddress,
    i18n: I18n,
}

#[derive(Template)]
#[template(path = "admin/notification_edit.html")]
struct DestinationTemplate {
    frame: Frame,
    id: i64,
    /// The saved name, for the title and the delete question.
    saved_name: String,
    form: DestinationForm,
    form_error: Option<FieldError>,
    email_ready: bool,
    i18n: I18n,
}

#[derive(Template)]
#[template(path = "admin/_notification_test.html")]
struct TestResultFragment {
    /// The tone of the verdict; none when the form itself was refused.
    tone: Option<&'static str>,
    message: String,
}

/// The template helpers shared by the two pages that hold the form.
trait DestinationFields {
    fn form(&self) -> &DestinationForm;
    fn form_error(&self) -> Option<&FieldError>;

    fn error_for(&self, field: Field) -> Option<&str> {
        self.form_error()
            .filter(|error| error.field == field)
            .map(|error| error.message.as_str())
    }

    fn kinds(&self) -> [ChannelKind; 8] {
        ChannelKind::ALL
    }

    fn languages(&self) -> &'static [crate::i18n::Language] {
        LANGUAGES
    }

    fn chosen(&self) -> ChannelKind {
        self.form().kind.parse().unwrap_or(ChannelKind::ALL[0])
    }
}

impl DestinationFields for NotificationsTemplate {
    fn form(&self) -> &DestinationForm {
        &self.form
    }
    fn form_error(&self) -> Option<&FieldError> {
        self.form_error.as_ref()
    }
}

impl DestinationFields for DestinationTemplate {
    fn form(&self) -> &DestinationForm {
        &self.form
    }
    fn form_error(&self) -> Option<&FieldError> {
        self.form_error.as_ref()
    }
}

#[derive(Deserialize, Default)]
pub struct ListQuery {
    added: Option<i64>,
    saved: Option<i64>,
    deleted: Option<String>,
    address: Option<String>,
}

impl ListQuery {
    /// The name is read back from the database, so the address bar cannot
    /// make the receipt say anything.
    async fn notice(&self, state: &AppState) -> Result<Option<ListNotice>, AppError> {
        let name_of = |id| NotificationRepository::find_channel(&state.pool, id);
        if let Some(id) = self.added {
            return Ok(name_of(id).await?.map(|c| ListNotice::Added(c.name)));
        }
        if let Some(id) = self.saved {
            return Ok(name_of(id).await?.map(|c| ListNotice::Saved(c.name)));
        }
        if self.deleted.is_some() {
            return Ok(Some(ListNotice::Deleted));
        }
        Ok(self.address.is_some().then_some(ListNotice::AddressSaved))
    }
}

/// What the list page says on top of the saved destinations.
struct ListPage {
    notice: Option<ListNotice>,
    form: DestinationForm,
    form_error: Option<FieldError>,
    address_error: Option<(String, String)>,
}

pub async fn list(
    RequireAdmin(user): RequireAdmin,
    State(state): State<AppState>,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Response, AppError> {
    let page = ListPage {
        notice: query.notice(&state).await?,
        form: DestinationForm::fresh(),
        form_error: None,
        address_error: None,
    };
    render_list(&state, &user, csrf_token, i18n, &headers, page).await
}

async fn render_list(
    state: &AppState,
    user: &User,
    csrf_token: String,
    i18n: I18n,
    headers: &HeaderMap,
    page: ListPage,
) -> Result<Response, AppError> {
    let channels = NotificationRepository::list_channels(&state.pool).await?;
    let rows = destination_rows(state, channels, &i18n).await?;
    let add_open = rows.is_empty() || page.form_error.is_some();
    let address = page_address_field(state, headers, page.address_error, &i18n).await?;
    let frame = Frame::load(&state.pool, Some(user), csrf_token, &i18n).await?;
    render(&NotificationsTemplate {
        frame,
        rows,
        notice: page.notice,
        form: page.form,
        form_error: page.form_error,
        add_open,
        email_ready: state.notifier.mail_ready(),
        page: address,
        i18n,
    })
}

async fn destination_rows(
    state: &AppState,
    channels: Vec<Channel>,
    i18n: &I18n,
) -> Result<Vec<DestinationRow>, AppError> {
    let outcomes = NotificationRepository::latest_outcomes(&state.pool).await?;
    let retrying = NotificationRepository::retrying(&state.pool).await?;
    Ok(channels
        .into_iter()
        .map(|channel| DestinationRow {
            id: channel.id,
            tool: tool_line(&channel, i18n),
            receives: receives(&channel, i18n),
            last: last_send(channel.id, &retrying, &outcomes, i18n),
            name: channel.name,
        })
        .collect())
}

fn tool_line(channel: &Channel, i18n: &I18n) -> String {
    let language = language(&channel.locale).map_or(channel.locale.as_str(), |l| l.name);
    format!("{}{SEPARATOR}{language}", i18n.t(channel.kind.label_key()))
}

/// What a destination receives, as one sentence: "Incidents, annonces".
fn receives(channel: &Channel, i18n: &I18n) -> String {
    let parts: Vec<&str> = [
        (channel.on_incidents, "notifications.receives_incidents"),
        (
            channel.on_maintenances,
            "notifications.receives_maintenances",
        ),
        (
            channel.on_publications,
            "notifications.receives_publications",
        ),
        (channel.on_detected, "notifications.receives_detected"),
    ]
    .into_iter()
    .filter(|(ticked, _)| *ticked)
    .map(|(_, key)| i18n.t(key))
    .collect();
    if parts.is_empty() {
        return i18n.t("notifications.paused").to_string();
    }
    let joined = parts.join(", ");
    let mut chars = joined.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// A failure being tried again comes first: it is what an admin can act on.
fn last_send(
    channel_id: i64,
    retrying: &HashMap<i64, Delivery>,
    outcomes: &HashMap<i64, Delivery>,
    i18n: &I18n,
) -> LastSend {
    if let Some(delivery) = retrying.get(&channel_id) {
        let reason = reason_of(delivery.failure.as_deref(), i18n);
        let next = i18n.format_time(&delivery.next_attempt_at);
        let text = i18n.tf(
            "notifications.retrying",
            &[("time", &next), ("reason", &reason)],
        );
        return LastSend::Trouble("minor", text);
    }
    let Some(delivery) = outcomes.get(&channel_id) else {
        return LastSend::Never;
    };
    if delivery.status == DeliveryStatus::Sent {
        let at = delivery.sent_at.unwrap_or(delivery.created_at);
        let when = i18n.format_datetime(&at);
        return LastSend::Sent(i18n.tf("notifications.sent", &[("when", &when)]));
    }
    let reason = reason_of(delivery.failure.as_deref(), i18n);
    let when = i18n.format_datetime(&delivery.next_attempt_at);
    let text = i18n.tf(
        "notifications.failed",
        &[("when", &when), ("reason", &reason)],
    );
    LastSend::Trouble("crit", text)
}

fn reason_of(code: Option<&str>, i18n: &I18n) -> String {
    code.and_then(Failure::from_code).map_or_else(
        || i18n.t("notifications.failure_unknown").to_string(),
        |failure| failure_text(failure, i18n),
    )
}

/// Why a message did not go out, in words an admin can act on, with the
/// code the tool or the mail server answered when there is one.
fn failure_text(failure: Failure, i18n: &I18n) -> String {
    let (key, status) = match failure {
        Failure::Status(status @ (404 | 410)) => ("notifications.failure_not_found", Some(status)),
        Failure::Status(status @ (401 | 403)) => ("notifications.failure_forbidden", Some(status)),
        Failure::Status(status) if status >= 500 => ("notifications.failure_server", Some(status)),
        Failure::Status(status) => ("notifications.failure_rejected", Some(status)),
        Failure::TooManyRequests(_) => ("notifications.failure_too_many", None),
        Failure::Redirected => ("notifications.failure_redirected", None),
        Failure::Transport(finding) => (transport_key(finding), None),
        Failure::Unsupported => ("notifications.failure_unsupported", None),
        Failure::Gone => ("notifications.failure_gone", None),
        Failure::MailRefused(status) => ("notifications.failure_mail_refused", status),
        Failure::MailBusy(status) => ("notifications.failure_mail_busy", status),
        Failure::MailUnreachable => ("notifications.failure_mail_unreachable", None),
        Failure::NoMailServer => ("notifications.failure_no_mail_server", None),
    };
    let text = i18n.t(key);
    status.map_or_else(|| text.to_string(), |status| format!("{text} ({status})"))
}

fn transport_key(finding: Finding) -> &'static str {
    match finding {
        Finding::Refused => "notifications.failure_refused",
        Finding::TimedOut => "notifications.failure_timeout",
        Finding::NameNotFound => "notifications.failure_name",
        Finding::Certificate => "notifications.failure_certificate",
        _ => "notifications.failure_unreachable",
    }
}

/// The address the messages link to: the server's, or the stored one, or
/// failing both the address this page was opened at, to be saved.
async fn page_address_field(
    state: &AppState,
    headers: &HeaderMap,
    error: Option<(String, String)>,
    i18n: &I18n,
) -> Result<PageAddress, AppError> {
    if let Some(url) = &state.public_url {
        return Ok(PageAddress {
            value: url.clone(),
            fixed: true,
            error: None,
        });
    }
    if let Some((typed, key)) = error {
        return Ok(PageAddress {
            value: typed,
            fixed: false,
            error: Some(i18n.t(&key).to_string()),
        });
    }
    let stored = page_address(&state.pool, None).await?;
    Ok(PageAddress {
        value: stored.unwrap_or_else(|| origin(state, headers)),
        fixed: false,
        error: None,
    })
}

pub async fn create(
    RequireAdmin(user): RequireAdmin,
    State(state): State<AppState>,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
    headers: HeaderMap,
    HtmlForm(form): HtmlForm<DestinationForm>,
) -> Result<Response, AppError> {
    let input = match form.check(state.notifier.mail_ready()) {
        Ok(input) => input,
        Err(refusal) => {
            let page = ListPage {
                notice: None,
                form_error: Some(field_error(refusal, &i18n)),
                form,
                address_error: None,
            };
            return render_list(&state, &user, csrf_token, i18n, &headers, page).await;
        }
    };
    let channel = NotificationRepository::create_channel(&state.pool, &input).await?;
    remember_page_address(&state, &headers).await?;
    Ok(Redirect::to(&format!("/admin/notifications?added={}", channel.id)).into_response())
}

/// The first destination keeps the address the admin uses, so the messages
/// carry a link from the start; the foot of the list shows it and changes it.
/// An address emptied on purpose stays empty.
async fn remember_page_address(state: &AppState, headers: &HeaderMap) -> Result<(), AppError> {
    let never_set = SettingsRepository::get(&state.pool, PAGE_ADDRESS_SETTING)
        .await?
        .is_none();
    if state.public_url.is_none() && never_set {
        let address = origin(state, headers);
        SettingsRepository::set(&state.pool, PAGE_ADDRESS_SETTING, &address).await?;
    }
    Ok(())
}

pub async fn edit(
    RequireAdmin(user): RequireAdmin,
    State(state): State<AppState>,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let channel = find(&state, id).await?;
    let form = DestinationForm::of(&channel);
    render_edit(&state, &user, csrf_token, i18n, channel, form, None).await
}

async fn find(state: &AppState, id: i64) -> Result<Channel, AppError> {
    NotificationRepository::find_channel(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)
}

async fn render_edit(
    state: &AppState,
    user: &User,
    csrf_token: String,
    i18n: I18n,
    channel: Channel,
    form: DestinationForm,
    form_error: Option<FieldError>,
) -> Result<Response, AppError> {
    let frame = Frame::load(&state.pool, Some(user), csrf_token, &i18n).await?;
    render(&DestinationTemplate {
        frame,
        id: channel.id,
        saved_name: channel.name,
        form,
        form_error,
        email_ready: state.notifier.mail_ready(),
        i18n,
    })
}

pub async fn update(
    RequireAdmin(user): RequireAdmin,
    State(state): State<AppState>,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
    Path(id): Path<i64>,
    HtmlForm(form): HtmlForm<DestinationForm>,
) -> Result<Response, AppError> {
    let channel = find(&state, id).await?;
    let input = match form.check(state.notifier.mail_ready()) {
        Ok(input) => input,
        Err(refusal) => {
            let error = Some(field_error(refusal, &i18n));
            return render_edit(&state, &user, csrf_token, i18n, channel, form, error).await;
        }
    };
    NotificationRepository::update_channel(&state.pool, id, &input).await?;
    Ok(Redirect::to(&format!("/admin/notifications?saved={id}")).into_response())
}

pub async fn delete(
    _admin: RequireAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    if !NotificationRepository::delete_channel(&state.pool, id).await? {
        return Err(AppError::NotFound);
    }
    Ok(Redirect::to("/admin/notifications?deleted=1").into_response())
}

/// Sends a test message to the destination the form describes, without
/// saving anything, and says how it went.
pub async fn test(
    _admin: RequireAdmin,
    State(state): State<AppState>,
    Locale(i18n): Locale,
    headers: HeaderMap,
    HtmlForm(form): HtmlForm<DestinationForm>,
) -> Result<Response, AppError> {
    let (kind, target) = match form.destination(state.notifier.mail_ready()) {
        Ok(destination) => destination,
        Err((_, key)) => {
            return render(&TestResultFragment {
                tone: None,
                message: i18n.t(key).to_string(),
            });
        }
    };
    let channel = trial_channel(&form, kind, target);
    let page = page_address(&state.pool, state.public_url.as_deref())
        .await?
        .unwrap_or_else(|| origin(&state, &headers));
    let (notice, facts) = test_message(&channel, page);
    let result = state.notifier.send(&channel, &notice, &facts).await;
    let (tone, message) = match result {
        Ok(()) => ("ok", i18n.t("notifications.test_sent").to_string()),
        Err(failure) => {
            let reason = failure_text(failure, &i18n);
            let text = i18n.tf("notifications.test_failed", &[("reason", &reason)]);
            ("crit", text)
        }
    };
    render(&TestResultFragment {
        tone: Some(tone),
        message,
    })
}

/// The test message, in the language of the destination tried.
fn test_message(channel: &Channel, page: String) -> (Notice, Facts) {
    let origin = Origin {
        instance: crate::brand_name(),
        page: Some(page),
    };
    let now = Utc::now();
    let language = I18n::new(&channel.locale);
    let notice = Notice::write(
        Happening::Test,
        None,
        &Subject::Test,
        &origin,
        &language,
        now,
    );
    let facts = Facts::of(Happening::Test, None, &Subject::Test, &origin, now);
    (notice, facts)
}

/// The destination of a test, never saved.
fn trial_channel(form: &DestinationForm, kind: ChannelKind, target: String) -> Channel {
    let now = Utc::now();
    Channel {
        id: 0,
        name: form.name.trim().to_string(),
        kind,
        target,
        locale: form.language().to_string(),
        on_incidents: true,
        on_maintenances: true,
        on_publications: true,
        on_detected: true,
        created_at: now,
        updated_at: now,
    }
}

#[derive(Deserialize)]
pub struct PageAddressForm {
    #[serde(default)]
    page_address: String,
}

/// Saves the address the messages link to. An empty field forgets it: the
/// messages then go without a link.
pub async fn save_page_address(
    RequireAdmin(user): RequireAdmin,
    State(state): State<AppState>,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
    headers: HeaderMap,
    HtmlForm(form): HtmlForm<PageAddressForm>,
) -> Result<Response, AppError> {
    let typed = form.page_address.trim().trim_end_matches('/').to_string();
    if let Some(key) = page_address_refusal(&typed) {
        let page = ListPage {
            notice: None,
            form: DestinationForm::fresh(),
            form_error: None,
            address_error: Some((form.page_address, key.to_string())),
        };
        return render_list(&state, &user, csrf_token, i18n, &headers, page).await;
    }
    SettingsRepository::set(&state.pool, PAGE_ADDRESS_SETTING, &typed).await?;
    Ok(Redirect::to("/admin/notifications?address=1").into_response())
}

/// A web address as a destination takes one, without a query, or nothing.
fn page_address_refusal(address: &str) -> Option<&'static str> {
    let valid = address.is_empty()
        || (destination_allowed(ChannelKind::Webhook, address).is_ok() && !address.contains('?'));
    (!valid).then_some("notifications.page_address_invalid")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(kind: &str, target: &str) -> DestinationForm {
        DestinationForm {
            name: " Canal #informatique ".to_string(),
            kind: kind.to_string(),
            target: format!(" {target} "),
            locale: "xx".to_string(),
            on_incidents: Some("on".to_string()),
            ..DestinationForm::default()
        }
    }

    #[test]
    fn a_checked_form_is_trimmed_and_speaks_a_shipped_language() {
        let input = form("slack", "https://hooks.slack.com/services/a")
            .check(false)
            .unwrap();
        assert_eq!(input.name, "Canal #informatique");
        assert_eq!(input.target, "https://hooks.slack.com/services/a");
        assert_eq!(input.locale, I18n::default().locale());
        assert!(input.on_incidents && !input.on_maintenances && !input.on_detected);
    }

    #[test]
    fn a_refusal_names_its_field() {
        let mut nameless = form("slack", "https://hooks.slack.com/services/a");
        nameless.name.clear();
        assert_eq!(
            nameless.check(false).err(),
            Some((Field::Name, "notifications.name_required"))
        );
        assert_eq!(
            form("discord", "http://discord.com/api/webhooks/1")
                .check(false)
                .err(),
            Some((Field::Target, "notifications.target_https"))
        );
        assert_eq!(
            form("pager", "https://example.com").check(false).err(),
            Some((Field::Kind, "error.invalid_data"))
        );
        assert_eq!(
            form("email", "it@example.com").check(false).err(),
            Some((Field::Kind, "notifications.email_unavailable"))
        );
        let ready = form("email", "IT@example.com, board@example.com").check(true);
        assert_eq!(ready.unwrap().target, "it@example.com, board@example.com");
    }

    #[test]
    fn what_a_destination_receives_reads_as_one_sentence() {
        let i18n = I18n::new("fr");
        let mut channel = trial_channel(&form("slack", "x"), ChannelKind::Slack, String::new());
        channel.on_maintenances = false;
        channel.on_detected = false;
        assert_eq!(receives(&channel, &i18n), "Incidents, annonces");
        channel.on_incidents = false;
        channel.on_publications = false;
        assert_eq!(receives(&channel, &i18n), "En pause");
    }

    #[test]
    fn a_failure_is_worded_with_its_status() {
        let i18n = I18n::new("en");
        assert!(failure_text(Failure::Status(404), &i18n).contains("404"));
        assert_ne!(
            failure_text(Failure::Status(503), &i18n),
            failure_text(Failure::Status(400), &i18n)
        );
        assert_eq!(
            reason_of(Some("nonsense"), &i18n),
            i18n.t("notifications.failure_unknown")
        );
    }

    #[test]
    fn the_page_address_is_a_plain_web_address() {
        assert_eq!(page_address_refusal(""), None);
        assert_eq!(page_address_refusal("https://status.example.com"), None);
        assert_eq!(
            page_address_refusal("http://192.168.1.10:3000/status"),
            None
        );
        for refused in [
            "status.example.com",
            "ftp://x.y",
            "https://a:b@x.y",
            "https://x.y/?a=1",
        ] {
            assert_eq!(
                page_address_refusal(refused),
                Some("notifications.page_address_invalid"),
                "{refused}"
            );
        }
    }
}
