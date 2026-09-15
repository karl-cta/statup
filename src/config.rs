//! Application configuration loaded from environment variables.

use std::env;
use std::net::IpAddr;
use std::time::Duration;

use tracing::level_filters::LevelFilter;

/// Errors that can occur when loading or validating configuration.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("invalid value for {key}: {message}")]
    InvalidValue { key: String, message: String },
}

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
    /// Session lifetime.
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
    /// Public mode: read-only pages accessible without login (REQ-16).
    pub public_mode: bool,
    /// Read the client IP from `Forwarded` / `X-Forwarded-For` for rate limiting.
    /// Only enable behind a reverse proxy that sets those headers: with no proxy
    /// in front, a client can forge them and walk around the rate limit.
    pub trust_proxy_headers: bool,
    /// Address visitors use to reach the instance, without a trailing slash.
    /// Feed readers need absolute links, and the `Host` header is not reliable
    /// enough behind a proxy that rewrites it. Falls back to the request host.
    pub public_url: Option<String>,
}

impl Config {
    /// Load configuration from environment variables (`.env` file supported via dotenvy).
    ///
    /// # Errors
    ///
    /// Returns `ConfigError` if a value is invalid. Every setting has a default.
    pub fn from_env() -> Result<Self, ConfigError> {
        dotenvy::dotenv().ok();

        let database_url = get_env_or("DATABASE_URL", DEFAULT_DATABASE_URL);
        let host = get_env_or("HOST", "0.0.0.0")
            .parse::<IpAddr>()
            .map_err(|e| ConfigError::InvalidValue {
                key: "HOST".into(),
                message: e.to_string(),
            })?;
        let port =
            get_env_or("PORT", "3000")
                .parse::<u16>()
                .map_err(|e| ConfigError::InvalidValue {
                    key: "PORT".into(),
                    message: e.to_string(),
                })?;
        let session_expiry_secs = get_env_or("SESSION_EXPIRY", "604800")
            .parse::<u64>()
            .map_err(|e| ConfigError::InvalidValue {
                key: "SESSION_EXPIRY".into(),
                message: e.to_string(),
            })?;
        let log_level = parse_log_level(&get_env_or("LOG_LEVEL", "info"))?;
        let db_max_connections = get_env_or("DB_MAX_CONNECTIONS", "10")
            .parse::<u32>()
            .map_err(|e| ConfigError::InvalidValue {
                key: "DB_MAX_CONNECTIONS".into(),
                message: e.to_string(),
            })?;
        let admin_email = env::var("ADMIN_EMAIL").ok().filter(|s| !s.is_empty());
        let admin_password = env::var("ADMIN_PASSWORD").ok().filter(|s| !s.is_empty());
        let upload_dir = get_env_or("UPLOAD_DIR", "data/uploads");
        let public_mode = get_env_or("PUBLIC_MODE", "false")
            .parse::<bool>()
            .map_err(|e| ConfigError::InvalidValue {
                key: "PUBLIC_MODE".into(),
                message: e.to_string(),
            })?;
        let trust_proxy_headers = get_env_or("TRUST_PROXY_HEADERS", "false")
            .parse::<bool>()
            .map_err(|e| ConfigError::InvalidValue {
                key: "TRUST_PROXY_HEADERS".into(),
                message: e.to_string(),
            })?;

        let public_url = env::var("PUBLIC_URL")
            .ok()
            .map(|s| s.trim().trim_end_matches('/').to_string())
            .filter(|s| !s.is_empty());

        let config = Self {
            database_url,
            host,
            port,
            session_expiry: Duration::from_secs(session_expiry_secs),
            log_level,
            db_max_connections,
            admin_email,
            admin_password,
            upload_dir,
            public_mode,
            trust_proxy_headers,
            public_url,
        };

        config.validate()?;
        Ok(config)
    }

    /// Validate configuration values.
    ///
    /// # Errors
    ///
    /// Returns `ConfigError::InvalidValue` if any value fails validation.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.port == 0 {
            return Err(ConfigError::InvalidValue {
                key: "PORT".into(),
                message: "must be greater than 0".into(),
            });
        }

        Ok(())
    }

    /// Return the full socket address for binding.
    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// Whether the session cookie is marked `Secure`. Only when visitors reach
    /// the instance over HTTPS: a browser refuses a secure cookie on a plain
    /// HTTP page, and nobody could sign in to a local install.
    pub fn secure_cookies(&self) -> bool {
        serves_https(self.public_url.as_deref())
    }
}

fn serves_https(public_url: Option<&str>) -> bool {
    public_url.is_some_and(|url| url.to_ascii_lowercase().starts_with("https://"))
}

/// Read an env var or return a default.
fn get_env_or(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

/// Initialize the tracing subscriber with the given log level.
///
/// Uses `RUST_LOG` env var if set, otherwise falls back to the config `log_level`.
/// Pretty format in dev, compact format otherwise.
pub fn init_logging(level: LevelFilter) {
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::fmt;

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level.to_string()));

    fmt()
        .with_env_filter(env_filter)
        .with_target(true)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .init();
}

/// Parse a log level string into a `LevelFilter`.
fn parse_log_level(s: &str) -> Result<LevelFilter, ConfigError> {
    match s.to_lowercase().as_str() {
        "trace" => Ok(LevelFilter::TRACE),
        "debug" => Ok(LevelFilter::DEBUG),
        "info" => Ok(LevelFilter::INFO),
        "warn" => Ok(LevelFilter::WARN),
        "error" => Ok(LevelFilter::ERROR),
        "off" => Ok(LevelFilter::OFF),
        _ => Err(ConfigError::InvalidValue {
            key: "LOG_LEVEL".into(),
            message: format!("unknown level '{s}', expected trace|debug|info|warn|error|off"),
        }),
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
}
