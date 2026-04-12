//! Event model and related enums (`EventType`, `EventStatus`, `Impact`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::Service;

/// Type of event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type, Serialize, Deserialize)]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    Incident,
    MaintenanceScheduled,
    MaintenanceUrgent,
    Changelog,
    Info,
}

/// Current status of an event in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type, Serialize, Deserialize)]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum EventStatus {
    Scheduled,
    Investigating,
    Identified,
    InProgress,
    Monitoring,
    Resolved,
    Cancelled,
}

/// Impact level of an event on affected services.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type, Serialize, Deserialize)]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum Impact {
    None,
    Minor,
    Major,
    Critical,
}

impl EventStatus {
    /// Returns `true` if the event is still active (not terminal).
    pub fn is_active(self) -> bool {
        !matches!(self, Self::Resolved | Self::Cancelled)
    }

    /// Returns the list of statuses this status can transition to.
    pub fn allowed_transitions(self) -> &'static [Self] {
        match self {
            Self::Scheduled => &[Self::InProgress, Self::Cancelled],
            Self::Investigating => &[
                Self::Identified,
                Self::InProgress,
                Self::Monitoring,
                Self::Resolved,
            ],
            Self::Identified => &[Self::InProgress, Self::Monitoring, Self::Resolved],
            Self::InProgress => &[Self::Monitoring, Self::Resolved],
            Self::Monitoring => &[Self::Resolved, Self::InProgress],
            Self::Resolved | Self::Cancelled => &[],
        }
    }

    /// Check whether transitioning to `target` is allowed.
    pub fn can_transition_to(self, target: Self) -> bool {
        self.allowed_transitions().contains(&target)
    }

    /// Human-readable label in French.
    pub fn label(self) -> &'static str {
        match self {
            Self::Scheduled => "Planifié",
            Self::Investigating => "Investigation",
            Self::Identified => "Identifié",
            Self::InProgress => "En cours",
            Self::Monitoring => "Surveillance",
            Self::Resolved => "Résolu",
            Self::Cancelled => "Annulé",
        }
    }

    /// Translation key for i18n.
    pub fn i18n_key(self) -> &'static str {
        match self {
            Self::Scheduled => "status.event.scheduled",
            Self::Investigating => "status.event.investigating",
            Self::Identified => "status.event.identified",
            Self::InProgress => "status.event.in_progress",
            Self::Monitoring => "status.event.monitoring",
            Self::Resolved => "status.event.resolved",
            Self::Cancelled => "status.event.cancelled",
        }
    }

    /// Tailwind CSS class for the status badge.
    pub fn css_class(self) -> &'static str {
        match self {
            Self::Scheduled => "badge-scheduled",
            Self::Investigating => "badge-investigating",
            Self::Identified => "badge-identified",
            Self::InProgress => "badge-in-progress",
            Self::Monitoring => "badge-monitoring",
            Self::Resolved => "badge-resolved",
            Self::Cancelled => "badge-cancelled",
        }
    }

    /// Short identifier for serialization in form values.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Scheduled => "scheduled",
            Self::Investigating => "investigating",
            Self::Identified => "identified",
            Self::InProgress => "in_progress",
            Self::Monitoring => "monitoring",
            Self::Resolved => "resolved",
            Self::Cancelled => "cancelled",
        }
    }

    /// User-facing i18n key, merges "Identified" into "In Progress" visually.
    /// Users don't need to distinguish between "root cause found" and "working on it".
    pub fn display_i18n_key(self) -> &'static str {
        match self {
            Self::Identified => Self::InProgress.i18n_key(),
            other => other.i18n_key(),
        }
    }

    /// User-facing CSS class, merges "Identified" into "In Progress" visually.
    pub fn display_css_class(self) -> &'static str {
        match self {
            Self::Identified => Self::InProgress.css_class(),
            other => other.css_class(),
        }
    }
}

