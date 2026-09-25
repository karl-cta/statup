//! The dimensions of an event (kind, severity, category, lifecycle) and
//! what they do to the services it names.

use serde::Deserialize;

use crate::models::ServiceStatus;

/// Status an open event gives its services. Maintenance under way sets
/// Maintenance; an incident follows its severity, minor when none was given.
pub fn derive_status(kind: Kind, severity: Option<Severity>) -> Option<ServiceStatus> {
    match kind {
        Kind::Incident => Some(match severity {
            Some(Severity::Critical) => ServiceStatus::MajorOutage,
            Some(Severity::Minor) | None => ServiceStatus::Degraded,
        }),
        Kind::Maintenance => Some(ServiceStatus::Maintenance),
        Kind::Publication => None,
    }
}

/// Whether an event in `lifecycle` sets the state of its services: an
/// incident until it is under watch, a maintenance while it runs unless it
/// keeps them up. The same rule as `DRIVES_SERVICES` in the event
/// repository, which the status, the banner and the services page read.
pub fn drives_services(kind: Kind, lifecycle: Option<Lifecycle>, keeps_services_up: bool) -> bool {
    match (kind, lifecycle) {
        (Kind::Incident, Some(Lifecycle::Investigating | Lifecycle::InProgress)) => true,
        (Kind::Maintenance, Some(Lifecycle::InProgress)) => !keeps_services_up,
        _ => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type, Deserialize)]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Incident,
    Maintenance,
    Publication,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type, Deserialize)]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Minor,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type, Deserialize)]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Changelog,
    Info,
}

/// What the activity card may show, each ticked or not by an administrator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityKind {
    MinorIncidents,
    MajorIncidents,
    Maintenance,
    Announcements,
}

impl ActivityKind {
    pub const ALL: [Self; 4] = [
        Self::MinorIncidents,
        Self::MajorIncidents,
        Self::Maintenance,
        Self::Announcements,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::MinorIncidents => "incident_minor",
            Self::MajorIncidents => "incident_critical",
            Self::Maintenance => "maintenance",
            Self::Announcements => "publication",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    pub fn label_key(self) -> &'static str {
        match self {
            Self::MinorIncidents => "modules.recent_activity.show_incident_minor",
            Self::MajorIncidents => "modules.recent_activity.show_incident_critical",
            Self::Maintenance => "modules.recent_activity.show_maintenance",
            Self::Announcements => "modules.recent_activity.show_publication",
        }
    }

    /// Shown until an administrator chooses: maintenance has its own card.
    pub fn shown_by_default(self) -> bool {
        self != Self::Maintenance
    }
}

/// Workflow state. Valid values per kind:
/// - incident: `investigating`, `in_progress`, `monitoring`, `resolved`, `cancelled`
/// - maintenance: `scheduled`, `in_progress`, `completed`, `cancelled`
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type, Deserialize)]
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

/// Semantic hue of a status mark, resolved to a `--sem-*` token by CSS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tone {
    Neutral,
    /// An announcement: the team speaking, not a state.
    Ink,
    Ok,
    Info,
    Minor,
    Crit,
}

impl Tone {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Neutral => "neutral",
            Self::Ink => "ink",
            Self::Ok => "ok",
            Self::Info => "info",
            Self::Minor => "minor",
            Self::Crit => "crit",
        }
    }
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

    /// States reachable from `current`. Empty for terminal states and for a
    /// state that does not belong to this kind.
    pub fn allowed_transitions(self, current: Lifecycle) -> &'static [Lifecycle] {
        use Lifecycle as L;
        match (self, current) {
            (Self::Incident, L::Investigating) => {
                &[L::InProgress, L::Monitoring, L::Resolved, L::Cancelled]
            }
            (Self::Incident, L::InProgress) => &[L::Monitoring, L::Resolved, L::Cancelled],
            (Self::Incident, L::Monitoring) => &[L::InProgress, L::Resolved],
            (Self::Maintenance, L::Scheduled) => &[L::InProgress, L::Completed, L::Cancelled],
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
            Self::Critical => "severity.critical",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Minor => "minor",
            Self::Critical => "critical",
        }
    }

    pub fn tone(self) -> Tone {
        match self {
            Self::Minor => Tone::Minor,
            Self::Critical => Tone::Crit,
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

    /// Label of this state for an event of `kind`: the words differ, and
    /// French agrees them with the feminine "maintenance".
    pub fn label_key(self, kind: Kind) -> &'static str {
        match (kind, self) {
            (Kind::Maintenance, Self::InProgress) => "lifecycle.maintenance.in_progress",
            (Kind::Maintenance, Self::Cancelled) => "lifecycle.maintenance.cancelled",
            (_, Self::Investigating) => "lifecycle.investigating",
            (_, Self::InProgress) => "lifecycle.in_progress",
            (_, Self::Monitoring) => "lifecycle.monitoring",
            (_, Self::Resolved) => "lifecycle.resolved",
            (_, Self::Cancelled) => "lifecycle.cancelled",
            (_, Self::Scheduled) => "lifecycle.scheduled",
            (_, Self::Completed) => "lifecycle.completed",
        }
    }

    /// Steps an incident may be declared at: any step before it is closed.
    pub fn opens_incident(self) -> bool {
        matches!(
            self,
            Self::Investigating | Self::InProgress | Self::Monitoring
        )
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Resolved | Self::Cancelled | Self::Completed)
    }

    pub fn is_active(self) -> bool {
        !self.is_terminal()
    }
}

/// Filter of the events list: open work or what is over. Announcements
/// have no state and fall in neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleGroup {
    Active,
    Closed,
}

impl LifecycleGroup {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Closed => "closed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "active" => Some(Self::Active),
            "closed" => Some(Self::Closed),
            _ => None,
        }
    }

    pub fn lifecycles(self) -> &'static [Lifecycle] {
        match self {
            Self::Active => &[
                Lifecycle::Investigating,
                Lifecycle::InProgress,
                Lifecycle::Monitoring,
                Lifecycle::Scheduled,
            ],
            Self::Closed => &[
                Lifecycle::Resolved,
                Lifecycle::Cancelled,
                Lifecycle::Completed,
            ],
        }
    }
}
