//! Status banner, pinned above every dashboard.
//!
//! Answers one question: do the tools work right now? It names the services
//! that do not, each with the open incident that explains it. Maintenance
//! work, announcements and history have their own blocks; the banner only
//! turns to maintenance when nothing is disrupted. A fresh install says
//! nothing is watched rather than claiming that everything is fine.

use askama::Template;
use async_trait::async_trait;
use chrono::Utc;

use crate::error::AppError;
use crate::i18n::I18n;
use crate::models::{EventSummary, Kind, Service, ServiceStatus, Tone};
use crate::repositories::{EventRepository, ServiceRepository};

use super::{ColumnWidth, Module, ModuleContext, ModuleRenderContext, render_template};

const UPDATE_EXCERPT_CHARS: usize = 180;

pub struct StatusBannerModule;

/// One affected service.
pub struct BannerRow {
    pub tone: &'static str,
    pub service: String,
    pub state: String,
    pub anchor: String,
    pub cause: Option<Cause>,
}

/// The open event that puts a service in its state.
pub struct Cause {
    pub event_id: i64,
    pub title: String,
    pub since: Option<String>,
    pub update: Option<String>,
    pub update_time: Option<String>,
}

impl BannerRow {
    /// The event that explains the service, or the service on this page.
    pub fn href(&self) -> String {
        match &self.cause {
            Some(cause) => format!("/events/{}", cause.event_id),
            None => format!("#{}", self.anchor),
        }
    }

    pub fn drawer_url(&self) -> Option<String> {
        self.cause
            .as_ref()
            .map(|cause| format!("/events/{}/drawer", cause.event_id))
    }
}

#[derive(Template)]
#[template(path = "modules/status_banner.html")]
struct StatusBannerTemplate {
    has_services: bool,
    tone: &'static str,
    headline: String,
    rows: Vec<BannerRow>,
    refreshed_at: String,
    can_publish: bool,
    i18n: I18n,
}

impl StatusBannerTemplate {
    /// The latest word on the work, when a single service is affected.
    fn single_update(&self) -> Option<(&str, Option<&str>)> {
        let [only] = self.rows.as_slice() else {
            return None;
        };
        let cause = only.cause.as_ref()?;
        let update = cause.update.as_deref()?;
        Some((update, cause.update_time.as_deref()))
    }
}

#[async_trait]
impl Module for StatusBannerModule {
    fn id(&self) -> &'static str {
        "status_banner"
    }

    fn name_key(&self) -> &'static str {
        "modules.status_banner.name"
    }

    fn description_key(&self) -> &'static str {
        "modules.status_banner.description"
    }

    fn contexts(&self) -> &'static [ModuleContext] {
        &[ModuleContext::Public, ModuleContext::Admin]
    }

    fn default_position(&self) -> i64 {
        10
    }

    fn column_width(&self) -> ColumnWidth {
        ColumnWidth::Full
    }

    async fn render(&self, ctx: &ModuleRenderContext<'_>) -> Result<String, AppError> {
        let services = ServiceRepository::list_all(ctx.pool).await?;
        let events = EventRepository::list_open_for_banner(ctx.pool).await?;
        let i18n = ctx.i18n;
        let affected = affected_services(&services);
        let template = StatusBannerTemplate {
            has_services: !services.is_empty(),
            tone: banner_tone(&affected).as_str(),
            headline: headline(&affected, i18n),
            rows: affected
                .iter()
                .map(|service| row(service, &events, i18n))
                .collect(),
            refreshed_at: refreshed_label(i18n),
            can_publish: ctx.can_publish(),
            i18n: i18n.clone(),
        };
        render_template(self.id(), &template)
    }
}

/// The services worth a line, worst first: the disrupted ones, or, when
/// nothing is disrupted, those under maintenance.
fn affected_services(services: &[Service]) -> Vec<&Service> {
    let mut affected: Vec<&Service> = services
        .iter()
        .filter(|s| s.status.is_disruption())
        .collect();
    if affected.is_empty() {
        affected = services
            .iter()
            .filter(|s| s.status == ServiceStatus::Maintenance)
            .collect();
    }
    affected.sort_by_key(|s| std::cmp::Reverse(s.status.priority()));
    affected
}

fn row(service: &Service, events: &[EventSummary], i18n: &I18n) -> BannerRow {
    BannerRow {
        tone: service.status.tone().as_str(),
        service: service.name.clone(),
        state: i18n.t(service.status.i18n_key()).to_string(),
        anchor: service.anchor(),
        cause: cause_of(service, events).map(|event| cause(event, i18n)),
    }
}

/// The worst incident at work on the service, or the maintenance under way
/// on it. The events come worst first.
fn cause_of<'a>(service: &Service, events: &'a [EventSummary]) -> Option<&'a EventSummary> {
    let kind = if service.status == ServiceStatus::Maintenance {
        Kind::Maintenance
    } else {
        Kind::Incident
    };
    events
        .iter()
        .find(|event| event.kind == kind && event.services().contains(&service.name.as_str()))
}

fn cause(event: &EventSummary, i18n: &I18n) -> Cause {
    Cause {
        event_id: event.id,
        title: event.title.clone(),
        since: event.elapsed_text(i18n),
        update: event.latest_update_excerpt(UPDATE_EXCERPT_CHARS),
        update_time: event.latest_update_at.map(|t| {
            i18n.tf(
                "banner.latest_update",
                &[("time", &i18n.format_datetime(&t))],
            )
        }),
    }
}

