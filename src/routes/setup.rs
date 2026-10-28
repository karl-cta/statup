//! First launch, after the administrator's account: the page's name, logo
//! and audience, the services it follows, then its address to share. Each
//! step is a page of its own, and the browser glides from one to the next
//! where it can (`css/setup-transitions.css`).

use askama::Template;
use axum::extract::{Multipart, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;

use super::checks::{CheckFields, CheckInput, parse_check};
use super::dashboard::origin;
use super::render;
use crate::clock;
use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::{CsrfToken, HtmlForm, RequireAdmin};
use crate::models::{
    Accent, Ambiance, CheckKind, JournalKind, MAX_ICON_SIZE, Service, ServiceCheck, User,
};
use crate::repositories::{NewJournalEntry, ServiceRepository};
use crate::services::{LogoService, ServiceService, SettingsService};
use crate::state::AppState;

/// Rows the preview draws before counting the rest.
const PREVIEW_ROWS: usize = 6;

/// More than any office needs at once; the rest is added from Services.
const MAX_NEW_SERVICES: usize = 50;

/// What most offices run, each with the built-in icon it gets: common words
/// through the translations, product names as they are.
const SUGGESTIONS: [(&str, &str); 17] = [
    ("setup.suggest_mail", "envelope"),
    ("Intranet", "globe"),
    ("VPN", "lock-closed"),
    ("Wi-Fi", "signal"),
    ("setup.suggest_phone", "phone"),
    ("setup.suggest_printers", "printer"),
    ("setup.suggest_files", "server-stack"),
    ("setup.suggest_payroll", "credit-card"),
    ("Microsoft 365", "cloud"),
    ("Outlook", "envelope"),
    ("Teams", "chat-bubble"),
    ("Slack", "chat-bubble"),
    ("Zoom", "users"),
    ("Salesforce", "chart-bar"),
    ("SAP", "database"),
    ("Jira", "wrench-screwdriver"),
    ("GitLab", "code-bracket"),
];

/// The four steps named above the form, and where the reader stands.
pub struct Progress {
    pub step: u8,
}

impl Progress {
    const STEPS: [&'static str; 4] = [
        "setup.step_account",
        "setup.step_page",
        "setup.step_services",
        "setup.step_ready",
    ];

    /// Each step's label key and state: done, current, or still ahead. The
    /// last step leaves every bar full.
    pub fn steps(&self) -> Vec<(&'static str, &'static str)> {
        (1..)
            .zip(Self::STEPS)
            .map(|(step, key)| (key, self.state_of(step)))
            .collect()
    }

    fn state_of(&self, step: u8) -> &'static str {
        if step < self.step || self.step == 4 {
            "is-done"
        } else if step == self.step {
            "is-current"
        } else {
            ""
        }
    }
}

/// The page as it will look, drawn beside every step.
pub struct Preview {
    pub custom_name: Option<String>,
    pub logo_url: Option<String>,
    pub public_mode: bool,
    pub services: Vec<Service>,
    /// Services beyond the rows drawn.
    pub more: usize,
}

impl Preview {
    pub async fn load(state: &AppState) -> Result<Self, AppError> {
        let mut services = ServiceRepository::list_all(&state.pool).await?;
        let more = services.len().saturating_sub(PREVIEW_ROWS);
        services.truncate(PREVIEW_ROWS);
        let name = crate::instance_name();
        Ok(Self {
            custom_name: (!name.is_empty()).then_some(name),
            logo_url: crate::instance_logo_url(),
            public_mode: state.is_public_mode(),
            services,
            more,
        })
    }
    /// "+ 3 other services" under the rows drawn.
    pub fn more_label(&self, i18n: &I18n) -> String {
        i18n.tf("setup.preview_more", &[("n", &self.more.to_string())])
    }
}

pub struct Suggestion {
    pub label: String,
    pub icon: &'static str,
    /// Already followed: shown ticked, and not sent again.
    pub taken: bool,
}

