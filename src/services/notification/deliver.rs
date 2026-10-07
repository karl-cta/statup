//! The queue of messages, sent in the background.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::task::{AbortHandle, JoinSet};
use tokio::time::{MissedTickBehavior, interval};

use super::{Facts, Failure, Notice, Notifier, Origin, Sender, Subject};
use crate::db::DbPool;
use crate::error::AppError;
use crate::i18n::I18n;
use crate::models::{Channel, Delivery};
use crate::repositories::{
    EventRepository, NotificationRepository, OutageRepository, ServiceRepository,
    SettingsRepository,
};

pub(crate) const PAGE_ADDRESS_SETTING: &str = "page_address";

const DELIVERY_INTERVAL: Duration = Duration::from_secs(5);
const PURGE_INTERVAL: Duration = Duration::from_secs(3600);
const KEEP_HISTORY: chrono::Duration = chrono::Duration::days(30);
/// The wait before each new attempt, after a failure that waiting may fix.
const RETRY_DELAYS: [Duration; 4] = [
    Duration::from_secs(30),
    Duration::from_secs(120),
    Duration::from_secs(600),
    Duration::from_secs(1800),
];
/// A tool that asks for a longer wait than this is not obeyed.
const LONGEST_ASKED_WAIT: Duration = Duration::from_secs(600);

/// The address of the page: `PUBLIC_URL` when set, else the one an admin
/// stored.
pub async fn page_address(
    pool: &DbPool,
    public_url: Option<&str>,
) -> Result<Option<String>, AppError> {
    if let Some(url) = public_url {
        return Ok(Some(url.to_string()));
    }
    let stored = SettingsRepository::get(pool, PAGE_ADDRESS_SETTING).await?;
    Ok(stored
        .map(|address| address.trim_end_matches('/').to_string())
        .filter(|address| !address.is_empty()))
}

/// A message ready to leave, with everything it is written from.
struct Outgoing {
    delivery: Delivery,
    channel: Channel,
    notice: Notice,
    facts: Facts,
}

/// Sends the first waiting message of each destination whose time has come,
/// then the ones behind them, until nothing is left to send. Returns how many
/// went out.
pub async fn deliver_due<S: Sender + Send + Sync + 'static>(
    pool: &DbPool,
    sender: &Arc<S>,
    public_url: Option<&str>,
    now: DateTime<Utc>,
) -> Result<usize, AppError> {
    let origin = Origin {
        instance: crate::brand_name(),
        page: page_address(pool, public_url).await?,
    };
    let mut sent = 0;
    let mut previous = Vec::new();
    loop {
        let heads = NotificationRepository::due(pool, now).await?;
        let ids: Vec<i64> = heads.iter().map(|delivery| delivery.id).collect();
        // The same heads again mean none could be dealt with: stop here.
        if ids.is_empty() || ids == previous {
            return Ok(sent);
        }
        previous = ids;
        sent += deliver_batch(pool, sender, &origin, heads, now).await?;
    }
}

/// Sends the heads together, one destination never waiting for another. A
/// send that panics leaves its message waiting for the next round.
async fn deliver_batch<S: Sender + Send + Sync + 'static>(
    pool: &DbPool,
    sender: &Arc<S>,
    origin: &Origin,
    heads: Vec<Delivery>,
    now: DateTime<Utc>,
) -> Result<usize, AppError> {
    let mut sends = JoinSet::new();
    for message in prepare(pool, origin, heads, now).await? {
        let sender = Arc::clone(sender);
        sends.spawn(async move {
            let result = sender
                .send(&message.channel, &message.notice, &message.facts)
                .await;
            (message, result)
        });
    }
    let mut sent = 0;
    while let Some(done) = sends.join_next().await {
        match done {
            Ok((message, result)) => {
                settle(pool, &message.delivery, &message.channel, result, now).await?;
                sent += usize::from(result.is_ok());
            }
            Err(e) => tracing::warn!(error = %e, "A notification send stopped before its result"),
        }
    }
    Ok(sent)
}

