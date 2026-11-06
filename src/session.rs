//! Sessions: the `SQLite` store, the cookie, the lifetime of a signed-in
//! session and the stamp that ties it to a password.

use async_trait::async_trait;
use serde::Serialize;
use serde::de::DeserializeOwned;
use time::{Duration, OffsetDateTime};
use tokio::task::AbortHandle;
use tower_sessions::cookie::SameSite;
use tower_sessions::session::{Id, Record};
use tower_sessions::session_store::{self, SessionStore};
use tower_sessions::{Expiry, Session, SessionManagerLayer};

use crate::db::DbPool;
use crate::error::AppError;

pub const USER_ID_KEY: &str = "user_id";

/// Session key for the stamp of the password the session was opened with.
const CREDENTIAL_KEY: &str = "credential";

/// Session key for whether the person asked to stay signed in.
const REMEMBER_KEY: &str = "remember";

/// Session key for the Unix time the signed-in lifetime was last extended.
const REFRESHED_AT_KEY: &str = "refreshed_at";

const REMEMBERED_LIFETIME: Duration = Duration::days(30);
const DEFAULT_LIFETIME: Duration = Duration::hours(24);

/// Activity extends a signed-in session at most this often, so an active
/// person costs one session write an hour.
const REFRESH_INTERVAL_SECS: i64 = 60 * 60;

const CLEANUP_PERIOD: std::time::Duration = std::time::Duration::from_secs(60);

pub async fn read_value<T: DeserializeOwned>(
    session: &Session,
    key: &str,
) -> Result<Option<T>, AppError> {
    session
        .get(key)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("session read failed: {e}")))
}

/// Writes a session value, saved with the response.
///
/// # Errors
///
/// Returns `AppError::Internal` when the store cannot load the session.
pub async fn write_value(
    session: &Session,
    key: &str,
    value: impl Serialize + Send,
) -> Result<(), AppError> {
    session
        .insert(key, value)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("session write failed: {e}")))
}

/// Gives the session a new id and deletes the old one, so an id known
/// before a sign-in or a password change is worth nothing after it.
///
/// # Errors
///
/// Returns `AppError::Internal` when the store fails.
pub async fn rotate_id(session: &Session) -> Result<(), AppError> {
    session
        .cycle_id()
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("session id rotation failed: {e}")))
}

/// The tail of the Argon2 hash output. It changes with every new password
/// (fresh salt), and is too short to be worth anything outside the database.
fn credential_stamp(password_hash: &str) -> &str {
    let start = password_hash.len().saturating_sub(16);
    password_hash.get(start..).unwrap_or(password_hash)
}

/// Ties the session to the current password, so that a password set
/// elsewhere (a reset by the host, a change on another device) signs it out.
///
/// # Errors
///
/// Returns `AppError::Internal` when the store fails.
pub async fn stamp_credential(session: &Session, password_hash: &str) -> Result<(), AppError> {
    write_value(session, CREDENTIAL_KEY, credential_stamp(password_hash)).await
}

/// Whether the session was opened with the password the account has now.
pub async fn credential_matches(session: &Session, password_hash: &str) -> bool {
    read_value::<String>(session, CREDENTIAL_KEY)
        .await
        .ok()
        .flatten()
        .is_some_and(|stamp| stamp == credential_stamp(password_hash))
}

/// Starts the signed-in lifetime: 30 days without a visit when the person
/// asked to stay signed in, 24 hours otherwise.
///
/// # Errors
///
/// Returns `AppError::Internal` when the store fails.
pub async fn start_signed_in(session: &Session, remember: bool) -> Result<(), AppError> {
    write_value(session, REMEMBER_KEY, remember).await?;
    write_value(session, REFRESHED_AT_KEY, now_unix()).await?;
    session.set_expiry(Some(signed_in_expiry(remember)));
    Ok(())
}

/// Extends a signed-in session on activity, at most once an hour, so its
/// lifetime counts from the last visit rather than from the sign-in.
///
/// # Errors
///
/// Returns `AppError::Internal` when the store fails.
pub async fn refresh_signed_in(session: &Session) -> Result<(), AppError> {
    let refreshed_at: i64 = read_value(session, REFRESHED_AT_KEY).await?.unwrap_or(0);
    if now_unix().saturating_sub(refreshed_at) < REFRESH_INTERVAL_SECS {
        return Ok(());
    }
    write_value(session, REFRESHED_AT_KEY, now_unix()).await?;
    apply_signed_in_expiry(session).await
}

/// Every save writes the expiry held by the session object, which starts as
/// the default of the layer. A modified signed-in session gets its own
/// lifetime back before the response saves it.
///
/// # Errors
///
/// Returns `AppError::Internal` when the store fails.
pub async fn apply_signed_in_expiry(session: &Session) -> Result<(), AppError> {
    let remember = read_value::<bool>(session, REMEMBER_KEY)
        .await?
        .unwrap_or(false);
    session.set_expiry(Some(signed_in_expiry(remember)));
    Ok(())
}

