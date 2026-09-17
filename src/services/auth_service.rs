//! Authentication service: password hashing, accounts and sign-in.

use std::num::NonZeroUsize;
use std::sync::LazyLock;

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::Rng;
use rand::distributions::Alphanumeric;
use tokio::sync::Semaphore;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::{Role, User};
use crate::repositories::{NewUser, UserRepository};

/// Minimum password length, in characters.
const MIN_PASSWORD_LENGTH: usize = 12;

/// Long enough to resist guessing, short enough to read out over a call.
const TEMP_PASSWORD_LENGTH: usize = 16;

/// Hashed when an account is unknown, so the answer takes as long as for a
/// wrong password and does not tell which emails exist.
const TIMING_DECOY: &str = "dummy_password_for_timing";

/// Each hash holds 19 MiB and a core for tens of milliseconds. One per core
/// at most: more would not finish sooner, and a burst of sign-in attempts
/// cannot exhaust memory.
static HASHING_PERMITS: LazyLock<Semaphore> = LazyLock::new(|| {
    let cores = std::thread::available_parallelism().map_or(1, NonZeroUsize::get);
    Semaphore::new(cores.max(2))
});

/// An account to create, before its email is checked and its password hashed.
pub struct NewAccount<'a> {
    pub email: &'a str,
    pub password: &'a str,
    pub display_name: &'a str,
    pub role: Role,
    /// The password was chosen by someone else and must be replaced.
    pub must_change_password: bool,
}

pub struct AuthService;

impl AuthService {
    /// Hash a password with Argon2id, off the async runtime.
    pub async fn hash_password(password: &str) -> Result<String, AppError> {
        let password = password.to_owned();
        off_runtime(move || hash_blocking(&password)).await
    }

    /// Verify a password against a PHC-format hash, off the async runtime.
    pub async fn verify_password(password: &str, hash: &str) -> Result<bool, AppError> {
        let (password, hash) = (password.to_owned(), hash.to_owned());
        off_runtime(move || verify_blocking(&password, &hash)).await
    }

    /// Validate password strength: at least 12 characters, not bytes.
    pub fn validate_password(password: &str) -> Result<(), AppError> {
        if password.chars().count() < MIN_PASSWORD_LENGTH {
            return Err(AppError::Validation(
                "validation.password_min_length".to_string(),
            ));
        }
        Ok(())
    }

    /// The email as stored: trimmed, lowercase, and well formed.
    pub fn normalize_email(raw: &str) -> Result<String, AppError> {
        let email = raw.trim().to_lowercase();
        if is_well_formed_email(&email) {
            Ok(email)
        } else {
            Err(AppError::Validation("validation.email_invalid".to_string()))
        }
    }

    /// A password chosen by someone else, to be handed over and replaced.
    pub fn temporary_password() -> String {
        rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(TEMP_PASSWORD_LENGTH)
            .map(char::from)
            .collect()
    }

    /// Give an active account a new temporary password, for the host to hand
    /// over. The person replaces it at next sign-in, and their open sessions
    /// no longer match the password, so they are signed out.
    pub async fn reset_password(pool: &DbPool, email: &str) -> Result<String, AppError> {
        let user = UserRepository::find_by_email(pool, email.trim())
            .await?
            .ok_or(AppError::NotFound)?;
        let password = Self::temporary_password();
        let hash = Self::hash_password(&password).await?;
        UserRepository::set_temporary_password(pool, user.id, &hash).await?;
        tracing::info!(user_id = user.id, "Password reset by the host");
        Ok(password)
    }

    /// Create an account for a member, with a temporary password returned
    /// once for the admin to hand over.
    pub async fn add_member(
        pool: &DbPool,
        email: &str,
        display_name: &str,
        role: Role,
    ) -> Result<(User, String), AppError> {
        let password = Self::temporary_password();
        let account = NewAccount {
            email,
            password: &password,
            display_name,
            role,
            must_change_password: true,
        };
        let user = Self::create_account(pool, &account).await?;
        Ok((user, password))
    }

    /// Create an account. An email used by any account, a disabled one
    /// included, is refused with a message rather than a database error.
    pub async fn create_account(pool: &DbPool, account: &NewAccount<'_>) -> Result<User, AppError> {
        let email = Self::normalize_email(account.email)?;
        if UserRepository::email_exists(pool, &email).await? {
            return Err(email_taken());
        }
        Self::validate_password(account.password)?;
        let password_hash = Self::hash_password(account.password).await?;
        let new_user = NewUser {
            email: &email,
            password_hash: &password_hash,
            display_name: account.display_name,
            role: account.role,
            must_change_password: account.must_change_password,
        };
        let user = UserRepository::insert(pool, &new_user)
            .await
            .map_err(taken_on_conflict)?;
        tracing::info!(user_id = user.id, "Account created");
        Ok(user)
    }

