//! The automatic checks at work: every minute, probe the checked services,
//! apply the rules of [`super::monitoring`] and record what changes.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::sync::Semaphore;
use tokio::task::{AbortHandle, JoinSet};

use super::ServiceService;
use super::monitoring::{
    Observation, Outcome, Transition, looks_like_own_failure, next_state, skip_round,
};
use super::probe::{CHECK_TIMEOUT, Prober, Probes};
use crate::db::DbPool;
use crate::error::AppError;
use crate::models::CheckedService;
use crate::repositories::{EventRepository, ServiceRepository};

const ROUND_INTERVAL: Duration = Duration::from_secs(60);
/// Checks running at once, so a long list never floods the network.
const MAX_CONCURRENT_CHECKS: usize = 16;

/// What the checks remember between rounds. It lives in memory: writing a
/// count every minute would churn the database, and a restart only delays
/// a new outage by a few rounds.
#[derive(Debug, Default)]
pub struct MonitorState {
    streaks: HashMap<i64, Streak>,
    skipped_rounds: u8,
}

/// Consecutive failures of one service, and the first of them outside a
/// maintenance: when the outage they make began.
#[derive(Debug, Clone, Copy)]
struct Streak {
    failures: u8,
    since: Option<DateTime<Utc>>,
}

impl MonitorState {
    fn failures(&self, service_id: i64) -> u8 {
        self.streaks.get(&service_id).map_or(0, |s| s.failures)
    }

    /// Keeps the new count and returns when the streak began. Failures
    /// during a maintenance do not date the outage found once it ends.
    fn record(
        &mut self,
        service_id: i64,
        failures: u8,
        silenced: bool,
        now: DateTime<Utc>,
    ) -> DateTime<Utc> {
        if failures == 0 {
            self.streaks.remove(&service_id);
            return now;
        }
        let streak = self.streaks.entry(service_id).or_insert(Streak {
            failures,
            since: None,
        });
        streak.failures = failures;
        if silenced {
            streak.since = None;
            return now;
        }
        *streak.since.get_or_insert(now)
    }

    /// A service whose check was removed starts afresh if it gets one back.
    fn keep_only(&mut self, services: &[CheckedService]) {
        let checked: HashSet<i64> = services.iter().map(|s| s.id).collect();
        self.streaks.retain(|id, _| checked.contains(id));
    }
}

/// Checks every service once and records the outages that start or end.
pub async fn run_round<P: Prober>(
    pool: &DbPool,
    prober: &Arc<P>,
    state: &mut MonitorState,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    let services = ServiceRepository::list_checked(pool).await?;
    state.keep_only(&services);
    if services.is_empty() {
        return Ok(());
    }
    let silenced: HashSet<i64> = EventRepository::services_under_maintenance(pool)
        .await?
        .into_iter()
        .collect();
    let outcomes = probe_all(prober, &services).await;
    let observed: Vec<(i64, Observation)> = services
        .iter()
        .filter_map(|service| {
            let outcome = *outcomes.get(&service.id)?;
            let observation = Observation {
                failures: state.failures(service.id),
                detected: service.detected_status.is_some(),
                outcome,
            };
            Some((service.id, observation))
        })
        .collect();
    if skip_this_round(state, &observed) {
        return Ok(());
    }
    for (service_id, observation) in observed {
        let silenced = silenced.contains(&service_id);
        apply(pool, state, service_id, observation, silenced, now).await?;
    }
    Ok(())
}

/// Every check of the round, at most [`MAX_CONCURRENT_CHECKS`] at once. A
/// check that panics is left out, and its service keeps its state.
async fn probe_all<P: Prober>(
    prober: &Arc<P>,
    services: &[CheckedService],
) -> HashMap<i64, Outcome> {
    let permits = Arc::new(Semaphore::new(MAX_CONCURRENT_CHECKS));
    let mut checks = JoinSet::new();
    for service in services.iter().cloned() {
        let prober = Arc::clone(prober);
        let permits = Arc::clone(&permits);
        checks.spawn(async move {
            let _permit = permits.acquire_owned().await;
            (service.id, prober.check(&service).await)
        });
    }
    let mut outcomes = HashMap::new();
    while let Some(done) = checks.join_next().await {
        match done {
            Ok((service_id, outcome)) => {
                outcomes.insert(service_id, outcome);
            }
            Err(e) => tracing::warn!(error = %e, "A check stopped before its result"),
        }
    }
    outcomes
}

