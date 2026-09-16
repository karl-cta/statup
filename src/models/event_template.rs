//! Event template: a reusable preset suggested while typing a title.

use super::event::{SEPARATOR, describe_kind};
use super::{Category, Kind, Severity};
use crate::i18n::I18n;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EventTemplate {
    pub id: i64,
    pub title: String,
    pub description: String,
    pub kind: Kind,
    pub severity: Option<Severity>,
    pub planned: bool,
    pub category: Option<Category>,
    pub usage_count: i64,
}

impl EventTemplate {
    /// "Incident · Major outage · used 3 times".
    pub fn summary(&self, i18n: &I18n) -> String {
        let kind = describe_kind(self.kind, self.severity, self.category, i18n);
        match usize::try_from(self.usage_count) {
            Ok(count) if count > 0 => {
                format!(
                    "{kind}{SEPARATOR}{}",
                    i18n.plural("events.template_used", count)
                )
            }
            _ => kind,
        }
    }
}
