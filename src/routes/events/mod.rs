//! Event pages: the list and its search, the event page and its side panel,
//! the forms, updates, state changes, reviews and templates.

use serde::de::value::{I64Deserializer, StrDeserializer};
use serde::{Deserialize, Deserializer};

use crate::db::DbPool;
use crate::journal;
use crate::models::{JournalKind, User};
use crate::repositories::{EventRepository, NewJournalEntry};

mod detail;
mod form;
mod list;
mod review;
mod templates;
mod updates;

pub use detail::{detail, drawer_content};
pub use form::{create, edit_form, new_form, update};
pub use list::list;
pub use review::{publish_review, remove_review};
pub use templates::{template_delete, template_detail, template_search};
pub use updates::{
    add_update, add_update_in_panel, delete, delete_update, revert_lifecycle, stop_repeat,
};

/// The title an event has now, which the journal copies to name it. A failed
/// read leaves the line unnamed rather than failing what was already done.
async fn title_of(pool: &DbPool, id: i64) -> String {
    match EventRepository::find_by_id(pool, id).await {
        Ok(Some(event)) => event.title,
        _ => String::new(),
    }
}

/// Writes what a member did to an event, named by the title it has now,
/// read after the action and only when the line will be kept.
async fn record_event(
    pool: &DbPool,
    kind: JournalKind,
    user: &User,
    id: i64,
    detail: Option<&str>,
) {
    let detailed = journal::captures_details(pool).await;
    if !journal::kept(kind, detailed) {
        return;
    }
    let mut entry = NewJournalEntry::new(kind)
        .by(user)
        .about(title_of(pool, id).await);
    if let Some(detail) = detail {
        entry = entry.with_detail(detail);
    }
    journal::record_with(pool, entry, detailed).await;
}

/// A field left blank means none. A filled one is read as text, or as a
/// number when the field holds one, since a form sends everything as text.
pub(crate) fn deserialize_blank_as_none<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let raw = String::deserialize(deserializer)?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    match T::deserialize(StrDeserializer::<D::Error>::new(&raw)) {
        Ok(value) => Ok(Some(value)),
        Err(error) => match trimmed.parse::<i64>() {
            Ok(number) => T::deserialize(I64Deserializer::<D::Error>::new(number)).map(Some),
            Err(_) => Err(error),
        },
    }
}
