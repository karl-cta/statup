//! Event model and related enums.
//!
//! Dimensions:
//! - `kind`, event nature (incident, maintenance, publication).
//! - `severity`, business severity. Optional for incident and maintenance,
//!   absent for publication.
//! - `planned`, boolean, distinguishes a scheduled maintenance from an
//!   unplanned intervention. `false` for an incident, ignored for publication.
//! - `lifecycle`, workflow state, valid values depend on `kind`. Absent for
//!   publication. Consistency is enforced by SQL CHECK constraints.
//! - `category`, sub-category of a publication (`changelog`, `info`). Absent
//!   for incident and maintenance.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::Service;

#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type, Serialize, Deserialize)]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Incident,
    Maintenance,
    Publication,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type, Serialize, Deserialize)]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Minor,
    Major,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type, Serialize, Deserialize)]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Changelog,
    Info,
}

/// Workflow state. Valid values per kind:
/// - incident:    `investigating`, `in_progress`, `monitoring`, `resolved`, `cancelled`
/// - maintenance: `scheduled`, `in_progress`, `completed`, `cancelled`
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type, Serialize, Deserialize)]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    Investigating,
    InProgress,
    Monitoring,
    Resolved,
    Cancelled,
    Scheduled,
    Completed,
}

impl Kind {
    pub fn i18n_key(self) -> &'static str {
        match self {
            Self::Incident => "kind.incident",
            Self::Maintenance => "kind.maintenance",
            Self::Publication => "kind.publication",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Incident => "incident",
            Self::Maintenance => "maintenance",
            Self::Publication => "publication",
        }
    }

    pub fn has_lifecycle(self) -> bool {
        !matches!(self, Self::Publication)
    }

    pub fn has_severity(self) -> bool {
        !matches!(self, Self::Publication)
    }

    pub fn has_category(self) -> bool {
        matches!(self, Self::Publication)
    }

    pub fn dot_class(self) -> &'static str {
        match self {
            Self::Incident => "bg-red-500",
            Self::Maintenance => "bg-blue-500",
            Self::Publication => "bg-emerald-500",
        }
    }

    /// Tailwind class for the event card's vertical strip. Width reflects the
    /// "weight" (active incident = thick, others = thin or none), color
    /// reflects kind and severity.
    pub fn card_strip_class(
        self,
        severity: Option<Severity>,
        lifecycle: Option<Lifecycle>,
    ) -> &'static str {
        let is_active = lifecycle.is_some_and(Lifecycle::is_active);
        match self {
            Self::Incident if is_active => match severity {
                Some(Severity::Critical) => "border-l-4 border-l-red-500 dark:border-l-red-400",
                Some(Severity::Major) => "border-l-4 border-l-orange-500 dark:border-l-orange-400",
                Some(Severity::Minor) => "border-l-4 border-l-yellow-600 dark:border-l-yellow-500",
                None => "border-l-4 border-l-stone-400 dark:border-l-stone-500",
            },
            Self::Maintenance if is_active => "border-l-2 border-l-blue-400 dark:border-l-blue-500",
            Self::Incident | Self::Maintenance => {
                "border-l-2 border-l-stone-300 dark:border-l-stone-600"
            }
            Self::Publication => "border-l-0",
        }
    }

    /// Allowed transitions from `current` for this kind. Returns an empty
    /// slice for terminal states or for an invalid (kind, lifecycle) pair.
    pub fn allowed_transitions(self, current: Lifecycle) -> &'static [Lifecycle] {
        use Lifecycle as L;
        match (self, current) {
            (Self::Incident, L::Investigating) => {
                &[L::InProgress, L::Monitoring, L::Resolved, L::Cancelled]
            }
            (Self::Incident, L::InProgress) => &[L::Monitoring, L::Resolved],
            (Self::Incident, L::Monitoring) => &[L::InProgress, L::Resolved],
            (Self::Maintenance, L::Scheduled) => &[L::InProgress, L::Cancelled],
            (Self::Maintenance, L::InProgress) => &[L::Completed, L::Cancelled],
            _ => &[],
        }
    }

    pub fn can_transition(self, from: Lifecycle, to: Lifecycle) -> bool {
        self.allowed_transitions(from).contains(&to)
    }
}

impl Severity {
    pub fn i18n_key(self) -> &'static str {
        match self {
            Self::Minor => "severity.minor",
            Self::Major => "severity.major",
            Self::Critical => "severity.critical",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Minor => "minor",
            Self::Major => "major",
            Self::Critical => "critical",
        }
    }

    pub fn css_class(self) -> &'static str {
        match self {
            Self::Minor => "badge-severity-minor",
            Self::Major => "badge-severity-major",
            Self::Critical => "badge-severity-critical",
        }
    }

    pub fn level(self) -> u8 {
        match self {
            Self::Minor => 1,
            Self::Major => 2,
            Self::Critical => 3,
        }
    }

    pub fn dot_class(self) -> &'static str {
        match self {
            Self::Minor => "bg-yellow-500 dark:bg-yellow-400",
            Self::Major => "bg-orange-500 dark:bg-orange-400",
            Self::Critical => "bg-red-500 dark:bg-red-400",
        }
    }
}

