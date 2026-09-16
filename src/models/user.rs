//! User model, Role enum and the rules for a display name.

use std::str::FromStr;

/// User role determining access level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
pub enum Role {
    Reader,
    Publisher,
    Admin,
}

impl Role {
    /// Every role, in the order a form lists them, least powerful first.
    pub const ALL: [Role; 3] = [Role::Reader, Role::Publisher, Role::Admin];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reader => "reader",
            Self::Publisher => "publisher",
            Self::Admin => "admin",
        }
    }

    pub fn i18n_key(self) -> &'static str {
        match self {
            Self::Reader => "role.reader",
            Self::Publisher => "role.publisher",
            Self::Admin => "role.admin",
        }
    }

    /// Returns `true` if the role allows publishing events.
    pub fn can_publish(self) -> bool {
        matches!(self, Self::Publisher | Self::Admin)
    }

    /// Returns `true` if the role allows admin operations.
    pub fn can_admin(self) -> bool {
        matches!(self, Self::Admin)
    }
}

/// A role name that is not one of [`Role::ALL`].
#[derive(Debug, PartialEq, Eq)]
pub struct UnknownRole;

impl FromStr for Role {
    type Err = UnknownRole;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|role| role.as_str() == s)
            .ok_or(UnknownRole)
    }
}

/// Full user record as stored in the database.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct User {
    pub id: i64,
    pub email: String,
    pub password_hash: String,
    pub display_name: String,
    pub role: Role,
    pub is_active: bool,
    pub last_seen_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub preferred_locale: Option<String>,
    pub must_change_password: bool,
}

/// Longest display name, in characters.
pub const DISPLAY_NAME_MAX_CHARS: usize = 100;

/// The trimmed display name, or the message key saying why it is refused:
/// empty, longer than 100 characters, or holding control characters.
pub fn check_display_name(raw: &str) -> Result<String, &'static str> {
    let name = raw.trim();
    if name.is_empty() {
        return Err("validation.display_name_required");
    }
    if name.chars().count() > DISPLAY_NAME_MAX_CHARS {
        return Err("validation.display_name_too_long");
    }
    if name.chars().any(char::is_control) {
        return Err("validation.display_name_invalid");
    }
    Ok(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_cannot_publish_or_admin() {
        assert!(!Role::Reader.can_publish());
        assert!(!Role::Reader.can_admin());
    }

    #[test]
    fn publisher_can_publish_but_not_admin() {
        assert!(Role::Publisher.can_publish());
        assert!(!Role::Publisher.can_admin());
    }

    #[test]
    fn admin_can_publish_and_admin() {
        assert!(Role::Admin.can_publish());
        assert!(Role::Admin.can_admin());
    }

    #[test]
    fn roles_parse_from_their_form_value() {
        for role in Role::ALL {
            assert_eq!(role.as_str().parse::<Role>(), Ok(role));
        }
        assert!("superadmin".parse::<Role>().is_err());
        assert!("Admin".parse::<Role>().is_err());
    }

    #[test]
    fn display_names_are_trimmed_and_bounded() {
        assert_eq!(check_display_name("  Alice  "), Ok("Alice".to_string()));
        assert_eq!(
            check_display_name("   "),
            Err("validation.display_name_required")
        );
        assert_eq!(
            check_display_name(&"é".repeat(100)),
            Ok("é".repeat(100)),
            "the limit counts characters, not bytes"
        );
        assert_eq!(
            check_display_name(&"a".repeat(101)),
            Err("validation.display_name_too_long")
        );
        assert_eq!(
            check_display_name("Ali\u{1}ce"),
            Err("validation.display_name_invalid")
        );
    }
}
