//! Translations and locale-aware formatting.
//!
//! Strings are embedded at compile time from `locales/*.json`. Every absolute
//! time is shown in the instance time zone (see [`crate::clock`]).

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::LazyLock;

use async_trait::async_trait;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use chrono::{DateTime, Datelike, NaiveDate, Utc};

use crate::clock;
use crate::models::Countdown;

type TranslationMap = HashMap<String, String>;

/// Supported locale codes.
pub const LOCALES: &[&str] = &["fr", "en"];

/// Largest number of `Accept-Language` entries considered.
const MAX_ACCEPT_LANGUAGE_ENTRIES: usize = 16;

static TRANSLATIONS: LazyLock<HashMap<&'static str, TranslationMap>> = LazyLock::new(|| {
    HashMap::from([
        ("fr", parse_locale(include_str!("../locales/fr.json"))),
        ("en", parse_locale(include_str!("../locales/en.json"))),
    ])
});

/// `DEFAULT_LOCALE` when it names a supported locale, French otherwise.
static DEFAULT_LOCALE: LazyLock<String> = LazyLock::new(|| {
    let requested = std::env::var("DEFAULT_LOCALE").unwrap_or_default();
    if LOCALES.contains(&requested.as_str()) {
        return requested;
    }
    if !requested.is_empty() {
        tracing::warn!(value = %requested, "DEFAULT_LOCALE is not fr or en, using fr");
    }
    "fr".to_string()
});

/// A malformed file is caught by the tests below; at runtime it degrades to
/// raw keys instead of taking the server down.
fn parse_locale(raw: &str) -> TranslationMap {
    serde_json::from_str(raw).unwrap_or_default()
}

/// Per-request translation context, handed to every template.
#[derive(Clone, Debug)]
pub struct I18n {
    locale: &'static str,
}

impl Default for I18n {
    fn default() -> Self {
        Self::new(&DEFAULT_LOCALE)
    }
}

impl I18n {
    /// Falls back to the default locale when `locale` is not supported.
    pub fn new(locale: &str) -> Self {
        let locale = LOCALES
            .iter()
            .find(|l| **l == locale)
            .or_else(|| LOCALES.iter().find(|l| **l == DEFAULT_LOCALE.as_str()))
            .copied()
            .unwrap_or("fr");
        Self { locale }
    }

