//! Application configuration loaded from environment variables.

use std::env;
use std::fmt::Display;
use std::io::IsTerminal;
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::OnceLock;
use std::time::Duration;

use axum::http::HeaderName;
use tracing::level_filters::LevelFilter;

use crate::journal::{JournalInbox, journal_layer};
use crate::middleware::client_ip::{ClientIpSource, X_FORWARDED_FOR};

/// Errors that can occur when loading or validating configuration.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("invalid value for {key}: {message}")]
    InvalidValue { key: String, message: String },
}

/// How long a sign-in form stays valid. Short, because every visit to the
/// sign-in page stores one of these sessions.
const FORM_SESSION_SECS: u64 = 60 * 60;

/// Where the database lives when `DATABASE_URL` is not set.
pub const DEFAULT_DATABASE_URL: &str = "./statup.db";

/// Where the uploaded icons and logo live when `UPLOAD_DIR` is not set.
pub const DEFAULT_UPLOAD_DIR: &str = "data/uploads";

/// The folder named `backups` next to the database, unless `BACKUP_DIR`
/// says otherwise.
const BACKUP_FOLDER: &str = "backups";

/// The file chosen with `--env`, read instead of `.env`.
static ENV_FILE: OnceLock<PathBuf> = OnceLock::new();

/// Application configuration.
// Each flag is a setting of its own, read from its own variable.
#[allow(clippy::struct_excessive_bools)]
pub struct Config {
    /// `SQLite` database path.
    pub database_url: String,
    /// Server listen address.
    pub host: IpAddr,
    /// Server listen port.
    pub port: u16,
    /// Lifetime of a session that nobody signed in with (the sign-in form).
    pub session_expiry: Duration,
    /// Logging level filter.
    pub log_level: LevelFilter,
    /// Maximum database connections in the pool.
    pub db_max_connections: u32,
    /// Initial admin email (first run only).
    pub admin_email: Option<String>,
    /// Initial admin password (first run only).
    pub admin_password: Option<String>,
    /// Directory for user-uploaded files.
    pub upload_dir: String,
    /// Directory the backups taken by `statup backup` are written to.
    pub backup_dir: String,
    /// Whether visitors without an account can read the pages, until an
    /// admin chooses in the settings.
    pub public_mode: bool,
    /// Read the client address from `client_ip_header` and the scheme from
    /// `X-Forwarded-Proto`. Only behind a reverse proxy that sets them:
    /// without one, a client can forge them and walk around the rate limits.
    pub trust_proxy_headers: bool,
    /// The header the reverse proxy writes the client address in.
    pub client_ip_header: HeaderName,
    /// Address visitors use to reach the instance, without a trailing slash.
    /// Feed readers need absolute links, and the `Host` header is not reliable
    /// enough behind a proxy that rewrites it. Falls back to the request host.
    pub public_url: Option<String>,
    /// Ask GitHub once a day whether a newer version is published.
    pub update_check: bool,
    /// Check every minute that the services set up for it answer.
    pub monitoring: bool,
    /// The mail server the email destinations send through, when set.
    pub smtp: Option<SmtpConfig>,
}

/// How to reach the mail server. No `Debug`: it holds the password.
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub security: SmtpSecurity,
    /// User name and password, when the server asks for them.
    pub credentials: Option<(String, String)>,
    /// The sender of the messages, `Statup <status@example.com>`.
    pub from: String,
}

/// How the connection to the mail server is protected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmtpSecurity {
    /// Plain connection upgraded to TLS, port 587.
    StartTls,
    /// TLS from the first byte, port 465.
    Tls,
    /// No TLS, for a relay on the same network, port 25.
    None,
}

impl SmtpSecurity {
    fn default_port(self) -> u16 {
        match self {
            Self::StartTls => 587,
            Self::Tls => 465,
            Self::None => 25,
        }
    }
}

impl FromStr for SmtpSecurity {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "starttls" => Ok(Self::StartTls),
            "tls" => Ok(Self::Tls),
            "none" => Ok(Self::None),
            _ => Err("must be starttls, tls or none".to_string()),
        }
    }
}

