//! A maintenance announced once that comes back on its own.

use chrono::{DateTime, TimeDelta, Utc};

use super::{CreateEventInput, Kind, Recurrence, Rhythm};
use crate::clock;

/// A repeating maintenance. Each session copies its title, description,
/// services and "no downtime"; `next_start` is when the session after the
/// one already scheduled begins.
#[derive(Debug, Clone)]
pub struct MaintenanceSeries {
    pub id: i64,
    pub recurrence: Recurrence,
    pub duration_minutes: i64,
    pub title: String,
    pub description: String,
    pub keeps_services_up: bool,
    pub next_start: DateTime<Utc>,
    pub author_id: i64,
    pub stopped_at: Option<DateTime<Utc>>,
}

impl MaintenanceSeries {
    /// When the next session begins: the slot the series waits for, or the
    /// first one after `now` when that slot went by while the server was
    /// off. A missed session is skipped, never caught up.
    pub fn next_session_start(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        if self.next_start > now {
            Some(self.next_start)
        } else {
            self.recurrence.next_after(now)
        }
    }

    /// The session of this series that begins at `start`.
    pub fn session(&self, start: DateTime<Utc>, service_ids: Vec<i64>) -> CreateEventInput {
        CreateEventInput {
            kind: Kind::Maintenance,
            severity: None,
            planned: true,
            category: None,
            title: self.title.clone(),
            description: self.description.clone(),
            planned_start: Some(start),
            planned_end: Some(start + TimeDelta::minutes(self.duration_minutes)),
            started_at: None,
            opening_step: None,
            keeps_services_up: self.keeps_services_up,
            service_ids,
            follows_event_id: None,
            author_id: self.author_id,
            api_key_id: None,
            next_update: None,
        }
    }
}

/// The rule and timing a first session sets for its series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewSeries {
    pub recurrence: Recurrence,
    pub duration_minutes: i64,
    pub next_start: DateTime<Utc>,
}

impl NewSeries {
    /// The series a first session starts, read on the instance clock.
    /// `None` without a start, or without an end after it: a session with
    /// no end would never complete.
    pub fn from_first(rhythm: Rhythm, first: &CreateEventInput) -> Option<Self> {
        let start = first.planned_start?;
        let duration_minutes = (first.planned_end? - start).num_minutes();
        if duration_minutes <= 0 {
            return None;
        }
        let recurrence = Recurrence::from_first(rhythm, clock::local(&start).naive_local());
        Some(Self {
            recurrence,
            duration_minutes,
            next_start: recurrence.next_after(start)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Kind;
    use chrono::{Datelike, Duration, TimeZone};

    fn first(start: DateTime<Utc>, end: Option<DateTime<Utc>>) -> CreateEventInput {
        CreateEventInput {
            kind: Kind::Maintenance,
            severity: None,
            planned: true,
            category: None,
            title: "Backups".to_string(),
            description: String::new(),
            planned_start: Some(start),
            planned_end: end,
            started_at: None,
            opening_step: None,
            keeps_services_up: false,
            service_ids: vec![],
            follows_event_id: None,
            author_id: 1,
            api_key_id: None,
            next_update: None,
        }
    }

    #[test]
    fn a_first_session_sets_the_rule_the_length_and_the_next_start() {
        // Mid-November: no daylight saving change within a week in any zone
        // the tests may have set.
        let start = Utc.with_ymd_and_hms(2026, 11, 15, 2, 0, 0).unwrap();
        let series = NewSeries::from_first(
            Rhythm::Weekly,
            &first(start, Some(start + Duration::hours(2))),
        )
        .unwrap();
        assert_eq!(series.duration_minutes, 120);
        assert_eq!(series.next_start, start + Duration::weeks(1));
        assert_eq!(series.recurrence.rhythm(), Rhythm::Weekly);
        assert_eq!(
            series.recurrence.weekday(),
            clock::local(&start).naive_local().date().weekday()
        );
    }

    fn weekly_series(next_start: DateTime<Utc>) -> MaintenanceSeries {
        let input = first(
            next_start - Duration::weeks(1),
            Some(next_start - Duration::weeks(1) + Duration::minutes(90)),
        );
        let new = NewSeries::from_first(Rhythm::Weekly, &input).unwrap();
        MaintenanceSeries {
            id: 1,
            recurrence: new.recurrence,
            duration_minutes: new.duration_minutes,
            title: input.title,
            description: "Copies of every disk.".to_string(),
            keeps_services_up: true,
            next_start,
            author_id: 7,
            stopped_at: None,
        }
    }

    #[test]
    fn the_next_session_waits_for_its_slot_and_skips_the_missed_ones() {
        let next = Utc.with_ymd_and_hms(2026, 11, 22, 2, 0, 0).unwrap();
        let series = weekly_series(next);

        assert_eq!(
            series.next_session_start(next - Duration::days(3)),
            Some(next)
        );
        assert_eq!(
            series.next_session_start(next),
            Some(next + Duration::weeks(1))
        );
        let after_three_weeks_off = next + Duration::weeks(2) + Duration::hours(1);
        assert_eq!(
            series.next_session_start(after_three_weeks_off),
            Some(next + Duration::weeks(3))
        );
    }

    #[test]
    fn a_session_repeats_what_the_first_one_said() {
        let next = Utc.with_ymd_and_hms(2026, 11, 22, 2, 0, 0).unwrap();
        let series = weekly_series(next);

        let session = series.session(next, vec![3, 4]);

        assert_eq!(session.kind, Kind::Maintenance);
        assert!(session.planned);
        assert_eq!(session.title, "Backups");
        assert_eq!(session.description, "Copies of every disk.");
        assert_eq!(session.planned_start, Some(next));
        assert_eq!(session.planned_end, Some(next + Duration::minutes(90)));
        assert!(session.keeps_services_up);
        assert_eq!(session.service_ids, vec![3, 4]);
        assert_eq!(session.author_id, 7);
    }

    #[test]
    fn a_session_without_an_end_after_its_start_cannot_repeat() {
        let start = Utc.with_ymd_and_hms(2026, 11, 15, 2, 0, 0).unwrap();
        assert!(NewSeries::from_first(Rhythm::Weekly, &first(start, None)).is_none());
        assert!(NewSeries::from_first(Rhythm::Monthly, &first(start, Some(start))).is_none());
    }
}
