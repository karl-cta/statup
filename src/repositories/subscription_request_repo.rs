//! The requests of visitors who asked to subscribe, until they confirm.

use sqlx::AssertSqlSafe;

use crate::db::DbPool;
use crate::models::{Choices, SubscriptionRequest};

/// How long a link confirms its request, as an `SQLite` date modifier.
const LINK_LIFETIME: &str = "+1 day";

/// Whether the last hour leaves room for one more request, for the address
/// and for the whole page. Binds the address, then the two limits.
const UNDER_LIMITS: &str = "(SELECT COUNT(*) FROM subscription_requests \
     WHERE email = ? AND created_at > datetime('now', '-1 hour')) < ? \
     AND (SELECT COUNT(*) FROM subscription_requests \
     WHERE created_at > datetime('now', '-1 hour')) < ?";

/// What a visitor asked for, with the hash of the secret their link carries.
pub struct NewSubscriptionRequest<'a> {
    pub email: &'a str,
    pub secret_hash: &'a str,
    pub locale: &'a str,
    pub choices: Choices,
}

/// How many requests the last hour may hold before new ones are refused.
#[derive(Debug, Clone, Copy)]
pub struct RequestLimits {
    pub per_address: i64,
    pub per_instance: i64,
}

pub struct SubscriptionRequestRepository;

impl SubscriptionRequestRepository {
    /// Store a request and return its id, unless the last hour already holds
    /// as many requests as `limits` allows, for this address or for the
    /// whole page. Counting and storing are one statement, so requests at
    /// the same moment cannot pass the limit together.
    pub async fn insert_within_limits(
        pool: &DbPool,
        request: &NewSubscriptionRequest<'_>,
        limits: RequestLimits,
    ) -> Result<Option<i64>, sqlx::Error> {
        sqlx::query_scalar(AssertSqlSafe(format!(
            "INSERT INTO subscription_requests (email, secret_hash, locale, on_incidents, \
             on_maintenances, on_publications, expires_at) \
             SELECT ?, ?, ?, ?, ?, ?, datetime('now', ?) WHERE {UNDER_LIMITS} RETURNING id"
        )))
        .bind(request.email)
        .bind(request.secret_hash)
        .bind(request.locale)
        .bind(request.choices.incidents)
        .bind(request.choices.maintenances)
        .bind(request.choices.publications)
        .bind(LINK_LIFETIME)
        .bind(request.email)
        .bind(limits.per_address)
        .bind(limits.per_instance)
        .fetch_optional(pool)
        .await
    }

    /// Whether a request for this address would pass `limits` right now.
    /// A cheap look before the costly hashing, not a reservation:
    /// `insert_within_limits` has the last word.
    pub async fn within_limits(
        pool: &DbPool,
        email: &str,
        limits: RequestLimits,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar(AssertSqlSafe(format!("SELECT {UNDER_LIMITS}")))
            .bind(email)
            .bind(limits.per_address)
            .bind(limits.per_instance)
            .fetch_one(pool)
            .await
    }