impl Config {
    /// Load configuration from the environment, after reading `.env` from
    /// the working directory when it exists.
    ///
    /// # Errors
    ///
    /// Returns `ConfigError` if `.env` is malformed or a value is invalid.
    /// Every setting has a default.
    pub fn from_env() -> Result<Self, ConfigError> {
        load_dotenv()?;
        let database_url = env_or("DATABASE_URL", DEFAULT_DATABASE_URL);
        let config = Self {
            host: parse_env("HOST", IpAddr::V4(Ipv4Addr::UNSPECIFIED))?,
            port: parse_env("PORT", 3000)?,
            session_expiry: Duration::from_secs(parse_env("SESSION_EXPIRY", FORM_SESSION_SECS)?),
            log_level: parse_log_level(&env_or("LOG_LEVEL", "info"))?,
            db_max_connections: parse_env("DB_MAX_CONNECTIONS", 10)?,
            admin_email: non_empty_env("ADMIN_EMAIL"),
            admin_password: non_empty_env("ADMIN_PASSWORD"),
            upload_dir: env_or("UPLOAD_DIR", DEFAULT_UPLOAD_DIR),
            backup_dir: backup_dir_for(&database_url),
            public_mode: parse_env("PUBLIC_MODE", false)?,
            trust_proxy_headers: parse_env("TRUST_PROXY_HEADERS", false)?,
            client_ip_header: parse_env("CLIENT_IP_HEADER", X_FORWARDED_FOR)?,
            public_url: non_empty_env("PUBLIC_URL")
                .map(|url| url.trim().trim_end_matches('/').to_string()),
            update_check: parse_env("UPDATE_CHECK", true)?,
            monitoring: parse_env("MONITORING", true)?,
            smtp: smtp_from_env()?,
            database_url,
        };
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.port == 0 {
            return Err(invalid("PORT", "must be greater than 0"));
        }
        if self.db_max_connections == 0 {
            return Err(invalid("DB_MAX_CONNECTIONS", "must be greater than 0"));
        }
        let expiry_secs = self.session_expiry.as_secs();
        if expiry_secs == 0 || i64::try_from(expiry_secs).is_err() {
            return Err(invalid(
                "SESSION_EXPIRY",
                "must be a positive number of seconds",
            ));
        }
        Ok(())
    }

    /// Return the full socket address for binding.
    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// Where the client address is read from.
    pub fn client_ip_source(&self) -> ClientIpSource {
        if self.trust_proxy_headers {
            ClientIpSource::Header(self.client_ip_header.clone())
        } else {
            ClientIpSource::Peer
        }
    }

    /// Whether cookies are marked `Secure`. Only when visitors reach the
    /// instance over HTTPS: a browser refuses a secure cookie on a plain
    /// HTTP page, and nobody could sign in to a local install.
    pub fn secure_cookies(&self) -> bool {
        serves_https(self.public_url.as_deref())
    }
}

/// Whether `PUBLIC_URL` says visitors reach the instance over HTTPS.
pub fn serves_https(public_url: Option<&str>) -> bool {
    public_url.is_some_and(|url| url.to_ascii_lowercase().starts_with("https://"))
}

/// Makes [`load_dotenv`] read `path` instead of `.env`. Only the first call
/// counts.
pub fn use_env_file(path: PathBuf) {
    // A second call keeps the first file: the command line is read once.
    let _ = ENV_FILE.set(path);
}

/// Reads the file chosen with `--env`, or else `.env` from the working
/// directory only, never from a parent directory, where an unrelated project
/// could keep its own. A missing `.env` is not an error; a missing chosen
/// file is.
///
/// # Errors
///
/// Returns `ConfigError` when the chosen file is missing, or when a file
/// exists but cannot be read or parsed.
pub fn load_dotenv() -> Result<(), ConfigError> {
    if let Some(path) = ENV_FILE.get() {
        return dotenvy::from_path(path)
            .map_err(|e| invalid("--env", &format!("{}: {e}", path.display())));
    }
    match dotenvy::from_path(".env") {
        Err(e) if !e.not_found() => Err(invalid(".env", &e.to_string())),
        _ => Ok(()),
    }
}

/// The database file named by `DATABASE_URL`, or `None` for an in-memory
/// database.
pub fn database_file(database_url: &str) -> Option<PathBuf> {
    let path = database_url
        .strip_prefix("sqlite://")
        .or_else(|| database_url.strip_prefix("sqlite:"))
        .unwrap_or(database_url);
    let path = path.split('?').next().unwrap_or_default();
    (!path.is_empty() && path != ":memory:").then(|| PathBuf::from(path))
}

/// Where backups are written: `BACKUP_DIR`, or a `backups` folder next to
/// the database file.
pub fn backup_dir_for(database_url: &str) -> String {
    default_or_chosen_backup_dir(non_empty_env("BACKUP_DIR"), database_url)
}