impl Suggestion {
    pub fn icon_paths(&self) -> Vec<&'static str> {
        crate::models::find_builtin_icon(self.icon)
            .map(|icon| icon.paths.split("|||").collect())
            .unwrap_or_default()
    }
}

fn suggestion_label(label: &'static str, i18n: &I18n) -> String {
    if label.starts_with("setup.") {
        i18n.t(label).to_string()
    } else {
        label.to_string()
    }
}

fn suggestions(existing: &[Service], i18n: &I18n) -> Vec<Suggestion> {
    SUGGESTIONS
        .iter()
        .map(|(label, icon)| {
            let label = suggestion_label(label, i18n);
            let taken = existing.iter().any(|s| s.name.eq_ignore_ascii_case(&label));
            Suggestion { label, icon, taken }
        })
        .collect()
}

/// The built-in icon of a suggested name, none for a name typed freely.
fn icon_for(name: &str, i18n: &I18n) -> Option<&'static str> {
    SUGGESTIONS
        .iter()
        .find(|(label, _)| suggestion_label(label, i18n).eq_ignore_ascii_case(name))
        .map(|(_, icon)| *icon)
}

/// A name typed in and monitored gets the built-in icon of its kind until
/// its own logo is set from its page.
fn icon_for_kind(kind: CheckKind) -> &'static str {
    match kind {
        CheckKind::Http => "globe",
        CheckKind::Tcp => "server-stack",
    }
}

#[derive(Template)]
#[template(path = "setup/page.html")]
struct PageStepTemplate {
    csrf_token: String,
    progress: Progress,
    preview: Preview,
    instance_name: String,
    error: Option<String>,
    i18n: I18n,
}

#[derive(Template)]
#[template(path = "setup/services.html")]
struct ServicesStepTemplate {
    csrf_token: String,
    progress: Progress,
    preview: Preview,
    suggestions: Vec<Suggestion>,
    error: Option<String>,
    /// The name typed and its monitoring, shown again after a refusal.
    custom: String,
    check_fields: CheckFields,
    /// The shared monitoring block reads it; a refusal here is said above
    /// the step, with the name of its service.
    check_error: Option<String>,
    /// The shared block asks for it; the first launch has no delay field.
    outage_error: Option<String>,
    i18n: I18n,
}

impl ServicesStepTemplate {
    fn check(&self) -> &CheckFields {
        &self.check_fields
    }
}

#[derive(Template)]
#[template(path = "setup/done.html")]
struct DoneStepTemplate {
    csrf_token: String,
    progress: Progress,
    preview: Preview,
    address: String,
    i18n: I18n,
}

async fn render_page_step(
    state: &AppState,
    csrf_token: String,
    i18n: I18n,
    instance_name: String,
    error: Option<String>,
) -> Result<Response, AppError> {
    render(&PageStepTemplate {
        csrf_token,
        progress: Progress { step: 2 },
        preview: Preview::load(state).await?,
        instance_name,
        error,
        i18n,
    })
}

pub async fn page_form(
    RequireAdmin(_admin): RequireAdmin,
    State(state): State<AppState>,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    render_page_step(&state, csrf_token, i18n, crate::instance_name(), None).await
}

/// The general settings, read from one multipart body: the page step of
/// the setup and the settings page send the same fields, the setup without
/// a zone (it takes the browser's at sign-up).
#[derive(Default)]
pub(super) struct GeneralInput {
    pub(super) instance_name: String,
    pub(super) access: String,
    pub(super) time_zone: Option<String>,
    /// Absent from the setup, which leaves the theme as it is.
    pub(super) accent: Option<String>,
    pub(super) ambiance: Option<String>,
    logo: Vec<u8>,
}