    /// The request with this id, if its link still confirms it. A newer
    /// request for the same address replaces it.
    pub async fn find_live(
        pool: &DbPool,
        id: i64,
    ) -> Result<Option<SubscriptionRequest>, sqlx::Error> {
        sqlx::query_as::<_, SubscriptionRequest>(
            "SELECT r.email, r.secret_hash, r.locale, r.on_incidents, \
             r.on_maintenances, r.on_publications FROM subscription_requests r \
             WHERE r.id = ? AND r.expires_at > datetime('now') \
               AND NOT EXISTS (SELECT 1 FROM subscription_requests newer \
                               WHERE newer.email = r.email AND newer.id > r.id)",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
    }

    /// Forget the requests older than a day: past their link's lifetime,
    /// and no longer counted against their address.
    pub async fn delete_stale(pool: &DbPool) -> Result<(), sqlx::Error> {
        sqlx::query(
            "DELETE FROM subscription_requests WHERE created_at < datetime('now', '-1 day')",
        )
        .execute(pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::test_pool;

    const EVERYTHING: Choices = Choices {
        incidents: true,
        maintenances: true,
        publications: true,
    };
    const NO_LIMIT: RequestLimits = RequestLimits {
        per_address: 100,
        per_instance: 100,
    };

    fn asked(email: &str) -> NewSubscriptionRequest<'_> {
        NewSubscriptionRequest {
            email,
            secret_hash: "hash",
            locale: "fr",
            choices: EVERYTHING,
        }
    }

    async fn insert(pool: &DbPool, email: &str) -> i64 {
        SubscriptionRequestRepository::insert_within_limits(pool, &asked(email), NO_LIMIT)
            .await
            .unwrap()
            .unwrap()
    }

    async fn age(pool: &DbPool, id: i64, created: &str, expires: &str) {
        sqlx::query(
            "UPDATE subscription_requests SET created_at = datetime('now', ?), \
                                              expires_at = datetime('now', ?) WHERE id = ?",
        )
        .bind(created)
        .bind(expires)
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn requests(pool: &DbPool) -> Vec<i64> {
        sqlx::query_scalar("SELECT id FROM subscription_requests ORDER BY id")
            .fetch_all(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_new_request_keeps_what_the_visitor_asked() {
        let pool = test_pool().await;
        let only_incidents = Choices {
            incidents: true,
            maintenances: false,
            publications: false,
        };
        let new = NewSubscriptionRequest {
            choices: only_incidents,
            locale: "de",
            ..asked("ana@example.com")
        };

        let id = SubscriptionRequestRepository::insert_within_limits(&pool, &new, NO_LIMIT)
            .await
            .unwrap()
            .unwrap();
        let request = SubscriptionRequestRepository::find_live(&pool, id)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(request.email, "ana@example.com");
        assert_eq!(request.secret_hash, "hash");
        assert_eq!(request.locale, "de");
        assert_eq!(request.choices, only_incidents);
    }

    #[tokio::test]
    async fn a_request_for_nothing_is_refused() {
        let pool = test_pool().await;
        let nothing = Choices {
            incidents: false,
            maintenances: false,
            publications: false,
        };
        let new = NewSubscriptionRequest {
            choices: nothing,
            ..asked("ana@example.com")
        };

        let stored =
            SubscriptionRequestRepository::insert_within_limits(&pool, &new, NO_LIMIT).await;

        assert!(stored.is_err());
    }

    #[tokio::test]
    async fn a_link_a_day_old_no_longer_confirms() {
        let pool = test_pool().await;
        let id = insert(&pool, "ana@example.com").await;

        age(&pool, id, "-25 hours", "-1 hour").await;

        let live = SubscriptionRequestRepository::find_live(&pool, id).await;
        assert!(live.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_newer_request_replaces_the_older_one_whatever_the_case() {
        let pool = test_pool().await;
        let first = insert(&pool, "ana@example.com").await;
        let other = insert(&pool, "ben@example.com").await;
        let second = insert(&pool, "Ana@Example.com").await;

        let live = |id| SubscriptionRequestRepository::find_live(&pool, id);
        assert!(live(first).await.unwrap().is_none());
        assert!(live(second).await.unwrap().is_some());
        assert!(
            live(other).await.unwrap().is_some(),
            "another address's request stays"
        );
    }

    #[tokio::test]
    async fn the_limits_count_the_last_hour_of_an_address_and_of_the_page() {
        let pool = test_pool().await;
        let old = insert(&pool, "ana@example.com").await;
        age(&pool, old, "-2 hours", "+22 hours").await;
        let limits = RequestLimits {
            per_address: 2,
            per_instance: 3,
        };
        let within = |email| {
            let pool = pool.clone();
            async move {
                SubscriptionRequestRepository::insert_within_limits(&pool, &asked(email), limits)
                    .await
                    .unwrap()
            }
        };

        assert!(within("ana@example.com").await.is_some());
        assert!(within("ANA@example.com").await.is_some());
        assert!(
            within("ana@example.com").await.is_none(),
            "two for this address"
        );
        assert!(within("ben@example.com").await.is_some());
        assert!(
            within("cleo@example.com").await.is_none(),
            "three for the page"
        );
    }

    #[tokio::test]
    async fn within_limits_looks_at_the_same_counts_without_storing() {
        let pool = test_pool().await;
        let limits = RequestLimits {
            per_address: 1,
            per_instance: 2,
        };
        let within = |email| SubscriptionRequestRepository::within_limits(&pool, email, limits);

        assert!(within("ana@example.com").await.unwrap());
        assert_eq!(requests(&pool).await.len(), 0);

        insert(&pool, "ana@example.com").await;
        assert!(!within("ANA@example.com").await.unwrap(), "one for Ana");
        assert!(within("ben@example.com").await.unwrap());

        insert(&pool, "ben@example.com").await;
        assert!(
            !within("cleo@example.com").await.unwrap(),
            "two for the page"
        );
    }

    #[tokio::test]
    async fn requests_a_day_old_are_forgotten() {
        let pool = test_pool().await;
        let stale = insert(&pool, "ana@example.com").await;
        age(&pool, stale, "-25 hours", "-1 hour").await;
        let recent = insert(&pool, "ana@example.com").await;

        SubscriptionRequestRepository::delete_stale(&pool)
            .await
            .unwrap();

        assert_eq!(requests(&pool).await, [recent]);
    }
}
