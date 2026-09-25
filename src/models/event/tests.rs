use chrono::Utc;

use super::*;
use crate::i18n::I18n;
use crate::models::ServiceStatus;

#[test]
fn incidents_follow_their_severity() {
    assert_eq!(
        derive_status(Kind::Incident, Some(Severity::Critical)),
        Some(ServiceStatus::MajorOutage)
    );
    assert_eq!(
        derive_status(Kind::Incident, Some(Severity::Minor)),
        Some(ServiceStatus::Degraded)
    );
}

#[test]
fn an_incident_without_severity_still_counts() {
    assert_eq!(
        derive_status(Kind::Incident, None),
        Some(ServiceStatus::Degraded)
    );
}

#[test]
fn maintenance_and_announcements() {
    assert_eq!(
        derive_status(Kind::Maintenance, Some(Severity::Critical)),
        Some(ServiceStatus::Maintenance)
    );
    assert_eq!(derive_status(Kind::Publication, None), None);
}

#[test]
fn only_work_under_way_says_what_it_does_to_its_services() {
    let under_way = Some(Lifecycle::InProgress);
    assert!(drives_services(Kind::Incident, under_way, false));
    assert!(!drives_services(
        Kind::Incident,
        Some(Lifecycle::Monitoring),
        false
    ));
    assert!(drives_services(Kind::Maintenance, under_way, false));
    assert!(!drives_services(Kind::Maintenance, under_way, true));
    assert!(!drives_services(
        Kind::Maintenance,
        Some(Lifecycle::Scheduled),
        false
    ));
    assert!(!drives_services(Kind::Publication, None, false));
}

fn summary(kind: Kind, severity: Option<Severity>, lifecycle: Option<Lifecycle>) -> EventSummary {
    EventSummary {
        id: 1,
        kind,
        severity,
        planned: false,
        lifecycle,
        category: None,
        title: "t".to_string(),
        description: String::new(),
        planned_start: None,
        planned_end: None,
        started_at: None,
        ended_at: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        keeps_services_up: false,
        author_id: 1,
        service_names: String::new(),
        service_icons: String::new(),
        last_activity_at: None,
        latest_update: None,
        latest_update_at: None,
    }
}

fn input(kind: Kind, planned: bool) -> CreateEventInput {
    CreateEventInput {
        kind,
        severity: None,
        planned,
        category: None,
        title: String::new(),
        description: String::new(),
        planned_start: None,
        planned_end: None,
        started_at: None,
        opening_step: None,
        keeps_services_up: false,
        service_ids: vec![],
        follows_event_id: None,
        author_id: 1,
    }
}

#[test]
fn terminal_lifecycles() {
    assert!(Lifecycle::Resolved.is_terminal());
    assert!(Lifecycle::Cancelled.is_terminal());
    assert!(Lifecycle::Completed.is_terminal());
    assert!(!Lifecycle::Investigating.is_terminal());
    assert!(!Lifecycle::Scheduled.is_terminal());
}

#[test]
fn incident_transitions_from_investigating() {
    let t = Kind::Incident.allowed_transitions(Lifecycle::Investigating);
    assert!(t.contains(&Lifecycle::InProgress));
    assert!(t.contains(&Lifecycle::Monitoring));
    assert!(t.contains(&Lifecycle::Resolved));
    assert!(t.contains(&Lifecycle::Cancelled));
}

#[test]
fn maintenance_can_finish_straight_from_scheduled() {
    let t = Kind::Maintenance.allowed_transitions(Lifecycle::Scheduled);
    assert_eq!(
        t,
        &[
            Lifecycle::InProgress,
            Lifecycle::Completed,
            Lifecycle::Cancelled
        ]
    );
}

#[test]
fn no_transitions_from_terminal() {
    for kind in [Kind::Incident, Kind::Maintenance] {
        for terminal in [
            Lifecycle::Resolved,
            Lifecycle::Cancelled,
            Lifecycle::Completed,
        ] {
            assert!(
                kind.allowed_transitions(terminal).is_empty(),
                "{kind:?} + {terminal:?}"
            );
        }
    }
}

#[test]
fn cross_kind_transitions_are_rejected() {
    assert!(
        Kind::Incident
            .allowed_transitions(Lifecycle::Scheduled)
            .is_empty()
    );
    assert!(
        Kind::Maintenance
            .allowed_transitions(Lifecycle::Investigating)
            .is_empty()
    );
}

#[test]
fn labels_follow_the_kind() {
    assert_eq!(
        Lifecycle::InProgress.label_key(Kind::Maintenance),
        "lifecycle.maintenance.in_progress"
    );
    assert_eq!(
        Lifecycle::InProgress.label_key(Kind::Incident),
        "lifecycle.in_progress"
    );
    assert_eq!(
        Lifecycle::Cancelled.label_key(Kind::Maintenance),
        "lifecycle.maintenance.cancelled"
    );
}

#[test]
fn tone_carries_the_severity_only_while_work_is_open() {
    let open = summary(
        Kind::Incident,
        Some(Severity::Critical),
        Some(Lifecycle::Investigating),
    );
    assert_eq!(open.tone(), Tone::Crit);
    let watched = summary(
        Kind::Incident,
        Some(Severity::Critical),
        Some(Lifecycle::Monitoring),
    );
    assert_eq!(watched.tone(), Tone::Ok);
    let planned = summary(Kind::Maintenance, None, Some(Lifecycle::Scheduled));
    assert_eq!(planned.tone(), Tone::Info);
    let note = summary(Kind::Publication, None, None);
    assert_eq!(note.tone(), Tone::Ink);
}

