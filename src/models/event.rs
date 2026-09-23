//! Events: incidents, maintenances and announcements.
//!
//! Five orthogonal dimensions, checked by SQL constraints:
//! - `kind`: incident, maintenance or publication (an announcement).
//! - `severity`: impact on services, absent for publications.
//! - `planned`: a maintenance announced ahead, which starts and ends on its
//!   own at the planned times.
//! - `lifecycle`: workflow state, its values depend on the kind, absent for
//!   publications.
//! - `category`: kind of announcement, publications only.

use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::Service;
use crate::clock;
use crate::i18n::I18n;

/// Unit separator used between service names in list queries, so that a
/// comma inside a name never splits it.
pub const NAME_SEPARATOR: char = '\u{1f}';

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
    Ok,
    Info,
    Minor,
    Crit,
}

impl Tone {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Neutral => "neutral",
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

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Resolved | Self::Cancelled | Self::Completed)
    }

    pub fn is_active(self) -> bool {
        !self.is_terminal()
    }

    /// Closing states ask for a closing message, which is published.
    pub fn needs_closing_message(self) -> bool {
        matches!(self, Self::Resolved | Self::Completed)
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

/// Hue of an event's current state: the severity while an incident is being
/// worked on, calm once the service is back, blue for maintenance work.
fn state_tone(kind: Kind, severity: Option<Severity>, lifecycle: Option<Lifecycle>) -> Tone {
    use Lifecycle as L;
    match (kind, lifecycle) {
        (Kind::Incident, Some(L::Investigating | L::InProgress)) => {
            severity.map_or(Tone::Minor, Severity::tone)
        }
        (_, Some(L::Monitoring | L::Resolved | L::Completed)) => Tone::Ok,
        (Kind::Maintenance, Some(L::Scheduled | L::InProgress)) => Tone::Info,
        _ => Tone::Neutral,
    }
}

/// Between two facts on one line; the dot never starts a line.
pub(crate) const SEPARATOR: &str = "\u{a0}· ";

/// Hue of what an event is, whatever its progress: an incident takes its
/// severity, maintenance its blue, an announcement none.
fn kind_tone(kind: Kind, severity: Option<Severity>) -> Tone {
    match kind {
        Kind::Incident => severity.map_or(Tone::Neutral, Severity::tone),
        Kind::Maintenance => Tone::Info,
        Kind::Publication => Tone::Neutral,
    }
}

/// What qualifies a kind: the severity of an incident, the category of an
/// announcement.
fn qualifier_key(
    kind: Kind,
    severity: Option<Severity>,
    category: Option<Category>,
) -> Option<&'static str> {
    match kind {
        Kind::Incident => severity.map(Severity::i18n_key),
        Kind::Publication => category.map(Category::i18n_key),
        Kind::Maintenance => None,
    }
}

/// Words for a kind and what qualifies it: "Incident · En panne",
/// "Annonce · Information".
pub(crate) fn describe_kind(
    kind: Kind,
    severity: Option<Severity>,
    category: Option<Category>,
    i18n: &I18n,
) -> String {
    let label = i18n.t(kind.i18n_key());
    match qualifier_key(kind, severity, category) {
        Some(key) => format!("{label}{SEPARATOR}{}", i18n.t(key)),
        None => label.to_string(),
    }
}

/// Time left before `start`. None once it has come: the schedule then
/// starts the maintenance on its own.
fn countdown_to(start: DateTime<Utc>) -> Option<Countdown> {
    let left = start - Utc::now();
    if left <= chrono::Duration::zero() {
        return None;
    }
    let (days, hours, minutes) = split_duration(left);
    Some(Countdown {
        days,
        hours,
        minutes,
    })
}

/// How long open work has lasted: an incident is open for a while, a
/// maintenance started some time ago.
fn elapsed_label(kind: Kind, parts: &(i64, i64, i64), i18n: &I18n) -> String {
    let key = if kind == Kind::Maintenance {
        "events.started_ago"
    } else {
        "events.open_for"
    };
    i18n.tf(key, &[("duration", &i18n.format_duration(parts))])
}

