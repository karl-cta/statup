//! Event pages: the list and its search, the event page and its side panel,
//! the forms, updates, state changes and templates.

use serde::{Deserialize, Deserializer};

mod detail;
mod form;
mod list;
mod templates;
mod updates;

pub use detail::{detail, drawer_content};
pub use form::{create, edit_form, new_form, update};
pub use list::list;
pub use templates::{template_delete, template_detail, template_search};
pub use updates::{add_update, add_update_in_panel, delete, delete_update, revert_lifecycle};

fn deserialize_blank_as_none<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let raw = String::deserialize(deserializer)?;
    if raw.trim().is_empty() {
        return Ok(None);
    }
    T::deserialize(serde::de::value::StringDeserializer::<D::Error>::new(raw)).map(Some)
}

/// A number field left blank means none; a filled one is parsed, since a
/// form sends text.
fn deserialize_blank_as_none_id<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    trimmed.parse().map(Some).map_err(serde::de::Error::custom)
}
