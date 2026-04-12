//! User model and Role enum.

use serde::{Deserialize, Serialize};

/// User role determining access level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type, Serialize, Deserialize)]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
pub enum Role {
    Reader,
    Publisher,
    Admin,
}

impl Role {
    /// Returns `true` if the role allows publishing events.
    pub fn can_publish(self) -> bool {
        matches!(self, Self::Publisher | Self::Admin)
    }

    /// Returns `true` if the role allows admin operations.
    pub fn can_admin(self) -> bool {
        matches!(self, Self::Admin)
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
}

/// Public-facing user data (without password hash).
#[derive(Debug, Clone, Serialize)]
pub struct UserPublic {
    pub id: i64,
    pub email: String,
    pub display_name: String,
    pub role: Role,
}

impl From<User> for UserPublic {
    fn from(user: User) -> Self {
        Self {
            id: user.id,
            email: user.email,
            display_name: user.display_name,
            role: user.role,
        }
    }
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
    fn user_public_from_user_strips_password() {
        let user = User {
            id: 1,
            email: "test@example.com".to_string(),
            password_hash: "secret_hash".to_string(),
            display_name: "Test".to_string(),
            role: Role::Admin,
            is_active: true,
            last_seen_at: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let public = UserPublic::from(user);

        assert_eq!(public.id, 1);
        assert_eq!(public.email, "test@example.com");
        assert_eq!(public.display_name, "Test");
        assert_eq!(public.role, Role::Admin);
    }
}
