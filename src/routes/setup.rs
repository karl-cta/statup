//! First launch, after the administrator's account: the page's name, logo
//! and audience, the services it follows, then its address to share. Each
//! step is a page of its own, and the browser glides from one to the next
//! where it can (`css/setup-transitions.css`).

use askama::Template;
use axum::extract::{Multipart, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;

use super::admin::{instance_name_refusal, save_instance_name, save_public_mode};
use super::dashboard::origin;
use super::render;
use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::{CsrfToken, HtmlForm, RequireAdmin};
use crate::models::{MAX_ICON_SIZE, Service};
use crate::repositories::ServiceRepository;
use crate::services::{LogoService, ServiceService};
use crate::state::AppState;

/// Rows the preview draws before counting the rest.
const PREVIEW_ROWS: usize = 6;

/// More than any office needs at once; the rest is added from Services.
const MAX_NEW_SERVICES: usize = 50;

/// What most offices run, each with the built-in icon it gets: common words
/// through the translations, product names as they are.
const SUGGESTIONS: [(&str, &str); 21] = [
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
    ("SharePoint", "document-text"),
    ("Google Workspace", "cloud"),
    ("Slack", "chat-bubble"),
    ("Zoom", "users"),
    ("Salesforce", "chart-bar"),
    ("SAP", "database"),
    ("Sage", "credit-card"),
    ("Jira", "wrench-screwdriver"),
    ("Confluence", "document-text"),
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
    i18n: I18n,
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

/// The fields of the page step, read from its multipart body.
#[derive(Default)]
struct PageInput {
    instance_name: String,
    access: String,
    logo: Vec<u8>,
}

async fn read_page_input(multipart: &mut Multipart) -> Result<PageInput, AppError> {
    let unreadable = |_| AppError::Validation("validation.invalid_form_data".to_string());
    let mut input = PageInput::default();
    while let Some(field) = multipart.next_field().await.map_err(unreadable)? {
        match field.name() {
            Some("instance_name") => {
                input.instance_name = field.text().await.map_err(unreadable)?;
            }
            Some("access") => input.access = field.text().await.map_err(unreadable)?,
            Some("file") => input.logo = field.bytes().await.map_err(unreadable)?.to_vec(),
            _ => {}
        }
    }
    if input.logo.len() > MAX_ICON_SIZE {
        return Err(AppError::Validation(
            "validation.file_too_large".to_string(),
        ));
    }
    Ok(input)
}

/// Name, logo and audience, each kept as the settings page keeps it.
async fn apply_page(state: &AppState, input: &PageInput) -> Result<(), AppError> {
    let name = input.instance_name.trim();
    if let Some(key) = instance_name_refusal(name) {
        return Err(AppError::Validation(key.to_string()));
    }
    let public = match input.access.as_str() {
        "everyone" => true,
        "members" => false,
        _ => {
            return Err(AppError::Validation(
                "validation.unknown_access".to_string(),
            ));
        }
    };
    if !input.logo.is_empty() {
        LogoService::replace(&state.pool, &state.upload_dir, input.logo.clone()).await?;
    }
    save_instance_name(state, name).await?;
    save_public_mode(state, public).await
}

pub async fn save_page(
    RequireAdmin(admin): RequireAdmin,
    State(state): State<AppState>,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
    mut multipart: Multipart,
) -> Result<Response, AppError> {
    let applied = match read_page_input(&mut multipart).await {
        Ok(input) => apply_page(&state, &input).await.map(|()| input),
        Err(e) => Err(e),
    };
    match applied {
        Ok(input) => {
            tracing::info!(
                admin_id = admin.id,
                public = input.access == "everyone",
                "Page set up"
            );
            Ok(Redirect::to("/setup/services").into_response())
        }
        Err(AppError::Validation(key)) => {
            let error = Some(i18n.t(&key).to_string());
            render_page_step(&state, csrf_token, i18n, crate::instance_name(), error).await
        }
        Err(e) => Err(e),
    }
}

async fn render_services_step(
    state: &AppState,
    csrf_token: String,
    i18n: I18n,
    error: Option<String>,
) -> Result<Response, AppError> {
    let existing = ServiceRepository::list_all(&state.pool).await?;
    render(&ServicesStepTemplate {
        csrf_token,
        progress: Progress { step: 3 },
        preview: Preview::load(state).await?,
        suggestions: suggestions(&existing, &i18n),
        error,
        i18n,
    })
}

pub async fn services_form(
    RequireAdmin(_admin): RequireAdmin,
    State(state): State<AppState>,
    CsrfToken(csrf_token): CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    render_services_step(&state, csrf_token, i18n, None).await
}

#[derive(Deserialize)]
pub struct ServicesInput {
    /// Ticked suggestions and names added with the script.
    #[serde(default)]
    services: Vec<String>,
    /// A name typed without the script, or not yet added.
    #[serde(default)]
    custom: String,
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

async fn create_services(state: &AppState, names: &[String], i18n: &I18n) -> Result<(), AppError> {
    if names.len() > MAX_NEW_SERVICES {
        return Err(AppError::Validation(
            "validation.too_many_services".to_string(),
        ));
    }
    for name in names {
        ServiceService::create(&state.pool, name, None, None, icon_for(name, i18n)).await?;
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
    match create_services(&state, &names, &i18n).await {
        Ok(()) => {
            tracing::info!(admin_id = admin.id, added = names.len(), "Services set up");
            Ok(Redirect::to("/setup/done").into_response())
        }
        Err(AppError::Validation(key)) => {
            let error = Some(i18n.t(&key).to_string());
            render_services_step(&state, csrf_token, i18n, error).await
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