impl EventType {
    /// Human-readable label in French.
    pub fn label(self) -> &'static str {
        match self {
            Self::Incident => "Incident",
            Self::MaintenanceScheduled => "Maintenance planifiée",
            Self::MaintenanceUrgent => "Maintenance urgente",
            Self::Changelog => "Changement",
            Self::Info => "Information",
        }
    }

    /// Translation key for i18n.
    pub fn i18n_key(self) -> &'static str {
        match self {
            Self::Incident => "type.event.incident",
            Self::MaintenanceScheduled => "type.event.maintenance_scheduled",
            Self::MaintenanceUrgent => "type.event.maintenance_urgent",
            Self::Changelog => "type.event.changelog",
            Self::Info => "type.event.info",
        }
    }

    /// Short identifier for serialization in form values.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Incident => "incident",
            Self::MaintenanceScheduled => "maintenance_scheduled",
            Self::MaintenanceUrgent => "maintenance_urgent",
            Self::Changelog => "changelog",
            Self::Info => "info",
        }
    }

    /// Whether this event type has a meaningful impact level to display.
    /// Only incidents show impact, maintenance IS its own impact,
    /// and publications have no impact at all.
    pub fn shows_impact(self) -> bool {
        matches!(self, Self::Incident)
    }

    /// Whether this event type has a lifecycle with status transitions.
    /// Changelog and Info are auto-resolved, no lifecycle to show.
    pub fn shows_lifecycle(self) -> bool {
        !matches!(self, Self::Changelog | Self::Info)
    }

    /// Left border accent class for event cards, based on family + impact.
    /// Incidents: colored by severity. Maintenance: blue. Publications: neutral.
    pub fn card_accent_class(self, impact: &Impact) -> &'static str {
        match self {
            Self::Incident => match *impact {
                Impact::Critical => "border-l-red-500 dark:border-l-red-400",
                Impact::Major => "border-l-orange-500 dark:border-l-orange-400",
                Impact::Minor => "border-l-yellow-600 dark:border-l-yellow-500",
                Impact::None => "border-l-stone-300 dark:border-l-stone-600",
            },
            Self::MaintenanceScheduled | Self::MaintenanceUrgent => {
                "border-l-blue-500 dark:border-l-blue-400"
            }
            Self::Changelog => "border-l-emerald-500/60 dark:border-l-emerald-500/40",
            Self::Info => "border-l-stone-300 dark:border-l-stone-600",
        }
    }

    /// Left border strip class with variable width, visual weight = severity.
    /// Active incidents get a thick strip, maintenance a thin one, publications none.
    pub fn card_strip_class(self, impact: &Impact, status: &EventStatus) -> &'static str {
        match self {
            Self::Incident => {
                if status.is_active() {
                    match *impact {
                        Impact::Critical => "border-l-4 border-l-red-500 dark:border-l-red-400",
                        Impact::Major => "border-l-4 border-l-orange-500 dark:border-l-orange-400",
                        Impact::Minor => "border-l-4 border-l-yellow-600 dark:border-l-yellow-500",
                        Impact::None => "border-l-4 border-l-stone-400 dark:border-l-stone-500",
                    }
                } else {
                    "border-l-2 border-l-stone-300 dark:border-l-stone-600"
                }
            }
            Self::MaintenanceScheduled | Self::MaintenanceUrgent => {
                if status.is_active() {
                    "border-l-2 border-l-blue-400 dark:border-l-blue-500"
                } else {
                    "border-l-2 border-l-stone-300 dark:border-l-stone-600"
                }
            }
            Self::Changelog | Self::Info => "border-l-0",
        }
    }

    /// CSS class for the top status strip on event cards.
    pub fn strip_class(self) -> &'static str {
        match self {
            Self::Incident => "strip-major",
            Self::MaintenanceScheduled => "strip-maintenance",
            Self::MaintenanceUrgent => "strip-partial",
            Self::Changelog => "strip-operational",
            Self::Info => "strip-degraded",
        }
    }

    /// Tailwind CSS classes for the event type indicator dot.
    pub fn dot_class(self) -> &'static str {
        match self {
            Self::Incident => "bg-red-500",
            Self::MaintenanceScheduled => "bg-blue-500",
            Self::MaintenanceUrgent => "bg-orange-500",
            Self::Changelog => "bg-emerald-500",
            Self::Info => "bg-slate-400 dark:bg-slate-500",
        }
    }

    /// Tailwind CSS classes for the colored top border on event cards.
    pub fn border_class(self) -> &'static str {
        match self {
            Self::Incident => "border-t-red-500 dark:border-t-red-400",
            Self::MaintenanceScheduled => "border-t-blue-500 dark:border-t-blue-400",
            Self::MaintenanceUrgent => "border-t-orange-500 dark:border-t-orange-400",
            Self::Changelog => "border-t-emerald-500 dark:border-t-emerald-400",
            Self::Info => "border-t-slate-300 dark:border-t-slate-600",
        }
    }
}