    /// Create the administrator of an empty instance. Returns `None` when an
    /// account exists already, even one created a moment ago by someone else.
    pub async fn create_first_admin(
        pool: &DbPool,
        email: &str,
        password: &str,
        display_name: &str,
    ) -> Result<Option<User>, AppError> {
        let email = Self::normalize_email(email)?;
        Self::validate_password(password)?;
        let password_hash = Self::hash_password(password).await?;
        let new_user = NewUser {
            email: &email,
            password_hash: &password_hash,
            display_name,
            role: Role::Admin,
            must_change_password: false,
        };
        let admin = UserRepository::insert_first(pool, &new_user).await?;
        if let Some(admin) = &admin {
            tracing::info!(user_id = admin.id, "First administrator created");
        }
        Ok(admin)
    }

    /// Destroy a session (logout).
    ///
    /// Flushes all session data and removes the cookie.
    pub async fn logout(session: &tower_sessions::Session) {
        if let Err(e) = session.flush().await {
            tracing::error!("Failed to flush session: {e}");
        }
    }

    /// Creates the first administrator from the given credentials when no
    /// account exists. Does nothing when accounts exist or a value is missing.
    pub async fn bootstrap_admin(
        pool: &DbPool,
        admin_email: Option<&str>,
        admin_password: Option<&str>,
    ) -> Result<(), AppError> {
        if UserRepository::count_all(pool).await? > 0 {
            tracing::debug!("Users already exist, skipping admin bootstrap");
            return Ok(());
        }

        let (Some(email), Some(password)) = (admin_email, admin_password) else {
            tracing::warn!(
                "No account exists yet: the first person to open /register becomes the \
                 administrator. Create it now, or set ADMIN_EMAIL and ADMIN_PASSWORD."
            );
            return Ok(());
        };

        let display_name = email.split('@').next().unwrap_or("Admin");
        Self::create_first_admin(pool, email, password, display_name).await?;
        Ok(())
    }

    /// Authenticate an active account by email and password.
    ///
    /// An unknown account costs a decoy hash, so the response time does not
    /// tell whether the email exists.
    pub async fn login(pool: &DbPool, email: &str, password: &str) -> Result<User, AppError> {
        let Some(user) = UserRepository::find_by_email(pool, email.trim()).await? else {
            Self::hash_password(TIMING_DECOY).await?;
            tracing::warn!("Failed login attempt: unknown account");
            return Err(invalid_credentials());
        };

        if !Self::verify_password(password, &user.password_hash).await? {
            tracing::warn!(user_id = user.id, "Failed login attempt: wrong password");
            return Err(invalid_credentials());
        }

        tracing::info!(user_id = user.id, "User logged in");
        Ok(user)
    }
}
/// The HTML5 rule for an email address: a local part made of the usual
/// characters, one `@`, then domain labels of letters, digits and inner
/// hyphens. Enough to catch a typo without refusing a valid address.
fn is_well_formed_email(email: &str) -> bool {
    let Some((local, domain)) = email.rsplit_once('@') else {
        return false;
    };
    let local_ok = !local.is_empty()
        && local.len() <= 64
        && local
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".!#$%&'*+/=?^_`{|}~-".contains(c));
    local_ok && domain.len() <= 253 && domain.split('.').all(is_domain_label)
}

fn is_domain_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 63
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label.chars().all(|c| c.is_alphanumeric() || c == '-')
}

fn email_taken() -> AppError {
    AppError::Validation("validation.email_taken".to_string())
}

fn invalid_credentials() -> AppError {
    AppError::Validation("validation.invalid_credentials".to_string())
}

/// The existence check and the insert are two statements: an account
/// created in between hits the unique index, which says the same thing.
fn taken_on_conflict(error: sqlx::Error) -> AppError {
    match &error {
        sqlx::Error::Database(db) if db.is_unique_violation() => email_taken(),
        _ => AppError::Database(error),
    }
}

/// Runs CPU-bound hashing on the blocking pool, a few at a time.
async fn off_runtime<T, F>(work: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
{
    let _permit = HASHING_PERMITS
        .acquire()
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("hashing queue closed: {e}")))?;
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("hashing task failed: {e}")))?
}

/// Argon2id with the OWASP parameters: 19 MiB, 2 iterations, 1 lane.
fn hash_blocking(password: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    let params = Params::new(19 * 1024, 2, 1, None)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("argon2 params error: {e}")))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("password hashing failed: {e}")))?;
    Ok(hash.to_string())
}

