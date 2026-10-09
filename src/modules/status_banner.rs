//! Status banner, pinned above every dashboard.
//!
//! Answers one question: do the tools work right now? It names the services
//! that do not, each with the open incident that explains it. Maintenance
//! work, announcements and history have their own blocks; the banner only
//! turns to maintenance when nothing is disrupted. It says until when work
//! runs, since the reader's second question is when it comes back, and,
//! when nothing is disrupted, names the next announced maintenance. A fresh install says nothing is watched
//! rather than claiming that everything is fine; its administrator gets the
//! first steps instead.

use std::collections::{HashMap, HashSet};

use askama::Template;
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};

use crate::error::AppError;
use crate::i18n::I18n;
use crate::models::{EventSummary, Kind, Lifecycle, Service, ServiceStatus, Tone, split_duration};
use crate::repositories::{EventRepository, OutageRepository, ServiceRepository};

use super::{ColumnWidth, Module, ModuleRenderContext, render_template};

const UPDATE_EXCERPT_CHARS: usize = 180;
/// How far ahead an announced maintenance is worth a line.
const NEXT_MAINTENANCE_DAYS: i64 = 7;

pub struct StatusBannerModule;

/// One affected service.
pub struct BannerRow {
    pub tone: &'static str,
    pub service: String,
    pub state: String,
    pub anchor: String,
    pub cause: Option<Cause>,
    /// The checks found the service down and no incident explains it yet.
    pub detected: Option<Detected>,
}

/// An outage the checks found, before anyone declared it.
pub struct Detected {
    pub since: Option<String>,
    /// The incident form, filled in, for those who may publish.
    pub declare_href: Option<String>,
}

/// The open event that puts a service in its state.
pub struct Cause {
    pub event_id: i64,
    pub title: String,
    pub since: Option<String>,
    pub latest: Option<LatestUpdate>,
}

/// The latest word on an event, kept on one line under its service.
pub struct LatestUpdate {
    pub when: String,
    pub text: String,
}

impl BannerRow {
    /// The event that explains the service, the incident to declare for an
    /// outage the checks found, or the service on this page.
    pub fn href(&self) -> String {
        if let Some(cause) = &self.cause {
            return format!("/events/{}", cause.event_id);
        }
        self.detected
            .as_ref()
            .and_then(|detected| detected.declare_href.clone())
            .unwrap_or_else(|| format!("#{}", self.anchor))
    }

    pub fn drawer_url(&self) -> Option<String> {
        self.cause
            .as_ref()
            .map(|cause| format!("/events/{}/drawer", cause.event_id))
    }
}

/// The soonest announced maintenance, when it starts within the week.
pub struct NextMaintenance {
    pub event_id: i64,
    pub when: String,
    pub title: String,
}

#[derive(Template)]
#[template(path = "modules/status_banner.html")]
struct StatusBannerTemplate {
    has_services: bool,
    tone: &'static str,
    headline: String,
    rows: Vec<BannerRow>,
    next_maintenance: Option<NextMaintenance>,
    can_publish: bool,
    /// The page address, when the first steps are shown.
    first_steps: Option<String>,
    i18n: I18n,
}