#[test]
fn only_an_incident_says_its_severity_and_an_announcement_its_category() {
    let i18n = I18n::new("en");
    let work = summary(
        Kind::Maintenance,
        Some(Severity::Minor),
        Some(Lifecycle::Completed),
    );
    assert_eq!(work.chip_label(&i18n), "Maintenance");
    let outage = summary(Kind::Incident, Some(Severity::Critical), None);
    assert_eq!(outage.chip_label(&i18n), "Major incident");
    let mut note = summary(Kind::Publication, None, None);
    note.category = Some(Category::Changelog);
    assert!(note.row_state(&i18n).is_none());
    assert_eq!(
        note.chip_label(&i18n),
        i18n.t(Category::Changelog.i18n_key())
    );
    assert_eq!(note.tone(), Tone::Ink);
}

#[test]
fn each_service_keeps_its_own_icon() {
    let mut event = summary(Kind::Incident, None, None);
    event.service_names = format!("Mail{NAME_SEPARATOR}Payroll");
    event.service_icons = format!("mail{ICON_SEPARATOR}{NAME_SEPARATOR}{ICON_SEPARATOR}logo.png");
    let tags = event.service_tags(&I18n::new("en"));
    assert_eq!(tags.len(), 2);
    assert!(tags[0].icon_url().is_none());
    assert_eq!(
        tags[1].icon_url().as_deref(),
        Some("/uploads/icons/logo.png")
    );
    assert!(tags[1].builtin_icon_paths().is_none());
}

#[test]
fn a_tag_says_what_the_open_incident_does_to_its_service() {
    let i18n = I18n::new("en");
    let mut event = summary(
        Kind::Incident,
        Some(Severity::Critical),
        Some(Lifecycle::Investigating),
    );
    event.service_names = "Mail".to_string();
    assert_eq!(
        event.service_tags(&i18n)[0].effect.as_deref(),
        Some(
            i18n.t(ServiceStatus::MajorOutage.i18n_key())
                .to_lowercase()
                .as_str()
        )
    );
    event.lifecycle = Some(Lifecycle::Resolved);
    assert!(event.service_tags(&i18n)[0].effect.is_none());
}

#[test]
fn services_split_on_the_separator_only() {
    let mut event = summary(Kind::Incident, None, None);
    event.service_names = format!("Mail, Calendar{NAME_SEPARATOR}Payroll");
    assert_eq!(event.services(), vec!["Mail, Calendar", "Payroll"]);
    assert_eq!(event.services_label(), "Mail, Calendar, Payroll");
}

#[test]
fn excerpt_counts_characters() {
    let mut event = summary(Kind::Incident, None, None);
    event.description = "é".repeat(150);
}

#[test]
fn update_excerpt_is_plain_text() {
    let mut event = summary(Kind::Incident, None, None);
    event.latest_update = Some("<p>Fix <strong>deployed</strong> &amp; watched</p>".to_string());
    assert_eq!(
        event.latest_update_excerpt(80).as_deref(),
        Some("Fix deployed & watched")
    );
}

#[test]
fn scheduled_maintenance_is_not_elapsed() {
    let event = Event {
        id: 1,
        kind: Kind::Maintenance,
        severity: None,
        planned: true,
        lifecycle: Some(Lifecycle::Scheduled),
        category: None,
        title: String::new(),
        description: String::new(),
        planned_start: Some(Utc::now() + chrono::Duration::days(3)),
        planned_end: None,
        started_at: None,
        ended_at: None,
        restored_at: None,
        follows_event_id: None,
        author_id: 1,
        previous_lifecycle: None,
        keeps_services_up: false,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    assert!(event.elapsed().is_none());
    assert!(event.countdown().is_some());
    let due = Event {
        planned_start: Some(Utc::now() - chrono::Duration::minutes(1)),
        ..event
    };
    assert!(due.countdown().is_none());
}

#[test]
fn initial_states() {
    assert_eq!(
        input(Kind::Incident, false).initial_lifecycle(),
        Some(Lifecycle::Investigating)
    );
    assert_eq!(
        input(Kind::Maintenance, true).initial_lifecycle(),
        Some(Lifecycle::Scheduled)
    );
    assert_eq!(
        input(Kind::Maintenance, false).initial_lifecycle(),
        Some(Lifecycle::InProgress)
    );
    assert!(
        input(Kind::Publication, false)
            .initial_lifecycle()
            .is_none()
    );
}

#[test]
fn an_incident_opens_at_the_step_its_author_chose_before_closing() {
    let at = |step| CreateEventInput {
        opening_step: Some(step),
        ..input(Kind::Incident, false)
    };
    assert_eq!(
        at(Lifecycle::InProgress).initial_lifecycle(),
        Some(Lifecycle::InProgress)
    );
    assert_eq!(
        at(Lifecycle::Monitoring).initial_lifecycle(),
        Some(Lifecycle::Monitoring)
    );
    assert_eq!(
        at(Lifecycle::Resolved).initial_lifecycle(),
        Some(Lifecycle::Investigating)
    );
    let maintenance = CreateEventInput {
        opening_step: Some(Lifecycle::Monitoring),
        ..input(Kind::Maintenance, false)
    };
    assert_eq!(maintenance.initial_lifecycle(), Some(Lifecycle::InProgress));
}

#[test]
fn only_announced_maintenance_waits_to_start() {
    assert!(
        input(Kind::Maintenance, true)
            .initial_started_at()
            .is_none()
    );
    assert!(input(Kind::Incident, false).initial_started_at().is_some());
    assert!(
        input(Kind::Publication, false)
            .initial_started_at()
            .is_some()
    );
}