fn signed_in_expiry(remember: bool) -> Expiry {
    Expiry::OnInactivity(if remember {
        REMEMBERED_LIFETIME
    } else {
        DEFAULT_LIFETIME
    })
}

fn now_unix() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

/// The `tower_sessions` table, in the layout and record encoding of the
/// `tower-sessions-sqlx-store` crate it once came from, so the sessions of an
/// instance survive the upgrade.
#[derive(Clone, Debug)]
pub struct SqliteSessionStore {
    pool: DbPool,
}

/// Creates the `tower_sessions` table and its expiry index when absent.
///
/// # Errors
///
/// Returns `sqlx::Error` if the schema cannot be created.
pub async fn create_session_store(pool: &DbPool) -> Result<SqliteSessionStore, sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS tower_sessions \
         (id TEXT PRIMARY KEY NOT NULL, data BLOB NOT NULL, expiry_date INTEGER NOT NULL)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_tower_sessions_expiry ON tower_sessions (expiry_date)",
    )
    .execute(pool)
    .await?;
    Ok(SqliteSessionStore { pool: pool.clone() })
}

fn encode(record: &Record) -> session_store::Result<Vec<u8>> {
    rmp_serde::to_vec(record).map_err(|e| session_store::Error::Encode(e.to_string()))
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "shaped for map_err, which hands the error over"
)]
fn backend(e: sqlx::Error) -> session_store::Error {
    session_store::Error::Backend(e.to_string())
}

