//! Writes the caught lines in batches, and forgets the lines older than
//! the retention once an hour.

use std::sync::atomic::Ordering;
use std::time::Duration;

use chrono::Utc;
use tokio::task::AbortHandle;

use super::{JournalInbox, TARGET, captures_details, kept, retention_days};
use crate::db::DbPool;
use crate::repositories::{JournalRepository, NewJournalEntry};

const BATCH: usize = 64;
const PURGE_INTERVAL: Duration = Duration::from_hours(1);

pub fn spawn_journal_writer(pool: DbPool, inbox: JournalInbox) -> AbortHandle {
    tokio::spawn(run(pool, inbox)).abort_handle()
}

async fn run(pool: DbPool, mut inbox: JournalInbox) {
    let mut purge = tokio::time::interval(PURGE_INTERVAL);
    loop {
        tokio::select! {
            received = inbox.receiver.recv() => {
                let Some(first) = received else { return };
                let mut batch = vec![first];
                while batch.len() < BATCH {
                    match inbox.receiver.try_recv() {
                        Ok(next) => batch.push(next),
                        Err(_) => break,
                    }
                }
                let detailed = captures_details(&pool).await;
                batch.retain(|entry| kept(entry.kind, detailed));
                write(&pool, &batch).await;
                report_dropped(&inbox);
            }
            _ = purge.tick() => forget_old(&pool).await,
        }
    }
}

async fn write(pool: &DbPool, batch: &[NewJournalEntry]) {
    if let Err(error) = JournalRepository::add_all(pool, batch).await {
        tracing::warn!(target: TARGET, %error, lines = batch.len(), "Journal lines not saved");
    }
}

fn report_dropped(inbox: &JournalInbox) {
    let dropped = inbox.dropped.swap(0, Ordering::Relaxed);
    if dropped > 0 {
        tracing::warn!(target: TARGET, dropped, "Journal lines dropped: too many at once");
    }
}

async fn forget_old(pool: &DbPool) {
    let days = retention_days(pool).await;
    let before = Utc::now() - chrono::Duration::days(days);
    match JournalRepository::purge(pool, before).await {
        Ok(0) => {}
        Ok(forgotten) => {
            tracing::debug!(target: TARGET, forgotten, days, "Old journal lines forgotten");
        }
        Err(error) => tracing::warn!(target: TARGET, %error, "Old journal lines not forgotten"),
    }
}
