//! A maintenance announced once that comes back on its own.

use chrono::{DateTime, Utc};

use super::{CreateEventInput, Recurrence, Rhythm};
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

    #[test]
    fn a_session_without_an_end_after_its_start_cannot_repeat() {
        let start = Utc.with_ymd_and_hms(2026, 11, 15, 2, 0, 0).unwrap();
        assert!(NewSeries::from_first(Rhythm::Weekly, &first(start, None)).is_none());
        assert!(NewSeries::from_first(Rhythm::Monthly, &first(start, Some(start))).is_none());
    }
}
