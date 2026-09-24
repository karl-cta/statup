//! The story of an event as one list, newest first: what was declared, each
//! word published, when the service came back, when it closed, and for work
//! still to come, the times announced.

use chrono::{DateTime, Utc};

use crate::i18n::I18n;
use crate::models::{Event, EventUpdateWithAuthor, Kind, Lifecycle, SEPARATOR, Tone, User};
use crate::services::can_delete_update;

/// A milestone and a message this close are one moment: closing an event
/// publishes its last word along with the new state.
const SAME_MOMENT_SECS: i64 = 120;

pub struct TimelineEntry {
    pub at: DateTime<Utc>,
    pub time: String,
    pub iso: String,
    /// What happened, when the moment is more than a message.
    pub label: Option<String>,
    pub tone: &'static str,
    /// A time announced but not reached yet.
    pub upcoming: bool,
    /// Staff names are shown to members only.
    pub author: Option<String>,
    pub html: String,
    pub update_id: Option<i64>,
    pub can_delete: bool,
}

struct Milestone {
    at: DateTime<Utc>,
    label: String,
    tone: Tone,
    upcoming: bool,
    /// Closing and restoring publish a message at the same moment.
    joins_message: bool,
}

/// Everything that happened to the event, newest first. The opening entry
/// carries the description and the author.
pub fn build(
    event: &Event,
    description_html: String,
    author: Option<String>,
    updates: Vec<EventUpdateWithAuthor>,
    user: Option<&User>,
    i18n: &I18n,
) -> Vec<TimelineEntry> {
    let mut milestones = milestones(event, i18n);
    let mut entries: Vec<TimelineEntry> = updates
        .into_iter()
        .map(|update| {
            let milestone = take_same_moment(&mut milestones, update.created_at);
            update_entry(update, milestone, event, user, i18n)
        })
        .collect();
    entries.extend(milestones.into_iter().map(|m| milestone_entry(m, i18n)));
    entries.push(opening_entry(event, description_html, author, i18n));
    entries.sort_by(|a, b| b.at.cmp(&a.at));
    entries
}

fn opening_entry(
    event: &Event,
    description_html: String,
    author: Option<String>,
    i18n: &I18n,
) -> TimelineEntry {
    let opening = i18n.t(opening_key(event));
    let label = match event.qualifier(i18n) {
        Some(qualifier) => format!("{opening}{SEPARATOR}{qualifier}"),
        None => opening.to_string(),
    };
    TimelineEntry {
        label: Some(label),
        tone: event.kind_tone(),
        author,
        html: description_html,
        ..entry_at(opened_at(event), i18n)
    }
}

/// An announcement opens when it is published; work started on the spot
/// opens when it began, which a report written afterwards puts earlier.
fn opened_at(event: &Event) -> DateTime<Utc> {
    if event.planned || event.kind == Kind::Publication {
        event.created_at
    } else {
        event.started_at.unwrap_or(event.created_at)
    }
}

/// An incident its author says began before they declared it. A report
/// written after the incident closed is not one: its record is written late.
fn declared_late(event: &Event) -> bool {
    let Some(began) = event.started_at else {
        return false;
    };
    event.kind == Kind::Incident
        && (event.created_at - began).num_seconds() > SAME_MOMENT_SECS
        && event.ended_at.is_none_or(|end| event.created_at <= end)
}

fn opening_key(event: &Event) -> &'static str {
    match (event.kind, event.planned) {
        (Kind::Incident, _) if declared_late(event) => "timeline.began",
        (Kind::Incident, _) => "timeline.declared",
        (Kind::Maintenance, true) => "timeline.announced",
        (Kind::Maintenance, false) => "timeline.started",
        (Kind::Publication, _) => "timeline.published",
    }
}

/// The moments the event's dates record, after its opening.
fn milestones(event: &Event, i18n: &I18n) -> Vec<Milestone> {
    let now = Utc::now();
    let active = event.lifecycle.is_some_and(Lifecycle::is_active);
    let mut list = Vec::new();
    let mut add = |at: DateTime<Utc>, key: &str, tone: Tone, joins_message: bool| {
        list.push(Milestone {
            at,
            label: i18n.t(key).to_string(),
            tone,
            upcoming: at > now,
            joins_message,
        });
    };
    if event.kind == Kind::Maintenance && event.planned {
        match event.started_at {
            Some(start) => add(start, "timeline.started", Tone::Info, false),
            None => {
                if let Some(start) = event.planned_start {
                    add(start, "timeline.planned_start", Tone::Info, false);
                }
            }
        }
    }
    if declared_late(event) {
        add(event.created_at, "timeline.declared", Tone::Neutral, false);
    }
    if let Some(end) = event.planned_end.filter(|end| active && *end > now) {
        add(end, "timeline.planned_end", Tone::Info, false);
    }
    if let Some(at) = event.restored_at {
        add(at, "timeline.restored", Tone::Ok, true);
    }
    if let Some((at, lifecycle)) = closing(event) {
        let tone = if lifecycle == Lifecycle::Cancelled {
            Tone::Neutral
        } else {
            Tone::Ok
        };
        add(at, lifecycle.label_key(event.kind), tone, true);
    }
    list
}