impl GeneralInput {
    /// What this save would change, named in English. Read before saving.
    pub(super) fn changed_fields(&self, state: &AppState) -> Vec<&'static str> {
        let changes = [
            ("name", self.instance_name.trim() != crate::instance_name()),
            ("logo", !self.logo.is_empty()),
            (
                "time_zone",
                self.time_zone
                    .as_deref()
                    .is_some_and(|zone| zone != clock::zone().name()),
            ),
            (
                "access",
                (self.access == "everyone") != state.is_public_mode(),
            ),
            (
                "accent",
                differs(self.accent.as_deref(), crate::instance_accent().as_str()),
            ),
            (
                "ambiance",
                differs(
                    self.ambiance.as_deref(),
                    crate::instance_ambiance().as_str(),
                ),
            ),
        ];
        changes
            .into_iter()
            .filter_map(|(field, changed)| changed.then_some(field))
            .collect()
    }
}

fn differs(sent: Option<&str>, saved: &str) -> bool {
    sent.is_some_and(|value| value != saved)
}

/// A theme choice from the form, refused unless it names one of the list.
fn parse_choice<T>(
    sent: Option<&str>,
    parse: fn(&str) -> Option<T>,
) -> Result<Option<T>, AppError> {
    sent.map(|value| parse(value).ok_or_else(|| AppError::validation("validation.unknown_theme")))
        .transpose()
}

/// The general settings were saved, from the settings page or the setup.
pub(super) async fn record_settings_saved(state: &AppState, admin: &User, changed: &[&str]) {
    let mut entry = NewJournalEntry::new(JournalKind::SettingsSaved).by(admin);
    if !changed.is_empty() {
        entry = entry.with_detail(changed.join(", "));
    }
    entry.record(&state.pool).await;
}

pub(super) async fn read_general(multipart: &mut Multipart) -> Result<GeneralInput, AppError> {
    let unreadable = |_| AppError::validation("validation.invalid_form_data");
    let mut input = GeneralInput::default();
    while let Some(field) = multipart.next_field().await.map_err(unreadable)? {
        match field.name() {
            Some("instance_name") => {
                input.instance_name = field.text().await.map_err(unreadable)?;
            }
            Some("access") => input.access = field.text().await.map_err(unreadable)?,
            Some("time_zone") => input.time_zone = Some(field.text().await.map_err(unreadable)?),
            Some("accent") => input.accent = Some(field.text().await.map_err(unreadable)?),
            Some("ambiance") => input.ambiance = Some(field.text().await.map_err(unreadable)?),
            Some("file") => input.logo = field.bytes().await.map_err(unreadable)?.to_vec(),
            _ => {}
        }
    }
    if input.logo.len() > MAX_ICON_SIZE {
        return Err(AppError::validation("validation.file_too_large"));
    }
    Ok(input)
}

/// Name, logo, zone, theme and audience, all checked before any is written.
pub(super) async fn apply_general(state: &AppState, input: &GeneralInput) -> Result<(), AppError> {
    let name = input.instance_name.trim();
    if let Some(key) = SettingsService::name_refusal(name) {
        return Err(AppError::validation(key));
    }
    let public = match input.access.as_str() {
        "everyone" => true,
        "members" => false,
        _ => return Err(AppError::validation("validation.unknown_access")),
    };
    let zone = match &input.time_zone {
        Some(name) => Some(
            clock::parse_zone(name)
                .ok_or_else(|| AppError::validation("validation.unknown_zone"))?,
        ),
        None => None,
    };
    let accent = parse_choice(input.accent.as_deref(), Accent::parse)?;
    let ambiance = parse_choice(input.ambiance.as_deref(), Ambiance::parse)?;
    if !input.logo.is_empty() {
        LogoService::replace(&state.pool, &state.upload_dir, input.logo.clone()).await?;
    }
    SettingsService::set_name(&state.pool, name).await?;
    if let Some(zone) = zone {
        SettingsService::set_time_zone(&state.pool, zone).await?;
    }
    if accent.is_some() || ambiance.is_some() {
        let accent = accent.unwrap_or_else(crate::instance_accent);
        let ambiance = ambiance.unwrap_or_else(crate::instance_ambiance);
        SettingsService::set_theme(&state.pool, accent, ambiance).await?;
    }
    SettingsService::set_public_mode(state, public).await
}

