//! Internationalization (i18n) module.
//!
//! Provides translation lookup, locale detection from cookies/headers,
//! and date formatting helpers. Translations are embedded at compile time
//! from JSON files in the `locales/` directory.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::LazyLock;

use async_trait::async_trait;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use chrono::{DateTime, Datelike, NaiveDate, Utc};

type TranslationMap = HashMap<String, String>;

/// All supported translations, keyed by locale code.
static TRANSLATIONS: LazyLock<HashMap<&'static str, TranslationMap>> = LazyLock::new(|| {
    let mut map = HashMap::new();

    let fr: TranslationMap = serde_json::from_str(include_str!("../locales/fr.json"))
        .expect("Failed to parse locales/fr.json");
    let en: TranslationMap = serde_json::from_str(include_str!("../locales/en.json"))
        .expect("Failed to parse locales/en.json");

    map.insert("fr", fr);
    map.insert("en", en);
    map
});

/// Default locale, configurable via `DEFAULT_LOCALE` env var at startup.
static DEFAULT_LOCALE: LazyLock<String> =
    LazyLock::new(|| std::env::var("DEFAULT_LOCALE").unwrap_or_else(|_| "fr".to_string()));

/// Available locale codes.
pub const LOCALES: &[&str] = &["fr", "en"];

/// Per-request internationalization context.
///
/// Holds the current locale and provides translation lookup.
/// Passed to every Askama template for string localization.
#[derive(Clone, Debug)]
pub struct I18n {
    locale: String,
}

impl Default for I18n {
    fn default() -> Self {
        Self::new(&DEFAULT_LOCALE)
    }
}

impl I18n {
    /// Create a new `I18n` for the given locale.
    /// Falls back to the default locale if the requested one is unsupported.
    pub fn new(locale: &str) -> Self {
        let locale = if TRANSLATIONS.contains_key(locale) {
            locale.to_string()
        } else {
            DEFAULT_LOCALE.clone()
        };
        Self { locale }
    }

    /// Look up a translation key. Returns the translated string for the
    /// current locale, falling back to the default locale, then to the raw key.
    pub fn t<'a>(&'a self, key: &'a str) -> &'a str {
        TRANSLATIONS
            .get(self.locale.as_str())
            .and_then(|map| map.get(key))
            .or_else(|| {
                TRANSLATIONS
                    .get(DEFAULT_LOCALE.as_str())
                    .and_then(|map| map.get(key))
            })
            .map_or(key, String::as_str)
    }

    /// Current locale code (e.g. "fr", "en").
    pub fn locale(&self) -> &str {
        &self.locale
    }

    /// Whether the current locale is French.
    pub fn is_fr(&self) -> bool {
        self.locale == "fr"
    }

    /// Whether the current locale is English.
    pub fn is_en(&self) -> bool {
        self.locale == "en"
    }

    /// Format a datetime for display (short form, no timezone).
    /// FR: "05/02/2026 14:30", EN: "02/05/2026 2:30 PM"
    pub fn format_datetime(&self, dt: &DateTime<Utc>) -> String {
        match self.locale.as_str() {
            "en" => dt.format("%m/%d/%Y %I:%M %p").to_string(),
            _ => dt.format("%d/%m/%Y %H:%M").to_string(),
        }
    }

    /// Format just the time part of a datetime.
    /// FR: "14:30", EN: "02:30 PM"
    pub fn format_time(&self, dt: &DateTime<Utc>) -> String {
        match self.locale.as_str() {
            "en" => dt.format("%I:%M %p").to_string(),
            _ => dt.format("%H:%M").to_string(),
        }
    }

    /// Format a datetime for display (long form, with timezone label).
    /// FR: "05/02/2026 à 14:30 UTC", EN: "02/05/2026 at 2:30 PM UTC"
    pub fn format_datetime_long(&self, dt: &DateTime<Utc>) -> String {
        match self.locale.as_str() {
            "en" => dt.format("%m/%d/%Y at %I:%M %p UTC").to_string(),
            _ => dt.format("%d/%m/%Y à %H:%M UTC").to_string(),
        }
    }

    /// Format a date with a locale-aware month name (e.g. "5 février" or "February 5").
    pub fn format_date_month_day(&self, date: &NaiveDate) -> String {
        let month_key = format!("month.{}", date.month());
        let month = self.t(&month_key);
        match self.locale.as_str() {
            "en" => format!("{} {}", month, date.day()),
            _ => format!("{} {}", date.day(), month),
        }
    }

    /// Format a date with month name and year (e.g. "5 février 2025" or "February 5, 2025").
    pub fn format_date_full(&self, date: &NaiveDate) -> String {
        let month_key = format!("month.{}", date.month());
        let month = self.t(&month_key);
        match self.locale.as_str() {
            "en" => format!("{} {}, {}", month, date.day(), date.year()),
            _ => format!("{} {} {}", date.day(), month, date.year()),
        }
    }

    /// Relative date label: "Today", "Yesterday", or a formatted date.
    pub fn date_label(&self, date: &NaiveDate) -> String {
        let today = Utc::now().date_naive();
        let yesterday = today - chrono::Duration::days(1);

        if *date == today {
            self.t("date.today").to_string()
        } else if *date == yesterday {
            self.t("date.yesterday").to_string()
        } else if date.year() == today.year() {
            self.format_date_month_day(date)
        } else {
            self.format_date_full(date)
        }
    }

