//! The link that lets a member who forgot their password choose a new one.
//!
//! A link carries `<id>.<secret>` (see `link_token`): the id finds the
//! stored link, the secret is checked against its SHA-256, so a copy of the
//! database opens no account. Only the newest link of an account works,
//! for an hour, once.

use super::AuthService;
use super::link_token::{matches, new_secret, parse, secret_hash, token};
use crate::db::DbPool;
use crate::error::AppError;
use crate::models::User;
use crate::repositories::{PasswordResetRepository, UserRepository};

/// Links an account can be sent in an hour, so the form cannot flood an inbox.
const LINKS_PER_HOUR: i64 = 3;

/// A link to send, and the account it opens.
pub struct ResetLink {
    pub user: User,
    pub token: String,
}

/// What a request for a link came to.
pub enum LinkRequest {
    /// No active account has this email.
    NoAccount,
    /// The account had its links for the hour.
    LimitReached,
    Issued(ResetLink),
}

pub struct PasswordResetService;

impl PasswordResetService {
    /// A new link for the active account with this email. The visitor is
    /// told the same thing whatever the outcome.
    pub async fn request(pool: &DbPool, email: &str) -> Result<LinkRequest, AppError> {
        PasswordResetRepository::delete_stale(pool).await?;
        let Some(user) = UserRepository::find_by_email(pool, email.trim()).await? else {
            return Ok(LinkRequest::NoAccount);
        };
        let secret = new_secret();
        let secret_hash = secret_hash(&secret);
        let stored = PasswordResetRepository::insert_within_limit(
            pool,
            user.id,
            &secret_hash,
            LINKS_PER_HOUR,
        );
        let Some(id) = stored.await? else {
            return Ok(LinkRequest::LimitReached);
        };
        Ok(LinkRequest::Issued(ResetLink {
            user,
            token: token(id, &secret),
        }))
    }

    /// The active account this token opens, if it still opens one.
    pub async fn account_for(pool: &DbPool, token: &str) -> Result<Option<User>, AppError> {
        let Some((id, secret)) = parse(token) else {
            return Ok(None);
        };
        let Some(link) = PasswordResetRepository::find_live(pool, id).await? else {
            return Ok(None);
        };
        if !matches(secret, &link.secret_hash) {
            return Ok(None);
        }
        let user = UserRepository::find_by_id(pool, link.user_id).await?;
        Ok(user.filter(|user| user.is_active))
    }