/// When and how the event closed; a call-off keeps no end, its last change
/// stands for it.
fn closing(event: &Event) -> Option<(DateTime<Utc>, Lifecycle)> {
    let lifecycle = event.lifecycle.filter(|l| l.is_terminal())?;
    Some((event.ended_at.unwrap_or(event.updated_at), lifecycle))
}

fn take_same_moment(milestones: &mut Vec<Milestone>, at: DateTime<Utc>) -> Option<Milestone> {
    let index = milestones
        .iter()
        .position(|m| m.joins_message && (m.at - at).num_seconds().abs() <= SAME_MOMENT_SECS)?;
    Some(milestones.remove(index))
}

fn update_entry(
    update: EventUpdateWithAuthor,
    milestone: Option<Milestone>,
    event: &Event,
    user: Option<&User>,
    i18n: &I18n,
) -> TimelineEntry {
    let (label, tone) = milestone.map_or((None, Tone::Neutral), |m| (Some(m.label), m.tone));
    TimelineEntry {
        label,
        tone: tone.as_str(),
        can_delete: user.is_some_and(|u| can_delete_update(event, update.author_id, u)),
        author: user.map(|_| update.author_name),
        update_id: Some(update.id),
        html: update.message,
        ..entry_at(update.created_at, i18n)
    }
}

fn milestone_entry(milestone: Milestone, i18n: &I18n) -> TimelineEntry {
    TimelineEntry {
        label: Some(milestone.label),
        tone: milestone.tone.as_str(),
        upcoming: milestone.upcoming,
        ..entry_at(milestone.at, i18n)
    }
}

fn entry_at(at: DateTime<Utc>, i18n: &I18n) -> TimelineEntry {
    TimelineEntry {
        at,
        time: i18n.format_datetime(&at),
        iso: at.to_rfc3339(),
        label: None,
        tone: Tone::Neutral.as_str(),
        upcoming: false,
        author: None,
        html: String::new(),
        update_id: None,
        can_delete: false,
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;
    use crate::models::Severity;

    fn event(kind: Kind, planned: bool, lifecycle: Lifecycle) -> Event {
        let now = Utc::now();
        Event {
            id: 1,
            kind,
            severity: (kind == Kind::Incident).then_some(Severity::Critical),
            planned,
            lifecycle: Some(lifecycle),
            category: None,
            title: "Payroll down".to_string(),
            description: String::new(),
            planned_start: None,
            planned_end: None,
            started_at: Some(now - Duration::hours(3)),
            ended_at: None,
            restored_at: None,
            author_id: 1,
            previous_lifecycle: None,
            follows_event_id: None,
            created_at: now - Duration::hours(3),
            updated_at: now,
        }
    }

    fn update(id: i64, at: DateTime<Utc>, message: &str) -> EventUpdateWithAuthor {
        EventUpdateWithAuthor {
            id,
            event_id: 1,
            message: message.to_string(),
            author_id: 1,
            created_at: at,
            author_name: "Jane".to_string(),
        }
    }

    #[test]
    fn closing_and_its_last_word_are_one_moment_newest_first() {
        let i18n = I18n::new("en");
        let mut incident = event(Kind::Incident, false, Lifecycle::Resolved);
        let closed = Utc::now() - Duration::minutes(10);
        incident.ended_at = Some(closed);
        let updates = vec![
            update(1, closed - Duration::hours(1), "Cause found"),
            update(2, closed + Duration::seconds(1), "Fixed"),
        ];
        let entries = build(&incident, "<p>Down</p>".into(), None, updates, None, &i18n);

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].update_id, Some(2));
        assert!(
            entries[0].label.is_some(),
            "the last word carries the closing"
        );
        assert_eq!(entries[0].tone, "ok");
        assert_eq!(entries[1].label, None);
        assert!(
            entries[2]
                .label
                .as_deref()
                .is_some_and(|l| l.starts_with("Incident declared"))
        );
        assert!(
            entries.iter().all(|e| e.author.is_none()),
            "names are for members"
        );
    }

    #[test]
    fn an_incident_declared_late_shows_when_it_began() {
        let i18n = I18n::new("en");
        let mut incident = event(Kind::Incident, false, Lifecycle::Investigating);
        incident.started_at = Some(incident.created_at - Duration::minutes(20));
        let entries = build(&incident, String::new(), None, Vec::new(), None, &i18n);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].label.as_deref(), Some("Incident declared"));
        assert!(
            entries[1]
                .label
                .as_deref()
                .is_some_and(|l| l.starts_with("Incident began"))
        );
    }

    #[test]
    fn announced_work_shows_the_times_still_to_come() {
        let i18n = I18n::new("en");
        let mut work = event(Kind::Maintenance, true, Lifecycle::Scheduled);
        work.started_at = None;
        work.planned_start = Some(Utc::now() + Duration::days(1));
        work.planned_end = Some(Utc::now() + Duration::days(1) + Duration::hours(2));
        let entries = build(&work, String::new(), None, Vec::new(), None, &i18n);

        let upcoming: Vec<bool> = entries.iter().map(|e| e.upcoming).collect();
        assert_eq!(upcoming, [true, true, false]);
    }
}
