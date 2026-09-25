//! The instance's own settings: its name, who can read it and its time
//! zone. Stored in the database and held in memory for every request. The
//! logo has a service of its own.

use chrono_tz::Tz;

use crate::clock;
use crate::db::DbPool;
use crate::error::AppError;
use crate::repositories::SettingsRepository;
use crate::services::LOGO_SETTING;
use crate::state::AppState;

const NAME_SETTING: &str = "instance_name";
const PUBLIC_MODE_SETTING: &str = "public_mode";
const ZONE_SETTING: &str = "time_zone";

/// Long enough for a company and a purpose, short enough to stay on one line
/// of the masthead beside the mark.
const NAME_MAX_CHARS: usize = 40;

pub struct SettingsService;

impl SettingsService {
    /// Puts the stored name, logo and zone in memory at start. Returns
    /// whether visitors without an account read the page: `default_public`
    /// until an administrator chooses.
    pub async fn load(pool: &DbPool, default_public: bool) -> Result<bool, AppError> {
        if let Some(name) = SettingsRepository::get(pool, NAME_SETTING).await? {
            crate::set_instance_name(&name);
        }
        if let Some(logo) = SettingsRepository::get(pool, LOGO_SETTING).await? {
            crate::set_instance_logo(&logo);
        }
        let zone = SettingsRepository::get(pool, ZONE_SETTING).await?;
        if let Some(zone) = zone.as_deref().and_then(clock::parse_zone) {
            clock::set_zone(zone);
        }
        let public = SettingsRepository::get(pool, PUBLIC_MODE_SETTING).await?;
        Ok(public.map_or(default_public, |stored| stored == "true"))
    }

    /// The message key refusing a name, if any.
    pub fn name_refusal(name: &str) -> Option<&'static str> {
        if name.chars().count() > NAME_MAX_CHARS {
            Some("validation.instance_name_too_long")
        } else if name.chars().any(char::is_control) {
            Some("validation.display_name_invalid")
        } else {
            None
        }
    }

    /// The name shown in place of Statup, checked by [`Self::name_refusal`].
    pub async fn set_name(pool: &DbPool, name: &str) -> Result<(), AppError> {
        SettingsRepository::set(pool, NAME_SETTING, name).await?;
        crate::set_instance_name(name);
        Ok(())
    }

    /// The database is written first: memory never holds a choice a restart
    /// would lose.
    pub async fn set_public_mode(state: &AppState, public: bool) -> Result<(), AppError> {
        let stored = if public { "true" } else { "false" };
        SettingsRepository::set(&state.pool, PUBLIC_MODE_SETTING, stored).await?;
        state.set_public_mode(public);
        Ok(())
    }

    /// The zone dates are shown and typed in.
    pub async fn set_time_zone(pool: &DbPool, zone: Tz) -> Result<(), AppError> {
        SettingsRepository::set(pool, ZONE_SETTING, zone.name()).await?;
        clock::set_zone(zone);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_names_are_bounded_and_printable() {
        let refusal = SettingsService::name_refusal;
        assert_eq!(refusal("Acme Status"), None);
        assert_eq!(refusal(&"é".repeat(40)), None);
        assert_eq!(
            refusal(&"a".repeat(41)),
            Some("validation.instance_name_too_long")
        );
        assert_eq!(
            refusal("Acme\u{7}"),
            Some("validation.display_name_invalid")
        );
    }
}