/// Writes each message. One whose destination was deleted is left to the
/// cascade; one whose subject is gone fails on its own.
async fn prepare(
    pool: &DbPool,
    origin: &Origin,
    heads: Vec<Delivery>,
    now: DateTime<Utc>,
) -> Result<Vec<Outgoing>, AppError> {
    let mut outgoing = Vec::with_capacity(heads.len());
    for delivery in heads {
        let Some(channel) = NotificationRepository::find_channel(pool, delivery.channel_id).await?
        else {
            continue;
        };
        let Some(subject) = load_subject(pool, &delivery).await? else {
            settle(pool, &delivery, &channel, Err(Failure::Gone), now).await?;
            continue;
        };
        let i18n = I18n::new(&channel.locale);
        let at = delivery.created_at;
        let notice = Notice::write(
            delivery.happening,
            delivery.lifecycle,
            &subject,
            origin,
            &i18n,
            at,
        );
        let facts = Facts::of(delivery.happening, delivery.lifecycle, &subject, origin, at);
        outgoing.push(Outgoing {
            delivery,
            channel,
            notice,
            facts,
        });
    }
    Ok(outgoing)
}

/// What the message is about as it stands now, or `None` when it is gone.
async fn load_subject(pool: &DbPool, delivery: &Delivery) -> Result<Option<Subject>, AppError> {
    if let Some(event_id) = delivery.event_id {
        return load_event(pool, event_id, delivery.update_id).await;
    }
    let Some(service_id) = delivery.service_id else {
        return Ok(Some(Subject::Test));
    };
    let Some(service) = ServiceRepository::find_by_id(pool, service_id).await? else {
        return Ok(None);
    };
    Ok(Some(Subject::Service {
        name: service.name,
        down_since: OutageRepository::latest_start(pool, service_id).await?,
    }))
}

async fn load_event(
    pool: &DbPool,
    event_id: i64,
    update_id: Option<i64>,
) -> Result<Option<Subject>, AppError> {
    let Some(found) = EventRepository::find_with_services(pool, event_id).await? else {
        return Ok(None);
    };
    let update = match update_id {
        Some(update_id) => EventRepository::find_update(pool, event_id, update_id)
            .await?
            .map(|update| update.message),
        None => None,
    };
    Ok(Some(Subject::Event {
        event: found.event,
        services: found.services.into_iter().map(|s| s.name).collect(),
        update,
    }))
}

/// Records how sending went, and logs it without the address.
async fn settle(
    pool: &DbPool,
    delivery: &Delivery,
    channel: &Channel,
    result: Result<(), Failure>,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    let Err(failure) = result else {
        NotificationRepository::mark_sent(pool, delivery.id, now).await?;
        tracing::debug!(channel_id = channel.id, kind = ?channel.kind, "Notification delivered");
        return Ok(());
    };
    let code = failure.code();
    tracing::warn!(channel_id = channel.id, kind = ?channel.kind, failure = %code, "Notification not delivered");
    if failure == Failure::Gone {
        NotificationRepository::fail(pool, delivery.id, &code, now).await?;
        return Ok(());
    }
    let retry = if failure.is_permanent() {
        None
    } else {
        retry_at(delivery.attempts, failure, now)
    };
    match retry {
        Some(at) => NotificationRepository::retry_later(pool, delivery.id, &code, at).await?,
        None => {
            NotificationRepository::fail_waiting(pool, channel.id, &code, now).await?;
        }
    }
    Ok(())
}

/// When to try again after `attempts` attempts, or `None` once they are used.
fn retry_at(attempts: i64, failure: Failure, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let delay = *RETRY_DELAYS.get(usize::try_from(attempts).ok()?)?;
    let asked = match failure {
        Failure::TooManyRequests(Some(asked)) => asked.min(LONGEST_ASKED_WAIT),
        _ => Duration::ZERO,
    };
    Some(now + chrono::Duration::from_std(delay.max(asked)).ok()?)
}

