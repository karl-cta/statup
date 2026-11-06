//! The emails to subscribers, sent in the background apart from the alert
//! destinations, so that a long list never holds back a message to the team.
//!
//! Each email is written when its turn comes, for what its reader may see
//! then: a visitor sees the public services, a member what their account
//! opens. An event they may no longer see is skipped, never sent later.

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::sync::Semaphore;
use tokio::task::{AbortHandle, JoinSet};
use tokio::time::{MissedTickBehavior, interval, sleep};

use super::deliver::{KEEP_HISTORY, RETRY_DELAYS, Reading, load_event, retry_at};
use super::{
    Facts, Failure, Notice, Notifier, Origin, SubscriberEmail, SubscriberSender, Unsubscribe,
    page_address,
};
use crate::db::DbPool;
use crate::error::AppError;
use crate::i18n::I18n;
use crate::journal;
use crate::models::{Happening, JournalKind, Lifecycle, Scope, SubscriberMessage};
use crate::repositories::{
    NewJournalEntry, SubscriberDeliveryRepository, SubscriptionRequestRepository, UserRepository,
};
use crate::services::{SubscriptionService, scope_of};

const ROUND_INTERVAL: Duration = Duration::from_secs(10);
const PURGE_INTERVAL: Duration = Duration::from_secs(3600);
/// Emails on their way at once: enough to go fast, few enough for the mail
/// server to take.
const SENDS_AT_ONCE: usize = 4;
/// Subscribers looked at in one batch.
const BATCH: i64 = 100;
/// A round stops after this many emails, so that a shutdown or the purge
/// never waits long.
const SENDS_PER_ROUND: usize = 600;
/// News still waiting after this long is given up: that late, it would
/// mislead more than it informs.
const KEEP_WAITING: chrono::Duration = chrono::Duration::hours(48);

/// Why an email was left out: the page or the subscriptions of visitors are
/// closed, the member's account is disabled, or the event is not theirs to
/// see.
const CLOSED: &str = "closed";
const INACTIVE: &str = "inactive";
const HIDDEN: &str = "hidden";
/// What a round did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Round {
    pub sent: usize,
    /// Emails given up, by reason.
    pub given_up: BTreeMap<String, usize>,
    /// The mail server failed as a whole, and the round stopped there.
    pub server_failure: Option<Failure>,
}

impl Round {
    fn give_up(&mut self, code: &str, count: u64) {
        if let Ok(count @ 1..) = usize::try_from(count) {
            *self.given_up.entry(code.to_string()).or_default() += count;
        }
    }
}

/// One email ready to leave.
struct Outgoing {
    id: i64,
    attempts: i64,
    to: String,
    locale: String,
    notice: Notice,
    facts: Facts,
    unsubscribe: Unsubscribe,
}

impl Outgoing {
    fn mail(&self) -> SubscriberEmail<'_> {
        SubscriberEmail {
            to: &self.to,
            notice: &self.notice,
            facts: &self.facts,
            locale: &self.locale,
            unsubscribe: &self.unsubscribe,
        }
    }
}

/// Sends the first waiting email of each subscriber whose time has come, then
/// the ones behind them, until nothing is due, the round's share is sent, or
/// the mail server fails as a whole. `public_page` says whether visitors may
/// read the page at all.
pub async fn deliver_to_subscribers<S: SubscriberSender + Send + Sync + 'static>(
    pool: &DbPool,
    sender: &Arc<S>,
    public_url: Option<&str>,
    public_page: bool,
    now: DateTime<Utc>,
) -> Result<Round, AppError> {
    let mut round = Round::default();
    if !SubscriberDeliveryRepository::any_waiting(pool).await? {
        return Ok(round);
    }
    match Readers::ready(pool, sender.as_ref(), public_url, public_page).await? {
        Ok(mut readers) => {
            give_up(
                pool,
                Failure::Expired,
                Some(now - KEEP_WAITING),
                now,
                &mut round,
            )
            .await?;
            if readers.visitor_scope.is_none() {
                SubscriberDeliveryRepository::skip_visitors(pool, CLOSED, now).await?;
            }
            send_due(pool, sender, &mut readers, now, &mut round).await?;
        }
        // Nothing can be sent at all: every waiting email is given up.
        Err(failure) => give_up(pool, failure, None, now, &mut round).await?,
    }
    record_given_up(pool, &round).await;
    Ok(round)
}

