//! Application configuration loaded from environment variables.

use std::env;
use std::net::IpAddr;
use std::time::Duration;

use tracing::level_filters::LevelFilter;

/// Errors that can occur when loading or validating configuration.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("missing environment variable: {0}")]
    MissingVar(String),

    #[error("invalid value for {key}: {message}")]
    InvalidValue { key: String, message: String },
}

/// Application configuration.
pub struct Config {
    /// `SQLite` database path.
    pub database_url: String,
    /// Server listen address.
    pub host: IpAddr,
    /// Server listen port.
    pub port: u16,
    /// Secret key for session encryption (min 32 chars).
    pub session_secret: String,
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
}

impl Config {
    /// Load configuration from environment variables (`.env` file supported via dotenvy).
    ///
    /// # Errors
    ///
    /// Returns `ConfigError` if required variables are missing or values are invalid.
    pub fn from_env() -> Result<Self, ConfigError> {
        dotenvy::dotenv().ok();

        let database_url = get_env_or("DATABASE_URL", "./statup.db");
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
        let session_secret = get_env_required("SESSION_SECRET")?;
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

        let config = Self {
            database_url,
            host,
            port,
            session_secret,
            session_expiry: Duration::from_secs(session_expiry_secs),
            log_level,
            db_max_connections,
            admin_email,
            admin_password,
            upload_dir,
            public_mode,
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
        if self.session_secret.len() < 32 {
            return Err(ConfigError::InvalidValue {
                key: "SESSION_SECRET".into(),
                message: "must be at least 32 characters".into(),
            });
        }

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
}

/// Read an env var or return a default.
fn get_env_or(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

/// Read a required env var.
fn get_env_required(key: &str) -> Result<String, ConfigError> {
    env::var(key)
        .map_err(|_| ConfigError::MissingVar(key.into()))
        .and_then(|v| {
            if v.is_empty() {
                Err(ConfigError::MissingVar(key.into()))
            } else {
                Ok(v)
            }
        })
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
