//! The log file of a Statup that runs with no console, as a Windows service.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use chrono::{SecondsFormat, Utc};

use crate::config::database_file;

const FILE_NAME: &str = "statup.log";

/// Above this size, the file is set aside.
const SET_ASIDE_ABOVE: u64 = 10 * 1024 * 1024;

/// `statup.log` in the folder of the database, or in the working folder for
/// an in-memory database.
pub fn path_for(database_url: &str) -> PathBuf {
    database_file(database_url)
        .and_then(|file| file.parent().map(Path::to_path_buf))
        .unwrap_or_default()
        .join(FILE_NAME)
}

/// The log file. Once above 10 MB, it is renamed with a `.1` suffix, which
/// replaces the one set aside before, and a new file is started: at the
/// opening, and while it is written.
pub struct LogFile {
    path: PathBuf,
    limit: u64,
    file: File,
    size: u64,
}

/// Opens the file at `path` to add to it.
///
/// # Errors
///
/// The error of the rename or of the opening.
pub fn open(path: &Path) -> io::Result<LogFile> {
    LogFile::open_below(path, SET_ASIDE_ABOVE)
}

impl LogFile {
    fn open_below(path: &Path, limit: u64) -> io::Result<Self> {
        let size = fs::metadata(path).map_or(0, |meta| meta.len());
        let mut log = Self {
            path: path.to_path_buf(),
            limit,
            file: append_to(path)?,
            size,
        };
        if log.size > limit {
            log.set_aside()?;
        }
        Ok(log)
    }

    // Rust opens files with FILE_SHARE_DELETE on Windows, so the file can
    // be renamed while it is open.
    fn set_aside(&mut self) -> io::Result<()> {
        fs::rename(&self.path, set_aside(&self.path))?;
        self.file = append_to(&self.path)?;
        self.size = 0;
        Ok(())
    }
}

impl Write for LogFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // A rename that fails keeps the lines in the current file, and is
        // tried again after another limit's worth of them.
        if self.size > self.limit && self.set_aside().is_err() {
            self.size = 0;
        }
        let written = self.file.write(buf)?;
        self.size += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

fn append_to(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

fn set_aside(path: &Path) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(".1");
    PathBuf::from(name)
}

/// Adds `message` as an error line, for a failure that comes before the
/// logging is set up.
///
/// # Errors
///
/// The error of the opening or of the write.
pub fn write_error(path: &Path, message: &str) -> io::Result<()> {
    let at = Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true);
    writeln!(open(path)?, "{at} ERROR {message}")
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Folder(PathBuf);

    impl Folder {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("statup-log-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_log_sits_next_to_the_database() {
        assert_eq!(
            path_for("sqlite:///data/statup.db"),
            Path::new("/data/statup.log")
        );
        assert_eq!(
            path_for("sqlite:C:/ProgramData/Statup/statup.db"),
            Path::new("C:/ProgramData/Statup/statup.log")
        );
        assert_eq!(path_for("statup.db"), Path::new("statup.log"));
        assert_eq!(path_for("sqlite::memory:"), Path::new("statup.log"));
    }

    #[test]
    fn lines_are_added_to_a_small_file() {
        let folder = Folder::new();
        let path = folder.0.join(FILE_NAME);
        fs::write(&path, "first\n").unwrap();
        writeln!(LogFile::open_below(&path, 1024).unwrap(), "second").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "first\nsecond\n");
        assert!(!set_aside(&path).exists());
    }

    #[test]
    fn a_big_file_is_set_aside_and_replaces_the_previous_one() {
        let folder = Folder::new();
        let path = folder.0.join(FILE_NAME);
        fs::write(set_aside(&path), "oldest").unwrap();
        fs::write(&path, "older").unwrap();
        writeln!(LogFile::open_below(&path, 4).unwrap(), "new").unwrap();

        assert_eq!(fs::read_to_string(set_aside(&path)).unwrap(), "older");
        assert_eq!(fs::read_to_string(&path).unwrap(), "new\n");
    }

    #[test]
    fn a_file_that_grows_past_the_limit_is_set_aside_while_written() {
        let folder = Folder::new();
        let path = folder.0.join(FILE_NAME);
        let mut log = LogFile::open_below(&path, 8).unwrap();
        writeln!(log, "first line").unwrap();
        writeln!(log, "second").unwrap();

        assert_eq!(
            fs::read_to_string(set_aside(&path)).unwrap(),
            "first line\n"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "second\n");
    }

    #[test]
    fn an_error_line_is_dated() {
        let folder = Folder::new();
        let path = folder.0.join(FILE_NAME);
        write_error(&path, "invalid value for PORT").unwrap();

        let line = fs::read_to_string(&path).unwrap();
        assert!(line.ends_with("Z ERROR invalid value for PORT\n"), "{line}");
    }
}