/// Sends batch after batch until nothing is due, the round's share is sent,
/// or the mail server fails as a whole.
async fn send_due<S: SubscriberSender + Send + Sync + 'static>(
    pool: &DbPool,
    sender: &Arc<S>,
    readers: &mut Readers,
    now: DateTime<Utc>,
    round: &mut Round,
) -> Result<(), AppError> {
    let mut previous = Vec::new();
    while round.sent < SENDS_PER_ROUND && round.server_failure.is_none() {
        let heads = SubscriberDeliveryRepository::due(pool, now, BATCH).await?;
        let ids: Vec<i64> = heads.iter().map(|head| head.id).collect();
        // The same heads again mean none could be dealt with: stop here.
        if ids.is_empty() || ids == previous {
            break;
        }
        previous = ids;
        let emails = readers.prepare(pool, heads, now).await?;
        send_batch(pool, sender, emails, now, round).await?;
    }
    Ok(())
}

/// Gives up the waiting emails queued before `queued_before`, or all of them,
/// and counts them for the journal.
async fn give_up(
    pool: &DbPool,
    failure: Failure,
    queued_before: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    round: &mut Round,
) -> Result<(), AppError> {
    let code = failure.code();
    let given_up =
        SubscriberDeliveryRepository::give_up_waiting(pool, &code, queued_before, now).await?;
    round.give_up(&code, given_up);
    Ok(())
}

/// What the emails of a round are written from: the page, and each reader
/// with what they may see, looked up once.
struct Readers {
    origin: Origin,
    page: String,
    /// What every visitor sees, or `None` while visitors may not receive.
    visitor_scope: Option<Scope>,
    members: HashMap<i64, Option<Member>>,
    notices: HashMap<NoticeKey, Option<(Notice, Facts)>>,
}

/// An active member, as their emails need them.
struct Member {
    email: String,
    locale: Option<String>,
    scope: Scope,
}

/// The emails of one piece of news read alike by readers who see the same
/// services in the same language, members and visitors together.
#[derive(PartialEq, Eq, Hash)]
struct NoticeKey {
    event_id: i64,
    update_id: Option<i64>,
    happening: Happening,
    lifecycle: Option<Lifecycle>,
    at: DateTime<Utc>,
    scope: Scope,
    locale: String,
}

impl Readers {
    /// The page and who may read it, or why nothing can be sent: no mail
    /// server, or no page address to link back to.
    async fn ready<S: SubscriberSender>(
        pool: &DbPool,
        sender: &S,
        public_url: Option<&str>,
        public_page: bool,
    ) -> Result<Result<Self, Failure>, AppError> {
        if !sender.mail_ready() {
            return Ok(Err(Failure::NoMailServer));
        }
        let Some(page) = page_address(pool, public_url).await? else {
            return Ok(Err(Failure::NoPageAddress));
        };
        let visitor_scope = if SubscriptionService::visitors_welcome(pool, public_page).await? {
            Some(scope_of(pool, None).await?)
        } else {
            None
        };
        Ok(Ok(Self {
            origin: Origin {
                instance: crate::brand_name(),
                page: Some(page.clone()),
            },
            page,
            visitor_scope,
            members: HashMap::new(),
            notices: HashMap::new(),
        }))
    }

    /// Writes each email, and leaves out the ones their reader may not
    /// receive now.
    async fn prepare(
        &mut self,
        pool: &DbPool,
        heads: Vec<SubscriberMessage>,
        now: DateTime<Utc>,
    ) -> Result<Vec<Outgoing>, AppError> {
        let mut emails = Vec::with_capacity(heads.len());
        for message in heads {
            match self.write(pool, &message).await? {
                Ok(email) => emails.push(email),
                Err(reason) => {
                    SubscriberDeliveryRepository::skip(pool, message.id, reason, now).await?;
                }
            }
        }
        Ok(emails)
    }

