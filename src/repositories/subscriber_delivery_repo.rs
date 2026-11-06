//! The queue of emails to subscribers.

use chrono::{DateTime, Utc};

use sqlx::AssertSqlSafe;

use crate::clock;
use crate::db::DbPool;
use crate::models::SubscriberMessage;

const MESSAGE_COLUMNS: &str = "d.id, d.subscriber_id, d.happening, d.event_id, d.update_id, \
     d.lifecycle, d.attempts, d.created_at, s.user_id, s.email, s.locale, s.unsubscribe_secret";

/// The newest email of each subscriber that was sent or given up, which the
/// list of subscribers shows: a skipped one says nothing of the address.
const LATEST_OUTCOMES: &str = "SELECT MAX(id) FROM subscriber_deliveries \
     WHERE status IN ('sent', 'failed') GROUP BY subscriber_id";

pub struct SubscriberDeliveryRepository;

impl SubscriberDeliveryRepository {
    /// Whether any email waits, due or not: most rounds find none.
    pub async fn any_waiting(pool: &DbPool) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM subscriber_deliveries WHERE status = 'pending')",
        )
        .fetch_one(pool)
        .await
    }

    /// The first waiting email of each subscriber whose time has come,
    /// oldest first, at most `limit`. The ones behind it wait their turn, so
    /// each subscriber receives the news in order.
    pub async fn due(
        pool: &DbPool,
        now: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<SubscriberMessage>, sqlx::Error> {
        sqlx::query_as::<_, SubscriberMessage>(AssertSqlSafe(format!(
            "SELECT {MESSAGE_COLUMNS} FROM subscriber_deliveries d \
             JOIN subscribers s ON s.id = d.subscriber_id \
             WHERE d.status = 'pending' AND d.next_attempt_at <= ? \
               AND d.id = (SELECT MIN(p.id) FROM subscriber_deliveries p \
                           WHERE p.subscriber_id = d.subscriber_id AND p.status = 'pending') \
             ORDER BY d.id LIMIT ?"
        )))
        .bind(clock::db(now))
        .bind(limit)
        .fetch_all(pool)
        .await
    }

    pub async fn mark_sent(pool: &DbPool, id: i64, now: DateTime<Utc>) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE subscriber_deliveries SET status = 'sent', attempts = attempts + 1, \
             failure = NULL, sent_at = ? WHERE id = ?",
        )
        .bind(clock::db(now))
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Counts a failed attempt and sets when the next one is made.
    pub async fn retry_later(
        pool: &DbPool,
        id: i64,
        failure: &str,
        next_attempt_at: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE subscriber_deliveries SET attempts = attempts + 1, failure = ?, \
             next_attempt_at = ? WHERE id = ? AND status = 'pending'",
        )
        .bind(failure)
        .bind(clock::db(next_attempt_at))
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Keeps why the email could not leave without counting an attempt: the
    /// mail server failed, not the address, and the email waits for it.
    pub async fn note(pool: &DbPool, id: i64, failure: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE subscriber_deliveries SET failure = ? WHERE id = ? AND status = 'pending'",
        )
        .bind(failure)
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Gives up on one email, the next ones of the subscriber going on.
    pub async fn fail(
        pool: &DbPool,
        id: i64,
        failure: &str,
        now: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        Self::close(pool, id, "failed", failure, now).await
    }

    /// Leaves out an email the subscriber may not receive when its turn
    /// comes: not a failure, and never sent later.
    pub async fn skip(
        pool: &DbPool,
        id: i64,
        reason: &str,
        now: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        Self::close(pool, id, "skipped", reason, now).await
    }

    async fn close(
        pool: &DbPool,
        id: i64,
        status: &str,
        why: &str,
        now: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE subscriber_deliveries SET status = ?, attempts = attempts + 1, \
             failure = ?, next_attempt_at = ? WHERE id = ? AND status = 'pending'",
        )
        .bind(status)
        .bind(why)
        .bind(clock::db(now))
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Leaves out every waiting email of every visitor: visitors may not
    /// receive any now, and none is sent later.
    pub async fn skip_visitors(
        pool: &DbPool,
        reason: &str,
        now: DateTime<Utc>,
    ) -> Result<u64, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE subscriber_deliveries SET status = 'skipped', attempts = attempts + 1, \
             failure = ?, next_attempt_at = ? WHERE status = 'pending' \
             AND subscriber_id IN (SELECT id FROM subscribers WHERE user_id IS NULL)",
        )
        .bind(reason)
        .bind(clock::db(now))
        .execute(pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Gives up on the waiting emails queued before `queued_before`, or on
    /// all of them, for `failure`: news that late would mislead more than it
    /// informs, and without a mail server none can leave.
    pub async fn give_up_waiting(
        pool: &DbPool,
        failure: &str,
        queued_before: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> Result<u64, sqlx::Error> {
        let mut sql = String::from(
            "UPDATE subscriber_deliveries SET status = 'failed', failure = ?, \
             next_attempt_at = ? WHERE status = 'pending'",
        );
        if queued_before.is_some() {
            sql.push_str(" AND created_at < ?");
        }
        let query = sqlx::query(AssertSqlSafe(sql))
            .bind(failure)
            .bind(clock::db(now));
        let query = match queued_before {
            Some(before) => query.bind(clock::db(before)),
            None => query,
        };
        Ok(query.execute(pool).await?.rows_affected())
    }

    /// Forgets the finished emails queued before `before`, except the last
    /// one of each subscriber, which the list of subscribers shows.
    pub async fn purge(pool: &DbPool, before: DateTime<Utc>) -> Result<u64, sqlx::Error> {
        let result = sqlx::query(AssertSqlSafe(format!(
            "DELETE FROM subscriber_deliveries \
             WHERE status != 'pending' AND created_at < ? AND id NOT IN ({LATEST_OUTCOMES})"
        )))
        .bind(clock::db(before))
        .execute(pool)
        .await?;
        Ok(result.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;
    use crate::models::{CreateEventInput, Happening, Kind, Role, Severity};
    use crate::repositories::{EventRepository, UserRepository};
    use crate::test_helpers::test_pool;

    type Repo = SubscriberDeliveryRepository;

    async fn visitor(pool: &DbPool, email: &str) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO subscribers (email, locale, unsubscribe_secret) \
             VALUES (?, 'en', 'secret') RETURNING id",
        )
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    async fn incident(pool: &DbPool) -> i64 {
        let author = UserRepository::create(pool, "ops@example.com", "hash", "Ops", Role::Admin)
            .await
            .unwrap();
        let input = CreateEventInput {
            kind: Kind::Incident,
            severity: Some(Severity::Minor),
            planned: false,
            category: None,
            title: "Network down".to_string(),
            description: String::new(),
            planned_start: None,
            planned_end: None,
            started_at: None,
            opening_step: None,
            keeps_services_up: false,
            service_ids: Vec::new(),
            follows_event_id: None,
            author_id: author.id,
            api_key_id: None,
            next_update: None,
        };
        EventRepository::create(pool, &input).await.unwrap().id
    }

    /// An email queued `ago` before `now`, due at once.
    async fn queue(pool: &DbPool, subscriber_id: i64, event_id: i64, ago: Duration) -> i64 {
        let at = clock::db(Utc::now() - ago);
        sqlx::query_scalar(
            "INSERT INTO subscriber_deliveries (subscriber_id, happening, event_id, \
             next_attempt_at, created_at) VALUES (?, 'opened', ?, ?, ?) RETURNING id",
        )
        .bind(subscriber_id)
        .bind(event_id)
        .bind(&at)
        .bind(&at)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    async fn rows(pool: &DbPool) -> Vec<(i64, String, i64, Option<String>)> {
        sqlx::query_as(
            "SELECT id, status, attempts, failure FROM subscriber_deliveries ORDER BY id",
        )
        .fetch_all(pool)
        .await
        .unwrap()
    }

    fn ids(messages: &[SubscriberMessage]) -> Vec<i64> {
        messages.iter().map(|message| message.id).collect()
    }

    #[tokio::test]
    async fn each_subscriber_waits_for_their_first_email_to_leave() {
        let pool = test_pool().await;
        let event = incident(&pool).await;
        let ana = visitor(&pool, "ana@example.com").await;
        let ben = visitor(&pool, "ben@example.com").await;
        let first = queue(&pool, ana, event, Duration::minutes(2)).await;
        let second = queue(&pool, ana, event, Duration::minutes(1)).await;
        let bens = queue(&pool, ben, event, Duration::minutes(1)).await;
        let now = Utc::now();

        let due = Repo::due(&pool, now, 100).await.unwrap();
        assert_eq!(ids(&due), [first, bens]);
        assert_eq!(due[0].happening, Happening::Opened);
        assert_eq!(due[0].email.as_deref(), Some("ana@example.com"));
        assert_eq!(due[0].unsubscribe_secret, "secret");

        Repo::mark_sent(&pool, first, now).await.unwrap();
        assert_eq!(
            ids(&Repo::due(&pool, now, 100).await.unwrap()),
            [second, bens]
        );
        assert_eq!(ids(&Repo::due(&pool, now, 1).await.unwrap()), [second]);
    }

    #[tokio::test]
    async fn an_email_tried_again_later_holds_back_the_next_ones() {
        let pool = test_pool().await;
        let event = incident(&pool).await;
        let ana = visitor(&pool, "ana@example.com").await;
        let first = queue(&pool, ana, event, Duration::minutes(2)).await;
        queue(&pool, ana, event, Duration::minutes(1)).await;
        let now = Utc::now();

        Repo::retry_later(&pool, first, "mail_busy_450", now + Duration::seconds(30))
            .await
            .unwrap();

        assert!(Repo::due(&pool, now, 100).await.unwrap().is_empty());
        let later = Repo::due(&pool, now + Duration::minutes(1), 100)
            .await
            .unwrap();
        assert_eq!(ids(&later), [first]);
        assert_eq!(later[0].attempts, 1);
    }

    #[tokio::test]
    async fn a_server_failure_is_kept_without_counting_an_attempt() {
        let pool = test_pool().await;
        let event = incident(&pool).await;
        let ana = visitor(&pool, "ana@example.com").await;
        let id = queue(&pool, ana, event, Duration::minutes(1)).await;

        Repo::note(&pool, id, "mail_unreachable").await.unwrap();

        let stored = rows(&pool).await;
        assert_eq!(
            stored,
            [(
                id,
                "pending".to_string(),
                0,
                Some("mail_unreachable".to_string())
            )]
        );
    }

    #[tokio::test]
    async fn a_failed_or_skipped_email_lets_the_next_one_go() {
        let pool = test_pool().await;
        let event = incident(&pool).await;
        let ana = visitor(&pool, "ana@example.com").await;
        let failed = queue(&pool, ana, event, Duration::minutes(3)).await;
        let skipped = queue(&pool, ana, event, Duration::minutes(2)).await;
        let next = queue(&pool, ana, event, Duration::minutes(1)).await;
        let now = Utc::now();

        Repo::fail(&pool, failed, "mail_refused_550", now)
            .await
            .unwrap();
        Repo::skip(&pool, skipped, "hidden", now).await.unwrap();

        assert_eq!(ids(&Repo::due(&pool, now, 100).await.unwrap()), [next]);
        let statuses: Vec<(String, Option<String>)> = rows(&pool)
            .await
            .into_iter()
            .map(|(_, status, _, failure)| (status, failure))
            .collect();
        assert_eq!(
            statuses,
            [
                ("failed".to_string(), Some("mail_refused_550".to_string())),
                ("skipped".to_string(), Some("hidden".to_string())),
                ("pending".to_string(), None),
            ]
        );
    }

    #[tokio::test]
    async fn waiting_emails_are_given_up_all_at_once_or_once_too_old() {
        let pool = test_pool().await;
        let event = incident(&pool).await;
        let ana = visitor(&pool, "ana@example.com").await;
        let ben = visitor(&pool, "ben@example.com").await;
        let old = queue(&pool, ana, event, Duration::hours(49)).await;
        let recent = queue(&pool, ben, event, Duration::hours(1)).await;
        let now = Utc::now();
        assert!(Repo::any_waiting(&pool).await.unwrap());

        let expired = Repo::give_up_waiting(&pool, "expired", Some(now - Duration::hours(48)), now)
            .await
            .unwrap();
        assert_eq!(expired, 1);
        assert_eq!(ids(&Repo::due(&pool, now, 100).await.unwrap()), [recent]);

        let failed = Repo::give_up_waiting(&pool, "no_mail_server", None, now)
            .await
            .unwrap();
        assert_eq!(failed, 1);
        let failures: Vec<(i64, Option<String>)> = rows(&pool)
            .await
            .into_iter()
            .map(|(id, _, _, failure)| (id, failure))
            .collect();
        assert_eq!(
            failures,
            [
                (old, Some("expired".to_string())),
                (recent, Some("no_mail_server".to_string())),
            ]
        );
        assert!(!Repo::any_waiting(&pool).await.unwrap());
    }

    #[tokio::test]
    async fn the_purge_keeps_waiting_emails_and_each_last_outcome() {
        let pool = test_pool().await;
        let event = incident(&pool).await;
        let ana = visitor(&pool, "ana@example.com").await;
        let old_sent = queue(&pool, ana, event, Duration::days(40)).await;
        let last_sent = queue(&pool, ana, event, Duration::days(35)).await;
        let waiting = queue(&pool, ana, event, Duration::days(31)).await;
        let now = Utc::now();
        Repo::mark_sent(&pool, old_sent, now).await.unwrap();
        Repo::mark_sent(&pool, last_sent, now).await.unwrap();

        let purged = Repo::purge(&pool, now - Duration::days(30)).await.unwrap();

        assert_eq!(purged, 1);
        let kept: Vec<i64> = rows(&pool).await.into_iter().map(|row| row.0).collect();
        assert_eq!(kept, [last_sent, waiting]);
    }
}