impl SqliteSessionStore {
    /// Inserts the record unless its id is taken, in a single statement that
    /// waits for the write lock like any other instead of failing on it.
    async fn insert_new(&self, record: &Record) -> session_store::Result<bool> {
        let result = sqlx::query(
            "INSERT INTO tower_sessions (id, data, expiry_date) VALUES (?, ?, ?) \
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(record.id.to_string())
        .bind(encode(record)?)
        .bind(record.expiry_date)
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(result.rows_affected() == 1)
    }
}

/// Deletes the sessions past their expiry. The dates are compared in the
/// format the store writes them in, which `datetime('now')` is not.
///
/// # Errors
///
/// Returns `sqlx::Error` if the statement fails.
pub async fn delete_expired(pool: &DbPool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM tower_sessions WHERE expiry_date < ?")
        .bind(OffsetDateTime::now_utc())
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

#[async_trait]
impl SessionStore for SqliteSessionStore {
    async fn create(&self, record: &mut Record) -> session_store::Result<()> {
        while !self.insert_new(record).await? {
            record.id = Id::default();
        }
        Ok(())
    }

    async fn save(&self, record: &Record) -> session_store::Result<()> {
        sqlx::query(
            "INSERT INTO tower_sessions (id, data, expiry_date) VALUES (?, ?, ?) \
             ON CONFLICT (id) DO UPDATE SET data = excluded.data, expiry_date = excluded.expiry_date",
        )
        .bind(record.id.to_string())
        .bind(encode(record)?)
        .bind(record.expiry_date)
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(())
    }

    async fn load(&self, id: &Id) -> session_store::Result<Option<Record>> {
        let data: Option<Vec<u8>> =
            sqlx::query_scalar("SELECT data FROM tower_sessions WHERE id = ? AND expiry_date > ?")
                .bind(id.to_string())
                .bind(OffsetDateTime::now_utc())
                .fetch_optional(&self.pool)
                .await
                .map_err(backend)?;
        data.map(|bytes| {
            rmp_serde::from_slice(&bytes).map_err(|e| session_store::Error::Decode(e.to_string()))
        })
        .transpose()
    }

    async fn delete(&self, id: &Id) -> session_store::Result<()> {
        sqlx::query("DELETE FROM tower_sessions WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(backend)?;
        Ok(())
    }
}

/// Builds the session layer: `HttpOnly`, `SameSite=Lax`, `Secure` when the
/// instance is served over HTTPS. `lifetime` applies to sessions nobody
/// signed in with; a sign-in sets its own lifetime.
pub fn session_layer(
    store: SqliteSessionStore,
    lifetime: std::time::Duration,
    secure: bool,
) -> SessionManagerLayer<SqliteSessionStore> {
    let seconds = i64::try_from(lifetime.as_secs()).unwrap_or(i64::MAX);
    SessionManagerLayer::new(store)
        .with_secure(secure)
        .with_same_site(SameSite::Lax)
        .with_http_only(true)
        .with_expiry(Expiry::OnInactivity(Duration::seconds(seconds)))
}

/// Deletes expired sessions every minute. A failed pass is logged and the
/// next one runs anyway.
pub fn spawn_cleanup_task(pool: DbPool) -> AbortHandle {
    let task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(CLEANUP_PERIOD);
        loop {
            interval.tick().await;
            if let Err(e) = delete_expired(&pool).await {
                tracing::warn!(error = %e, "Expired session cleanup failed");
            }
        }
    });
    task.abort_handle()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::test_helpers::test_pool;

    fn record(expiry_date: OffsetDateTime) -> Record {
        Record {
            id: Id::default(),
            data: std::collections::HashMap::default(),
            expiry_date,
        }
    }

    #[tokio::test]
    async fn created_sessions_load_back_and_expired_ones_are_deleted() {
        let pool = test_pool().await;
        let store = create_session_store(&pool).await.unwrap();

        let mut live = record(OffsetDateTime::now_utc() + Duration::hours(1));
        store.create(&mut live).await.unwrap();
        let mut expired = record(OffsetDateTime::now_utc() - Duration::hours(1));
        store.create(&mut expired).await.unwrap();

        assert!(store.load(&live.id).await.unwrap().is_some());
        assert_eq!(delete_expired(&pool).await.unwrap(), 1);
        assert!(store.load(&live.id).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn a_taken_id_is_replaced_on_creation() {
        let pool = test_pool().await;
        let store = create_session_store(&pool).await.unwrap();
        let expiry = OffsetDateTime::now_utc() + Duration::hours(1);

        let mut first = record(expiry);
        store.create(&mut first).await.unwrap();
        let mut second = record(expiry);
        second.id = first.id;
        store.create(&mut second).await.unwrap();

        assert_ne!(first.id, second.id);
        assert!(store.load(&second.id).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn a_saved_session_is_replaced_in_place_and_a_deleted_one_is_gone() {
        let pool = test_pool().await;
        let store = create_session_store(&pool).await.unwrap();
        let mut session = record(OffsetDateTime::now_utc() + Duration::hours(1));
        store.create(&mut session).await.unwrap();

        session
            .data
            .insert(USER_ID_KEY.to_string(), serde_json::json!(7));
        store.save(&session).await.unwrap();
        let loaded = store.load(&session.id).await.unwrap().unwrap();
        assert_eq!(loaded.data[USER_ID_KEY], serde_json::json!(7));

        session.expiry_date = OffsetDateTime::now_utc() - Duration::hours(1);
        store.save(&session).await.unwrap();
        assert!(store.load(&session.id).await.unwrap().is_none());

        store.delete(&session.id).await.unwrap();
        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tower_sessions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(rows, 0);
    }

    /// A record as encoded by tower-sessions 0.14, which Statup used up to
    /// 0.12.0: a person signed in before an upgrade stays signed in.
    #[tokio::test]
    async fn a_session_saved_by_an_earlier_release_still_loads() {
        const SAVED: &str =
            "93c4100123456789abcdef0123456789abcdef81a7757365725f69640799cd08340100000000000000";
        let bytes: Vec<u8> = (0..SAVED.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&SAVED[i..i + 2], 16).unwrap())
            .collect();
        let pool = test_pool().await;
        let store = create_session_store(&pool).await.unwrap();
        let id = Id(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef);
        sqlx::query("INSERT INTO tower_sessions (id, data, expiry_date) VALUES (?, ?, ?)")
            .bind(id.to_string())
            .bind(bytes)
            .bind(OffsetDateTime::from_unix_timestamp(4_102_444_800).unwrap())
            .execute(&pool)
            .await
            .unwrap();

        let loaded = store.load(&id).await.unwrap().unwrap();
        assert_eq!(loaded.id, id);
        assert_eq!(loaded.data[USER_ID_KEY], serde_json::json!(7));
    }

    #[test]
    fn remembered_sessions_live_longer() {
        assert_eq!(
            signed_in_expiry(true),
            Expiry::OnInactivity(Duration::days(30))
        );
        assert_eq!(
            signed_in_expiry(false),
            Expiry::OnInactivity(Duration::hours(24))
        );
    }

    #[tokio::test]
    async fn activity_extends_a_session_at_most_once_an_hour() {
        let store = Arc::new(tower_sessions::MemoryStore::default());
        let session = Session::new(None, store, None);
        write_value(&session, REMEMBER_KEY, true).await.unwrap();
        write_value(&session, REFRESHED_AT_KEY, now_unix())
            .await
            .unwrap();

        refresh_signed_in(&session).await.unwrap();
        assert_eq!(session.expiry(), None, "a recent refresh is left alone");

        write_value(
            &session,
            REFRESHED_AT_KEY,
            now_unix() - REFRESH_INTERVAL_SECS,
        )
        .await
        .unwrap();
        refresh_signed_in(&session).await.unwrap();
        assert_eq!(session.expiry(), Some(signed_in_expiry(true)));
    }
}