    /// The email, or why it is left out.
    async fn write(
        &mut self,
        pool: &DbPool,
        message: &SubscriberMessage,
    ) -> Result<Result<Outgoing, &'static str>, AppError> {
        let (scope, to, locale) = match (message.user_id, &message.email) {
            (Some(user_id), _) => {
                let Some(member) = self.member(pool, user_id).await? else {
                    return Ok(Err(INACTIVE));
                };
                let locale = member
                    .locale
                    .clone()
                    .unwrap_or_else(|| message.locale.clone());
                (member.scope.clone(), member.email.clone(), locale)
            }
            (None, Some(email)) => match &self.visitor_scope {
                Some(scope) => (scope.clone(), email.clone(), message.locale.clone()),
                None => return Ok(Err(CLOSED)),
            },
            (None, None) => return Ok(Err(CLOSED)),
        };
        let Some((notice, facts)) = self.notice(pool, message, scope, &locale).await? else {
            return Ok(Err(HIDDEN));
        };
        let i18n = I18n::new(&locale);
        let unsubscribe_token = SubscriptionService::unsubscribe_token(
            message.subscriber_id,
            &message.unsubscribe_secret,
        );
        let unsubscribe =
            Unsubscribe::new(&self.page, &unsubscribe_token, &self.origin.instance, &i18n);
        Ok(Ok(Outgoing {
            id: message.id,
            attempts: message.attempts,
            to,
            locale,
            notice,
            facts,
            unsubscribe,
        }))
    }

    /// The member, once per round, or `None` when their account is gone or
    /// disabled.
    async fn member(&mut self, pool: &DbPool, user_id: i64) -> Result<Option<&Member>, AppError> {
        if let Entry::Vacant(slot) = self.members.entry(user_id) {
            let user = UserRepository::find_by_id(pool, user_id).await?;
            let member = match user.filter(|user| user.is_active) {
                Some(user) => Some(Member {
                    scope: scope_of(pool, Some(&user)).await?,
                    email: user.email,
                    locale: user.preferred_locale,
                }),
                None => None,
            };
            slot.insert(member);
        }
        Ok(self.members.get(&user_id).and_then(Option::as_ref))
    }

    /// The news as a reader who sees `scope` reads it, written once for all
    /// who see it alike, or `None` when it is not theirs to see.
    async fn notice(
        &mut self,
        pool: &DbPool,
        message: &SubscriberMessage,
        scope: Scope,
        locale: &str,
    ) -> Result<Option<(Notice, Facts)>, AppError> {
        let key = NoticeKey {
            event_id: message.event_id,
            update_id: message.update_id,
            happening: message.happening,
            lifecycle: message.lifecycle,
            at: message.created_at,
            scope,
            locale: locale.to_string(),
        };
        if let Some(written) = self.notices.get(&key) {
            return Ok(written.clone());
        }
        let reading = Reading {
            happening: message.happening,
            update_id: message.update_id,
            scope: &key.scope,
        };
        let written = load_event(pool, message.event_id, &reading)
            .await?
            .map(|subject| {
                let (happening, lifecycle, at) =
                    (message.happening, message.lifecycle, message.created_at);
                let i18n = I18n::new(locale);
                let notice = Notice::write(happening, lifecycle, &subject, &self.origin, &i18n, at);
                let facts = Facts::of(happening, lifecycle, &subject, &self.origin, at);
                (notice, facts)
            });
        self.notices.insert(key, written.clone());
        Ok(written)
    }
}