/// A round where every service that answered fails at once points at
/// Statup's own connection: it is ignored, a few times in a row at most.
fn skip_this_round(state: &mut MonitorState, observed: &[(i64, Observation)]) -> bool {
    let observations: Vec<Observation> = observed.iter().map(|(_, o)| *o).collect();
    let (skip, skipped_rounds) =
        skip_round(looks_like_own_failure(&observations), state.skipped_rounds);
    state.skipped_rounds = skipped_rounds;
    if skip {
        tracing::warn!(
            services = observations.len(),
            "Every checked service failed at once: round ignored, Statup's own connection is suspected"
        );
    }
    skip
}

async fn apply(
    pool: &DbPool,
    state: &mut MonitorState,
    service_id: i64,
    observation: Observation,
    silenced: bool,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    let (failures, transition) = next_state(observation, silenced);
    let since = state.record(service_id, failures, silenced, now);
    match transition {
        Transition::Stay => return Ok(()),
        Transition::Down => {
            ServiceRepository::mark_detected_down(pool, service_id, since).await?;
            tracing::info!(service_id, "Service detected down");
        }
        Transition::Up => {
            ServiceRepository::mark_detected_up(pool, service_id, now).await?;
            tracing::info!(service_id, "Service detected back up");
        }
    }
    ServiceService::recalculate_status(pool, service_id).await
}