impl Category {
    pub fn i18n_key(self) -> &'static str {
        match self {
            Self::Changelog => "category.changelog",
            Self::Info => "category.info",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Changelog => "changelog",
            Self::Info => "info",
        }
    }
}

impl Lifecycle {
    pub fn i18n_key(self) -> &'static str {
        match self {
            Self::Investigating => "lifecycle.investigating",
            Self::InProgress => "lifecycle.in_progress",
            Self::Monitoring => "lifecycle.monitoring",
            Self::Resolved => "lifecycle.resolved",
            Self::Cancelled => "lifecycle.cancelled",
            Self::Scheduled => "lifecycle.scheduled",
            Self::Completed => "lifecycle.completed",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Investigating => "investigating",
            Self::InProgress => "in_progress",
            Self::Monitoring => "monitoring",
            Self::Resolved => "resolved",
            Self::Cancelled => "cancelled",
            Self::Scheduled => "scheduled",
            Self::Completed => "completed",
        }
    }

    pub fn css_class(self) -> &'static str {
        match self {
            Self::Investigating => "badge-investigating",
            Self::InProgress => "badge-in-progress",
            Self::Monitoring => "badge-monitoring",
            Self::Resolved => "badge-resolved",
            Self::Cancelled => "badge-cancelled",
            Self::Scheduled => "badge-scheduled",
            Self::Completed => "badge-completed",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Resolved | Self::Cancelled | Self::Completed)
    }

    pub fn is_active(self) -> bool {
        !self.is_terminal()
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Event {
    pub id: i64,
    pub kind: Kind,
    pub severity: Option<Severity>,
    pub planned: bool,
    pub lifecycle: Option<Lifecycle>,
    pub category: Option<Category>,
    pub title: String,
    pub description: String,
    pub planned_start: Option<DateTime<Utc>>,
    pub planned_end: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub icon_id: Option<i64>,
    pub author_id: i64,
    pub previous_lifecycle: Option<Lifecycle>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EventUpdate {
    pub id: i64,
    pub event_id: i64,
    pub message: String,
    pub author_id: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct EventWithServices {
    pub event: Event,
    pub services: Vec<Service>,
}

/// Lightweight projection for lists and dashboards.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct EventSummary {
    pub id: i64,
    pub kind: Kind,
    pub severity: Option<Severity>,
    pub planned: bool,
    pub lifecycle: Option<Lifecycle>,
    pub category: Option<Category>,
    pub title: String,
    #[sqlx(default)]
    pub description: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub author_id: i64,
    /// Service names concatenated via SQL `GROUP_CONCAT`.
    #[sqlx(default)]
    pub service_names: String,
    #[sqlx(default)]
    #[serde(skip)]
    pub icon_filename: Option<String>,
    /// Only populated by queries that target upcoming maintenances.
    #[sqlx(default)]
    pub planned_start: Option<DateTime<Utc>>,
}

impl EventSummary {
    pub fn services(&self) -> Vec<&str> {
        if self.service_names.is_empty() {
            Vec::new()
        } else {
            self.service_names.split(", ").collect()
        }
    }

    pub fn description_excerpt(&self, max_len: usize) -> String {
        let line = self.description.lines().next().unwrap_or("");
        let clean = line.trim_start_matches(['#', '-', '*', '>']).trim();
        if clean.len() <= max_len {
            clean.to_string()
        } else {
            let truncated: String = clean.chars().take(max_len).collect();
            format!("{truncated}…")
        }
    }

    pub fn icon_url(&self) -> Option<String> {
        self.icon_filename
            .as_ref()
            .map(|f| format!("/uploads/icons/{f}"))
    }

    /// Human countdown until the planned start (e.g. "3d 2h").
    pub fn countdown(&self) -> Option<String> {
        let start = self.planned_start?;
        let now = Utc::now();
        if start <= now {
            return Some("Now".to_string());
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

    /// True if the planned start is less than 3 days away.
    pub fn is_soon(&self) -> bool {
        self.countdown_parts()
            .and_then(|inner| inner)
            .is_some_and(|(days, _, _)| days < 3)
    }

    /// Chip style variant for log-rows, derived from kind + severity.
    pub fn chip_class(&self) -> &'static str {
        match (self.kind, self.severity) {
            (Kind::Incident, Some(Severity::Critical)) => "log-chip log-chip-crit",
            (Kind::Incident, Some(Severity::Major)) => "log-chip log-chip-major",
            (Kind::Incident, _) => "log-chip log-chip-minor",
            (Kind::Maintenance, _) => "log-chip log-chip-info",
            (Kind::Publication, _) => "log-chip log-chip-pub",
        }
    }

    /// Breakdown of the countdown into (days, hours, minutes) for rich display.
    /// `None` if there is no planned date, `Some(None)` if the deadline is past.
    pub fn countdown_parts(&self) -> Option<Option<(i64, i64, i64)>> {
        let start = self.planned_start?;
        let now = Utc::now();
        if start <= now {
            return Some(None);
        }
        let diff = start - now;
        let days = diff.num_days();
        let hours = diff.num_hours() % 24;
        let minutes = diff.num_minutes() % 60;
        Some(Some((days, hours, minutes)))
    }
}

#[derive(Debug, Clone)]
pub struct EventWithDetails {
    pub event: Event,
    pub services: Vec<Service>,
    pub author_name: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EventUpdateWithAuthor {
    pub id: i64,
    pub event_id: i64,
    pub message: String,
    pub author_id: i64,
    pub created_at: DateTime<Utc>,
    pub author_name: String,
}

#[derive(Debug)]
pub struct CreateEventInput {
    pub kind: Kind,
    pub severity: Option<Severity>,
    pub planned: bool,
    pub category: Option<Category>,
    pub title: String,
    pub description: String,
    pub planned_start: Option<DateTime<Utc>>,
    pub planned_end: Option<DateTime<Utc>>,
    pub service_ids: Vec<i64>,
    pub icon_id: Option<i64>,
    pub author_id: i64,
}

impl CreateEventInput {
    /// Initial lifecycle at creation, depending on `kind` and `planned`.
    /// Publications have no lifecycle.
    pub fn initial_lifecycle(&self) -> Option<Lifecycle> {
        match (self.kind, self.planned) {
            (Kind::Incident, _) => Some(Lifecycle::Investigating),
            (Kind::Maintenance, true) => Some(Lifecycle::Scheduled),
            (Kind::Maintenance, false) => Some(Lifecycle::InProgress),
            (Kind::Publication, _) => None,
        }
    }

    /// Initial `started_at`. `None` for a scheduled maintenance that has not
    /// started yet, `Some(now)` in every other case (incident, urgent
    /// maintenance, publication).
    pub fn initial_started_at(&self) -> Option<DateTime<Utc>> {
        match (self.kind, self.planned) {
            (Kind::Maintenance, true) => None,
            _ => Some(Utc::now()),
        }
    }
}

#[derive(Debug, Default)]
pub struct EventFilters {
    pub kind: Option<Kind>,
    pub lifecycle: Option<Lifecycle>,
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
    fn maintenance_transitions_from_scheduled() {
        let t = Kind::Maintenance.allowed_transitions(Lifecycle::Scheduled);
        assert_eq!(t, &[Lifecycle::InProgress, Lifecycle::Cancelled]);
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
    fn publication_has_no_lifecycle() {
        assert!(!Kind::Publication.has_lifecycle());
        assert!(Kind::Incident.has_lifecycle());
        assert!(Kind::Maintenance.has_lifecycle());
    }

    #[test]
    fn only_publication_has_category() {
        assert!(Kind::Publication.has_category());
        assert!(!Kind::Incident.has_category());
        assert!(!Kind::Maintenance.has_category());
    }

    #[test]
    fn publication_has_no_severity() {
        assert!(!Kind::Publication.has_severity());
        assert!(Kind::Incident.has_severity());
        assert!(Kind::Maintenance.has_severity());
    }

    #[test]
    fn initial_lifecycle_incident_is_investigating() {
        let input = sample_input(Kind::Incident, false, None);
        assert_eq!(input.initial_lifecycle(), Some(Lifecycle::Investigating));
    }

    #[test]
    fn initial_lifecycle_planned_maintenance_is_scheduled() {
        let input = sample_input(Kind::Maintenance, true, None);
        assert_eq!(input.initial_lifecycle(), Some(Lifecycle::Scheduled));
    }

    #[test]
    fn initial_lifecycle_unplanned_maintenance_is_in_progress() {
        let input = sample_input(Kind::Maintenance, false, None);
        assert_eq!(input.initial_lifecycle(), Some(Lifecycle::InProgress));
    }

    #[test]
    fn initial_lifecycle_publication_is_none() {
        let input = sample_input(Kind::Publication, false, Some(Category::Changelog));
        assert!(input.initial_lifecycle().is_none());
    }

    #[test]
    fn initial_started_at_none_for_planned_maintenance() {
        let input = sample_input(Kind::Maintenance, true, None);
        assert!(input.initial_started_at().is_none());
    }

    #[test]
    fn initial_started_at_some_for_incident() {
        let input = sample_input(Kind::Incident, false, None);
        assert!(input.initial_started_at().is_some());
    }

    #[test]
    fn initial_started_at_some_for_publication() {
        let input = sample_input(Kind::Publication, false, Some(Category::Info));
        assert!(input.initial_started_at().is_some());
    }

    fn sample_input(kind: Kind, planned: bool, category: Option<Category>) -> CreateEventInput {
        CreateEventInput {
            kind,
            severity: None,
            planned,
            category,
            title: String::new(),
            description: String::new(),
            planned_start: None,
            planned_end: None,
            service_ids: vec![],
            icon_id: None,
            author_id: 1,
        }
    }
}