/// Sends the emails a few at a time. Once the mail server fails as a whole,
/// the ones not yet on their way stay waiting for it, their attempts
/// untouched.
async fn send_batch<S: SubscriberSender + Send + Sync + 'static>(
    pool: &DbPool,
    sender: &Arc<S>,
    emails: Vec<Outgoing>,
    now: DateTime<Utc>,
    round: &mut Round,
) -> Result<(), AppError> {
    let permits = Arc::new(Semaphore::new(SENDS_AT_ONCE));
    let server_down = Arc::new(AtomicBool::new(false));
    let mut sends = JoinSet::new();
    for email in emails {
        let sender = Arc::clone(sender);
        let permits = Arc::clone(&permits);
        let server_down = Arc::clone(&server_down);
        sends.spawn(async move {
            let Ok(_permit) = permits.acquire_owned().await else {
                return (email, None);
            };
            if server_down.load(Ordering::Relaxed) {
                return (email, None);
            }
            let result = sender.send_to_subscriber(&email.mail()).await;
            if result.is_err_and(Failure::is_server_wide) {
                server_down.store(true, Ordering::Relaxed);
            }
            (email, Some(result))
        });
    }
    while let Some(done) = sends.join_next().await {
        match done {
            Ok((email, Some(result))) => settle(pool, &email, result, now, round).await?,
            Ok((_, None)) => {}
            Err(e) => {
                tracing::warn!(error = %e, "An email to a subscriber stopped before its result");
            }
        }
    }
    Ok(())
}

/// Records how sending went. A refused address fails its email alone and
/// never ends the subscription: a refusal can also come from a server set
/// up wrong.
async fn settle(
    pool: &DbPool,
    email: &Outgoing,
    result: Result<(), Failure>,
    now: DateTime<Utc>,
    round: &mut Round,
) -> Result<(), AppError> {
    let Err(failure) = result else {
        SubscriberDeliveryRepository::mark_sent(pool, email.id, now).await?;
        round.sent += 1;
        return Ok(());
    };
    let code = failure.code();
    if failure.is_server_wide() {
        SubscriberDeliveryRepository::note(pool, email.id, &code).await?;
        round.server_failure = Some(failure);
        return Ok(());
    }
    if let Some(at) = retry_at(email.attempts, failure, now) {
        SubscriberDeliveryRepository::retry_later(pool, email.id, &code, at).await?;
    } else {
        SubscriberDeliveryRepository::fail(pool, email.id, &code, now).await?;
        *round.given_up.entry(code).or_default() += 1;
    }
    Ok(())
}

/// One line per reason, with how many emails it cost: never an address.
async fn record_given_up(pool: &DbPool, round: &Round) {
    for (code, count) in &round.given_up {
        let entry = NewJournalEntry::new(JournalKind::SubscriberMailsAbandoned)
            .about(count.to_string())
            .with_detail(code.clone());
        journal::record(pool, entry).await;
    }
}

/// How long to wait before the next round: the usual interval, or longer and
/// longer while the mail server fails as a whole.
#[derive(Default)]
struct Pause {
    streak: usize,
}

impl Pause {
    fn wait(&self) -> Duration {
        match self.streak.checked_sub(1) {
            None => ROUND_INTERVAL,
            Some(step) => RETRY_DELAYS[step.min(RETRY_DELAYS.len() - 1)],
        }
    }

    /// Notes how the round went, and journals the start of a pause once.
    async fn after(&mut self, pool: &DbPool, server_failure: Option<Failure>) {
        let Some(failure) = server_failure else {
            self.streak = 0;
            return;
        };
        if self.streak == 0 {
            let entry = NewJournalEntry::new(JournalKind::SubscriberMailsPaused)
                .with_detail(failure.code());
            journal::record(pool, entry).await;
        }
        self.streak += 1;
    }
}

/// Sends what is due every few seconds for the life of the server, waits
/// longer while the mail server is down, and forgets the old finished emails
/// and requests once an hour.
pub fn spawn_subscriber_mail(
    pool: DbPool,
    notifier: Arc<Notifier>,
    public_url: Option<String>,
    public_page: Arc<AtomicBool>,
) -> AbortHandle {
    let task = tokio::spawn(async move {
        let mut purges = interval(PURGE_INTERVAL);
        purges.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut pause = Pause::default();
        loop {
            tokio::select! {
                () = sleep(pause.wait()) => {
                    let open = public_page.load(Ordering::Relaxed);
                    let round =
                        deliver_to_subscribers(&pool, &notifier, public_url.as_deref(), open, Utc::now());
                    match round.await {
                        Ok(round) => pause.after(&pool, round.server_failure).await,
                        Err(e) => tracing::warn!(error = %e, "Subscriber emails round failed"),
                    }
                }
                _ = purges.tick() => purge(&pool).await,
            }
        }
    });
    task.abort_handle()
}

