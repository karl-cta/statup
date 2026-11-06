//! How visitors subscribe to the emails of the page, and how anyone leaves.
//!
//! A visitor asks with their address and choices. The address is not theirs
//! until they open the link sent to it, `<id>.<secret>`: the id finds the
//! stored request, the secret is checked against its Argon2 hash, so a copy
//! of the database confirms nothing. Only the newest request of an address
//! confirms it, for a day, once.
//!
//! Every email carries an unsubscribe link, `<subscriber id>.<unsubscribe
//! secret>`, compared in constant time. That secret is stored in clear
//! because each email must repeat it, and it can only end its own
//! subscription.

use super::link_token::{matches, new_secret, parse, same_secret, secret_hash, token};
use crate::db::DbPool;
use crate::error::AppError;
use crate::models::{Choices, Subscriber, SubscriptionRequest};
use crate::repositories::{
    NewSubscriptionRequest, RequestLimits, SettingsRepository, SubscriberRepository,
    SubscriptionRequestRepository, VISITORS_SETTING,
};

/// Requests an address can make in an hour, so the form cannot flood an inbox.
const REQUESTS_PER_ADDRESS: i64 = 3;
/// Requests the whole page takes in an hour. High on purpose: many visitors
/// sign up during an outage, and none should be refused for it.
const REQUESTS_PER_INSTANCE: i64 = 500;

pub struct SubscriptionService;

impl SubscriptionService {
    pub async fn visitors_open(pool: &DbPool) -> Result<bool, AppError> {
        let value = SettingsRepository::get(pool, VISITORS_SETTING).await?;
        Ok(value.as_deref() == Some("true"))
    }

    pub async fn set_visitors_open(pool: &DbPool, open: bool) -> Result<(), AppError> {
        Ok(SettingsRepository::set(pool, VISITORS_SETTING, &open.to_string()).await?)
    }

    /// Visitors may subscribe and receive: the page is open to everyone
    /// (`public_mode`) and an administrator opened it to their subscriptions.
    pub async fn visitors_welcome(pool: &DbPool, public_mode: bool) -> Result<bool, AppError> {
        Ok(public_mode && Self::visitors_open(pool).await?)
    }

    /// The token of a confirmation link for this address, already normalized
    /// by the caller, or `None` when its hour of requests is used up.
    pub async fn request(
        pool: &DbPool,
        email: &str,
        locale: &str,
        choices: Choices,
    ) -> Result<Option<String>, AppError> {
        let limits = RequestLimits {
            per_address: REQUESTS_PER_ADDRESS,
            per_instance: REQUESTS_PER_INSTANCE,
        };
        if !SubscriptionRequestRepository::within_limits(pool, email, limits).await? {
            return Ok(None);
        }
        let secret = new_secret();
        let secret_hash = secret_hash(&secret);
        let new = NewSubscriptionRequest {
            email,
            secret_hash: &secret_hash,
            locale,
            choices,
        };
        let stored = SubscriptionRequestRepository::insert_within_limits(pool, &new, limits);
        Ok(stored.await?.map(|id| token(id, &secret)))
    }

    /// The request this link would confirm, if it still confirms one. Lets
    /// the page show the choices before the visitor clicks.
    pub async fn pending(
        pool: &DbPool,
        token: &str,
    ) -> Result<Option<SubscriptionRequest>, AppError> {
        let Some((id, secret)) = parse(token) else {
            return Ok(None);
        };
        let Some(request) = SubscriptionRequestRepository::find_live(pool, id).await? else {
            return Ok(None);
        };
        Ok(matches(secret, &request.secret_hash).then_some(request))
    }

    /// Subscribes the visitor whose link this is. `None` when the link is
    /// malformed, wrong, expired, replaced or already used.
    pub async fn confirm(pool: &DbPool, token: &str) -> Result<Option<Subscriber>, AppError> {
        let Some(request) = Self::pending(pool, token).await? else {
            return Ok(None);
        };
        let secret = new_secret();
        let Some(id) = SubscriberRepository::confirm_visitor(pool, &request, &secret).await? else {
            return Ok(None);
        };
        Ok(SubscriberRepository::find(pool, id).await?)
    }

    /// The subscriber an unsubscribe token names.
    pub async fn linked(pool: &DbPool, token: &str) -> Result<Option<Subscriber>, AppError> {
        let Some((id, secret)) = parse(token) else {
            return Ok(None);
        };
        let subscriber = SubscriberRepository::find(pool, id).await?;
        Ok(subscriber.filter(|subscriber| same_secret(secret, &subscriber.unsubscribe_secret)))
    }