/// Runs [`run_round`] every minute for the life of the server.
pub fn spawn_monitoring(pool: DbPool) -> AbortHandle {
    let task = tokio::spawn(async move {
        let probes = match Probes::new(CHECK_TIMEOUT) {
            Ok(probes) => Arc::new(probes),
            Err(e) => {
                tracing::warn!(error = %e, "Monitoring disabled: no HTTP client");
                return;
            }
        };
        let mut state = MonitorState::default();
        let mut ticker = tokio::time::interval(ROUND_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            if let Err(e) = run_round(&pool, &probes, &mut state, Utc::now()).await {
                tracing::warn!(error = %e, "Monitoring round failed");
            }
        }
    });
    task.abort_handle()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;
    use crate::models::{CheckKind, CreateEventInput, Kind, Role, ServiceCheck, ServiceStatus};
    use crate::repositories::{OutageRepository, UserRepository};
    use crate::test_helpers::test_pool;

    /// Gives every service the same outcome.
    struct Always(Outcome);

    impl Prober for Always {
        fn check(&self, _service: &CheckedService) -> impl Future<Output = Outcome> + Send {
            std::future::ready(self.0)
        }
    }

    fn minute(n: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 4, 12, 0, 0).unwrap() + chrono::Duration::minutes(n)
    }

    async fn checked_service(pool: &DbPool, name: &str) -> i64 {
        let service = ServiceService::create(pool, name, None, None, None)
            .await
            .unwrap();
        set_web_check(pool, service.id).await;
        service.id
    }

    async fn set_web_check(pool: &DbPool, service_id: i64) {
        let check = ServiceCheck {
            kind: CheckKind::Http,
            target: format!("https://service-{service_id}.example"),
            internal_cert: false,
        };
        ServiceRepository::set_check(pool, service_id, Some(&check), minute(0))
            .await
            .unwrap();
    }

    async fn rounds(pool: &DbPool, state: &mut MonitorState, outcome: Outcome, minutes: &[i64]) {
        let prober = Arc::new(Always(outcome));
        for &n in minutes {
            run_round(pool, &prober, state, minute(n)).await.unwrap();
        }
    }

    /// The status shown and the one the checks found.
    async fn states(pool: &DbPool, service_id: i64) -> (ServiceStatus, Option<ServiceStatus>) {
        let service = ServiceRepository::find_by_id(pool, service_id)
            .await
            .unwrap()
            .unwrap();
        (service.status, service.detected_status)
    }

    async fn maintenance(pool: &DbPool, service_id: i64, keeps_services_up: bool) -> i64 {
        let author = UserRepository::create(pool, "ops@example.com", "hash", "Ops", Role::Admin)
            .await
            .unwrap();
        let input = CreateEventInput {
            kind: Kind::Maintenance,
            severity: None,
            planned: false,
            category: None,
            title: "Server move".to_string(),
            description: String::new(),
            planned_start: None,
            planned_end: None,
            started_at: None,
            opening_step: None,
            keeps_services_up,
            service_ids: vec![service_id],
            follows_event_id: None,
            author_id: author.id,
        };
        EventRepository::create(pool, &input).await.unwrap().id
    }

    #[tokio::test]
    async fn three_failed_rounds_show_an_outage_from_the_first_failure() {
        let pool = test_pool().await;
        let id = checked_service(&pool, "Mail").await;
        let mut state = MonitorState::default();

        rounds(&pool, &mut state, Outcome::Failed, &[1, 2]).await;
        assert_eq!(states(&pool, id).await, (ServiceStatus::Operational, None));

        rounds(&pool, &mut state, Outcome::Failed, &[3]).await;
        let down = (ServiceStatus::MajorOutage, Some(ServiceStatus::MajorOutage));
        assert_eq!(states(&pool, id).await, down);
        let spans = OutageRepository::since(&pool, minute(0)).await.unwrap();
        assert_eq!(spans[&id][0].start, minute(1));
        assert_eq!(spans[&id][0].end, None);
    }

    #[tokio::test]
    async fn an_answer_ends_the_outage_and_restores_the_state_set_by_hand() {
        let pool = test_pool().await;
        let id = checked_service(&pool, "Wiki").await;
        ServiceService::set_manual_status(&pool, id, ServiceStatus::Degraded)
            .await
            .unwrap();
        let mut state = MonitorState::default();

        rounds(&pool, &mut state, Outcome::Failed, &[1, 2, 3]).await;
        assert_eq!(states(&pool, id).await.0, ServiceStatus::MajorOutage);

        rounds(&pool, &mut state, Outcome::Answered, &[4]).await;
        assert_eq!(states(&pool, id).await, (ServiceStatus::Degraded, None));
        let spans = OutageRepository::since(&pool, minute(0)).await.unwrap();
        assert_eq!(spans[&id][0].end, Some(minute(4)));
    }

    #[tokio::test]
    async fn a_maintenance_keeps_the_checks_quiet_until_it_ends() {
        let pool = test_pool().await;
        let id = checked_service(&pool, "Intranet").await;
        let event_id = maintenance(&pool, id, false).await;
        let mut state = MonitorState::default();

        rounds(&pool, &mut state, Outcome::Failed, &[1, 2, 3]).await;
        assert_eq!(states(&pool, id).await.1, None);

        sqlx::query("UPDATE events SET lifecycle = 'completed' WHERE id = ?")
            .bind(event_id)
            .execute(&pool)
            .await
            .unwrap();
        rounds(&pool, &mut state, Outcome::Failed, &[4]).await;
        assert_eq!(states(&pool, id).await.1, Some(ServiceStatus::MajorOutage));
        let spans = OutageRepository::since(&pool, minute(0)).await.unwrap();
        assert_eq!(spans[&id][0].start, minute(4));
    }

    #[tokio::test]
    async fn a_maintenance_without_downtime_does_not_silence_the_checks() {
        let pool = test_pool().await;
        let id = checked_service(&pool, "Portal").await;
        maintenance(&pool, id, true).await;
        let mut state = MonitorState::default();

        rounds(&pool, &mut state, Outcome::Failed, &[1, 2, 3]).await;
        assert_eq!(states(&pool, id).await.1, Some(ServiceStatus::MajorOutage));
    }

    #[tokio::test]
    async fn everything_failing_at_once_waits_five_rounds() {
        let pool = test_pool().await;
        let mut ids = Vec::new();
        for name in ["Mail", "Wiki", "VPN"] {
            ids.push(checked_service(&pool, name).await);
        }
        let mut state = MonitorState::default();

        rounds(&pool, &mut state, Outcome::Failed, &[1, 2, 3, 4, 5, 6, 7]).await;
        for &id in &ids {
            assert_eq!(states(&pool, id).await.1, None);
        }
        rounds(&pool, &mut state, Outcome::Failed, &[8]).await;
        for &id in &ids {
            assert_eq!(states(&pool, id).await.1, Some(ServiceStatus::MajorOutage));
        }
    }

    #[tokio::test]
    async fn services_without_a_check_are_left_alone() {
        let pool = test_pool().await;
        let service = ServiceService::create(&pool, "Paie", None, None, None)
            .await
            .unwrap();
        let mut state = MonitorState::default();

        rounds(&pool, &mut state, Outcome::Failed, &[1, 2, 3]).await;
        assert_eq!(
            states(&pool, service.id).await,
            (ServiceStatus::Operational, None)
        );
    }

    #[tokio::test]
    async fn removing_the_check_forgets_the_failures() {
        let pool = test_pool().await;
        let id = checked_service(&pool, "Drive").await;
        let mut state = MonitorState::default();

        rounds(&pool, &mut state, Outcome::Failed, &[1, 2]).await;
        ServiceRepository::set_check(&pool, id, None, minute(3))
            .await
            .unwrap();
        rounds(&pool, &mut state, Outcome::Failed, &[3]).await;
        set_web_check(&pool, id).await;
        rounds(&pool, &mut state, Outcome::Failed, &[4]).await;
        assert_eq!(states(&pool, id).await.1, None);
    }
}
