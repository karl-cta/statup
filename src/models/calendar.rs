//! A maintenance as a calendar takes it, for "Add to my calendar": a copy,
//! never followed by its later changes.

use chrono::{DateTime, Utc};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};

const DATE_FORMAT: &str = "%Y%m%dT%H%M%SZ";
const MAX_LINE_OCTETS: usize = 75;
const GOOGLE_RENDER_URL: &str = "https://calendar.google.com/calendar/render";

/// One event as a calendar takes it: a file Outlook, Apple Calendar and
/// others open, or a link that opens Google Calendar with it filled in.
#[derive(Debug, Clone)]
pub struct CalendarEntry {
    /// Stable across downloads, so a calendar that already holds the event
    /// can recognise it (e.g. `event-42@status.example.com`).
    pub uid: String,
    pub title: String,
    pub start: DateTime<Utc>,
    /// None when no end was announced: the entry then lasts no time at all.
    pub end: Option<DateTime<Utc>>,
    /// When the event last changed (`DTSTAMP`).
    pub stamp: DateTime<Utc>,
    /// Plain text, already composed by the caller. The page's address goes
    /// here, not in a `URL` property: Outlook and Google read only the text,
    /// and Apple Calendar would show the address twice.
    pub description: String,
}

impl CalendarEntry {
    /// The entry as an iCalendar file (RFC 5545) with CRLF line endings.
    pub fn ics(&self) -> String {
        let mut lines = vec![
            "BEGIN:VCALENDAR".to_string(),
            "VERSION:2.0".to_string(),
            format!("PRODID:-//Statup//Statup {}//EN", env!("CARGO_PKG_VERSION")),
            "CALSCALE:GREGORIAN".to_string(),
            "METHOD:PUBLISH".to_string(),
            "BEGIN:VEVENT".to_string(),
            format!("UID:{}", escape_text(&self.uid)),
            format!("DTSTAMP:{}", format_date(self.stamp)),
            format!("DTSTART:{}", format_date(self.start)),
        ];
        if let Some(end) = self.end {
            lines.push(format!("DTEND:{}", format_date(end)));
        }
        lines.push(format!("SUMMARY:{}", escape_text(&self.title)));
        if !self.description.is_empty() {
            lines.push(format!("DESCRIPTION:{}", escape_text(&self.description)));
        }
        lines.push("END:VEVENT".to_string());
        lines.push("END:VCALENDAR".to_string());
        lines.iter().map(|line| fold_line(line)).collect()
    }

    /// A link that opens Google Calendar with the event filled in.
    pub fn google_link(&self) -> String {
        let start = format_date(self.start);
        let end = format_date(self.end.unwrap_or(self.start));
        let text = utf8_percent_encode(&self.title, NON_ALPHANUMERIC);
        let details = if self.description.is_empty() {
            String::new()
        } else {
            let description = utf8_percent_encode(&self.description, NON_ALPHANUMERIC);
            format!("&details={description}")
        };
        format!("{GOOGLE_RENDER_URL}?action=TEMPLATE&text={text}&dates={start}/{end}{details}")
    }
}

fn format_date(moment: DateTime<Utc>) -> String {
    moment.format(DATE_FORMAT).to_string()
}