impl Impact {
    /// Human-readable label in French.
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Aucun",
            Self::Minor => "Mineur",
            Self::Major => "Majeur",
            Self::Critical => "Critique",
        }
    }

    /// Translation key for i18n.
    pub fn i18n_key(self) -> &'static str {
        match self {
            Self::None => "impact.none",
            Self::Minor => "impact.minor",
            Self::Major => "impact.major",
            Self::Critical => "impact.critical",
        }
    }

    /// Tailwind CSS class for the impact text color.
    pub fn css_class(self) -> &'static str {
        match self {
            Self::None => "text-impact-none",
            Self::Minor => "text-impact-minor",
            Self::Major => "text-impact-major",
            Self::Critical => "text-impact-critical",
        }
    }

    /// Tailwind CSS background class for a small colored dot.
    pub fn dot_class(self) -> &'static str {
        match self {
            Self::None => "bg-stone-400 dark:bg-stone-500",
            Self::Minor => "bg-yellow-500 dark:bg-yellow-400",
            Self::Major => "bg-orange-500 dark:bg-orange-400",
            Self::Critical => "bg-red-500 dark:bg-red-400",
        }
    }

    /// CSS component class for the impact badge.
    pub fn badge_class(self) -> &'static str {
        match self {
            Self::None => "badge-impact-none",
            Self::Minor => "badge-impact-minor",
            Self::Major => "badge-impact-major",
            Self::Critical => "badge-impact-critical",
        }
    }

    /// Short identifier for serialization in form values.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Minor => "minor",
            Self::Major => "major",
            Self::Critical => "critical",
        }
    }

    /// Numeric severity level (0=none, 1=minor, 2=major, 3=critical).
    pub fn level(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Minor => 1,
            Self::Major => 2,
            Self::Critical => 3,
        }
    }
}

/// An event (incident, maintenance, changelog, info).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Event {
    pub id: i64,
    pub event_type: EventType,
    pub status: EventStatus,
    pub title: String,
    pub description: String,
    pub impact: Impact,
    pub scheduled_start: Option<DateTime<Utc>>,
    pub scheduled_end: Option<DateTime<Utc>>,
    pub actual_start: Option<DateTime<Utc>>,
    pub actual_end: Option<DateTime<Utc>>,
    pub icon_id: Option<i64>,
    pub author_id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub previous_status: Option<EventStatus>,
}

/// A status update posted on an event.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EventUpdate {
    pub id: i64,
    pub event_id: i64,
    pub message: String,
    pub author_id: i64,
    pub created_at: DateTime<Utc>,
}

/// Event with its associated services (for detail pages).
#[derive(Debug, Clone)]
pub struct EventWithServices {
    pub event: Event,
    pub services: Vec<Service>,
}

/// Lightweight event projection for lists and dashboards.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct EventSummary {
    pub id: i64,
    pub event_type: EventType,
    pub status: EventStatus,
    pub title: String,
    /// Raw description text (markdown source).
    #[sqlx(default)]
    pub description: String,
    pub impact: Impact,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub author_id: i64,
    /// Comma-separated list of affected service names (from `GROUP_CONCAT`).
    #[sqlx(default)]
    pub service_names: String,
    /// Icon filename from JOIN with icons table (not always populated).
    #[sqlx(default)]
    #[serde(skip)]
    pub icon_filename: Option<String>,
    /// Scheduled start date (only populated for maintenance queries).
    #[sqlx(default)]
    pub scheduled_start: Option<DateTime<Utc>>,
}

