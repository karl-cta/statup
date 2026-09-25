//! What creates, changes and filters events.

use chrono::{DateTime, Utc};

use super::{Category, Kind, Lifecycle, LifecycleGroup, Severity};

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
    /// When an incident declared late really began.
    pub started_at: Option<DateTime<Utc>>,
    /// The step an incident its author already works on opens at.
    pub opening_step: Option<Lifecycle>,
    /// Maintenance its services stay up through.
    pub keeps_services_up: bool,
    pub service_ids: Vec<i64>,
    pub follows_event_id: Option<i64>,
    pub author_id: i64,
}

impl CreateEventInput {
    /// Initial state: an incident is being investigated unless its author
    /// says it is further along, an announced maintenance waits for its
    /// start, an urgent one is under way.
    pub fn initial_lifecycle(&self) -> Option<Lifecycle> {
        match (self.kind, self.planned) {
            (Kind::Incident, _) => Some(
                self.opening_step
                    .filter(|step| step.opens_incident())
                    .unwrap_or(Lifecycle::Investigating),
            ),
            (Kind::Maintenance, true) => Some(Lifecycle::Scheduled),
            (Kind::Maintenance, false) => Some(Lifecycle::InProgress),
            (Kind::Publication, _) => None,
        }
    }

    /// Everything starts now except an announced maintenance, and an
    /// incident its author says began earlier.
    pub fn initial_started_at(&self) -> Option<DateTime<Utc>> {
        match (self.kind, self.planned) {
            (Kind::Maintenance, true) => None,
            (Kind::Incident, _) => Some(self.started_at.unwrap_or_else(Utc::now)),
            _ => Some(Utc::now()),
        }
    }
}

/// Editable fields of an existing event. The kind never changes: its states
/// would not fit another kind.
#[derive(Debug)]
pub struct UpdateEventInput {
    pub severity: Option<Severity>,
    pub planned: bool,
    pub category: Option<Category>,
    pub title: String,
    pub description: String,
    pub planned_start: Option<DateTime<Utc>>,
    pub planned_end: Option<DateTime<Utc>>,
    pub service_ids: Vec<i64>,
    pub follows_event_id: Option<i64>,
}

#[derive(Debug, Default)]
pub struct EventFilters {
    pub kind: Option<Kind>,
    pub lifecycle_group: Option<LifecycleGroup>,
    pub service_id: Option<i64>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub q: Option<String>,
    pub limit: i64,
    pub offset: i64,
}