/// TEXT escaping of RFC 5545 section 3.3.11.
fn escape_text(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => escaped.push_str("\\\\"),
            ';' => escaped.push_str("\\;"),
            ',' => escaped.push_str("\\,"),
            '\r' => {
                chars.next_if_eq(&'\n');
                escaped.push_str("\\n");
            }
            '\n' => escaped.push_str("\\n"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// Splits a content line so no physical line exceeds 75 octets, continuation
/// space included, and ends each physical line with CRLF. An escape such as
/// `\n` stays on one line: the standard allows the cut, some readers do not.
fn fold_line(line: &str) -> String {
    let mut folded = String::with_capacity(line.len() + 8);
    let mut octets_in_line = 0;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        let mut unit = c.to_string();
        if c == '\\' {
            unit.extend(chars.next());
        }
        if octets_in_line + unit.len() > MAX_LINE_OCTETS {
            folded.push_str("\r\n ");
            octets_in_line = 1;
        }
        folded.push_str(&unit);
        octets_in_line += unit.len();
    }
    folded.push_str("\r\n");
    folded
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn entry() -> CalendarEntry {
        CalendarEntry {
            uid: "event-42@status.example.com".to_string(),
            title: "Mail maintenance".to_string(),
            start: Utc.with_ymd_and_hms(2026, 11, 2, 1, 0, 0).unwrap(),
            end: Some(Utc.with_ymd_and_hms(2026, 11, 2, 3, 30, 0).unwrap()),
            stamp: Utc.with_ymd_and_hms(2026, 10, 28, 9, 15, 0).unwrap(),
            description: String::new(),
        }
    }

    fn unfold(ics: &str) -> String {
        ics.replace("\r\n ", "")
    }

    #[test]
    fn the_file_has_the_expected_shape_and_crlf_endings() {
        let ics = entry().ics();
        assert!(ics.starts_with("BEGIN:VCALENDAR\r\n"));
        assert!(ics.ends_with("END:VCALENDAR\r\n"));
        assert_eq!(ics.matches('\n').count(), ics.matches("\r\n").count());
        assert!(ics.contains("DTSTART:20261102T010000Z\r\n"));
        assert!(ics.contains("DTEND:20261102T033000Z\r\n"));
        assert!(ics.contains("DTSTAMP:20261028T091500Z\r\n"));
        assert!(ics.contains("UID:event-42@status.example.com\r\n"));
        let producer = format!(
            "PRODID:-//Statup//Statup {}//EN\r\n",
            env!("CARGO_PKG_VERSION")
        );
        assert!(ics.contains(&producer));
        assert!(!ics.contains("DESCRIPTION"));
        assert!(!ics.contains("URL:"));
    }

    #[test]
    fn an_entry_without_end_has_no_dtend_and_a_zero_length_google_link() {
        let mut entry = entry();
        entry.end = None;
        assert!(!entry.ics().contains("DTEND"));
        assert!(
            entry
                .google_link()
                .contains("dates=20261102T010000Z/20261102T010000Z")
        );
    }

    #[test]
    fn text_values_are_escaped() {
        let mut entry = entry();
        entry.title = "Mail, VPN; files\\backup".to_string();
        entry.description = "First line\r\nSecond line\nThird\rFourth".to_string();
        let ics = entry.ics();
        assert!(ics.contains("SUMMARY:Mail\\, VPN\\; files\\\\backup\r\n"));
        assert!(ics.contains("DESCRIPTION:First line\\nSecond line\\nThird\\nFourth\r\n"));
    }

    #[test]
    fn a_title_cannot_add_lines_to_the_file() {
        let mut entry = entry();
        entry.title = "Mail\r\nEND:VEVENT\r\nBEGIN:VEVENT\nSUMMARY:Fake".to_string();
        let ics = entry.ics();
        assert_eq!(ics.matches("\r\nEND:VEVENT\r\n").count(), 1);
        assert_eq!(ics.matches("\r\nSUMMARY:").count(), 1);
    }

    #[test]
    fn long_lines_are_folded_on_character_boundaries() {
        let mut entry = entry();
        entry.title = format!("{} and more, with a comma", "é".repeat(60));
        let ics = entry.ics();
        let lines: Vec<&str> = ics.split("\r\n").collect();
        assert!(lines.iter().all(|line| line.len() <= MAX_LINE_OCTETS));
        let summary_start = lines
            .iter()
            .position(|line| line.starts_with("SUMMARY:"))
            .unwrap();
        assert!(lines[summary_start + 1].starts_with(' '));
        assert!(!lines[summary_start + 1].starts_with("  "));
        let expected = format!("SUMMARY:{}\r\n", escape_text(&entry.title));
        assert!(unfold(&ics).contains(&expected));
    }

    #[test]
    fn a_line_of_exactly_75_octets_is_not_folded() {
        let line = "x".repeat(MAX_LINE_OCTETS);
        assert_eq!(fold_line(&line), format!("{line}\r\n"));
        let longer = "x".repeat(MAX_LINE_OCTETS + 1);
        assert_eq!(fold_line(&longer), format!("{}\r\n x\r\n", "x".repeat(75)));
    }

    #[test]
    fn an_escape_is_never_cut_in_two() {
        let line = format!("{}\\n{}", "x".repeat(MAX_LINE_OCTETS - 1), "y");
        assert_eq!(
            fold_line(&line),
            format!("{}\r\n \\ny\r\n", "x".repeat(MAX_LINE_OCTETS - 1))
        );
    }

    #[test]
    fn the_google_link_encodes_the_title_and_formats_the_dates() {
        let mut entry = entry();
        entry.title = "Mail & VPN #1".to_string();
        entry.description = "Details at https://status.example.com".to_string();
        let link = entry.google_link();
        let query = link
            .strip_prefix("https://calendar.google.com/calendar/render?")
            .unwrap();
        let params: Vec<&str> = query.split('&').collect();
        assert_eq!(params.len(), 4);
        assert_eq!(params[0], "action=TEMPLATE");
        assert_eq!(params[1], "text=Mail%20%26%20VPN%20%231");
        assert_eq!(params[2], "dates=20261102T010000Z/20261102T033000Z");
        assert!(params[3].starts_with("details=Details%20at%20https%3A%2F%2F"));
    }

    #[test]
    fn the_google_link_has_no_details_without_a_description() {
        let link = entry().google_link();
        assert!(!link.contains("details"));
    }
}