    /// The translation of `key`, then the default locale's, then the key.
    pub fn t<'a>(&self, key: &'a str) -> &'a str {
        lookup(self.locale, key)
            .or_else(|| lookup(&DEFAULT_LOCALE, key))
            .unwrap_or(key)
    }

    /// The translation of `key` with each `{name}` replaced by its value.
    pub fn tf(&self, key: &str, args: &[(&str, &str)]) -> String {
        args.iter()
            .fold(self.t(key).to_string(), |text, (name, value)| {
                text.replace(&format!("{{{name}}}"), value)
            })
    }

    /// `tf` on `base.one` or `base.other`, with `{n}` set to `count`.
    pub fn plural(&self, base: &str, count: usize) -> String {
        let singular = if self.locale == "fr" {
            count <= 1
        } else {
            count == 1
        };
        let key = format!("{base}.{}", if singular { "one" } else { "other" });
        self.tf(&key, &[("n", &count.to_string())])
    }

    pub fn locale(&self) -> &'static str {
        self.locale
    }

    pub fn is_fr(&self) -> bool {
        self.locale == "fr"
    }

    /// The other supported locale, offered by the language switch.
    pub fn other_locale(&self) -> &'static str {
        if self.is_fr() { "en" } else { "fr" }
    }

    pub fn format_time(&self, dt: &DateTime<Utc>) -> String {
        let pattern = if self.is_fr() { "%H:%M" } else { "%-I:%M %p" };
        clock::local(dt).format(pattern).to_string()
    }

    /// "5 sept. à 19:02", with the year when it is not the current one.
    pub fn format_datetime(&self, dt: &DateTime<Utc>) -> String {
        self.tf(
            "datetime.pattern",
            &[
                ("date", &self.format_date_short(&clock::local_date(dt))),
                ("time", &self.format_time(dt)),
            ],
        )
    }

    /// `format_datetime` followed by the zone: "5 sept. à 19:02 (UTC+2)".
    pub fn format_datetime_long(&self, dt: &DateTime<Utc>) -> String {
        format!("{} ({})", self.format_datetime(dt), clock::offset_label(dt))
    }

    /// "5 sept." or "5 sept. 2025".
    pub fn format_date_short(&self, date: &NaiveDate) -> String {
        let month = self.t(month_key("month_short", date.month()));
        let key = if date.year() == clock::today().year() {
            "date.short"
        } else {
            "date.short_year"
        };
        self.tf(key, &as_refs(&date_args(*date, month)))
    }

    /// "5 septembre" or "5 septembre 2025".
    pub fn format_date_long(&self, date: &NaiveDate) -> String {
        let month = self.t(month_key("month", date.month()));
        let key = if date.year() == clock::today().year() {
            "date.long"
        } else {
            "date.long_year"
        };
        self.tf(key, &as_refs(&date_args(*date, month)))
    }

    /// "mercredi 16 sept.", the masthead dateline.
    pub fn format_dateline(&self, date: &NaiveDate) -> String {
        let weekday = self.t(weekday_key(date.weekday().number_from_monday()));
        let month = self.t(month_key("month_short", date.month()));
        let mut args = date_args(*date, month);
        args.push(("weekday", weekday.to_string()));
        self.tf("date.dateline", &as_refs(&args))
    }

    /// "Aujourd'hui", "Hier", or the date.
    pub fn date_label(&self, date: &NaiveDate) -> String {
        let today = clock::today();
        if *date == today {
            self.t("date.today").to_string()
        } else if Some(*date) == today.pred_opt() {
            self.t("date.yesterday").to_string()
        } else {
            self.format_date_long(date)
        }
    }

    /// "3 j 2 h", "3 j", "5 h 30 min", "5 h" or "42 min".
    pub fn format_duration(&self, parts: &(i64, i64, i64)) -> String {
        let (days, hours, minutes) = *parts;
        let (d, h, m) = (
            days.to_string(),
            hours.to_string(),
            minutes.max(1).to_string(),
        );
        match (days, hours, minutes) {
            (1.., 0, _) => self.tf("duration.days", &[("d", &d)]),
            (1.., _, _) => self.tf("duration.days_hours", &[("d", &d), ("h", &h)]),
            (_, 1.., 0) => self.tf("duration.hours", &[("h", &h)]),
            (_, 1.., _) => self.tf("duration.hours_minutes", &[("h", &h), ("m", &m)]),
            _ => self.tf("duration.minutes", &[("m", &m)]),
        }
    }

    /// "dans 3 j 2 h" before a planned start.
    pub fn format_countdown(&self, countdown: Option<Countdown>) -> Option<String> {
        let Countdown {
            days,
            hours,
            minutes,
        } = countdown?;
        Some(self.tf(
            "duration.in",
            &[("duration", &self.format_duration(&(days, hours, minutes)))],
        ))
    }

    /// Spoken summary of a 30 day availability strip, one label for the
    /// whole strip.
    pub fn format_availability(&self, ok: usize, incidents: usize, untracked: usize) -> String {
        let mut parts = vec![self.plural("availability.days_ok", ok)];
        if incidents > 0 {
            parts.push(self.plural("availability.days_incident", incidents));
        }
        if untracked > 0 {
            parts.push(self.plural("availability.days_untracked", untracked));
        }
        self.tf(
            "availability.summary",
            &[
                ("legend", self.t("availability.legend")),
                ("parts", &parts.join(", ")),
            ],
        )
    }
}

fn lookup(locale: &str, key: &str) -> Option<&'static str> {
    TRANSLATIONS
        .get(locale)
        .and_then(|map| map.get(key))
        .map(String::as_str)
}

fn date_args(date: NaiveDate, month: &str) -> Vec<(&'static str, String)> {
    vec![
        ("day", date.day().to_string()),
        ("month", month.to_string()),
        ("year", date.year().to_string()),
    ]
}

