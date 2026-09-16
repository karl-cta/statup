//! Instance time zone and stored timestamp format.
//!
//! Times are stored in UTC and shown in the zone of the server process, which
//! the standard `TZ` variable sets (`TZ=Europe/Paris`). Without it the system
//! zone applies, which is UTC in the container image.

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, NaiveTime, Offset, TimeZone, Utc};

/// Text form of a timestamp in the database. It matches what `SQLite`'s
/// `datetime()` writes, so stored values compare correctly as text.
pub fn db(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

pub fn local(dt: &DateTime<Utc>) -> DateTime<Local> {
    dt.with_timezone(&Local)
}

pub fn local_date(dt: &DateTime<Utc>) -> NaiveDate {
    local(dt).date_naive()
}

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

/// Reads the value of an `<input type="datetime-local">`, written in the
/// instance zone. `None` for an empty or malformed value.
pub fn parse_input(value: &str) -> Option<DateTime<Utc>> {
    parse_input_in(value, &Local)
}

/// The value an `<input type="datetime-local">` expects for `dt`.
pub fn format_input(dt: &DateTime<Utc>) -> String {
    local(dt).format("%Y-%m-%dT%H:%M").to_string()
}

/// Reads a `YYYY-MM-DD` filter bound as the first or last second of that
/// day in the instance zone.
pub fn parse_day_bound(value: &str, end_of_day: bool) -> Option<DateTime<Utc>> {
    let date = NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").ok()?;
    let time = if end_of_day {
        NaiveTime::from_hms_opt(23, 59, 59)?
    } else {
        NaiveTime::MIN
    };
    to_utc(&Local, date.and_time(time))
}

/// First instant of a day in the instance zone.
pub fn day_start(date: NaiveDate) -> Option<DateTime<Utc>> {
    to_utc(&Local, date.and_time(NaiveTime::MIN))
}

/// Offset of the instance zone at `dt`, as people write it: "UTC",
/// "UTC+2", "UTC-3:30".
pub fn offset_label(dt: &DateTime<Utc>) -> String {
    format_offset(local(dt).offset().fix().local_minus_utc())
}

/// Minutes to add to UTC to read the instance clock, for the script that
/// keeps the masthead time current.
pub fn offset_minutes(dt: &DateTime<Utc>) -> i32 {
    local(dt).offset().fix().local_minus_utc() / 60
}

fn parse_input_in<Tz: TimeZone>(value: &str, zone: &Tz) -> Option<DateTime<Utc>> {
    let naive = NaiveDateTime::parse_from_str(value.trim(), "%Y-%m-%dT%H:%M").ok()?;
    to_utc(zone, naive)
}

/// A wall-clock time skipped by a daylight saving change has no instant; the
/// earliest valid reading is taken for ambiguous ones.
fn to_utc<Tz: TimeZone>(zone: &Tz, naive: NaiveDateTime) -> Option<DateTime<Utc>> {
    zone.from_local_datetime(&naive)
        .earliest()
        .map(|dt| dt.with_timezone(&Utc))
}

fn format_offset(seconds: i32) -> String {
    if seconds == 0 {
        return "UTC".to_string();
    }
    let sign = if seconds < 0 { '-' } else { '+' };
    let minutes = seconds.unsigned_abs() / 60;
    let (hours, rest) = (minutes / 60, minutes % 60);
    if rest == 0 {
        format!("UTC{sign}{hours}")
    } else {
        format!("UTC{sign}{hours}:{rest:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    #[test]
    fn db_format_matches_sqlite_datetime() {
        let dt = Utc.with_ymd_and_hms(2026, 9, 16, 8, 5, 3).unwrap();
        assert_eq!(db(dt), "2026-09-16 08:05:03");
    }

    #[test]
    fn input_is_read_in_the_given_zone() {
        let paris_summer = FixedOffset::east_opt(2 * 3600).unwrap();
        let parsed = parse_input_in("2026-09-20T22:00", &paris_summer).unwrap();
        assert_eq!(parsed, Utc.with_ymd_and_hms(2026, 9, 20, 20, 0, 0).unwrap());
    }

    #[test]
    fn malformed_input_is_none() {
        assert!(parse_input_in("", &Utc).is_none());
        assert!(parse_input_in("20/09/2026 22:00", &Utc).is_none());
    }

    #[test]
    fn offsets_read_like_people_write_them() {
        assert_eq!(format_offset(0), "UTC");
        assert_eq!(format_offset(7200), "UTC+2");
        assert_eq!(format_offset(-12_600), "UTC-3:30");
        assert_eq!(format_offset(19_800), "UTC+5:30");
    }

    #[test]
    fn stored_text_decodes_back() {
        let dt = Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap();
        let parsed = NaiveDateTime::parse_from_str(&db(dt), "%Y-%m-%d %H:%M:%S")
            .unwrap()
            .and_utc();
        assert_eq!(parsed, dt);
    }
}