fn default_or_chosen_backup_dir(chosen: Option<String>, database_url: &str) -> String {
    if let Some(dir) = chosen {
        return dir;
    }
    let beside = database_file(database_url)
        .and_then(|file| file.parent().map(Path::to_path_buf))
        .filter(|dir| !dir.as_os_str().is_empty() && dir != Path::new("."));
    match beside {
        Some(dir) => dir.join(BACKUP_FOLDER).to_string_lossy().into_owned(),
        None => BACKUP_FOLDER.to_string(),
    }
}

/// The mail server, set by `SMTP_HOST`; the other variables only count with
/// it.
fn smtp_from_env() -> Result<Option<SmtpConfig>, ConfigError> {
    let Some(host) = non_empty_env("SMTP_HOST") else {
        return Ok(None);
    };
    let security: SmtpSecurity = parse_env("SMTP_SECURITY", SmtpSecurity::StartTls)?;
    let port = parse_env("SMTP_PORT", security.default_port())?;
    let credentials = match (
        non_empty_env("SMTP_USERNAME"),
        non_empty_env("SMTP_PASSWORD"),
    ) {
        (Some(user), Some(password)) => Some((user, password)),
        (None, None) => None,
        _ => {
            return Err(invalid(
                "SMTP_USERNAME",
                "SMTP_USERNAME and SMTP_PASSWORD go together",
            ));
        }
    };
    let from = non_empty_env("SMTP_FROM")
        .ok_or_else(|| invalid("SMTP_FROM", "required with SMTP_HOST"))?;
    Ok(Some(SmtpConfig {
        host: host.trim().to_string(),
        port,
        security,
        credentials,
        from: from.trim().to_string(),
    }))
}

fn invalid(key: &str, message: &str) -> ConfigError {
    ConfigError::InvalidValue {
        key: key.to_string(),
        message: message.to_string(),
    }
}

fn env_or(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

/// The raw value, untrimmed so a password keeps its spaces, unless blank.
fn non_empty_env(key: &str) -> Option<String> {
    env::var(key).ok().filter(|value| !value.trim().is_empty())
}

fn parse_env<T>(key: &str, default: T) -> Result<T, ConfigError>
where
    T: FromStr,
    T::Err: Display,
{
    match env::var(key) {
        Ok(raw) => raw
            .trim()
            .parse()
            .map_err(|e: T::Err| invalid(key, &e.to_string())),
        Err(_) => Ok(default),
    }
}

/// Installs the global subscriber. `RUST_LOG`, when it holds a valid filter,
/// replaces `LOG_LEVEL`, which otherwise applies to every target. Colors
/// are only written to a terminal, never to a container log. The app's
/// warnings and errors are also caught for the journal, whatever the level;
/// the returned inbox is handed to the journal writer once the database
/// is open.
pub fn init_logging(level: LevelFilter) -> JournalInbox {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::{EnvFilter, Layer, fmt};

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level.to_string()));
    let output = fmt::layer()
        .with_target(true)
        .with_ansi(std::io::stdout().is_terminal())
        .with_filter(env_filter);
    let (journal, inbox) = journal_layer();
    tracing_subscriber::registry()
        .with(output)
        .with(journal)
        .init();
    inbox
}

