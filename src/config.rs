//! Application configuration loaded from environment variables.

use std::env;
use std::fmt::Display;
use std::io::IsTerminal;
use std::net::{IpAddr, Ipv4Addr};
use std::str::FromStr;
use std::time::Duration;

use axum::http::HeaderName;
use tracing::level_filters::LevelFilter;

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

/// Application configuration.
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
        let config = Self {
            database_url: env_or("DATABASE_URL", DEFAULT_DATABASE_URL),
            host: parse_env("HOST", IpAddr::V4(Ipv4Addr::UNSPECIFIED))?,
            port: parse_env("PORT", 3000)?,
            session_expiry: Duration::from_secs(parse_env("SESSION_EXPIRY", FORM_SESSION_SECS)?),
            log_level: parse_log_level(&env_or("LOG_LEVEL", "info"))?,
            db_max_connections: parse_env("DB_MAX_CONNECTIONS", 10)?,
            admin_email: non_empty_env("ADMIN_EMAIL"),
            admin_password: non_empty_env("ADMIN_PASSWORD"),
            upload_dir: env_or("UPLOAD_DIR", "data/uploads"),
            public_mode: parse_env("PUBLIC_MODE", false)?,
            trust_proxy_headers: parse_env("TRUST_PROXY_HEADERS", false)?,
            client_ip_header: parse_env("CLIENT_IP_HEADER", X_FORWARDED_FOR)?,
            public_url: non_empty_env("PUBLIC_URL")
                .map(|url| url.trim().trim_end_matches('/').to_string()),
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

/// Reads `.env` from the working directory only, never from a parent
/// directory, where an unrelated project could keep its own. A missing file
/// is not an error.
///
/// # Errors
///
/// Returns `ConfigError` when the file exists but cannot be read or parsed.
pub fn load_dotenv() -> Result<(), ConfigError> {
    match dotenvy::from_path(".env") {
        Err(e) if !e.not_found() => Err(invalid(".env", &e.to_string())),
        _ => Ok(()),
    }
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
/// are only written to a terminal, never to a container log.
pub fn init_logging(level: LevelFilter) {
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::fmt;

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level.to_string()));

    fmt()
        .with_env_filter(env_filter)
        .with_target(true)
        .with_ansi(std::io::stdout().is_terminal())
        .init();
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

    #[test]
    fn log_levels_are_parsed_case_insensitively() {
        assert_eq!(parse_log_level("WARN").unwrap(), LevelFilter::WARN);
        assert_eq!(parse_log_level("off").unwrap(), LevelFilter::OFF);
        assert!(parse_log_level("verbose").is_err());
    }
}
