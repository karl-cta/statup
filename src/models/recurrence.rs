//! The rhythm of a repeating maintenance: every week, or every month on the
//! Nth weekday, read from the first start in the instance zone.

use chrono::{
    DateTime, Datelike, Days, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc, Weekday,
};

use crate::clock;
use crate::i18n::I18n;

/// How many months ahead a search goes before giving up.
const MAX_MONTHS_AHEAD: u32 = 14;

/// How often a maintenance repeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rhythm {
    Weekly,
    Monthly,
}

impl Rhythm {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Weekly => "weekly",
            Self::Monthly => "monthly",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [Self::Weekly, Self::Monthly]
            .into_iter()
            .find(|rhythm| rhythm.as_str() == value)
    }
}

/// Which occurrence of a weekday in a month.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rank {
    First,
    Second,
    Third,
    Fourth,
    Last,
}

impl Rank {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::First => "first",
            Self::Second => "second",
            Self::Third => "third",
            Self::Fourth => "fourth",
            Self::Last => "last",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::First,
            Self::Second,
            Self::Third,
            Self::Fourth,
            Self::Last,
        ]
        .into_iter()
        .find(|rank| rank.as_str() == value)
    }

    /// The rank a day of the month has for its weekday. The 29th to the 31st
    /// are always the last one, so a month with a fifth never skips.
    pub fn of_day(day: u32) -> Self {
        match day {
            0..=7 => Self::First,
            8..=14 => Self::Second,
            15..=21 => Self::Third,
            22..=28 => Self::Fourth,
            _ => Self::Last,
        }
    }

    /// Day of the month of the Nth `weekday` in `year` and `month`.
    fn day_in(self, year: i32, month: u32, weekday: Weekday) -> Option<NaiveDate> {
        let first = NaiveDate::from_ymd_opt(year, month, 1)?;
        let offset = weekday.days_since(first.weekday());
        let week = match self {
            Self::First => 0,
            Self::Second => 1,
            Self::Third => 2,
            Self::Fourth => 3,
            Self::Last => {
                let fifth = first.checked_add_days(Days::new(u64::from(offset) + 28));
                return fifth
                    .filter(|day| day.month() == month)
                    .or_else(|| first.checked_add_days(Days::new(u64::from(offset) + 21)));
            }
        };
        first.checked_add_days(Days::new(u64::from(offset) + 7 * week))
    }
}

/// A repeat rule, with the wall-clock time of each start in the instance zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recurrence {
    Weekly {
        weekday: Weekday,
        time: NaiveTime,
    },
    Monthly {
        weekday: Weekday,
        rank: Rank,
        time: NaiveTime,
    },
}

impl Recurrence {
    /// "Chaque dimanche", "Le premier dimanche du mois", as the form names
    /// the rhythm.
    pub fn describe(&self, i18n: &I18n) -> String {
        let weekday = i18n.weekday_name(self.weekday());
        match self.rank() {
            None => i18n.tf("events.repeat_weekly", &[("weekday", weekday)]),
            Some(rank) => {
                let rank_key = format!("events.repeat_rank_{}", rank.as_str());
                let args = [("rank", i18n.t(&rank_key)), ("weekday", weekday)];
                i18n.tf("events.repeat_monthly", &args)
            }
        }
    }

    /// The rule as its columns store it: rhythm, weekday from 0 for Monday,
    /// rank, and time of day; [`Self::from_stored`] reads it back.
    pub fn stored(&self) -> (&'static str, i64, Option<&'static str>, String) {
        (
            self.rhythm().as_str(),
            i64::from(self.weekday().num_days_from_monday()),
            self.rank().map(Rank::as_str),
            self.time().format("%H:%M").to_string(),
        )
    }

    /// The rule a first start, read on the instance clock, sets.
    pub fn from_first(rhythm: Rhythm, start: NaiveDateTime) -> Self {
        let (weekday, time) = (start.weekday(), start.time());
        match rhythm {
            Rhythm::Weekly => Self::Weekly { weekday, time },
            Rhythm::Monthly => Self::Monthly {
                weekday,
                rank: Rank::of_day(start.day()),
                time,
            },
        }
    }

    pub fn rhythm(&self) -> Rhythm {
        match self {
            Self::Weekly { .. } => Rhythm::Weekly,
            Self::Monthly { .. } => Rhythm::Monthly,
        }
    }

    pub fn weekday(&self) -> Weekday {
        match self {
            Self::Weekly { weekday, .. } | Self::Monthly { weekday, .. } => *weekday,
        }
    }

    pub fn time(&self) -> NaiveTime {
        match self {
            Self::Weekly { time, .. } | Self::Monthly { time, .. } => *time,
        }
    }

    pub fn rank(&self) -> Option<Rank> {
        match self {
            Self::Weekly { .. } => None,
            Self::Monthly { rank, .. } => Some(*rank),
        }
    }