/// Constant-time verification; the parameters are read from the hash.
fn verify_blocking(password: &str, hash: &str) -> Result<bool, AppError> {
    let parsed_hash = PasswordHash::new(hash)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("invalid password hash: {e}")))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::test_pool;

    async fn own_account(pool: &DbPool, email: &str, name: &str) -> Result<User, AppError> {
        let account = NewAccount {
            email,
            password: "a_password_123",
            display_name: name,
            role: Role::Reader,
            must_change_password: false,
        };
        AuthService::create_account(pool, &account).await
    }

    #[tokio::test]
    async fn test_hash_and_verify() {
        let password = "super_secure_password_123";
        let hash = AuthService::hash_password(password).await.unwrap();

        assert!(hash.starts_with("$argon2id$"));
        assert!(AuthService::verify_password(password, &hash).await.unwrap());
        assert!(
            !AuthService::verify_password("wrong_password", &hash)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn test_different_hashes_for_same_password() {
        let password = "super_secure_password_123";
        let hash1 = AuthService::hash_password(password).await.unwrap();
        let hash2 = AuthService::hash_password(password).await.unwrap();
        assert_ne!(hash1, hash2, "Each hash should use a unique salt");
    }

    #[test]
    fn test_validate_password_too_short() {
        assert!(AuthService::validate_password("short").is_err());
    }

    #[test]
    fn test_validate_password_ok() {
        assert!(AuthService::validate_password("valid_password_123").is_ok());
    }

    #[test]
    fn test_validate_password_exact_minimum() {
        assert!(AuthService::validate_password("123456789012").is_ok());
    }

    #[test]
    fn password_length_counts_characters() {
        assert!(AuthService::validate_password("éééééé").is_err());
        assert!(AuthService::validate_password("éééééééééééé").is_ok());
    }

    #[test]
    fn emails_are_trimmed_lowercased_and_checked() {
        assert_eq!(
            AuthService::normalize_email("  Alice@Example.COM ").unwrap(),
            "alice@example.com"
        );
        assert!(AuthService::normalize_email("not-an-email").is_err());
        assert!(AuthService::normalize_email("").is_err());
        assert!(AuthService::normalize_email("two@at@example.org").is_err());
        assert!(AuthService::normalize_email("a b@example.org").is_err());
        assert!(AuthService::normalize_email("a@-example.org").is_err());
        assert!(AuthService::normalize_email("a@example.").is_err());
        assert!(AuthService::normalize_email("o.neil+it@intranet").is_ok());
        assert!(AuthService::normalize_email("paie@société.fr").is_ok());
    }

    #[tokio::test]
    async fn a_disabled_account_keeps_its_email_taken() {
        let pool = test_pool().await;
        let user = own_account(&pool, "gone@x.com", "Gone").await.unwrap();
        UserRepository::set_active(&pool, user.id, false)
            .await
            .unwrap();

        let again = own_account(&pool, "Gone@X.com", "Back").await;
        assert!(matches!(again, Err(AppError::Validation(key)) if key == "validation.email_taken"));

        let member = AuthService::add_member(&pool, "gone@x.com", "Back", Role::Reader).await;
        assert!(
            matches!(member, Err(AppError::Validation(key)) if key == "validation.email_taken")
        );
    }

    #[tokio::test]
    async fn members_must_replace_their_temporary_password() {
        let pool = test_pool().await;
        let (member, password) =
            AuthService::add_member(&pool, "m@x.com", "Member", Role::Publisher)
                .await
                .unwrap();

        assert!(member.must_change_password);
        assert_eq!(member.role, Role::Publisher);
        assert!(
            AuthService::verify_password(&password, &member.password_hash)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn only_an_empty_instance_gets_a_first_admin() {
        let pool = test_pool().await;
        let first = AuthService::create_first_admin(&pool, "a@x.com", "first_password_1", "A")
            .await
            .unwrap();
        let second = AuthService::create_first_admin(&pool, "b@x.com", "second_password", "B")
            .await
            .unwrap();

        assert_eq!(first.map(|u| u.role), Some(Role::Admin));
        assert!(second.is_none());
    }

    #[tokio::test]
    async fn login_refuses_disabled_accounts() {
        let pool = test_pool().await;
        let user = own_account(&pool, "off@x.com", "Off").await.unwrap();
        assert!(
            AuthService::login(&pool, "off@x.com", "a_password_123")
                .await
                .is_ok()
        );

        UserRepository::set_active(&pool, user.id, false)
            .await
            .unwrap();

        assert!(
            AuthService::login(&pool, "off@x.com", "a_password_123")
                .await
                .is_err()
        );
    }
}