pub async fn save_page(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
    mut multipart: Multipart,
) -> Result<Response, AppError> {
    let applied = match read_general(&mut multipart).await {
        Ok(input) => {
            let changed = input.changed_fields(&state);
            apply_general(&state, &input).await.map(|()| changed)
        }
        Err(e) => Err(e),
    };
    match applied {
        Ok(changed) => {
            record_settings_saved(&state, &admin, &changed).await;
            Ok(Redirect::to("/setup/services").into_response())
        }
        Err(AppError::Validation(key)) => {
            let error = Some(i18n.t(&key).to_string());
            render_page_step(&state, csrf_token, i18n, crate::instance_name(), error).await
        }
        Err(e) => Err(e),
    }
}

/// What the step shows again after a refusal: the message, and the name
/// typed with its monitoring.
#[derive(Default)]
struct Refusal {
    error: Option<String>,
    custom: String,
    check: CheckFields,
}

async fn render_services_step(
    state: &AppState,
    csrf_token: String,
    i18n: I18n,
    refusal: Refusal,
) -> Result<Response, AppError> {
    let existing = ServiceRepository::list_all(&state.pool).await?;
    render(&ServicesStepTemplate {
        csrf_token,
        progress: Progress { step: 3 },
        preview: Preview::load(state).await?,
        suggestions: suggestions(&existing, &i18n),
        error: refusal.error,
        custom: refusal.custom,
        check_fields: refusal.check,
        check_error: None,
        outage_error: None,
        i18n,
    })
}

pub async fn services_form(
    RequireAdmin(_admin): RequireAdmin,
    State(state): State<AppState>,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    render_services_step(&state, csrf_token, i18n, Refusal::default()).await
}

#[derive(Default, Deserialize)]
pub struct ServicesInput {
    /// Ticked suggestions and names added with the script.
    #[serde(default)]
    services: Vec<String>,
    /// A name typed without the script, or not yet added.
    #[serde(default)]
    custom: String,
    /// The monitoring of `custom`, as the shared block sends it.
    #[serde(flatten)]
    check: CheckInput,
    /// The monitoring of the names added with the script, one entry each
    /// across these lists.
    #[serde(default)]
    checks: Vec<String>,
    #[serde(default)]
    check_kinds: Vec<String>,
    #[serde(default)]
    check_urls: Vec<String>,
    #[serde(default)]
    check_hosts: Vec<String>,
    #[serde(default)]
    check_ports: Vec<String>,
    #[serde(default)]
    check_certs: Vec<String>,
}

impl ServicesInput {
    fn refusal(&self, error: String) -> Refusal {
        Refusal {
            error: Some(error),
            custom: self.custom.clone(),
            check: self.check.fields(),
        }
    }

    /// The monitoring asked for each name, read before anything is created;
    /// a refused address is said with the name of its service.
    fn checks(&self, i18n: &I18n) -> Result<Vec<(String, ServiceCheck)>, String> {
        let count = self.checks.len();
        let lists = [
            &self.check_kinds,
            &self.check_urls,
            &self.check_hosts,
            &self.check_ports,
            &self.check_certs,
        ];
        if lists.iter().any(|list| list.len() != count) {
            return Err(i18n.t("error.invalid_data").to_string());
        }
        let mut checks = Vec::new();
        for (i, name) in self.checks.iter().enumerate() {
            let parsed = parse_check(
                entry(&self.check_kinds, i),
                entry(&self.check_urls, i),
                entry(&self.check_hosts, i),
                entry(&self.check_ports, i),
                entry(&self.check_certs, i) == "1",
            );
            keep_check(&mut checks, name.trim(), parsed, i18n)?;
        }
        if !self.custom.trim().is_empty() {
            keep_check(&mut checks, self.custom.trim(), self.check.check(), i18n)?;
        }
        Ok(checks)
    }
}

/// One entry of the parallel lists, empty when a list is short.
fn entry(list: &[String], i: usize) -> &str {
    list.get(i).map_or("", String::as_str)
}