    /// The rule a stored series describes: its rhythm, its weekday from 0
    /// for Monday, its rank when monthly and its `HH:MM` start. `None` when
    /// a part is not one.
    pub fn from_stored(rhythm: &str, weekday: i64, rank: Option<&str>, time: &str) -> Option<Self> {
        let weekday = Weekday::try_from(u8::try_from(weekday).ok()?).ok()?;
        let time = NaiveTime::parse_from_str(time, "%H:%M").ok()?;
        match (Rhythm::parse(rhythm)?, rank) {
            (Rhythm::Weekly, None) => Some(Self::Weekly { weekday, time }),
            (Rhythm::Monthly, Some(rank)) => Some(Self::Monthly {
                weekday,
                rank: Rank::parse(rank)?,
                time,
            }),
            _ => None,
        }
    }

    /// First start strictly after `after`, in the instance zone.
    pub fn next_after(&self, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        self.next_after_in(&clock::zone(), after)
    }

    /// First start strictly after `after`, read in `zone`. `None` when none
    /// falls within the search window.
    pub fn next_after_in<Tz: TimeZone>(
        &self,
        zone: &Tz,
        after: DateTime<Utc>,
    ) -> Option<DateTime<Utc>> {
        let today = after.with_timezone(zone).date_naive();
        self.days_from(today)
            .filter_map(|day| clock::to_utc_or_later(zone, day.and_time(self.time())))
            .find(|start| *start > after)
    }

    /// The days the rule lands on, oldest first, from `today` on.
    fn days_from(&self, today: NaiveDate) -> Box<dyn Iterator<Item = NaiveDate>> {
        match *self {
            Self::Weekly { weekday, .. } => {
                let ahead = weekday.days_since(today.weekday());
                let first = today.checked_add_days(Days::new(u64::from(ahead)));
                Box::new(
                    (0..3u64).filter_map(move |week| first?.checked_add_days(Days::new(7 * week))),
                )
            }
            Self::Monthly { weekday, rank, .. } => {
                Box::new((0..MAX_MONTHS_AHEAD).filter_map(move |months| {
                    let (year, month) = add_months(today, months)?;
                    rank.day_in(year, month, weekday)
                }))
            }
        }
    }
}