fn open_elapsed(
    lifecycle: Option<Lifecycle>,
    started_at: Option<DateTime<Utc>>,
) -> Option<(i64, i64, i64)> {
    let open = lifecycle.is_some_and(|l| l.is_active() && l != Lifecycle::Scheduled);
    started_at
        .filter(|_| open)
        .map(|start| split_duration(Utc::now() - start))
}

fn split_duration(diff: chrono::TimeDelta) -> (i64, i64, i64) {
    (
        diff.num_days().max(0),
        (diff.num_hours() % 24).max(0),
        (diff.num_minutes() % 60).max(0),
    )
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
    /// When the service came back, while the team may still be watching.
    pub restored_at: Option<DateTime<Utc>>,
    pub author_id: i64,
    pub previous_lifecycle: Option<Lifecycle>,
    /// The maintenance this announcement follows.
    pub follows_event_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EventUpdateWithAuthor {
    pub id: i64,
    pub event_id: i64,
    /// Sanitized HTML.
    pub message: String,
    pub author_id: i64,
    pub created_at: DateTime<Utc>,
    pub author_name: String,
}

#[derive(Debug, Clone)]
pub struct EventWithServices {
    pub event: Event,
    pub services: Vec<Service>,
}

impl Event {
    /// How long the event lasted, once both ends are known.
    pub fn duration(&self) -> Option<(i64, i64, i64)> {
        let (start, end) = (self.started_at?, self.ended_at?);
        (end > start).then(|| split_duration(end - start))
    }

    /// Time since the work began, while it is still open. `None` before a
    /// planned maintenance starts.
    pub fn elapsed(&self) -> Option<(i64, i64, i64)> {
        open_elapsed(self.lifecycle, self.started_at)
    }

    pub fn elapsed_text(&self, i18n: &I18n) -> Option<String> {
        self.elapsed()
            .map(|parts| elapsed_label(self.kind, &parts, i18n))
    }

    /// The title as a browser tab shows it.
    pub fn tab_title(&self) -> String {
        excerpt(&self.title, 60)
    }

    pub fn tone(&self) -> Tone {
        state_tone(self.kind, self.severity, self.lifecycle)
    }

    pub fn kind_tone(&self) -> &'static str {
        kind_tone(self.kind, self.severity).as_str()
    }

    /// The severity or the category, said after the kind.
    pub fn qualifier(&self, i18n: &I18n) -> Option<String> {
        qualifier_key(self.kind, self.severity, self.category).map(|key| i18n.t(key).to_string())
    }

    pub fn previous_lifecycle_key(&self) -> Option<&'static str> {
        self.previous_lifecycle.map(|l| l.label_key(self.kind))
    }

    pub fn countdown(&self) -> Option<Countdown> {
        (self.lifecycle == Some(Lifecycle::Scheduled))
            .then_some(self.planned_start)
            .flatten()
            .and_then(countdown_to)
    }
}

/// Projection used by lists, the dashboard and the feed.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EventSummary {
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
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub author_id: i64,
    /// Linked service names, joined with [`NAME_SEPARATOR`].
    #[sqlx(default)]
    pub service_names: String,
    /// Latest of the event's own update time and its last posted update.
    /// Only filled by the feed query.
    #[sqlx(default)]
    pub last_activity_at: Option<DateTime<Utc>>,
    /// Latest posted update, sanitized HTML. Only filled by the banner query.
    #[sqlx(default)]
    pub latest_update: Option<String>,
    #[sqlx(default)]
    pub latest_update_at: Option<DateTime<Utc>>,
}

impl EventSummary {
    pub fn services(&self) -> Vec<&str> {
        self.service_names
            .split(NAME_SEPARATOR)
            .filter(|name| !name.is_empty())
            .collect()
    }

    pub fn services_label(&self) -> String {
        self.services().join(", ")
    }

    pub fn has_services(&self) -> bool {
        !self.service_names.is_empty()
    }

    /// Plain text of the latest update, for the status banner.
    pub fn latest_update_excerpt(&self, max_chars: usize) -> Option<String> {
        let html = self.latest_update.as_deref()?;
        let text = html_to_text(html);
        (!text.is_empty()).then(|| excerpt(&text, max_chars))
    }

