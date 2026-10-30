//! Copies one database over another through `SQLite`'s online backup API.
//!
//! The copy is made page by page under `SQLite`'s own locks, so the target
//! may be the live database of a running server: other connections wait on
//! the busy timeout, then see the new content. A row-by-row copy would have
//! to know about the full-text index, the triggers and the sequences; pages
//! carry them exactly.
//!
//! sqlx does not wrap this API, so this is the one module of Statup that
//! calls `SQLite` directly. `libsqlite3-sys` is pinned to the version sqlx
//! links, which hands out the raw handles.

use std::ffi::{CStr, c_int};
use std::ptr::NonNull;
use std::time::{Duration, Instant};

use libsqlite3_sys::{
    SQLITE_BUSY, SQLITE_DONE, SQLITE_LOCKED, SQLITE_OK, sqlite3, sqlite3_backup_finish,
    sqlite3_backup_init, sqlite3_backup_step, sqlite3_errcode, sqlite3_errmsg,
};
use sqlx::SqliteConnection;

/// How long the copy waits for other connections to release the target.
const BUSY_WAIT: Duration = Duration::from_secs(10);

const MAIN: &CStr = c"main";

/// Replaces the whole content of `target` with the content of `source`.
///
/// # Errors
///
/// Fails when a connection is gone, the target stays locked longer than
/// the busy wait, or `SQLite` refuses the copy (different page sizes on a
/// WAL target, a read-only target).
pub async fn copy_database(
    source: &mut SqliteConnection,
    target: &mut SqliteConnection,
) -> Result<(), sqlx::Error> {
    let mut source = source.lock_handle().await?;
    let mut target = target.lock_handle().await?;
    copy_pages(source.as_raw_handle(), target.as_raw_handle())
}

fn copy_pages(source: NonNull<sqlite3>, target: NonNull<sqlite3>) -> Result<(), sqlx::Error> {
    // SAFETY: both handles come from `lock_handle`, whose guards are held by
    // the caller for the whole call: no sqlx worker uses them meanwhile.
    // `sqlite3_backup_init` returns null on failure, with the reason on the
    // target handle; a non-null backup is always released by
    // `sqlite3_backup_finish`.
    unsafe {
        let backup = sqlite3_backup_init(
            target.as_ptr(),
            MAIN.as_ptr(),
            source.as_ptr(),
            MAIN.as_ptr(),
        );
        if backup.is_null() {
            return Err(failure(target, sqlite3_errcode(target.as_ptr())));
        }
        let started = Instant::now();
        let mut code = sqlite3_backup_step(backup, -1);
        while (code == SQLITE_BUSY || code == SQLITE_LOCKED) && started.elapsed() < BUSY_WAIT {
            std::thread::sleep(Duration::from_millis(50));
            code = sqlite3_backup_step(backup, -1);
        }
        let finished = sqlite3_backup_finish(backup);
        if code != SQLITE_DONE {
            return Err(failure(target, code));
        }
        if finished != SQLITE_OK {
            return Err(failure(target, finished));
        }
    }
    Ok(())
}

/// The error `SQLite` reports on the target handle.
///
/// # Safety
///
/// `target` must be an open handle that no other thread uses.
unsafe fn failure(target: NonNull<sqlite3>, code: c_int) -> sqlx::Error {
    // SAFETY: the caller guarantees the handle; `sqlite3_errmsg` returns a
    // string owned by the handle, copied before the handle is used again.
    let message = unsafe { CStr::from_ptr(sqlite3_errmsg(target.as_ptr())) };
    sqlx::Error::Protocol(format!(
        "database copy failed ({code}): {}",
        message.to_string_lossy()
    ))
}