fn keep_check(
    checks: &mut Vec<(String, ServiceCheck)>,
    name: &str,
    parsed: Result<Option<ServiceCheck>, AppError>,
    i18n: &I18n,
) -> Result<(), String> {
    match parsed {
        Ok(Some(check)) => checks.push((name.to_string(), check)),
        Ok(None) => {}
        Err(AppError::Validation(key)) => {
            return Err(i18n.tf(
                "validation.setup_check_invalid",
                &[("service", name), ("reason", i18n.t(&key))],
            ));
        }
        Err(_) => return Err(i18n.t("error.invalid_data").to_string()),
    }
    Ok(())
}

/// The names to create: trimmed, once each, none already followed.
fn new_names(input: &ServicesInput, existing: &[Service]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for name in input.services.iter().chain(std::iter::once(&input.custom)) {
        let name = name.trim();
        let known = |other: &String| other.eq_ignore_ascii_case(name);
        if name.is_empty() || names.iter().any(known) {
            continue;
        }
        if existing.iter().any(|s| s.name.eq_ignore_ascii_case(name)) {
            continue;
        }
        names.push(name.to_string());
    }
    names
}

async fn create_services(
    state: &AppState,
    names: &[String],
    checks: &[(String, ServiceCheck)],
    i18n: &I18n,
) -> Result<(), AppError> {
    if names.len() > MAX_NEW_SERVICES {
        return Err(AppError::validation("validation.too_many_services"));
    }
    for name in names {
        let check = checks
            .iter()
            .find(|(checked, _)| checked.eq_ignore_ascii_case(name))
            .map(|(_, check)| check);
        let icon = icon_for(name, i18n).or_else(|| check.map(|check| icon_for_kind(check.kind)));
        let service = ServiceService::create(&state.pool, name, None, None, icon).await?;
        if check.is_some() {
            ServiceService::set_check(&state.pool, service.id, check).await?;
        }
    }
    Ok(())
}

pub async fn save_services(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
    HtmlForm(input): HtmlForm<ServicesInput>,
) -> Result<Response, AppError> {
    let existing = ServiceRepository::list_all(&state.pool).await?;
    let names = new_names(&input, &existing);
    let checks = match input.checks(&i18n) {
        Ok(checks) => checks,
        Err(message) => {
            let refusal = input.refusal(message);
            return render_services_step(&state, csrf_token, i18n, refusal).await;
        }
    };
    match create_services(&state, &names, &checks, &i18n).await {
        Ok(()) => {
            for name in &names {
                NewJournalEntry::new(JournalKind::ServiceCreated)
                    .by(&admin)
                    .about(name)
                    .record(&state.pool)
                    .await;
            }
            Ok(Redirect::to("/setup/done").into_response())
        }
        Err(AppError::Validation(key)) => {
            let refusal = input.refusal(i18n.t(&key).to_string());
            render_services_step(&state, csrf_token, i18n, refusal).await
        }
        Err(e) => Err(e),
    }
}

pub async fn done(
    RequireAdmin(_admin): RequireAdmin,
    State(state): State<AppState>,
    CsrfToken(csrf_token): CsrfToken,
    headers: HeaderMap,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    render(&DoneStepTemplate {
        csrf_token,
        progress: Progress { step: 4 },
        preview: Preview::load(&state).await?,
        address: origin(&state, &headers),
        i18n,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(services: &[&str], custom: &str) -> ServicesInput {
        ServicesInput {
            services: services.iter().map(ToString::to_string).collect(),
            custom: custom.to_string(),
            ..ServicesInput::default()
        }
    }

    #[test]
    fn each_new_name_is_kept_once_and_blanks_are_dropped() {
        let names = new_names(&input(&["VPN", " vpn ", "", "Teams"], "  Sage "), &[]);
        assert_eq!(names, ["VPN", "Teams", "Sage"]);
    }

    #[test]
    fn the_last_step_fills_every_bar() {
        let states = |step| -> Vec<&str> {
            Progress { step }
                .steps()
                .into_iter()
                .map(|(_, state)| state)
                .collect()
        };
        assert_eq!(states(4), ["is-done"; 4]);
        assert_eq!(states(2), ["is-done", "is-current", "", ""]);
    }
}