    pub fn is_open(&self) -> bool {
        self.lifecycle.is_some_and(Lifecycle::is_active)
    }

    pub fn tone(&self) -> Tone {
        state_tone(self.kind, self.severity, self.lifecycle)
    }

    pub fn lifecycle_key(&self) -> Option<&'static str> {
        self.lifecycle.map(|l| l.label_key(self.kind))
    }

    pub fn kind_label(&self, i18n: &I18n) -> String {
        describe_kind(self.kind, self.severity, self.category, i18n)
    }

    pub fn kind_tone(&self) -> &'static str {
        kind_tone(self.kind, self.severity).as_str()
    }

    /// The line under a title: the severity or the category, then the
    /// services.
    pub fn meta_parts(&self, i18n: &I18n) -> Vec<String> {
        let qualifier = qualifier_key(self.kind, self.severity, self.category)
            .map(|key| i18n.t(key).to_string());
        qualifier
            .into_iter()
            .chain(self.has_services().then(|| self.services_label()))
            .collect()
    }

    pub fn countdown(&self) -> Option<Countdown> {
        (self.lifecycle == Some(Lifecycle::Scheduled))
            .then_some(self.planned_start)
            .flatten()
            .and_then(countdown_to)
    }

    /// "open for 3 h", "started 3 h ago", while the work is open.
    pub fn elapsed_text(&self, i18n: &I18n) -> Option<String> {
        open_elapsed(self.lifecycle, self.started_at)
            .map(|parts| elapsed_label(self.kind, &parts, i18n))
    }

    /// When the event ended, or its last change for a cancelled one.
    pub fn closed_at(&self) -> DateTime<Utc> {
        self.ended_at.unwrap_or(self.updated_at)
    }
}

/// The first `max_chars` characters, with an ellipsis when some are cut.
pub fn excerpt(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let cut: String = text.chars().take(max_chars).collect();
    format!("{}…", cut.trim_end())
}

/// Text content of sanitized HTML: tags dropped, the entities ammonia
/// writes decoded, whitespace collapsed.
fn html_to_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                text.push(' ');
            }
            _ if !in_tag => text.push(ch),
            _ => {}
        }
    }
    let decoded = text
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Time left before a planned start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Countdown {
    pub days: i64,
    pub hours: i64,
    pub minutes: i64,
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
    pub follows_event_id: Option<i64>,
    pub author_id: i64,
}

impl CreateEventInput {
    /// Initial state: an incident is being investigated, an announced
    /// maintenance waits for its start, an urgent one is under way.
    pub fn initial_lifecycle(&self) -> Option<Lifecycle> {
        match (self.kind, self.planned) {
            (Kind::Incident, _) => Some(Lifecycle::Investigating),
            (Kind::Maintenance, true) => Some(Lifecycle::Scheduled),
            (Kind::Maintenance, false) => Some(Lifecycle::InProgress),
            (Kind::Publication, _) => None,
        }
    }

    /// Everything starts now except an announced maintenance.
    pub fn initial_started_at(&self) -> Option<DateTime<Utc>> {
        match (self.kind, self.planned) {
            (Kind::Maintenance, true) => None,
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

/// Events of one day, in the instance time zone.
pub struct DayGroup {
    pub label: String,
    pub events: Vec<EventSummary>,
}

/// Groups events by day, keeping their order. Expects newest first.
pub fn group_by_day(events: Vec<EventSummary>, i18n: &I18n) -> Vec<DayGroup> {
    let mut groups: Vec<DayGroup> = Vec::new();
    let mut current = None;
    for event in events {
        let date = clock::local_date(&event.created_at);
        if current != Some(date) {
            groups.push(DayGroup {
                label: i18n.date_label(&date),
                events: Vec::new(),
            });
            current = Some(date);
        }
        if let Some(last) = groups.last_mut() {
            last.events.push(event);
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(
        kind: Kind,
        severity: Option<Severity>,
        lifecycle: Option<Lifecycle>,
    ) -> EventSummary {
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
            author_id: 1,
            service_names: String::new(),
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
        assert_eq!(note.tone(), Tone::Neutral);
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
        event.latest_update =
            Some("<p>Fix <strong>deployed</strong> &amp; watched</p>".to_string());
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
}