fn as_refs<'a>(args: &'a [(&'a str, String)]) -> Vec<(&'a str, &'a str)> {
    args.iter().map(|(k, v)| (*k, v.as_str())).collect()
}

fn month_key(prefix: &str, month: u32) -> &'static str {
    const LONG: [&str; 12] = [
        "month.1", "month.2", "month.3", "month.4", "month.5", "month.6", "month.7", "month.8",
        "month.9", "month.10", "month.11", "month.12",
    ];
    const SHORT: [&str; 12] = [
        "month_short.1",
        "month_short.2",
        "month_short.3",
        "month_short.4",
        "month_short.5",
        "month_short.6",
        "month_short.7",
        "month_short.8",
        "month_short.9",
        "month_short.10",
        "month_short.11",
        "month_short.12",
    ];
    let table = if prefix == "month_short" {
        &SHORT
    } else {
        &LONG
    };
    let index = usize::try_from(month.clamp(1, 12) - 1).unwrap_or(0);
    table[index]
}

fn weekday_key(day: u32) -> &'static str {
    const DAYS: [&str; 7] = [
        "weekday.1",
        "weekday.2",
        "weekday.3",
        "weekday.4",
        "weekday.5",
        "weekday.6",
        "weekday.7",
    ];
    let index = usize::try_from(day.clamp(1, 7) - 1).unwrap_or(0);
    DAYS[index]
}

/// Detects the reader's locale from the `lang` cookie, then the
/// `Accept-Language` header, then `DEFAULT_LOCALE`.
pub struct Locale(pub I18n);

#[async_trait]
impl<S> FromRequestParts<S> for Locale
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let chosen = cookie_locale(&parts.headers).or_else(|| accept_language(&parts.headers));
        Ok(Locale(I18n::new(
            chosen.as_deref().unwrap_or(DEFAULT_LOCALE.as_str()),
        )))
    }
}

fn cookie_locale(headers: &axum::http::HeaderMap) -> Option<String> {
    let cookies = headers.get("cookie")?.to_str().ok()?;
    cookies
        .split(';')
        .filter_map(|c| c.trim().strip_prefix("lang="))
        .find(|lang| LOCALES.contains(lang))
        .map(ToOwned::to_owned)
}

/// The best supported language of the header. Weights that are not finite
/// numbers are dropped: they would break the ordering the sort relies on.
fn accept_language(headers: &axum::http::HeaderMap) -> Option<String> {
    let header = headers.get("accept-language")?.to_str().ok()?;
    let mut langs: Vec<(f32, String)> = header
        .split(',')
        .take(MAX_ACCEPT_LANGUAGE_ENTRIES)
        .filter_map(parse_language_range)
        .collect();
    langs.sort_by(|a, b| b.0.total_cmp(&a.0));
    langs
        .into_iter()
        .map(|(_, lang)| lang)
        .find(|lang| LOCALES.contains(&lang.as_str()))
}