/// Parse a log level string into a `LevelFilter`.
fn parse_log_level(s: &str) -> Result<LevelFilter, ConfigError> {
    match s.trim().to_lowercase().as_str() {
        "trace" => Ok(LevelFilter::TRACE),
        "debug" => Ok(LevelFilter::DEBUG),
        "info" => Ok(LevelFilter::INFO),
        "warn" => Ok(LevelFilter::WARN),
        "error" => Ok(LevelFilter::ERROR),
        "off" => Ok(LevelFilter::OFF),
        _ => Err(invalid(
            "LOG_LEVEL",
            &format!("unknown level '{s}', expected trace|debug|info|warn|error|off"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_cookie_is_secure_only_behind_an_https_address() {
        assert!(serves_https(Some("https://status.example.com")));
        assert!(serves_https(Some("HTTPS://status.example.com")));
        assert!(!serves_https(Some("http://status.example.com")));
        assert!(!serves_https(None));
    }

    fn default_backup_dir(database_url: &str) -> String {
        default_or_chosen_backup_dir(None, database_url)
    }

    #[test]
    fn backups_go_to_a_folder_next_to_the_database() {
        assert_eq!(default_backup_dir("./statup.db"), "backups");
        assert_eq!(default_backup_dir("statup.db"), "backups");
        assert_eq!(
            default_backup_dir("sqlite:///data/statup.db"),
            "/data/backups"
        );
        assert_eq!(
            default_backup_dir("sqlite://data/statup.db?mode=rwc"),
            "data/backups"
        );
        assert_eq!(default_backup_dir("/data/statup.db"), "/data/backups");
        assert_eq!(default_backup_dir("sqlite::memory:"), "backups");
    }

    #[test]
    fn backup_dir_setting_wins_over_the_default() {
        let chosen = Some("/mnt/nas/statup".to_string());
        assert_eq!(
            default_or_chosen_backup_dir(chosen, "/data/statup.db"),
            "/mnt/nas/statup"
        );
    }

    #[test]
    fn database_file_is_none_for_a_memory_database() {
        assert_eq!(database_file("sqlite::memory:"), None);
        assert_eq!(database_file(""), None);
        assert_eq!(
            database_file("sqlite:///data/statup.db?mode=rwc"),
            Some(PathBuf::from("/data/statup.db"))
        );
    }

    #[test]
    fn log_levels_are_parsed_case_insensitively() {
        assert_eq!(parse_log_level("WARN").unwrap(), LevelFilter::WARN);
        assert_eq!(parse_log_level("off").unwrap(), LevelFilter::OFF);
        assert!(parse_log_level("verbose").is_err());
    }

    fn config() -> Config {
        Config {
            database_url: DEFAULT_DATABASE_URL.to_string(),
            host: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port: 3000,
            session_expiry: Duration::from_secs(FORM_SESSION_SECS),
            log_level: LevelFilter::INFO,
            db_max_connections: 10,
            admin_email: None,
            admin_password: None,
            upload_dir: DEFAULT_UPLOAD_DIR.to_string(),
            backup_dir: BACKUP_FOLDER.to_string(),
            public_mode: false,
            trust_proxy_headers: false,
            client_ip_header: X_FORWARDED_FOR,
            public_url: None,
            update_check: true,
            monitoring: true,
            smtp: None,
        }
    }

    #[test]
    fn the_mail_security_names_its_port() {
        let parsed = |value: &str| value.parse::<SmtpSecurity>();
        assert_eq!(parsed("STARTTLS"), Ok(SmtpSecurity::StartTls));
        assert_eq!(parsed("tls").map(SmtpSecurity::default_port), Ok(465));
        assert_eq!(parsed("none").map(SmtpSecurity::default_port), Ok(25));
        assert!(parsed("ssl").is_err());
    }

    fn rejected_key(config: &Config) -> Option<String> {
        match config.validate() {
            Err(ConfigError::InvalidValue { key, .. }) => Some(key),
            Ok(()) => None,
        }
    }

    #[test]
    fn default_settings_are_valid() {
        assert!(config().validate().is_ok());
    }

    #[test]
    fn settings_that_would_break_the_server_name_the_faulty_key() {
        let mut zero_port = config();
        zero_port.port = 0;
        assert_eq!(rejected_key(&zero_port).as_deref(), Some("PORT"));

        let mut no_connections = config();
        no_connections.db_max_connections = 0;
        assert_eq!(
            rejected_key(&no_connections).as_deref(),
            Some("DB_MAX_CONNECTIONS")
        );
    }

    #[test]
    fn session_expiry_must_be_positive_and_fit_in_signed_seconds() {
        let mut instant = config();
        instant.session_expiry = Duration::ZERO;
        assert_eq!(rejected_key(&instant).as_deref(), Some("SESSION_EXPIRY"));

        let mut endless = config();
        endless.session_expiry = Duration::from_secs(u64::MAX);
        assert_eq!(rejected_key(&endless).as_deref(), Some("SESSION_EXPIRY"));
    }

    #[test]
    fn client_address_ignores_the_header_without_a_trusted_proxy() {
        let mut config = config();
        config.client_ip_header = HeaderName::from_static("true-client-ip");
        assert!(matches!(config.client_ip_source(), ClientIpSource::Peer));
    }

    #[test]
    fn client_address_is_read_from_the_header_behind_a_trusted_proxy() {
        let mut config = config();
        config.trust_proxy_headers = true;
        assert!(matches!(
            config.client_ip_source(),
            ClientIpSource::Header(name) if name == X_FORWARDED_FOR
        ));

        config.client_ip_header = HeaderName::from_static("true-client-ip");
        assert!(matches!(
            config.client_ip_source(),
            ClientIpSource::Header(name) if name == "true-client-ip"
        ));
    }
}