impl EventSummary {
    /// Returns the individual service names as a vector.
    pub fn services(&self) -> Vec<&str> {
        if self.service_names.is_empty() {
            Vec::new()
        } else {
            self.service_names.split(", ").collect()
        }
    }

    /// First line of the description, truncated to `max_len` characters.
    pub fn description_excerpt(&self, max_len: usize) -> String {
        let line = self.description.lines().next().unwrap_or("");
        // Strip leading markdown markers (# headers, list items, etc.)
        let clean = line.trim_start_matches(['#', '-', '*', '>']).trim();
        if clean.len() <= max_len {
            clean.to_string()
        } else {
            let truncated: String = clean.chars().take(max_len).collect();
            format!("{truncated}…")
        }
    }

    /// URL to the event icon (if any).
    pub fn icon_url(&self) -> Option<String> {
        self.icon_filename
            .as_ref()
            .map(|f| format!("/uploads/icons/{f}"))
    }

    /// Human-readable countdown until `scheduled_start` (e.g. "dans 3j 2h").
    pub fn countdown(&self) -> Option<String> {
        let start = self.scheduled_start?;
        let now = Utc::now();
        if start <= now {
            return Some("Maintenant".to_string());
        }
        let diff = start - now;
        let days = diff.num_days();
        let hours = diff.num_hours() % 24;
        let minutes = diff.num_minutes() % 60;
        if days > 0 {
            Some(format!("{days}j {hours}h"))
        } else if hours > 0 {
            Some(format!("{hours}h {minutes:02}min"))
        } else {
            Some(format!("{minutes}min"))
        }
    }

    /// Countdown components until `scheduled_start`: (days, hours, minutes).
    /// Returns `None` if no scheduled start, `Some(None)` if already started,
    /// `Some(Some((days, hours, minutes)))` if in the future.
    pub fn countdown_parts(&self) -> Option<Option<(i64, i64, i64)>> {
        let start = self.scheduled_start?;
        let now = Utc::now();
        if start <= now {
            return Some(None); // Already started
        }
        let diff = start - now;
        let days = diff.num_days();
        let hours = diff.num_hours() % 24;
        let minutes = diff.num_minutes() % 60;
        Some(Some((days, hours, minutes)))
    }
}

/// Full event detail with services and author name (for detail template).
#[derive(Debug, Clone)]
pub struct EventWithDetails {
    pub event: Event,
    pub services: Vec<Service>,
    pub author_name: String,
}

/// An event update enriched with the author display name.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EventUpdateWithAuthor {
    pub id: i64,
    pub event_id: i64,
    pub message: String,
    pub author_id: i64,
    pub created_at: DateTime<Utc>,
    pub author_name: String,
}

/// Input for creating a new event.
#[derive(Debug)]
pub struct CreateEventInput {
    pub event_type: EventType,
    pub title: String,
    pub description: String,
    pub impact: Impact,
    pub scheduled_start: Option<DateTime<Utc>>,
    pub scheduled_end: Option<DateTime<Utc>>,
    pub service_ids: Vec<i64>,
    pub icon_id: Option<i64>,
    pub author_id: i64,
}

impl CreateEventInput {
    /// Determine the initial status based on the event type.
    pub fn initial_status(&self) -> EventStatus {
        match self.event_type {
            EventType::MaintenanceScheduled => EventStatus::Scheduled,
            EventType::Incident | EventType::MaintenanceUrgent => EventStatus::Investigating,
            EventType::Changelog | EventType::Info => EventStatus::Resolved,
        }
    }

    /// Determine `actual_start`, immediate for non-scheduled types.
    pub fn actual_start(&self) -> Option<DateTime<Utc>> {
        match self.event_type {
            EventType::MaintenanceScheduled => None,
            _ => Some(Utc::now()),
        }
    }
}

/// Filters for listing events.
#[derive(Debug, Default)]
pub struct EventFilters {
    pub event_type: Option<EventType>,
    pub status: Option<EventStatus>,
    pub service_id: Option<i64>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub limit: i64,
    pub offset: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_statuses_are_not_terminal() {
        let active = [
            EventStatus::Scheduled,
            EventStatus::Investigating,
            EventStatus::Identified,
            EventStatus::InProgress,
            EventStatus::Monitoring,
        ];
        for s in active {
            assert!(s.is_active(), "{s:?} should be active");
        }
    }

