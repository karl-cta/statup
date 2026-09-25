//! An event as stored and as listed, with what its pages say about it:
//! its tone, its step, how long it has run, its excerpt, its day.

use chrono::{DateTime, Utc};

use super::{
    Category, ICON_SEPARATOR, Kind, Lifecycle, NAME_SEPARATOR, ServiceTag, Severity, Tone,
    derive_status, drives_services,
};
use crate::clock;
use crate::i18n::I18n;
use crate::models::Service;

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
        (Kind::Publication, _) => Tone::Ink,
        _ => Tone::Neutral,
    }
}

/// Between two facts on one line; the dot never starts a line.
pub(crate) const SEPARATOR: &str = "\u{a0}· ";

/// Hue of what an event is, whatever its progress: an incident takes its
/// severity, maintenance its blue, an announcement the ink of the text.
fn kind_tone(kind: Kind, severity: Option<Severity>) -> Tone {
    match kind {
        Kind::Incident => severity.map_or(Tone::Neutral, Severity::tone),
        Kind::Maintenance => Tone::Info,
        Kind::Publication => Tone::Ink,
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

/// Words for a kind and what qualifies it: "Incident · Majeur",
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
    /// Maintenance its services stay up through.
    pub keeps_services_up: bool,
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

    /// How long it has been open, or since the service came back while
    /// the team still watches.
    pub fn elapsed_text(&self, i18n: &I18n) -> Option<String> {
        if let Some(restored) = self
            .restored_at
            .filter(|_| self.lifecycle == Some(Lifecycle::Monitoring))
        {
            let parts = split_duration(Utc::now() - restored);
            return Some(i18n.tf(
                "events.restored_for",
                &[("duration", &i18n.format_duration(&parts))],
            ));
        }
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

    /// The hue the event would take in `lifecycle`, for the state menu.
    pub fn tone_at(&self, lifecycle: Lifecycle) -> &'static str {
        state_tone(self.kind, self.severity, Some(lifecycle)).as_str()
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
    pub keeps_services_up: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub author_id: i64,
    /// Linked service names, joined with [`NAME_SEPARATOR`].
    #[sqlx(default)]
    pub service_names: String,
    /// Their icons, in the same order: built-in name and uploaded file,
    /// split by [`ICON_SEPARATOR`].
    #[sqlx(default)]
    pub service_icons: String,
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

    /// The services named, each with its icon, so a name under a title
    /// reads as a service at a glance; while the event sets their state,
    /// each says what it does to them.
    pub fn service_tags(&self, i18n: &I18n) -> Vec<ServiceTag> {
        let effect = drives_services(self.kind, self.lifecycle, self.keeps_services_up)
            .then(|| derive_status(self.kind, self.severity))
            .flatten()
            .map(|status| i18n.t(status.i18n_key()).to_lowercase());
        let mut icons = self.service_icons.split(NAME_SEPARATOR);
        self.services()
            .into_iter()
            .map(|name| {
                let (icon_name, icon_filename) = icons
                    .next()
                    .and_then(|icon| icon.split_once(ICON_SEPARATOR))
                    .unwrap_or_default();
                ServiceTag {
                    name: name.to_string(),
                    icon_name: (!icon_name.is_empty()).then(|| icon_name.to_string()),
                    icon_filename: (!icon_filename.is_empty()).then(|| icon_filename.to_string()),
                    effect: effect.clone(),
                }
            })
            .collect()
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

    /// Still worth the reader's eye: work open or to come, and every
    /// announcement, which never closes.
    pub fn is_current(&self) -> bool {
        self.is_open() || self.kind == Kind::Publication
    }

    pub fn tone(&self) -> Tone {
        state_tone(self.kind, self.severity, self.lifecycle)
    }

    pub fn lifecycle_key(&self) -> Option<&'static str> {
        self.lifecycle.map(|l| l.label_key(self.kind))
    }

    /// The word at the end of a row: the progress. An announcement has none.
    pub fn row_state(&self, i18n: &I18n) -> Option<String> {
        self.lifecycle_key().map(|key| i18n.t(key).to_string())
    }

    pub fn kind_label(&self, i18n: &I18n) -> String {
        describe_kind(self.kind, self.severity, self.category, i18n)
    }

    /// The word on the kind chip: an incident says its severity there, where
    /// its hue already shows it, and an announcement its category.
    pub fn chip_label(&self, i18n: &I18n) -> String {
        let key = match (self.kind, self.severity, self.category) {
            (Kind::Incident, Some(Severity::Minor), _) => "kind.incident_minor",
            (Kind::Incident, Some(Severity::Critical), _) => "kind.incident_critical",
            (Kind::Publication, _, Some(category)) => category.i18n_key(),
            (kind, _, _) => kind.i18n_key(),
        };
        i18n.t(key).to_string()
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