/// Ground of the banner: the worst state among the listed services.
/// Nothing tints a calm page.
fn banner_tone(affected: &[&Service]) -> Tone {
    affected
        .iter()
        .map(|s| s.status.tone())
        .max()
        .unwrap_or(Tone::Neutral)
}

fn headline(affected: &[&Service], i18n: &I18n) -> String {
    let disrupted = affected.iter().filter(|s| s.status.is_disruption()).count();
    match (disrupted, affected.is_empty()) {
        (0, true) => i18n.t("banner.all_clear").to_string(),
        (0, false) => i18n.t("banner.maintenance").to_string(),
        (n, _) => i18n.plural("banner.disrupted", n),
    }
}

/// The time the page was rendered, in the instance zone, for the reader who
/// wonders whether it is current.
fn refreshed_label(i18n: &I18n) -> String {
    i18n.tf(
        "banner.refreshed",
        &[("time", &i18n.format_time(&Utc::now()))],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Lifecycle, NAME_SEPARATOR, Severity};

    fn event(
        kind: Kind,
        severity: Option<Severity>,
        lifecycle: Lifecycle,
        services: &str,
    ) -> EventSummary {
        EventSummary {
            id: 1,
            kind,
            severity,
            planned: false,
            lifecycle: Some(lifecycle),
            category: None,
            title: "Outage".to_string(),
            description: String::new(),
            planned_start: None,
            planned_end: None,
            started_at: Some(Utc::now()),
            ended_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            author_id: 1,
            service_names: services.to_string(),
            last_activity_at: None,
            latest_update: None,
            latest_update_at: None,
        }
    }

    fn service(name: &str, status: ServiceStatus) -> Service {
        Service {
            id: 1,
            name: name.to_string(),
            slug: name.to_lowercase(),
            description: None,
            status,
            manual_status: status,
            icon_id: None,
            icon_name: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            icon_filename: None,
            has_history: false,
        }
    }

    #[test]
    fn only_disrupted_services_are_listed_worst_first() {
        let services = [
            service("Wiki", ServiceStatus::Degraded),
            service("Mail", ServiceStatus::MajorOutage),
            service("Paie", ServiceStatus::Maintenance),
            service("Intranet", ServiceStatus::Operational),
        ];
        let affected = affected_services(&services);
        let names: Vec<&str> = affected.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Mail", "Wiki"]);
        assert_eq!(banner_tone(&affected), Tone::Crit);
    }

    #[test]
    fn maintenance_shows_only_when_nothing_is_disrupted() {
        let services = [
            service("Paie", ServiceStatus::Maintenance),
            service("Mail", ServiceStatus::Operational),
        ];
        let affected = affected_services(&services);
        assert_eq!(affected.len(), 1);
        assert_eq!(banner_tone(&affected), Tone::Info);
        let i18n = I18n::new("en");
        assert_eq!(headline(&affected, &i18n), i18n.t("banner.maintenance"));
    }

    #[test]
    fn a_calm_page_stays_calm() {
        let services = [service("Mail", ServiceStatus::Operational)];
        let affected = affected_services(&services);
        assert!(affected.is_empty());
        assert_eq!(banner_tone(&affected), Tone::Neutral);
        let i18n = I18n::new("en");
        assert_eq!(headline(&affected, &i18n), i18n.t("banner.all_clear"));
    }

    #[test]
    fn the_headline_counts_disrupted_services() {
        let i18n = I18n::new("en");
        let one = [service("Mail", ServiceStatus::PartialOutage)];
        assert_eq!(
            headline(&affected_services(&one), &i18n),
            "1 service disrupted"
        );
        let two = [
            service("Mail", ServiceStatus::PartialOutage),
            service("Wiki", ServiceStatus::Degraded),
        ];
        assert_eq!(
            headline(&affected_services(&two), &i18n),
            "2 services disrupted"
        );
    }

    #[test]
    fn the_worst_open_incident_explains_a_service() {
        let mail = service("Mail", ServiceStatus::MajorOutage);
        let events = [
            event(Kind::Maintenance, None, Lifecycle::InProgress, "Mail"),
            event(
                Kind::Incident,
                Some(Severity::Critical),
                Lifecycle::Investigating,
                "Wiki",
            ),
            event(
                Kind::Incident,
                Some(Severity::Major),
                Lifecycle::InProgress,
                &format!("Mail{NAME_SEPARATOR}Wiki"),
            ),
        ];
        let found = cause_of(&mail, &events).map(|e| e.severity);
        assert_eq!(found, Some(Some(Severity::Major)));
    }

    #[test]
    fn a_service_set_down_by_hand_has_no_cause() {
        let mail = service("Mail", ServiceStatus::MajorOutage);
        let events = [event(
            Kind::Incident,
            Some(Severity::Critical),
            Lifecycle::Investigating,
            "Wiki",
        )];
        assert!(cause_of(&mail, &events).is_none());
        let row = row(&mail, &events, &I18n::new("en"));
        assert_eq!(row.href(), "#service-mail");
        assert!(row.drawer_url().is_none());
    }

    #[test]
    fn a_maintenance_explains_a_service_under_maintenance() {
        let paie = service("Paie", ServiceStatus::Maintenance);
        let events = [
            event(
                Kind::Incident,
                Some(Severity::Minor),
                Lifecycle::Investigating,
                "Paie",
            ),
            event(Kind::Maintenance, None, Lifecycle::InProgress, "Paie"),
        ];
        let found = cause_of(&paie, &events).map(|e| e.kind);
        assert_eq!(found, Some(Kind::Maintenance));
    }
}