    /// Countdown string (e.g. "3d 2h" or "45min").
    pub fn format_countdown(&self, days: i64, hours: i64, minutes: i64) -> String {
        match self.locale.as_str() {
            "en" => {
                if days > 0 {
                    format!("{days}d {hours}h")
                } else if hours > 0 {
                    format!("{hours}h {minutes:02}min")
                } else {
                    format!("{minutes}min")
                }
            }
            _ => {
                if days > 0 {
                    format!("{days}j {hours}h")
                } else if hours > 0 {
                    format!("{hours}h {minutes:02}min")
                } else {
                    format!("{minutes}min")
                }
            }
        }
    }

    /// Format countdown from `countdown_parts()` result. Returns `None` if not applicable.
    /// Designed for Askama templates to avoid destructuring issues.
    pub fn format_countdown_opt(&self, parts: Option<Option<(i64, i64, i64)>>) -> Option<String> {
        match parts {
            Some(Some((d, h, m))) => Some(self.format_countdown(d, h, m)),
            Some(None) => Some(self.t("time.now").to_string()),
            None => None,
        }
    }
}

/// Axum extractor that detects the user's preferred locale from:
/// 1. `lang` cookie
/// 2. `Accept-Language` header
/// 3. Default locale (env `DEFAULT_LOCALE`, defaults to "fr")
pub struct Locale(pub I18n);

#[async_trait]
impl<S> FromRequestParts<S> for Locale
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // 1. Check `lang` cookie
        if let Some(lang) = extract_cookie_lang(&parts.headers)
            && TRANSLATIONS.contains_key(lang.as_str())
        {
            return Ok(Locale(I18n::new(&lang)));
        }

        // 2. Check Accept-Language header
        if let Some(lang) = parse_accept_language(&parts.headers) {
            return Ok(Locale(I18n::new(&lang)));
        }

        // 3. Fall back to default
        Ok(Locale(I18n::new(&DEFAULT_LOCALE)))
    }
}

/// Extract the `lang` value from the Cookie header.
fn extract_cookie_lang(headers: &axum::http::HeaderMap) -> Option<String> {
    let cookie_header = headers.get("cookie")?.to_str().ok()?;
    cookie_header
        .split(';')
        .map(str::trim)
        .find(|c| c.starts_with("lang="))
        .map(|c| c[5..].to_string())
}

/// Parse the `Accept-Language` header and return the best supported locale.
fn parse_accept_language(headers: &axum::http::HeaderMap) -> Option<String> {
    let header = headers.get("accept-language")?.to_str().ok()?;

    let mut langs: Vec<(f32, String)> = header
        .split(',')
        .filter_map(|part| {
            let mut iter = part.trim().split(';');
            let lang = iter.next()?.trim().to_lowercase();
            let quality = iter
                .next()
                .and_then(|q| q.trim().strip_prefix("q="))
                .and_then(|q| q.parse::<f32>().ok())
                .unwrap_or(1.0);
            // Normalize to 2-char language code
            let code = lang.split('-').next()?.to_string();
            Some((quality, code))
        })
        .collect();

    // Sort by quality descending
    langs.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    langs
        .into_iter()
        .find(|(_, lang)| TRANSLATIONS.contains_key(lang.as_str()))
        .map(|(_, lang)| lang)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i18n_french_translation() {
        let i18n = I18n::new("fr");
        assert_eq!(i18n.t("nav.dashboard"), "Dashboard");
        assert_eq!(i18n.t("nav.events"), "Événements");
    }

    #[test]
    fn i18n_english_translation() {
        let i18n = I18n::new("en");
        assert_eq!(i18n.t("nav.events"), "Events");
        assert_eq!(i18n.t("nav.login"), "Sign in");
    }

    #[test]
    fn i18n_missing_key_returns_key() {
        let i18n = I18n::new("fr");
        assert_eq!(i18n.t("nonexistent.key"), "nonexistent.key");
    }

    #[test]
    fn i18n_unsupported_locale_falls_back() {
        let i18n = I18n::new("de");
        // Should fall back to default (fr)
        assert_eq!(i18n.t("nav.events"), "Événements");
    }

    #[test]
    fn i18n_date_formatting_fr() {
        let i18n = I18n::new("fr");
        let date = NaiveDate::from_ymd_opt(2026, 2, 5).unwrap();
        let formatted = i18n.format_date_month_day(&date);
        assert_eq!(formatted, "5 février");
    }

    #[test]
    fn i18n_date_formatting_en() {
        let i18n = I18n::new("en");
        let date = NaiveDate::from_ymd_opt(2026, 2, 5).unwrap();
        let formatted = i18n.format_date_month_day(&date);
        assert_eq!(formatted, "February 5");
    }

    #[test]
    fn i18n_countdown_fr() {
        let i18n = I18n::new("fr");
        assert_eq!(i18n.format_countdown(3, 2, 15), "3j 2h");
        assert_eq!(i18n.format_countdown(0, 5, 30), "5h 30min");
        assert_eq!(i18n.format_countdown(0, 0, 42), "42min");
    }

    #[test]
    fn i18n_countdown_en() {
        let i18n = I18n::new("en");
        assert_eq!(i18n.format_countdown(3, 2, 15), "3d 2h");
    }

    #[test]
    fn all_fr_keys_exist_in_en() {
        let fr = TRANSLATIONS.get("fr").expect("fr locale missing");
        let en = TRANSLATIONS.get("en").expect("en locale missing");
        for key in fr.keys() {
            assert!(en.contains_key(key), "key {key:?} missing in en.json");
        }
    }

    #[test]
    fn all_en_keys_exist_in_fr() {
        let fr = TRANSLATIONS.get("fr").expect("fr locale missing");
        let en = TRANSLATIONS.get("en").expect("en locale missing");
        for key in en.keys() {
            assert!(fr.contains_key(key), "key {key:?} missing in fr.json");
        }
    }
}