    /// Ends the subscription an unsubscribe token names, and returns it for
    /// the caller to journal. Of two clicks at the same moment, only the one
    /// that removed it gets it back.
    pub async fn unsubscribe(pool: &DbPool, token: &str) -> Result<Option<Subscriber>, AppError> {
        let Some(subscriber) = Self::linked(pool, token).await? else {
            return Ok(None);
        };
        let removed = SubscriberRepository::delete(pool, subscriber.id).await?;
        Ok(removed.then_some(subscriber))
    }

    /// The unsubscribe link token of every email sent to this subscriber.
    pub fn unsubscribe_token(subscriber_id: i64, unsubscribe_secret: &str) -> String {
        token(subscriber_id, unsubscribe_secret)
    }

    /// Subscribes a member, or replaces their choices. True when new.
    pub async fn save_member(
        pool: &DbPool,
        user_id: i64,
        locale: &str,
        choices: Choices,
    ) -> Result<bool, AppError> {
        let secret = new_secret();
        Ok(SubscriberRepository::save_member(pool, user_id, locale, choices, &secret).await?)
    }

    /// Ends a member's subscription. False when they had none.
    pub async fn stop_member(pool: &DbPool, user_id: i64) -> Result<bool, AppError> {
        let Some(subscriber) = SubscriberRepository::find_for_member(pool, user_id).await? else {
            return Ok(false);
        };
        Ok(SubscriberRepository::delete(pool, subscriber.id).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Role;
    use crate::repositories::UserRepository;
    use crate::test_helpers::test_pool;

    const EVERYTHING: Choices = Choices {
        incidents: true,
        maintenances: true,
        publications: true,
    };
    const ONLY_INCIDENTS: Choices = Choices {
        incidents: true,
        maintenances: false,
        publications: false,
    };

    async fn ask(pool: &DbPool, email: &str, choices: Choices) -> Option<String> {
        SubscriptionService::request(pool, email, "fr", choices)
            .await
            .unwrap()
    }

    async fn link(pool: &DbPool, email: &str, choices: Choices) -> String {
        ask(pool, email, choices).await.unwrap()
    }

    async fn subscribed(pool: &DbPool, email: &str, choices: Choices) -> Subscriber {
        let token = link(pool, email, choices).await;
        SubscriptionService::confirm(pool, &token)
            .await
            .unwrap()
            .unwrap()
    }

    async fn age_requests(pool: &DbPool, created: &str, expires: &str) {
        sqlx::query(
            "UPDATE subscription_requests SET created_at = datetime('now', ?), \
                                              expires_at = datetime('now', ?)",
        )
        .bind(created)
        .bind(expires)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn subscriber_count(pool: &DbPool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM subscribers")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_link_confirms_the_visitor_once_with_what_they_asked() {
        let pool = test_pool().await;
        let token = link(&pool, "ana@example.com", ONLY_INCIDENTS).await;

        let pending = SubscriptionService::pending(&pool, &token)
            .await
            .unwrap()
            .unwrap();
        let subscriber = SubscriptionService::confirm(&pool, &token)
            .await
            .unwrap()
            .unwrap();
        let again = SubscriptionService::confirm(&pool, &token).await.unwrap();

        assert_eq!(pending.email, "ana@example.com");
        assert_eq!(pending.locale, "fr");
        assert_eq!(pending.choices, ONLY_INCIDENTS);
        assert_eq!(subscriber.email.as_deref(), Some("ana@example.com"));
        let locale: String = sqlx::query_scalar("SELECT locale FROM subscribers WHERE id = ?")
            .bind(subscriber.id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(locale, "fr");
        assert_eq!(subscriber.choices, ONLY_INCIDENTS);
        assert!(again.is_none());
        assert_eq!(subscriber_count(&pool).await, 1);
    }

    #[tokio::test]
    async fn a_link_that_does_not_fit_confirms_nothing() {
        let pool = test_pool().await;
        let token = link(&pool, "ana@example.com", EVERYTHING).await;
        let (id, secret) = token.split_once('.').unwrap();
        let wrong_secret = format!("{id}.{}", secret.chars().rev().collect::<String>());

        for attempt in [wrong_secret.as_str(), "", "1", "x.y", id, secret] {
            let pending = SubscriptionService::pending(&pool, attempt).await;
            assert!(pending.unwrap().is_none(), "{attempt:?}");
        }
        assert_eq!(subscriber_count(&pool).await, 0);
    }

    #[tokio::test]
    async fn an_expired_link_confirms_nothing() {
        let pool = test_pool().await;
        let token = link(&pool, "ana@example.com", EVERYTHING).await;

        age_requests(&pool, "-25 hours", "-1 hour").await;

        let pending = SubscriptionService::pending(&pool, &token).await;
        assert!(pending.unwrap().is_none());
        let confirmed = SubscriptionService::confirm(&pool, &token).await;
        assert!(confirmed.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_newer_request_replaces_the_older_link() {
        let pool = test_pool().await;
        let first = link(&pool, "ana@example.com", EVERYTHING).await;
        let second = link(&pool, "ana@example.com", ONLY_INCIDENTS).await;

        let old = SubscriptionService::pending(&pool, &first).await;
        let new = SubscriptionService::pending(&pool, &second).await;

        assert!(old.unwrap().is_none());
        assert_eq!(new.unwrap().unwrap().choices, ONLY_INCIDENTS);
    }

    #[tokio::test]
    async fn an_address_gets_three_requests_an_hour() {
        let pool = test_pool().await;
        for _ in 0..3 {
            link(&pool, "ana@example.com", EVERYTHING).await;
        }

        let fourth = ask(&pool, "ANA@example.com", EVERYTHING).await;
        let other = ask(&pool, "ben@example.com", EVERYTHING).await;

        assert!(fourth.is_none());
        assert!(other.is_some());
    }

    #[tokio::test]
    async fn subscribing_again_replaces_the_choices_and_keeps_the_unsubscribe_link() {
        let pool = test_pool().await;
        let first = subscribed(&pool, "ana@example.com", EVERYTHING).await;
        let unsubscribe =
            SubscriptionService::unsubscribe_token(first.id, &first.unsubscribe_secret);

        let second = subscribed(&pool, "ana@example.com", ONLY_INCIDENTS).await;
        let linked = SubscriptionService::linked(&pool, &unsubscribe)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(first.id, second.id);
        assert_eq!(second.choices, ONLY_INCIDENTS);
        assert_eq!(linked.choices, ONLY_INCIDENTS);
        assert_eq!(
            SubscriptionService::unsubscribe_token(second.id, &second.unsubscribe_secret),
            unsubscribe
        );
    }

    #[tokio::test]
    async fn the_right_unsubscribe_link_removes_the_subscriber() {
        let pool = test_pool().await;
        let ana = subscribed(&pool, "ana@example.com", EVERYTHING).await;

        let token = SubscriptionService::unsubscribe_token(ana.id, &ana.unsubscribe_secret);
        let removed = SubscriptionService::unsubscribe(&pool, &token)
            .await
            .unwrap();

        assert_eq!(removed.map(|subscriber| subscriber.id), Some(ana.id));
        assert_eq!(subscriber_count(&pool).await, 0);
    }

    #[tokio::test]
    async fn a_wrong_unsubscribe_link_removes_nothing() {
        let pool = test_pool().await;
        let ana = subscribed(&pool, "ana@example.com", EVERYTHING).await;
        let mut secret = ana.unsubscribe_secret.clone().into_bytes();
        secret[0] = if secret[0] == b'a' { b'b' } else { b'a' };
        let off_by_one = format!("{}.{}", ana.id, String::from_utf8(secret).unwrap());
        let unknown_id = format!("{}.{}", ana.id + 1, ana.unsubscribe_secret);

        for attempt in [off_by_one.as_str(), unknown_id.as_str(), "", "x.y"] {
            let removed = SubscriptionService::unsubscribe(&pool, attempt).await;
            assert!(removed.unwrap().is_none(), "{attempt:?}");
        }
        assert_eq!(subscriber_count(&pool).await, 1);
    }

    #[tokio::test]
    async fn a_member_is_saved_once_and_can_stop() {
        let pool = test_pool().await;
        let ana = UserRepository::create(&pool, "ana@example.com", "hash", "Ana", Role::Reader)
            .await
            .unwrap();

        let first = SubscriptionService::save_member(&pool, ana.id, "fr", EVERYTHING).await;
        let second = SubscriptionService::save_member(&pool, ana.id, "fr", ONLY_INCIDENTS).await;
        let stopped = SubscriptionService::stop_member(&pool, ana.id).await;
        let stopped_again = SubscriptionService::stop_member(&pool, ana.id).await;

        assert!(first.unwrap());
        assert!(!second.unwrap());
        assert!(stopped.unwrap());
        assert!(!stopped_again.unwrap());
        assert_eq!(subscriber_count(&pool).await, 0);
    }

    #[tokio::test]
    async fn visitors_are_kept_out_until_the_page_opens_to_them() {
        let pool = test_pool().await;
        assert!(!SubscriptionService::visitors_open(&pool).await.unwrap());

        SubscriptionService::set_visitors_open(&pool, true)
            .await
            .unwrap();
        assert!(SubscriptionService::visitors_open(&pool).await.unwrap());

        SubscriptionService::set_visitors_open(&pool, false)
            .await
            .unwrap();
        assert!(!SubscriptionService::visitors_open(&pool).await.unwrap());
    }
}