    /// Sets the password chosen through the link. A refused password keeps
    /// the link; an accepted one uses up every link of the account in the
    /// same transaction as the new password. Returns the account as stored
    /// now, its new password hash included, which the session is tied to
    /// next.
    pub async fn redeem(
        pool: &DbPool,
        token: &str,
        password: &str,
        confirmation: &str,
    ) -> Result<User, AppError> {
        let invalid = || AppError::validation("validation.reset_link_invalid");
        let user = Self::account_for(pool, token).await?.ok_or_else(invalid)?;
        AuthService::check_new_password(password, confirmation)?;
        let hash = AuthService::hash_password(password).await?;
        if !PasswordResetRepository::use_up(pool, user.id, &hash).await? {
            return Err(invalid());
        }
        UserRepository::find_by_id(pool, user.id)
            .await?
            .ok_or(AppError::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Role;
    use crate::test_helpers::test_pool;

    const NEW_PASSWORD: &str = "a fresh passphrase to remember";

    async fn member(pool: &DbPool, email: &str) -> User {
        UserRepository::create(pool, email, "hash", "Ana", Role::Reader)
            .await
            .unwrap()
    }

    async fn link(pool: &DbPool, email: &str) -> String {
        match PasswordResetService::request(pool, email).await.unwrap() {
            LinkRequest::Issued(link) => link.token,
            _ => panic!("no link issued for {email}"),
        }
    }

    #[tokio::test]
    async fn a_link_opens_the_account_it_was_sent_for() {
        let pool = test_pool().await;
        let ana = member(&pool, "ana@example.com").await;

        let token = link(&pool, " ANA@example.com ").await;
        let opened = PasswordResetService::account_for(&pool, &token)
            .await
            .unwrap();

        assert_eq!(opened.map(|user| user.id), Some(ana.id));
    }

    #[tokio::test]
    async fn an_unknown_or_disabled_account_gets_no_link() {
        let pool = test_pool().await;
        let ben = member(&pool, "ben@example.com").await;
        UserRepository::set_active(&pool, ben.id, false)
            .await
            .unwrap();

        for email in ["nobody@example.com", "ben@example.com"] {
            let sent = PasswordResetService::request(&pool, email).await.unwrap();
            assert!(matches!(sent, LinkRequest::NoAccount), "{email}");
        }
    }

    #[tokio::test]
    async fn an_account_gets_three_links_an_hour() {
        let pool = test_pool().await;
        member(&pool, "ana@example.com").await;

        for _ in 0..3 {
            link(&pool, "ana@example.com").await;
        }
        let fourth = PasswordResetService::request(&pool, "ana@example.com")
            .await
            .unwrap();

        assert!(matches!(fourth, LinkRequest::LimitReached));
    }

    #[tokio::test]
    async fn a_link_is_kept_as_the_sha256_of_its_secret() {
        let pool = test_pool().await;
        member(&pool, "ana@example.com").await;

        let token = link(&pool, "ana@example.com").await;
        let (id, secret) = parse(&token).unwrap();
        let stored = PasswordResetRepository::find_live(&pool, id)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(stored.secret_hash, secret_hash(secret));
    }

    #[tokio::test]
    async fn a_wrong_or_malformed_token_opens_nothing() {
        let pool = test_pool().await;
        member(&pool, "ana@example.com").await;
        let token = link(&pool, "ana@example.com").await;
        let (id, secret) = token.split_once('.').unwrap();
        let wrong_secret = format!("{id}.{}", secret.chars().rev().collect::<String>());

        for attempt in [wrong_secret.as_str(), "", "1", "x.y", id, secret] {
            let opened = PasswordResetService::account_for(&pool, attempt)
                .await
                .unwrap();
            assert!(opened.is_none(), "{attempt:?}");
        }
    }

    #[tokio::test]
    async fn a_link_sets_the_password_once() {
        let pool = test_pool().await;
        let ana = member(&pool, "ana@example.com").await;
        let token = link(&pool, "ana@example.com").await;

        let user = PasswordResetService::redeem(&pool, &token, NEW_PASSWORD, NEW_PASSWORD)
            .await
            .unwrap();
        let again = PasswordResetService::redeem(&pool, &token, NEW_PASSWORD, NEW_PASSWORD).await;

        assert_eq!(user.id, ana.id);
        assert!(
            AuthService::verify_password(NEW_PASSWORD, &user.password_hash)
                .await
                .unwrap()
        );
        assert!(
            matches!(again, Err(AppError::Validation(key)) if key == "validation.reset_link_invalid")
        );
    }

    #[tokio::test]
    async fn a_refused_password_keeps_the_link() {
        let pool = test_pool().await;
        member(&pool, "ana@example.com").await;
        let token = link(&pool, "ana@example.com").await;

        let weak = PasswordResetService::redeem(&pool, &token, "short", "short").await;
        let mismatch =
            PasswordResetService::redeem(&pool, &token, NEW_PASSWORD, "something else").await;

        assert!(
            matches!(weak, Err(AppError::Validation(key)) if key == "validation.password_too_weak")
        );
        assert!(
            matches!(mismatch, Err(AppError::Validation(key)) if key == "validation.passwords_mismatch")
        );
        assert!(
            PasswordResetService::account_for(&pool, &token)
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn a_link_clears_a_temporary_password() {
        let pool = test_pool().await;
        let ana = member(&pool, "ana@example.com").await;
        UserRepository::set_temporary_password(&pool, ana.id, "temp-hash")
            .await
            .unwrap();
        let token = link(&pool, "ana@example.com").await;

        let user = PasswordResetService::redeem(&pool, &token, NEW_PASSWORD, NEW_PASSWORD)
            .await
            .unwrap();

        assert!(!user.must_change_password);
    }

    #[tokio::test]
    async fn a_link_of_an_account_disabled_since_opens_nothing() {
        let pool = test_pool().await;
        let ana = member(&pool, "ana@example.com").await;
        let token = link(&pool, "ana@example.com").await;

        UserRepository::set_active(&pool, ana.id, false)
            .await
            .unwrap();

        assert!(
            PasswordResetService::account_for(&pool, &token)
                .await
                .unwrap()
                .is_none()
        );
    }
}