impl StatusBannerTemplate {
    /// Everything runs and nothing is coming: the banner holds on one line.
    fn is_calm(&self) -> bool {
        self.tone == "neutral" && self.rows.is_empty() && self.next_maintenance.is_none()
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

    fn default_position(&self) -> i64 {
        10
    }

    fn column_width(&self) -> ColumnWidth {
        ColumnWidth::Full
    }

    async fn render(&self, ctx: &ModuleRenderContext<'_>) -> Result<String, AppError> {
        let services = ServiceRepository::list_all(ctx.pool).await?;
        let events = EventRepository::list_open_for_banner(ctx.pool).await?;
        let maintenance = EventRepository::list_open_maintenance(ctx.pool).await?;
        // Only a service the checks found down has an outage to date.
        let outage_starts = if services.iter().any(|s| s.detected_status.is_some()) {
            OutageRepository::open_starts(ctx.pool).await?
        } else {
            HashMap::new()
        };
        let sources = RowSources {
            events: &events,
            outage_starts: &outage_starts,
            can_publish: ctx.can_publish(),
        };
        let i18n = ctx.i18n;
        let affected = affected_services(&services);
        let mut rows: Vec<BannerRow> = affected
            .iter()
            .map(|service| row(service, &sources, i18n))
            .collect();
        keep_each_update_once(&mut rows);
        let template = StatusBannerTemplate {
            has_services: !services.is_empty(),
            tone: banner_tone(&affected).as_str(),
            headline: headline(&affected, i18n),
            rows,
            next_maintenance: next_maintenance(&services, &maintenance, i18n),
            can_publish: ctx.can_publish(),
            first_steps: first_steps(ctx, services.is_empty()),
            i18n: i18n.clone(),
        };
        render_template(self.id(), &template)
    }
}

/// An administrator on an empty instance is walked through adding services
/// and sharing the page.
fn first_steps(ctx: &ModuleRenderContext<'_>, empty: bool) -> Option<String> {
    let admin = ctx.user.is_some_and(|u| u.role.can_admin());
    (empty && admin).then(|| ctx.page_address.to_string())
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

/// What a row draws on besides the service itself.
struct RowSources<'a> {
    events: &'a [EventSummary],
    outage_starts: &'a HashMap<i64, DateTime<Utc>>,
    can_publish: bool,
}

fn row(service: &Service, sources: &RowSources<'_>, i18n: &I18n) -> BannerRow {
    let cause = cause_of(service, sources.events).map(|event| cause(event, i18n));
    let detected = (cause.is_none() && service.detected_status.is_some()).then(|| Detected {
        since: sources.outage_starts.get(&service.id).map(|start| {
            let parts = split_duration(Utc::now() - *start);
            i18n.tf(
                "banner.detected_for",
                &[("duration", &i18n.format_duration(&parts))],
            )
        }),
        declare_href: sources
            .can_publish
            .then(|| format!("/events/new?service={}", service.id)),
    });
    BannerRow {
        tone: service.status.tone().as_str(),
        service: service.name.clone(),
        state: i18n.t(service.status.i18n_key()).to_string(),
        anchor: service.anchor(),
        cause,
        detected,
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
        since: until_text(event, i18n).or_else(|| event.elapsed_text(i18n)),
        latest: event
            .latest_update_excerpt(UPDATE_EXCERPT_CHARS)
            .zip(event.latest_update_at)
            .map(|(text, at)| LatestUpdate {
                when: i18n.format_ago(&at),
                text,
            }),
    }
}

/// An incident on several services gives its latest word once, on its
/// first row, so the banner never repeats itself.
fn keep_each_update_once(rows: &mut [BannerRow]) {
    let mut told = HashSet::new();
    for cause in rows.iter_mut().filter_map(|row| row.cause.as_mut()) {
        if !told.insert(cause.event_id) {
            cause.latest = None;
        }
    }
}

/// The time alone today, the date and time otherwise.
fn short_when(at: &DateTime<Utc>, i18n: &I18n) -> String {
    if crate::clock::local_date(at) == crate::clock::today() {
        i18n.format_time(at)
    } else {
        i18n.format_datetime(at)
    }
}

/// "until 18:00" for work with an announced end: what the reader waits for,
/// rather than how long it has lasted.
fn until_text(event: &EventSummary, i18n: &I18n) -> Option<String> {
    if event.kind != Kind::Maintenance {
        return None;
    }
    let end = event.planned_end?;
    Some(i18n.tf("maintenance.until", &[("when", &short_when(&end, i18n))]))
}

/// The soonest maintenance still to come within the week. The list comes
/// work under way first, then by start. A disruption keeps the banner on
/// itself; the Maintenance block still lists what is coming.
fn next_maintenance(
    services: &[Service],
    open: &[EventSummary],
    i18n: &I18n,
) -> Option<NextMaintenance> {
    if services.iter().any(|s| s.status.is_disruption()) {
        return None;
    }
    let horizon = Utc::now() + Duration::days(NEXT_MAINTENANCE_DAYS);
    open.iter()
        .filter(|m| m.lifecycle == Some(Lifecycle::Scheduled))
        .find_map(|m| {
            let start = m.planned_start.filter(|start| *start <= horizon)?;
            Some(NextMaintenance {
                event_id: m.id,
                when: i18n.tf(
                    "banner.next_maintenance",
                    &[("when", &i18n.format_datetime(&start))],
                ),
                title: m.title.clone(),
            })
        })
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
            keeps_services_up: false,
            author_id: 1,
            service_names: services.to_string(),
            service_icons: String::new(),
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
            check_kind: None,
            check_target: None,
            check_internal_cert: false,
            detected_status: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            icon_filename: None,
            has_history: false,
        }
    }

    fn sources(events: &[EventSummary], can_publish: bool) -> RowSources<'_> {
        static NO_OUTAGES: std::sync::LazyLock<HashMap<i64, DateTime<Utc>>> =
            std::sync::LazyLock::new(HashMap::new);
        RowSources {
            events,
            outage_starts: &NO_OUTAGES,
            can_publish,
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
        let one = [service("Mail", ServiceStatus::MajorOutage)];
        assert_eq!(
            headline(&affected_services(&one), &i18n),
            "1 service disrupted"
        );
        let two = [
            service("Mail", ServiceStatus::MajorOutage),
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
                Some(Severity::Minor),
                Lifecycle::InProgress,
                &format!("Mail{NAME_SEPARATOR}Wiki"),
            ),
        ];
        let found = cause_of(&mail, &events).map(|e| e.severity);
        assert_eq!(found, Some(Some(Severity::Minor)));
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
        let row = row(&mail, &sources(&events, false), &I18n::new("en"));
        assert_eq!(row.href(), "#service-mail");
        assert!(row.drawer_url().is_none());
        assert!(row.detected.is_none());
    }

    #[test]
    fn an_outage_the_checks_found_says_so_until_an_incident_explains_it() {
        let mut mail = service("Mail", ServiceStatus::MajorOutage);
        mail.detected_status = Some(ServiceStatus::MajorOutage);
        let i18n = I18n::new("en");

        let visitor = row(&mail, &sources(&[], false), &i18n);
        assert!(visitor.detected.is_some());
        assert_eq!(visitor.href(), "#service-mail");

        let editor = row(&mail, &sources(&[], true), &i18n);
        assert_eq!(editor.href(), "/events/new?service=1");

        let events = [event(
            Kind::Incident,
            Some(Severity::Critical),
            Lifecycle::Investigating,
            "Mail",
        )];
        let explained = row(&mail, &sources(&events, true), &i18n);
        assert!(explained.detected.is_none());
        assert_eq!(explained.href(), "/events/1");
    }

    #[test]
    fn an_incident_on_several_services_gives_its_latest_word_once() {
        let mut outage = event(
            Kind::Incident,
            Some(Severity::Critical),
            Lifecycle::Investigating,
            &format!("Mail{NAME_SEPARATOR}Wiki"),
        );
        outage.latest_update = Some("<p>Supplier on it</p>".to_string());
        outage.latest_update_at = Some(Utc::now());
        let events = [outage];
        let i18n = I18n::new("en");
        let mut rows: Vec<BannerRow> = [
            service("Mail", ServiceStatus::MajorOutage),
            service("Wiki", ServiceStatus::Degraded),
        ]
        .iter()
        .map(|s| row(s, &sources(&events, false), &i18n))
        .collect();
        keep_each_update_once(&mut rows);
        let texts: Vec<Option<&str>> = rows
            .iter()
            .map(|row| {
                let latest = row.cause.as_ref()?.latest.as_ref()?;
                Some(latest.text.as_str())
            })
            .collect();
        assert_eq!(texts, [Some("Supplier on it"), None]);
    }

    #[test]
    fn the_latest_update_says_how_long_ago_it_was_posted() {
        let mut outage = event(
            Kind::Incident,
            Some(Severity::Critical),
            Lifecycle::Investigating,
            "Mail",
        );
        outage.latest_update = Some("<p>Supplier on it</p>".to_string());
        outage.latest_update_at = Some(Utc::now() - Duration::hours(2));
        let found = cause(&outage, &I18n::new("en"));
        let when = found.latest.map(|latest| latest.when);
        assert_eq!(when.as_deref(), Some("2h ago"));
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

    #[test]
    fn the_next_maintenance_is_the_soonest_within_the_week() {
        let i18n = I18n::new("en");
        let at = |days: i64, title: &str| {
            let mut m = event(Kind::Maintenance, None, Lifecycle::Scheduled, "Paie");
            m.title = title.to_string();
            m.planned_start = Some(Utc::now() + Duration::days(days));
            m
        };
        let calm = [service("Paie", ServiceStatus::Operational)];
        let open = [at(2, "Soon"), at(3, "Later")];
        let next = next_maintenance(&calm, &open, &i18n).map(|m| m.title);
        assert_eq!(next.as_deref(), Some("Soon"));
        assert!(next_maintenance(&calm, &[at(9, "Far")], &i18n).is_none());
        let disrupted = [service("Mail", ServiceStatus::Degraded)];
        assert!(
            next_maintenance(&disrupted, &open, &i18n).is_none(),
            "a disruption keeps the banner on itself"
        );
    }

    #[test]
    fn work_with_an_announced_end_says_until_when() {
        let i18n = I18n::new("en");
        let mut work = event(Kind::Maintenance, None, Lifecycle::InProgress, "Paie");
        assert!(until_text(&work, &i18n).is_none());
        work.planned_end = Some(Utc::now() + Duration::days(2));
        assert!(until_text(&work, &i18n).is_some());
        let incident = event(Kind::Incident, None, Lifecycle::Investigating, "Paie");
        assert!(until_text(&incident, &i18n).is_none());
    }
}