/// Year and month `months` after the month of `date`.
fn add_months(date: NaiveDate, months: u32) -> Option<(i32, u32)> {
    let index = date.year().checked_mul(12)? + i32::try_from(date.month0() + months).ok()?;
    let month = u32::try_from(index.rem_euclid(12)).ok()? + 1;
    Some((index.div_euclid(12), month))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::{America::New_York, Europe::Paris};

    fn utc(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, minute, 0)
            .unwrap()
    }

    fn at(hour: u32, minute: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(hour, minute, 0).unwrap()
    }

    fn weekly(weekday: Weekday, time: NaiveTime) -> Recurrence {
        Recurrence::Weekly { weekday, time }
    }

    fn monthly(weekday: Weekday, rank: Rank, time: NaiveTime) -> Recurrence {
        Recurrence::Monthly {
            weekday,
            rank,
            time,
        }
    }

    fn first_start(rhythm: Rhythm, year: i32, month: u32, day: u32, hour: u32) -> Recurrence {
        let start = NaiveDate::from_ymd_opt(year, month, day)
            .unwrap()
            .and_time(at(hour, 0));
        Recurrence::from_first(rhythm, start)
    }

    #[test]
    fn rank_of_day_changes_at_each_week_boundary() {
        let expected = [
            (1, Rank::First),
            (7, Rank::First),
            (8, Rank::Second),
            (14, Rank::Second),
            (15, Rank::Third),
            (21, Rank::Third),
            (22, Rank::Fourth),
            (28, Rank::Fourth),
            (29, Rank::Last),
            (31, Rank::Last),
        ];
        for (day, rank) in expected {
            assert_eq!(Rank::of_day(day), rank, "day {day}");
        }
    }

    #[test]
    fn weekly_next_is_strictly_after_the_start() {
        let rule = first_start(Rhythm::Weekly, 2026, 11, 1, 2);
        assert_eq!(rule, weekly(Weekday::Sun, at(2, 0)));
        let next = rule.next_after_in(&Utc, utc(2026, 11, 1, 2, 0));
        assert_eq!(next, Some(utc(2026, 11, 8, 2, 0)));
    }

    #[test]
    fn monthly_first_sunday_lands_in_the_next_month() {
        let rule = first_start(Rhythm::Monthly, 2026, 11, 1, 2);
        assert_eq!(rule, monthly(Weekday::Sun, Rank::First, at(2, 0)));
        let next = rule.next_after_in(&Utc, utc(2026, 11, 1, 2, 0));
        assert_eq!(next, Some(utc(2026, 12, 6, 2, 0)));
    }

    #[test]
    fn monthly_last_sunday_follows_the_month_length() {
        let rule = first_start(Rhythm::Monthly, 2026, 11, 29, 2);
        assert_eq!(rule, monthly(Weekday::Sun, Rank::Last, at(2, 0)));
        let next = rule.next_after_in(&Utc, utc(2026, 11, 29, 2, 0));
        assert_eq!(next, Some(utc(2026, 12, 27, 2, 0)));
    }

    #[test]
    fn monthly_fourth_tuesday_from_a_day_between_22_and_28() {
        let rule = first_start(Rhythm::Monthly, 2026, 10, 27, 9);
        assert_eq!(rule, monthly(Weekday::Tue, Rank::Fourth, at(9, 0)));
        let next = rule.next_after_in(&Utc, utc(2026, 10, 27, 9, 0));
        assert_eq!(next, Some(utc(2026, 11, 24, 9, 0)));
    }

    #[test]
    fn monthly_last_monday_crosses_february() {
        let rule = monthly(Weekday::Mon, Rank::Last, at(10, 0));
        let in_january = rule.next_after_in(&Utc, utc(2027, 1, 25, 10, 0));
        assert_eq!(in_january, Some(utc(2027, 2, 22, 10, 0)));
        let in_february = rule.next_after_in(&Utc, utc(2027, 2, 22, 10, 0));
        assert_eq!(in_february, Some(utc(2027, 3, 29, 10, 0)));
    }

    #[test]
    fn after_a_long_stop_the_next_start_is_the_first_one_after_now() {
        let rule = weekly(Weekday::Sun, at(2, 0));
        let next = rule.next_after_in(&Utc, utc(2027, 1, 15, 12, 0));
        assert_eq!(next, Some(utc(2027, 1, 17, 2, 0)));
    }

    #[test]
    fn a_start_later_the_same_day_is_that_day() {
        let rule = weekly(Weekday::Sun, at(2, 0));
        let next = rule.next_after_in(&Utc, utc(2026, 11, 1, 1, 0));
        assert_eq!(next, Some(utc(2026, 11, 1, 2, 0)));
    }

    #[test]
    fn paris_start_skipped_by_daylight_saving_moves_to_the_first_valid_minute() {
        let rule = weekly(Weekday::Sun, at(2, 30));
        let next = rule.next_after_in(&Paris, utc(2027, 3, 21, 1, 30));
        assert_eq!(next, Some(utc(2027, 3, 28, 1, 0)));
    }

    #[test]
    fn paris_start_read_twice_takes_the_first_reading() {
        let rule = weekly(Weekday::Sun, at(2, 30));
        let next = rule.next_after_in(&Paris, utc(2026, 10, 18, 0, 30));
        assert_eq!(next, Some(utc(2026, 10, 25, 0, 30)));
    }

    #[test]
    fn new_york_start_skipped_by_daylight_saving_moves_to_the_first_valid_minute() {
        let rule = weekly(Weekday::Sun, at(2, 30));
        let next = rule.next_after_in(&New_York, utc(2027, 3, 7, 7, 30));
        assert_eq!(next, Some(utc(2027, 3, 14, 7, 0)));
    }

    #[test]
    fn new_york_start_read_twice_takes_the_first_reading() {
        let rule = weekly(Weekday::Sun, at(1, 30));
        let next = rule.next_after_in(&New_York, utc(2026, 10, 25, 5, 30));
        assert_eq!(next, Some(utc(2026, 11, 1, 5, 30)));
    }

    #[test]
    fn names_round_trip_and_unknown_values_are_refused() {
        for rhythm in [Rhythm::Weekly, Rhythm::Monthly] {
            assert_eq!(Rhythm::parse(rhythm.as_str()), Some(rhythm));
        }
        for rank in [
            Rank::First,
            Rank::Second,
            Rank::Third,
            Rank::Fourth,
            Rank::Last,
        ] {
            assert_eq!(Rank::parse(rank.as_str()), Some(rank));
        }
        assert_eq!(Rhythm::parse("daily"), None);
        assert_eq!(Rank::parse("fifth"), None);
    }

    #[test]
    fn a_stored_rule_reads_back_and_a_broken_one_is_refused() {
        let time = NaiveTime::from_hms_opt(2, 30, 0).unwrap();
        assert_eq!(
            Recurrence::from_stored("weekly", 6, None, "02:30"),
            Some(Recurrence::Weekly {
                weekday: Weekday::Sun,
                time
            })
        );
        assert_eq!(
            Recurrence::from_stored("monthly", 1, Some("second"), "02:30"),
            Some(Recurrence::Monthly {
                weekday: Weekday::Tue,
                rank: Rank::Second,
                time
            })
        );
        assert_eq!(Recurrence::from_stored("weekly", 7, None, "02:30"), None);
        assert_eq!(
            Recurrence::from_stored("weekly", 0, Some("first"), "02:30"),
            None
        );
        assert_eq!(Recurrence::from_stored("monthly", 0, None, "02:30"), None);
        assert_eq!(Recurrence::from_stored("weekly", 0, None, "2h30"), None);
    }
}