fn parse_language_range(part: &str) -> Option<(f32, String)> {
    let mut iter = part.trim().split(';');
    let code = iter.next()?.trim().split('-').next()?.to_lowercase();
    let quality = match iter.next() {
        Some(q) => q.trim().strip_prefix("q=")?.parse::<f32>().ok()?,
        None => 1.0,
    };
    quality.is_finite().then_some((quality, code))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderValue};
    use chrono::TimeZone;

    fn headers(name: &'static str, value: &'static str) -> HeaderMap {
        let mut map = HeaderMap::new();
        map.insert(name, HeaderValue::from_static(value));
        map
    }

    #[test]
    fn translations_are_found_in_both_locales() {
        assert_eq!(I18n::new("fr").t("nav.login"), "Connexion");
        assert_eq!(I18n::new("en").t("nav.login"), "Sign in");
    }

    #[test]
    fn missing_key_returns_the_key() {
        assert_eq!(I18n::new("fr").t("nonexistent.key"), "nonexistent.key");
    }

    #[test]
    fn unsupported_locale_falls_back() {
        assert_eq!(I18n::new("de").locale(), "fr");
    }

    #[test]
    fn placeholders_are_filled() {
        let i18n = I18n::new("en");
        assert_eq!(
            i18n.tf("duration.hours_minutes", &[("h", "5"), ("m", "30")]),
            "5h 30 min"
        );
    }

    #[test]
    fn a_duration_drops_a_zero_part() {
        let en = I18n::new("en");
        assert_eq!(en.format_duration(&(3, 0, 20)), "3d");
        assert_eq!(en.format_duration(&(3, 2, 0)), "3d 2h");
        assert_eq!(en.format_duration(&(0, 3, 0)), "3h");
        assert_eq!(en.format_duration(&(0, 3, 5)), "3h 5 min");
        assert_eq!(en.format_duration(&(0, 0, 0)), "1 min");
    }

    #[test]
    fn plural_follows_each_language() {
        assert_eq!(
            I18n::new("fr").plural("availability.days_ok", 0),
            "0 jour sans incident"
        );
        assert_eq!(
            I18n::new("en").plural("availability.days_ok", 0),
            "0 days without an incident"
        );
        assert_eq!(
            I18n::new("en").plural("availability.days_ok", 1),
            "1 day without an incident"
        );
    }

    #[test]
    fn durations_use_the_largest_units() {
        let fr = I18n::new("fr");
        assert_eq!(fr.format_duration(&(3, 2, 15)), "3\u{a0}j 2\u{a0}h");
        assert_eq!(fr.format_duration(&(0, 5, 30)), "5\u{a0}h 30\u{a0}min");
        assert_eq!(fr.format_duration(&(0, 0, 0)), "1\u{a0}min");
    }

    #[test]
    fn dates_carry_the_year_only_when_needed() {
        let fr = I18n::new("fr");
        let old = NaiveDate::from_ymd_opt(2020, 2, 5).unwrap();
        assert_eq!(fr.format_date_long(&old), "5 février 2020");
        assert_eq!(fr.format_date_short(&old), "5 févr. 2020");
        let en = I18n::new("en");
        assert_eq!(en.format_date_long(&old), "February 5, 2020");
    }

    #[test]
    fn dateline_names_the_weekday() {
        let date = NaiveDate::from_ymd_opt(2026, 9, 16).unwrap();
        assert_eq!(I18n::new("fr").format_dateline(&date), "mercredi 16 sept.");
        assert_eq!(I18n::new("en").format_dateline(&date), "Wednesday, Sep 16");
    }

    #[test]
    fn a_countdown_reads_as_time_left() {
        let en = I18n::new("en");
        let text = en.format_countdown(Some(Countdown {
            days: 0,
            hours: 0,
            minutes: 42,
        }));
        assert_eq!(text.as_deref(), Some("in 42 min"));
        assert!(en.format_countdown(None).is_none());
    }

    #[test]
    fn availability_summary_is_one_sentence() {
        let en = I18n::new("en");
        assert_eq!(
            en.format_availability(28, 2, 0),
            "Last 30 days: 28 days without an incident, 2 days with an incident"
        );
    }

    #[test]
    fn times_follow_the_language() {
        let dt = Utc.with_ymd_and_hms(2026, 9, 16, 8, 5, 0).unwrap();
        assert!(I18n::new("en").format_time(&dt).ends_with('M'));
        assert_eq!(I18n::new("fr").format_time(&dt).len(), 5);
    }

    #[test]
    fn cookie_wins_over_header() {
        let mut map = headers("cookie", "theme=dark; lang=en");
        map.insert("accept-language", HeaderValue::from_static("fr-FR"));
        assert_eq!(cookie_locale(&map).as_deref(), Some("en"));
    }

    #[test]
    fn accept_language_prefers_the_highest_weight() {
        let map = headers("accept-language", "de;q=1.0, en;q=0.9, fr;q=0.8");
        assert_eq!(accept_language(&map).as_deref(), Some("en"));
    }

    #[test]
    fn non_finite_weights_are_ignored() {
        let map = headers("accept-language", "en;q=NaN, fr;q=0.5, en;q=inf");
        assert_eq!(accept_language(&map).as_deref(), Some("fr"));
    }

    #[test]
    fn locale_files_parse_and_share_their_keys() {
        let fr: TranslationMap =
            serde_json::from_str(include_str!("../locales/fr.json")).expect("fr.json parses");
        let en: TranslationMap =
            serde_json::from_str(include_str!("../locales/en.json")).expect("en.json parses");
        let missing_in_en: Vec<_> = fr.keys().filter(|k| !en.contains_key(*k)).collect();
        let missing_in_fr: Vec<_> = en.keys().filter(|k| !fr.contains_key(*k)).collect();
        assert!(missing_in_en.is_empty(), "missing in en: {missing_in_en:?}");
        assert!(missing_in_fr.is_empty(), "missing in fr: {missing_in_fr:?}");
    }

    /// Every key named in a template or in Rust is translated: a missing one
    /// would be shown to people as a raw key.
    #[test]
    fn every_key_named_in_the_code_is_translated() {
        let fr: TranslationMap =
            serde_json::from_str(include_str!("../locales/fr.json")).expect("fr.json parses");
        let namespaces: std::collections::HashSet<&str> =
            fr.keys().filter_map(|key| key.split('.').next()).collect();
        let is_translated = |key: &str| {
            fr.contains_key(key)
                || (fr.contains_key(&format!("{key}.one"))
                    && fr.contains_key(&format!("{key}.other")))
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut missing = Vec::new();
        for file in source_files(&root.join("src"))
            .into_iter()
            .chain(source_files(&root.join("templates")))
        {
            let text = std::fs::read_to_string(&file).expect("source file reads");
            let code = text
                .split("#[cfg(test)]\nmod tests")
                .next()
                .unwrap_or_default();
            let keys = code.split('"').filter(|s| looks_like_key(s));
            for key in keys {
                let namespace = key.split('.').next().unwrap_or_default();
                if namespaces.contains(namespace) && !is_translated(key) {
                    missing.push(format!("{}: {key}", file.display()));
                }
            }
        }
        assert!(missing.is_empty(), "untranslated keys: {missing:#?}");
    }

    /// And the other way round: a key that no template and no Rust names is
    /// a text nobody sees any more. A plural is named by its base.
    #[test]
    fn every_translated_key_is_named_in_the_code() {
        let fr: TranslationMap =
            serde_json::from_str(include_str!("../locales/fr.json")).expect("fr.json parses");
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut named = std::collections::HashSet::new();
        for file in source_files(&root.join("src"))
            .into_iter()
            .chain(source_files(&root.join("templates")))
        {
            let text = std::fs::read_to_string(&file).expect("source file reads");
            let code = text
                .split("#[cfg(test)]\nmod tests")
                .next()
                .unwrap_or_default();
            named.extend(code.split('"').map(ToOwned::to_owned));
        }
        let unused: Vec<&String> = fr
            .keys()
            .filter(|key| {
                let base = key
                    .strip_suffix(".one")
                    .or_else(|| key.strip_suffix(".other"))
                    .unwrap_or(key);
                !named.contains(key.as_str()) && !named.contains(base)
            })
            .collect();
        assert!(unused.is_empty(), "keys named nowhere: {unused:#?}");
    }

    fn source_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let entries = std::fs::read_dir(dir).expect("source directory reads");
        let mut files = Vec::new();
        for path in entries.map(|entry| entry.expect("directory entry").path()) {
            if path.is_dir() {
                files.extend(source_files(&path));
            } else if path
                .extension()
                .is_some_and(|ext| ext == "rs" || ext == "html")
            {
                files.push(path);
            }
        }
        files
    }

    /// `namespace.key`, not a file name and not a prefix ending in `_`.
    fn looks_like_key(text: &str) -> bool {
        let shaped = text.contains('.')
            && text
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.');
        let file_name = [".html", ".xml", ".css", ".js", ".json", ".svg", ".png"]
            .iter()
            .any(|ext| text.ends_with(ext));
        shaped && !file_name && !text.starts_with('.') && !text.ends_with(['.', '_'])
    }
}