    #[test]
    fn resolved_and_cancelled_are_terminal() {
        assert!(!EventStatus::Resolved.is_active());
        assert!(!EventStatus::Cancelled.is_active());
    }

    #[test]
    fn terminal_statuses_have_no_transitions() {
        assert!(EventStatus::Resolved.allowed_transitions().is_empty());
        assert!(EventStatus::Cancelled.allowed_transitions().is_empty());
    }

    #[test]
    fn scheduled_can_transition_to_in_progress_or_cancelled() {
        let transitions = EventStatus::Scheduled.allowed_transitions();
        assert!(transitions.contains(&EventStatus::InProgress));
        assert!(transitions.contains(&EventStatus::Cancelled));
        assert_eq!(transitions.len(), 2);
    }

    #[test]
    fn investigating_transitions() {
        let transitions = EventStatus::Investigating.allowed_transitions();
        assert!(transitions.contains(&EventStatus::Identified));
        assert!(transitions.contains(&EventStatus::InProgress));
        assert!(transitions.contains(&EventStatus::Monitoring));
        assert!(transitions.contains(&EventStatus::Resolved));
        assert!(!transitions.contains(&EventStatus::Scheduled));
        assert!(!transitions.contains(&EventStatus::Cancelled));
    }

    #[test]
    fn can_transition_to_agrees_with_allowed_transitions() {
        let all = [
            EventStatus::Scheduled,
            EventStatus::Investigating,
            EventStatus::Identified,
            EventStatus::InProgress,
            EventStatus::Monitoring,
            EventStatus::Resolved,
            EventStatus::Cancelled,
        ];
        for from in all {
            for to in all {
                assert_eq!(
                    from.can_transition_to(to),
                    from.allowed_transitions().contains(&to),
                    "Mismatch for {from:?} -> {to:?}"
                );
            }
        }
    }

    #[test]
    fn all_event_statuses_have_labels_and_css() {
        let all = [
            EventStatus::Scheduled,
            EventStatus::Investigating,
            EventStatus::Identified,
            EventStatus::InProgress,
            EventStatus::Monitoring,
            EventStatus::Resolved,
            EventStatus::Cancelled,
        ];
        for s in all {
            assert!(!s.label().is_empty());
            assert!(!s.css_class().is_empty());
            assert!(!s.as_str().is_empty());
        }
    }

    #[test]
    fn shows_impact_only_for_incidents() {
        assert!(EventType::Incident.shows_impact());
        assert!(!EventType::MaintenanceUrgent.shows_impact());
        assert!(!EventType::MaintenanceScheduled.shows_impact());
        assert!(!EventType::Changelog.shows_impact());
        assert!(!EventType::Info.shows_impact());
    }

    #[test]
    fn display_methods_merge_identified_into_in_progress() {
        assert_eq!(
            EventStatus::Identified.display_i18n_key(),
            EventStatus::InProgress.i18n_key()
        );
        assert_eq!(
            EventStatus::Identified.display_css_class(),
            EventStatus::InProgress.css_class()
        );
        assert_eq!(
            EventStatus::Investigating.display_i18n_key(),
            EventStatus::Investigating.i18n_key()
        );
    }

    #[test]
    fn card_accent_class_varies_by_family() {
        let incident_crit = EventType::Incident.card_accent_class(&Impact::Critical);
        let incident_minor = EventType::Incident.card_accent_class(&Impact::Minor);
        let maint = EventType::MaintenanceScheduled.card_accent_class(&Impact::None);
        let changelog = EventType::Changelog.card_accent_class(&Impact::None);

        assert!(incident_crit.contains("red"));
        assert!(incident_minor.contains("yellow"));
        assert!(maint.contains("blue"));
        assert!(changelog.contains("emerald"));
    }

