//! Authentication service - password hashing, verification, registration, login, logout.

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::{Role, User};
use crate::repositories::UserRepository;

/// Minimum password length.
const MIN_PASSWORD_LENGTH: usize = 12;

pub struct AuthService;

impl AuthService {
    /// Hash a password with Argon2id using secure defaults.
    pub fn hash_password(password: &str) -> Result<String, AppError> {
        let salt = SaltString::generate(&mut OsRng);

        // OWASP recommended: Argon2id, 19 MiB memory, 2 iterations, 1 parallelism
        let params = Params::new(19 * 1024, 2, 1, None)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("argon2 params error: {e}")))?;

        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

        let hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("password hashing failed: {e}")))?;

        Ok(hash.to_string())
    }

    /// Verify a password against a PHC-format hash (constant-time).
    pub fn verify_password(password: &str, hash: &str) -> Result<bool, AppError> {
        let parsed_hash = PasswordHash::new(hash)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("invalid password hash: {e}")))?;

        // Argon2 default verify is already constant-time
        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_ok())
    }

    /// Validate password strength. Returns an error if too short.
    pub fn validate_password(password: &str) -> Result<(), AppError> {
        if password.len() < MIN_PASSWORD_LENGTH {
            return Err(AppError::Validation(
                "validation.password_min_length".to_string(),
            ));
        }
        Ok(())
    }

    /// Register a new user account.
    ///
    /// Validates email format and uniqueness, validates password, hashes it,
    /// and creates the user with the `Reader` role.
    pub async fn register(
        pool: &DbPool,
        email: &str,
        password: &str,
        display_name: &str,
    ) -> Result<User, AppError> {
        if !email.contains('@') || email.len() < 5 {
            return Err(AppError::Validation("validation.email_invalid".to_string()));
        }

        if UserRepository::find_by_email(pool, email).await?.is_some() {
            return Err(AppError::Validation("validation.email_taken".to_string()));
        }

        Self::validate_password(password)?;
        let password_hash = Self::hash_password(password)?;

        let user =
            UserRepository::create(pool, email, &password_hash, display_name, Role::Reader).await?;

        tracing::info!(user_id = user.id, email = email, "New user registered");
        Ok(user)
    }

    /// Destroy a session (logout).
    ///
    /// Flushes all session data and removes the cookie.
    pub async fn logout(session: &tower_sessions::Session) {
        if let Err(e) = session.flush().await {
            tracing::error!("Failed to flush session: {e}");
        }
    }

    /// Create the initial admin user if no users exist in the database.
    ///
    /// Reads `ADMIN_EMAIL` and `ADMIN_PASSWORD` from the provided config.
    /// Does nothing if users already exist or if the env vars are not set.
    pub async fn bootstrap_admin(
        pool: &DbPool,
        admin_email: Option<&str>,
        admin_password: Option<&str>,
    ) -> Result<(), AppError> {
        let user_count = UserRepository::count_all(pool).await?;
        if user_count > 0 {
            tracing::debug!("Users already exist, skipping admin bootstrap");
            return Ok(());
        }

        let (Some(email), Some(password)) = (admin_email, admin_password) else {
            tracing::warn!(
                "No users in database and ADMIN_EMAIL/ADMIN_PASSWORD not set, \
                 register the first user via the web UI"
            );
            return Ok(());
        };

        Self::validate_password(password)?;
        let password_hash = Self::hash_password(password)?;

        let display_name = email.split('@').next().unwrap_or("Admin");

        let user =
            UserRepository::create(pool, email, &password_hash, display_name, Role::Admin).await?;

        tracing::info!(
            user_id = user.id,
            email = email,
            "Initial admin user created"
        );
        Ok(())
    }

    /// Authenticate a user by email and password.
    ///
    /// Uses constant-time comparison to prevent timing attacks.
    /// Performs a dummy hash when the user is not found to avoid leaking
    /// whether the email exists.
    pub async fn login(pool: &DbPool, email: &str, password: &str) -> Result<User, AppError> {
        let maybe_user = UserRepository::find_by_email(pool, email).await?;

        if let Some(user) = maybe_user {
            if Self::verify_password(password, &user.password_hash)? {
                UserRepository::update_last_seen(pool, user.id).await?;
                tracing::info!(user_id = user.id, "User logged in");
                Ok(user)
            } else {
                tracing::warn!(email = email, "Failed login attempt: wrong password");
                Err(AppError::Validation(
                    "validation.invalid_credentials".to_string(),
                ))
            }
        } else {
            // Dummy hash to prevent timing-based user enumeration
            let _ = Self::hash_password("dummy_password_for_timing");
            tracing::warn!(email = email, "Failed login attempt: user not found");
            Err(AppError::Validation(
                "validation.invalid_credentials".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_and_verify() {
        let password = "super_secure_password_123";
        let hash = AuthService::hash_password(password).unwrap();

        assert!(hash.starts_with("$argon2id$"));
        assert!(AuthService::verify_password(password, &hash).unwrap());
        assert!(!AuthService::verify_password("wrong_password", &hash).unwrap());
    }

    #[test]
    fn test_different_hashes_for_same_password() {
        let password = "super_secure_password_123";
        let hash1 = AuthService::hash_password(password).unwrap();
        let hash2 = AuthService::hash_password(password).unwrap();
        assert_ne!(hash1, hash2, "Each hash should use a unique salt");
    }

    #[test]
    fn test_validate_password_too_short() {
        let result = AuthService::validate_password("short");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_password_ok() {
        let result = AuthService::validate_password("valid_password_123");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_password_exact_minimum() {
        let result = AuthService::validate_password("123456789012"); // 12 chars
        assert!(result.is_ok());
    }
}