async fn purge(pool: &DbPool) {
    let before = Utc::now() - KEEP_HISTORY;
    if let Err(e) = SubscriberDeliveryRepository::purge(pool, before).await {
        tracing::warn!(error = %e, "Old subscriber emails not purged");
    }
    if let Err(e) = SubscriptionRequestRepository::delete_stale(pool).await {
        tracing::warn!(error = %e, "Old subscription requests not purged");
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;

    use chrono::Duration as Delay;

    use super::*;
    use crate::models::{
        Choices, CreateEventInput, Grants, Kind, Role, Severity, User, Visibility,
    };
    use crate::repositories::{ServiceRepository, SubscriberRepository};
    use crate::services::EventService;
    use crate::test_helpers::test_pool;

    const PAGE: &str = "https://status.example";

    /// What the fake was asked to send.
    #[derive(Clone)]
    struct Sent {
        to: String,
        title: String,
        services: Vec<String>,
        unsubscribe: String,
        locale: String,
    }

    /// Records what it is asked to send, answers from a script per address
    /// (success once spent), and counts the sends under way at once.
    struct Script {
        ready: bool,
        sent: Mutex<Vec<Sent>>,
        answers: Mutex<HashMap<String, VecDeque<Result<(), Failure>>>>,
        under_way: AtomicUsize,
        most_at_once: AtomicUsize,
    }

    impl Script {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                ready: true,
                sent: Mutex::new(Vec::new()),
                answers: Mutex::new(HashMap::new()),
                under_way: AtomicUsize::new(0),
                most_at_once: AtomicUsize::new(0),
            })
        }

        fn answer(&self, to: &str, results: &[Result<(), Failure>]) {
            self.answers
                .lock()
                .unwrap()
                .insert(to.to_string(), results.iter().copied().collect());
        }

        fn sent(&self) -> Vec<Sent> {
            self.sent.lock().unwrap().clone()
        }

        fn sent_to(&self) -> Vec<String> {
            self.sent
                .lock()
                .unwrap()
                .iter()
                .map(|s| s.to.clone())
                .collect()
        }
    }

    impl SubscriberSender for Script {
        fn mail_ready(&self) -> bool {
            self.ready
        }

        async fn send_to_subscriber(&self, mail: &SubscriberEmail<'_>) -> Result<(), Failure> {
            let now = self.under_way.fetch_add(1, Ordering::SeqCst) + 1;
            self.most_at_once.fetch_max(now, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(5)).await;
            self.under_way.fetch_sub(1, Ordering::SeqCst);
            let event = mail.facts.event.as_ref();
            self.sent.lock().unwrap().push(Sent {
                to: mail.to.to_string(),
                title: event.map(|e| e.title.clone()).unwrap_or_default(),
                services: event.map(|e| e.services.clone()).unwrap_or_default(),
                unsubscribe: mail.unsubscribe.url.clone(),
                locale: mail.locale.to_string(),
            });
            let mut answers = self.answers.lock().unwrap();
            answers
                .get_mut(mail.to)
                .and_then(VecDeque::pop_front)
                .unwrap_or(Ok(()))
        }
    }

    async fn setup() -> (DbPool, User) {
        let pool = test_pool().await;
        SubscriptionService::set_visitors_open(&pool, true)
            .await
            .unwrap();
        let author = UserRepository::create(&pool, "ops@example.com", "hash", "Ops", Role::Admin)
            .await
            .unwrap();
        (pool, author)
    }

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

    async fn member(pool: &DbPool, email: &str, role: Role) -> i64 {
        let user = UserRepository::create(pool, email, "hash", "Member", role)
            .await
            .unwrap();
        let everything = Choices {
            incidents: true,
            maintenances: true,
            publications: true,
        };
        SubscriptionService::save_member(pool, user.id, "en", everything)
            .await
            .unwrap();
        user.id
    }

    async fn service(pool: &DbPool, name: &str, visibility: Visibility) -> i64 {
        let slug = name.to_lowercase();
        let id = ServiceRepository::create(pool, name, &slug, None, None, None)
            .await
            .unwrap()
            .id;
        ServiceRepository::set_visibility(pool, id, visibility, Grants::default())
            .await
            .unwrap();
        id
    }

    async fn incident(pool: &DbPool, author: &User, title: &str, service_ids: Vec<i64>) {
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
            service_ids,
            follows_event_id: None,
            author_id: author.id,
            api_key_id: None,
            next_update: None,
        };
        EventService::create(pool, input).await.unwrap();
    }

    async fn round(pool: &DbPool, script: &Arc<Script>, public_page: bool) -> Round {
        deliver_to_subscribers(pool, script, Some(PAGE), public_page, Utc::now())
            .await
            .unwrap()
    }

    /// Each email's address, status, attempts and reason, in order.
    async fn rows(pool: &DbPool) -> Vec<(String, String, i64, Option<String>)> {
        sqlx::query_as(
            "SELECT COALESCE(s.email, u.email), d.status, d.attempts, d.failure \
             FROM subscriber_deliveries d JOIN subscribers s ON s.id = d.subscriber_id \
             LEFT JOIN users u ON u.id = s.user_id ORDER BY d.id",
        )
        .fetch_all(pool)
        .await
        .unwrap()
    }

    fn row(
        email: &str,
        status: &str,
        attempts: i64,
        failure: Option<&str>,
    ) -> (String, String, i64, Option<String>) {
        (
            email.to_string(),
            status.to_string(),
            attempts,
            failure.map(str::to_string),
        )
    }

    #[tokio::test]
    async fn a_visitor_gets_the_news_with_its_way_out() {
        let (pool, author) = setup().await;
        let id = visitor(&pool, "ana@example.com").await;
        let mail = service(&pool, "Mail", Visibility::Everyone).await;
        incident(&pool, &author, "Mail down", vec![mail]).await;
        let script = Script::new();

        let done = round(&pool, &script, true).await;

        assert_eq!(done.sent, 1);
        let sent = script.sent();
        assert_eq!(sent[0].to, "ana@example.com");
        assert_eq!(sent[0].title, "Mail down");
        assert_eq!(sent[0].services, ["Mail"]);
        assert_eq!(sent[0].locale, "en");
        assert_eq!(
            sent[0].unsubscribe,
            format!("{PAGE}/unsubscribe?token={id}.secret")
        );
        assert_eq!(rows(&pool).await, [row("ana@example.com", "sent", 1, None)]);
    }

    #[tokio::test]
    async fn each_reader_gets_only_what_they_may_see() {
        let (pool, author) = setup().await;
        visitor(&pool, "ana@example.com").await;
        member(&pool, "lead@example.com", Role::Publisher).await;
        let mail = service(&pool, "Mail", Visibility::Everyone).await;
        let vault = service(&pool, "Vault", Visibility::Members).await;
        incident(&pool, &author, "Vault down", vec![vault]).await;
        incident(&pool, &author, "Mail and vault down", vec![mail, vault]).await;
        let script = Script::new();

        round(&pool, &script, true).await;

        let sent = script.sent();
        let seen: Vec<(&str, &str, Vec<String>)> = sent
            .iter()
            .map(|s| (s.to.as_str(), s.title.as_str(), s.services.clone()))
            .collect();
        assert!(seen.contains(&(
            "ana@example.com",
            "Mail and vault down",
            vec!["Mail".to_string()]
        )));
        assert!(seen.contains(&("lead@example.com", "Vault down", vec!["Vault".to_string()])));
        assert!(seen.contains(&(
            "lead@example.com",
            "Mail and vault down",
            vec!["Mail".to_string(), "Vault".to_string()]
        )));
        assert_eq!(sent.len(), 3, "the visitor never hears of the vault alone");
        assert!(
            rows(&pool)
                .await
                .contains(&row("ana@example.com", "skipped", 1, Some(HIDDEN)))
        );
    }

    #[tokio::test]
    async fn visitors_hear_nothing_once_the_page_or_their_subscriptions_close() {
        let (pool, author) = setup().await;
        visitor(&pool, "ana@example.com").await;
        member(&pool, "ben@example.com", Role::Reader).await;
        incident(&pool, &author, "First", Vec::new()).await;
        let script = Script::new();

        round(&pool, &script, false).await;
        assert_eq!(script.sent_to(), ["ben@example.com"]);

        incident(&pool, &author, "Second", Vec::new()).await;
        SubscriptionService::set_visitors_open(&pool, false)
            .await
            .unwrap();
        round(&pool, &script, true).await;

        assert_eq!(script.sent_to(), ["ben@example.com", "ben@example.com"]);
        let skipped: Vec<_> = rows(&pool)
            .await
            .into_iter()
            .filter(|(email, ..)| email == "ana@example.com")
            .collect();
        assert_eq!(
            skipped,
            [
                row("ana@example.com", "skipped", 1, Some(CLOSED)),
                row("ana@example.com", "skipped", 1, Some(CLOSED)),
            ]
        );
    }

    #[tokio::test]
    async fn a_disabled_member_is_left_out() {
        let (pool, author) = setup().await;
        let ben = member(&pool, "ben@example.com", Role::Reader).await;
        incident(&pool, &author, "Down", Vec::new()).await;
        UserRepository::set_active(&pool, ben, false).await.unwrap();
        let script = Script::new();

        round(&pool, &script, true).await;

        assert_eq!(script.sent_to(), Vec::<String>::new());
        assert_eq!(
            rows(&pool).await,
            [row("ben@example.com", "skipped", 1, Some(INACTIVE))]
        );
    }

    #[tokio::test]
    async fn a_refused_address_fails_its_email_and_keeps_its_subscription() {
        let (pool, author) = setup().await;
        let ana = visitor(&pool, "ana@example.com").await;
        visitor(&pool, "ben@example.com").await;
        incident(&pool, &author, "Down", Vec::new()).await;
        let script = Script::new();
        script.answer("ana@example.com", &[Err(Failure::MailRefused(Some(550)))]);

        let done = round(&pool, &script, true).await;

        assert_eq!(done.sent, 1);
        assert_eq!(
            done.given_up,
            BTreeMap::from([("mail_refused_550".to_string(), 1)])
        );
        assert!(rows(&pool).await.contains(&row(
            "ana@example.com",
            "failed",
            1,
            Some("mail_refused_550")
        )));
        assert!(
            SubscriberRepository::find(&pool, ana)
                .await
                .unwrap()
                .is_some()
        );
        let lines: Vec<(String, Option<String>, Option<String>)> =
            sqlx::query_as("SELECT kind, subject, detail FROM journal_entries")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(lines.contains(&(
            "subscriber_mails_abandoned".to_string(),
            Some("1".to_string()),
            Some("mail_refused_550".to_string())
        )));
    }

    #[tokio::test]
    async fn a_busy_server_gets_the_email_again_later() {
        let (pool, author) = setup().await;
        visitor(&pool, "ana@example.com").await;
        incident(&pool, &author, "Down", Vec::new()).await;
        let script = Script::new();
        script.answer("ana@example.com", &[Err(Failure::MailBusy(Some(450)))]);
        let now = Utc::now();

        let first = deliver_to_subscribers(&pool, &script, Some(PAGE), true, now)
            .await
            .unwrap();
        let later =
            deliver_to_subscribers(&pool, &script, Some(PAGE), true, now + Delay::seconds(31))
                .await
                .unwrap();

        assert_eq!((first.sent, later.sent), (0, 1));
        assert_eq!(rows(&pool).await, [row("ana@example.com", "sent", 2, None)]);
    }

    #[tokio::test]
    async fn a_server_down_stops_the_round_and_spends_no_attempt() {
        let (pool, author) = setup().await;
        for name in ["a", "b", "c", "d", "e", "f"] {
            visitor(&pool, &format!("{name}@example.com")).await;
        }
        incident(&pool, &author, "Down", Vec::new()).await;
        let script = Script::new();
        let refused = Err(Failure::MailRefused(Some(535)));
        for name in ["a", "b", "c", "d", "e", "f"] {
            script.answer(&format!("{name}@example.com"), &[refused]);
        }

        let done = round(&pool, &script, true).await;

        assert_eq!(done.server_failure, Some(Failure::MailRefused(Some(535))));
        assert!(done.given_up.is_empty());
        assert!(
            script.sent_to().len() < 6,
            "the emails not yet on their way wait"
        );
        for (_, status, attempts, _) in rows(&pool).await {
            assert_eq!((status.as_str(), attempts), ("pending", 0));
        }
    }

    #[tokio::test]
    async fn without_a_mail_server_or_a_page_address_waiting_emails_are_given_up() {
        let (pool, author) = setup().await;
        visitor(&pool, "ana@example.com").await;
        incident(&pool, &author, "First", Vec::new()).await;
        let script = Script::new();

        deliver_to_subscribers(&pool, &script, None, true, Utc::now())
            .await
            .unwrap();
        incident(&pool, &author, "Second", Vec::new()).await;
        let no_server = Arc::new(Script {
            ready: false,
            ..Arc::into_inner(Script::new()).unwrap()
        });
        let done = round(&pool, &no_server, true).await;

        assert_eq!(script.sent_to(), Vec::<String>::new());
        assert_eq!(
            done.given_up,
            BTreeMap::from([("no_mail_server".to_string(), 1)])
        );
        assert_eq!(
            rows(&pool).await,
            [
                row("ana@example.com", "failed", 0, Some("no_page_address")),
                row("ana@example.com", "failed", 0, Some("no_mail_server")),
            ]
        );
    }

    #[tokio::test]
    async fn news_that_waited_two_days_is_given_up() {
        let (pool, author) = setup().await;
        visitor(&pool, "ana@example.com").await;
        incident(&pool, &author, "Old", Vec::new()).await;
        sqlx::query("UPDATE subscriber_deliveries SET created_at = datetime('now', '-49 hours')")
            .execute(&pool)
            .await
            .unwrap();
        let script = Script::new();

        let done = round(&pool, &script, true).await;

        assert_eq!(script.sent_to(), Vec::<String>::new());
        assert_eq!(
            rows(&pool).await,
            [row("ana@example.com", "failed", 0, Some("expired"))]
        );
        assert_eq!(done.given_up, BTreeMap::from([("expired".to_string(), 1)]));
        assert_eq!(Failure::from_code("expired"), Some(Failure::Expired));
    }

    #[tokio::test]
    async fn emails_leave_in_order_a_few_at_a_time() {
        let (pool, author) = setup().await;
        for n in 0..10 {
            visitor(&pool, &format!("v{n}@example.com")).await;
        }
        incident(&pool, &author, "First", Vec::new()).await;
        incident(&pool, &author, "Second", Vec::new()).await;
        let script = Script::new();

        let done = round(&pool, &script, true).await;

        assert_eq!(done.sent, 20);
        assert!(script.most_at_once.load(Ordering::SeqCst) <= SENDS_AT_ONCE);
        let titles: Vec<String> = script
            .sent()
            .iter()
            .filter(|s| s.to == "v3@example.com")
            .map(|s| s.title.clone())
            .collect();
        assert_eq!(titles, ["First", "Second"]);
    }

    #[tokio::test]
    async fn the_pause_grows_while_the_server_is_down_and_is_journaled_once() {
        let pool = test_pool().await;
        let mut pause = Pause::default();
        assert_eq!(pause.wait(), ROUND_INTERVAL);

        let down = Some(Failure::MailUnreachable);
        pause.after(&pool, down).await;
        assert_eq!(pause.wait(), RETRY_DELAYS[0]);
        pause.after(&pool, down).await;
        assert_eq!(pause.wait(), RETRY_DELAYS[1]);
        for _ in 0..10 {
            pause.after(&pool, down).await;
        }
        assert_eq!(pause.wait(), RETRY_DELAYS[RETRY_DELAYS.len() - 1]);
        pause.after(&pool, None).await;
        assert_eq!(pause.wait(), ROUND_INTERVAL);

        let paused: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM journal_entries WHERE kind = 'subscriber_mails_paused'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(paused, 1);
    }
}