    #[test]
    fn card_strip_class_varies_by_weight() {
        // Active incident: thick strip with color
        let cls =
            EventType::Incident.card_strip_class(&Impact::Critical, &EventStatus::Investigating);
        assert!(cls.contains("border-l-4"));
        assert!(cls.contains("red"));

        // Resolved incident: thin muted strip
        let cls = EventType::Incident.card_strip_class(&Impact::Major, &EventStatus::Resolved);
        assert!(cls.contains("border-l-2"));
        assert!(cls.contains("stone"));

        // Active maintenance: thin blue strip
        let cls = EventType::MaintenanceScheduled
            .card_strip_class(&Impact::None, &EventStatus::Scheduled);
        assert!(cls.contains("border-l-2"));
        assert!(cls.contains("blue"));

        // Publication: no strip
        let cls = EventType::Changelog.card_strip_class(&Impact::None, &EventStatus::Resolved);
        assert!(cls.contains("border-l-0"));
    }

    #[test]
    fn shows_lifecycle_for_actionable_types() {
        assert!(EventType::Incident.shows_lifecycle());
        assert!(EventType::MaintenanceScheduled.shows_lifecycle());
        assert!(EventType::MaintenanceUrgent.shows_lifecycle());
        assert!(!EventType::Changelog.shows_lifecycle());
        assert!(!EventType::Info.shows_lifecycle());
    }

    #[test]
    fn all_event_types_have_labels() {
        let all = [
            EventType::Incident,
            EventType::MaintenanceScheduled,
            EventType::MaintenanceUrgent,
            EventType::Changelog,
            EventType::Info,
        ];
        for t in all {
            assert!(!t.label().is_empty());
            assert!(!t.as_str().is_empty());
        }
    }

    #[test]
    fn all_impacts_have_labels_and_css() {
        let all = [Impact::None, Impact::Minor, Impact::Major, Impact::Critical];
        for i in all {
            assert!(!i.label().is_empty());
            assert!(!i.css_class().is_empty());
            assert!(!i.as_str().is_empty());
        }
    }

    #[test]
    fn initial_status_for_incident_is_investigating() {
        let input = CreateEventInput {
            event_type: EventType::Incident,
            title: String::new(),
            description: String::new(),
            impact: Impact::Major,
            scheduled_start: None,
            scheduled_end: None,
            service_ids: vec![],
            icon_id: None,
            author_id: 1,
        };
        assert_eq!(input.initial_status(), EventStatus::Investigating);
    }

    #[test]
    fn initial_status_for_scheduled_maintenance_is_scheduled() {
        let input = CreateEventInput {
            event_type: EventType::MaintenanceScheduled,
            title: String::new(),
            description: String::new(),
            impact: Impact::Minor,
            scheduled_start: None,
            scheduled_end: None,
            service_ids: vec![],
            icon_id: None,
            author_id: 1,
        };
        assert_eq!(input.initial_status(), EventStatus::Scheduled);
    }

    #[test]
    fn initial_status_for_changelog_is_resolved() {
        let input = CreateEventInput {
            event_type: EventType::Changelog,
            title: String::new(),
            description: String::new(),
            impact: Impact::None,
            scheduled_start: None,
            scheduled_end: None,
            service_ids: vec![],
            icon_id: None,
            author_id: 1,
        };
        assert_eq!(input.initial_status(), EventStatus::Resolved);
    }

    #[test]
    fn actual_start_is_none_for_scheduled_maintenance() {
        let input = CreateEventInput {
            event_type: EventType::MaintenanceScheduled,
            title: String::new(),
            description: String::new(),
            impact: Impact::None,
            scheduled_start: None,
            scheduled_end: None,
            service_ids: vec![],
            icon_id: None,
            author_id: 1,
        };
        assert!(input.actual_start().is_none());
    }

    #[test]
    fn actual_start_is_some_for_incident() {
        let input = CreateEventInput {
            event_type: EventType::Incident,
            title: String::new(),
            description: String::new(),
            impact: Impact::Critical,
            scheduled_start: None,
            scheduled_end: None,
            service_ids: vec![],
            icon_id: None,
            author_id: 1,
        };
        assert!(input.actual_start().is_some());
    }
}
