//! Journal lines: written one by one or in batches, and forgotten once
//! older than the retention.

use chrono::{DateTime, Utc};
use sqlx::{QueryBuilder, Sqlite};

use super::escape_like;
use crate::clock;
use crate::db::DbPool;
use crate::models::{JournalEntry, JournalFilters, JournalKind};

/// A line to write; its family follows from its kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewJournalEntry {
    pub kind: JournalKind,
    pub actor_id: Option<i64>,
    pub actor_name: Option<String>,
    pub subject: Option<String>,
    pub detail: Option<String>,
    pub ip: Option<String>,
}

impl NewJournalEntry {
    pub fn new(kind: JournalKind) -> Self {
        Self {
            kind,
            actor_id: None,
            actor_name: None,
            subject: None,
            detail: None,
            ip: None,
        }
    }
}

pub struct JournalRepository;

impl JournalRepository {
    pub async fn add(pool: &DbPool, entry: &NewJournalEntry) -> Result<(), sqlx::Error> {
        Self::add_all(pool, std::slice::from_ref(entry)).await
    }

    /// Writes the lines in one statement, in their order.
    pub async fn add_all(pool: &DbPool, entries: &[NewJournalEntry]) -> Result<(), sqlx::Error> {
        if entries.is_empty() {
            return Ok(());
        }
        let mut query = QueryBuilder::<Sqlite>::new(
            "INSERT INTO journal_entries \
             (kind, category, actor_id, actor_name, subject, detail, ip) ",
        );
        query.push_values(entries, |mut row, entry| {
            row.push_bind(entry.kind)
                .push_bind(entry.kind.category())
                .push_bind(entry.actor_id)
                .push_bind(entry.actor_name.as_deref())
                .push_bind(entry.subject.as_deref())
                .push_bind(entry.detail.as_deref())
                .push_bind(entry.ip.as_deref());
        });
        query.build().execute(pool).await?;
        Ok(())
    }

    /// One page of lines, newest first.
    pub async fn list_page(
        pool: &DbPool,
        filters: &JournalFilters,
    ) -> Result<Vec<JournalEntry>, sqlx::Error> {
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT kind, actor_name, subject, detail, ip, created_at \
             FROM journal_entries",
        );
        push_filters(&mut query, filters);
        query
            .push(" ORDER BY created_at DESC, id DESC LIMIT ")
            .push_bind(filters.limit)
            .push(" OFFSET ")
            .push_bind(filters.offset);
        query.build_query_as().fetch_all(pool).await
    }

    pub async fn count_filtered(
        pool: &DbPool,
        filters: &JournalFilters,
    ) -> Result<i64, sqlx::Error> {
        let mut query = QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM journal_entries");
        push_filters(&mut query, filters);
        query.build_query_scalar().fetch_one(pool).await
    }

    /// Forgets the lines written before `before`.
    pub async fn purge(pool: &DbPool, before: DateTime<Utc>) -> Result<u64, sqlx::Error> {
        let done = sqlx::query("DELETE FROM journal_entries WHERE created_at < ?")
            .bind(clock::db(before))
            .execute(pool)
            .await?;
        Ok(done.rows_affected())
    }
}

fn push_filters(query: &mut QueryBuilder<Sqlite>, filters: &JournalFilters) {
    query.push(" WHERE 1 = 1");
    if let Some(category) = filters.category {
        query.push(" AND category = ").push_bind(category);
    }
    if let Some(actor_id) = filters.actor_id {
        query.push(" AND actor_id = ").push_bind(actor_id);
    }
    if let Some(from) = filters.from {
        query.push(" AND created_at >= ").push_bind(clock::db(from));
    }
    if let Some(to) = filters.to {
        query.push(" AND created_at <= ").push_bind(clock::db(to));
    }
    if let Some(words) = filters.q.as_deref() {
        let pattern = format!("%{}%", escape_like(words));
        query
            .push(" AND (subject LIKE ")
            .push_bind(pattern.clone())
            .push(" ESCAPE '\\' OR detail LIKE ")
            .push_bind(pattern.clone())
            .push(" ESCAPE '\\' OR actor_name LIKE ")
            .push_bind(pattern)
            .push(" ESCAPE '\\')");
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;
    use crate::test_helpers::test_pool;

    async fn rows(pool: &DbPool) -> Vec<(JournalKind, Option<String>)> {
        sqlx::query_as("SELECT kind, subject FROM journal_entries ORDER BY id")
            .fetch_all(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_line_keeps_the_family_and_importance_of_its_kind() {
        let pool = test_pool().await;
        let mut error = NewJournalEntry::new(JournalKind::AppError);
        error.subject = Some("Database error".into());
        let warning = NewJournalEntry::new(JournalKind::AppWarning);

        JournalRepository::add_all(&pool, &[error, warning])
            .await
            .unwrap();

        assert_eq!(
            rows(&pool).await,
            vec![
                (JournalKind::AppError, Some("Database error".into())),
                (JournalKind::AppWarning, None),
            ]
        );
        let stored: Vec<String> =
            sqlx::query_scalar("SELECT category FROM journal_entries ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(stored, ["system", "system"]);
        assert!(JournalKind::AppError.is_essential());
        assert!(!JournalKind::AppWarning.is_essential());
    }

    #[tokio::test]
    async fn the_list_is_narrowed_by_family_and_words() {
        let pool = test_pool().await;
        let mut locked = NewJournalEntry::new(JournalKind::AppError);
        locked.subject = Some("Database is locked".into());
        let mut percent = NewJournalEntry::new(JournalKind::AppWarning);
        percent.subject = Some("Disk at 95% full".into());
        JournalRepository::add_all(&pool, &[locked, percent])
            .await
            .unwrap();
        let filters = |q: &str| JournalFilters {
            q: Some(q.into()),
            limit: 10,
            ..JournalFilters::default()
        };

        let found = JournalRepository::list_page(&pool, &filters("LOCKED"))
            .await
            .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, JournalKind::AppError);
        assert_eq!(
            JournalRepository::count_filtered(&pool, &filters("5%"))
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            JournalRepository::count_filtered(&pool, &filters("_"))
                .await
                .unwrap(),
            0
        );
        let members = JournalFilters {
            category: Some(crate::models::JournalCategory::Members),
            limit: 10,
            ..JournalFilters::default()
        };
        assert_eq!(
            JournalRepository::count_filtered(&pool, &members)
                .await
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn the_purge_forgets_only_the_older_lines() {
        let pool = test_pool().await;
        JournalRepository::add(&pool, &NewJournalEntry::new(JournalKind::AppError))
            .await
            .unwrap();
        sqlx::query("UPDATE journal_entries SET created_at = datetime('now', '-100 days')")
            .execute(&pool)
            .await
            .unwrap();
        JournalRepository::add(&pool, &NewJournalEntry::new(JournalKind::AppWarning))
            .await
            .unwrap();

        let forgotten = JournalRepository::purge(&pool, Utc::now() - Duration::days(90))
            .await
            .unwrap();

        assert_eq!(forgotten, 1);
        assert_eq!(rows(&pool).await.len(), 1);
    }
}