/// Sends what is due every few seconds for the life of the server, and
/// forgets the old finished messages once an hour.
pub fn spawn_notifications(
    pool: DbPool,
    notifier: Arc<Notifier>,
    public_url: Option<String>,
) -> AbortHandle {
    let task = tokio::spawn(async move {
        let mut deliveries = interval(DELIVERY_INTERVAL);
        deliveries.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut purges = interval(PURGE_INTERVAL);
        purges.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = deliveries.tick() => {
                    let now = Utc::now();
                    if let Err(e) = deliver_due(&pool, &notifier, public_url.as_deref(), now).await {
                        tracing::warn!(error = %e, "Notifications round failed");
                    }
                }
                _ = purges.tick() => {
                    if let Err(e) = NotificationRepository::purge(&pool, Utc::now() - KEEP_HISTORY).await {
                        tracing::warn!(error = %e, "Old notifications not purged");
                    }
                }
            }
        }
    });
    task.abort_handle()
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, VecDeque};
    use std::sync::{Mutex, PoisonError};

    use chrono::{Duration as Delay, TimeZone};

    use super::*;
    use crate::models::{
        ChannelInput, ChannelKind, CreateEventInput, DeliveryStatus, Happening, Kind, Lifecycle,
        Notification, Role, Severity,
    };
    use crate::repositories::UserRepository;
    use crate::services::Finding;
    use crate::test_helpers::test_pool;

    /// Records what it is asked to send and answers from a script per
    /// destination, success once the script is spent.
    #[derive(Default)]
    struct Script {
        sent: Mutex<Vec<(i64, String)>>,
        answers: Mutex<HashMap<i64, VecDeque<Result<(), Failure>>>>,
    }

    impl Script {
        fn answering(channel_id: i64, answers: &[Result<(), Failure>]) -> Self {
            let script = Self::default();
            script
                .answers
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(channel_id, answers.iter().copied().collect());
            script
        }

        fn sent(&self) -> Vec<(i64, String)> {
            self.sent
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        }
    }

    impl Sender for Script {
        fn send(
            &self,
            channel: &Channel,
            notice: &Notice,
            _facts: &Facts,
        ) -> impl Future<Output = Result<(), Failure>> + Send {
            self.sent
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((channel.id, notice.headline.clone()));
            let answer = self
                .answers
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get_mut(&channel.id)
                .and_then(VecDeque::pop_front)
                .unwrap_or(Ok(()));
            std::future::ready(answer)
        }
    }

    fn at(minute: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 7, 9, 0, 0).unwrap() + Delay::minutes(minute)
    }

    async fn channel(pool: &DbPool, name: &str, locale: &str) -> i64 {
        let input = ChannelInput {
            name: name.to_string(),
            kind: ChannelKind::Slack,
            target: format!("https://hooks.slack.com/services/{name}"),
            locale: locale.to_string(),
            on_incidents: true,
            on_maintenances: true,
            on_publications: true,
            on_detected: true,
        };
        NotificationRepository::create_channel(pool, &input)
            .await
            .unwrap()
            .id
    }

    async fn incident(pool: &DbPool, title: &str) -> i64 {
        let author = match UserRepository::find_by_email(pool, "ops@example.com")
            .await
            .unwrap()
        {
            Some(user) => user,
            None => UserRepository::create(pool, "ops@example.com", "hash", "Ops", Role::Admin)
                .await
                .unwrap(),
        };
        let input = CreateEventInput {
            kind: Kind::Incident,
            severity: Some(Severity::Critical),
            planned: false,
            category: None,
            title: title.to_string(),
            description: String::new(),
            planned_start: None,
            planned_end: None,
            started_at: None,
            opening_step: None,
            keeps_services_up: false,
            service_ids: Vec::new(),
            follows_event_id: None,
            author_id: author.id,
        };
        EventRepository::create(pool, &input).await.unwrap().id
    }

    async fn announce(pool: &DbPool, event_id: i64, minute: i64) {
        let notification = Notification::event(
            Happening::Opened,
            Kind::Incident,
            event_id,
            Some(Lifecycle::Investigating),
        );
        NotificationRepository::enqueue(pool, &notification, at(minute))
            .await
            .unwrap();
    }

    async fn rows(pool: &DbPool, channel_id: i64) -> Vec<Delivery> {
        sqlx::query_as("SELECT * FROM notification_deliveries WHERE channel_id = ? ORDER BY id")
            .bind(channel_id)
            .fetch_all(pool)
            .await
            .unwrap()
    }

    fn timed_out() -> Result<(), Failure> {
        Err(Failure::Transport(Finding::TimedOut))
    }

    #[tokio::test]
    async fn the_messages_of_a_destination_go_out_in_order_in_one_call() {
        let pool = test_pool().await;
        let team = channel(&pool, "team", "en").await;
        let first = incident(&pool, "First outage").await;
        let second = incident(&pool, "Second outage").await;
        announce(&pool, first, 0).await;
        announce(&pool, second, 1).await;
        let script = Arc::new(Script::default());

        let sent = deliver_due(&pool, &script, None, at(2)).await.unwrap();

        assert_eq!(sent, 2);
        let headlines = script.sent();
        assert!(headlines[0].1.contains("First outage"), "{headlines:?}");
        assert!(headlines[1].1.contains("Second outage"), "{headlines:?}");
        let statuses: Vec<_> = rows(&pool, team).await.iter().map(|d| d.status).collect();
        assert_eq!(statuses, [DeliveryStatus::Sent, DeliveryStatus::Sent]);
    }

    #[tokio::test]
    async fn a_transient_failure_is_retried_later_and_spares_other_destinations() {
        let pool = test_pool().await;
        let shaky = channel(&pool, "shaky", "en").await;
        let steady = channel(&pool, "steady", "en").await;
        announce(&pool, incident(&pool, "Outage").await, 0).await;
        let script = Arc::new(Script::answering(shaky, &[timed_out()]));

        let sent = deliver_due(&pool, &script, None, at(1)).await.unwrap();

        assert_eq!(sent, 1);
        let waiting = &rows(&pool, shaky).await[0];
        assert_eq!(waiting.status, DeliveryStatus::Pending);
        assert_eq!(waiting.attempts, 1);
        assert_eq!(waiting.failure.as_deref(), Some("timed_out"));
        assert_eq!(waiting.next_attempt_at, at(1) + Delay::seconds(30));
        assert_eq!(rows(&pool, steady).await[0].status, DeliveryStatus::Sent);
    }

    #[tokio::test]
    async fn the_queue_fails_once_every_attempt_is_used() {
        let pool = test_pool().await;
        let team = channel(&pool, "team", "en").await;
        let event = incident(&pool, "Outage").await;
        announce(&pool, event, 0).await;
        announce(&pool, event, 0).await;
        let script = Arc::new(Script::answering(team, &[timed_out(); 5]));

        for round in 0..4 {
            deliver_due(&pool, &script, None, at(60 * (round + 1)))
                .await
                .unwrap();
            let statuses: Vec<_> = rows(&pool, team).await.iter().map(|d| d.status).collect();
            assert_eq!(statuses, [DeliveryStatus::Pending; 2], "round {round}");
        }
        deliver_due(&pool, &script, None, at(300)).await.unwrap();

        let failed = rows(&pool, team).await;
        assert!(failed.iter().all(|d| d.status == DeliveryStatus::Failed));
        assert!(
            failed
                .iter()
                .all(|d| d.failure.as_deref() == Some("timed_out"))
        );
        assert_eq!(script.sent().len(), 5);
    }

    #[tokio::test]
    async fn a_permanent_failure_fails_the_whole_queue_at_once() {
        let pool = test_pool().await;
        let team = channel(&pool, "team", "en").await;
        let event = incident(&pool, "Outage").await;
        for minute in 0..3 {
            announce(&pool, event, minute).await;
        }
        let script = Arc::new(Script::answering(team, &[Err(Failure::Status(404))]));

        let sent = deliver_due(&pool, &script, None, at(5)).await.unwrap();

        assert_eq!(sent, 0);
        assert_eq!(script.sent().len(), 1);
        let failed = rows(&pool, team).await;
        assert_eq!(failed.len(), 3);
        assert!(failed.iter().all(|d| d.status == DeliveryStatus::Failed));
        assert!(
            failed
                .iter()
                .all(|d| d.failure.as_deref() == Some("status_404"))
        );
    }

    #[tokio::test]
    async fn a_tool_asking_to_slow_down_is_waited_for() {
        let pool = test_pool().await;
        let team = channel(&pool, "team", "en").await;
        announce(&pool, incident(&pool, "Outage").await, 0).await;
        let asked = Failure::TooManyRequests(Some(Duration::from_secs(300)));
        let script = Arc::new(Script::answering(team, &[Err(asked)]));

        deliver_due(&pool, &script, None, at(1)).await.unwrap();

        let waiting = &rows(&pool, team).await[0];
        assert_eq!(waiting.status, DeliveryStatus::Pending);
        assert_eq!(waiting.failure.as_deref(), Some("too_many_requests"));
        assert_eq!(waiting.next_attempt_at, at(1) + Delay::minutes(5));
    }

    #[test]
    fn an_asked_wait_is_capped_and_never_shorter_than_the_usual_delay() {
        let asked = |seconds| Failure::TooManyRequests(Some(Duration::from_secs(seconds)));
        assert_eq!(retry_at(0, asked(3600), at(0)), Some(at(10)));
        assert_eq!(
            retry_at(0, asked(5), at(0)),
            Some(at(0) + Delay::seconds(30))
        );
        assert_eq!(retry_at(3, timed_out_failure(), at(0)), Some(at(30)));
        assert_eq!(retry_at(4, timed_out_failure(), at(0)), None);
    }

    fn timed_out_failure() -> Failure {
        Failure::Transport(Finding::TimedOut)
    }

    #[tokio::test]
    async fn each_destination_is_written_to_in_its_language() {
        let pool = test_pool().await;
        let english = channel(&pool, "english", "en").await;
        let french = channel(&pool, "french", "fr").await;
        announce(&pool, incident(&pool, "Outage").await, 0).await;
        let script = Arc::new(Script::default());

        deliver_due(&pool, &script, None, at(1)).await.unwrap();

        let sent = script.sent();
        let headline = |id| sent.iter().find(|(c, _)| *c == id).unwrap().1.clone();
        assert_ne!(headline(english), headline(french));
    }

    #[tokio::test]
    async fn the_page_address_is_the_public_url_then_the_setting_then_nothing() {
        let pool = test_pool().await;
        assert_eq!(page_address(&pool, None).await.unwrap(), None);

        SettingsRepository::set(&pool, PAGE_ADDRESS_SETTING, "https://status.example.com/")
            .await
            .unwrap();
        assert_eq!(
            page_address(&pool, None).await.unwrap().as_deref(),
            Some("https://status.example.com")
        );
        assert_eq!(
            page_address(&pool, Some("https://pages.example.org"))
                .await
                .unwrap()
                .as_deref(),
            Some("https://pages.example.org")
        );
    }

    #[tokio::test]
    async fn a_vanished_service_fails_its_message_and_the_queue_goes_on() {
        let pool = test_pool().await;
        let team = channel(&pool, "team", "en").await;
        let service = ServiceRepository::create(&pool, "Mail", "mail", None, None, None)
            .await
            .unwrap();
        let down = Notification::service(Happening::ServiceDown, service.id);
        NotificationRepository::enqueue(&pool, &down, at(0))
            .await
            .unwrap();
        announce(&pool, incident(&pool, "Outage").await, 1).await;
        // The cascade would drop the message with its service: this plays
        // a deletion that lands between the queue being read and the send.
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&pool)
            .await
            .unwrap();
        ServiceRepository::delete(&pool, service.id).await.unwrap();
        let script = Arc::new(Script::default());

        let sent = deliver_due(&pool, &script, None, at(2)).await.unwrap();

        assert_eq!(sent, 1);
        let outcomes = rows(&pool, team).await;
        assert_eq!(outcomes[0].status, DeliveryStatus::Failed);
        assert_eq!(outcomes[0].failure.as_deref(), Some("gone"));
        assert_eq!(outcomes[1].status, DeliveryStatus::Sent);
    }
}
