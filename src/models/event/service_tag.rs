//! The services an event names, with what draws their icon.

use crate::models::Service;

/// Unit separator used between service names in list queries, so that a
/// comma inside a name never splits it.
pub const NAME_SEPARATOR: char = '\u{1f}';
/// Between the built-in icon and the uploaded file of one service.
pub const ICON_SEPARATOR: char = '\u{1e}';

/// A service an event names, with what draws its icon.
pub struct ServiceTag {
    pub name: String,
    pub(super) icon_name: Option<String>,
    pub(super) icon_filename: Option<String>,
    /// What an event under way does to the service, "en panne".
    pub effect: Option<String>,
}

impl From<&Service> for ServiceTag {
    fn from(service: &Service) -> Self {
        Self {
            name: service.name.clone(),
            icon_name: service.icon_name.clone(),
            icon_filename: service.icon_filename.clone(),
            effect: None,
        }
    }
}

impl ServiceTag {
    /// SVG path data of the built-in icon, `|||` between paths.
    pub fn builtin_icon_paths(&self) -> Option<&'static str> {
        self.icon_name
            .as_deref()
            .and_then(crate::models::find_builtin_icon)
            .map(|i| i.paths)
    }

    pub fn icon_url(&self) -> Option<String> {
        self.icon_filename
            .as_ref()
            .map(|f| format!("/uploads/icons/{f}"))
    }
}
