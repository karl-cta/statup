//! Business logic layer: authentication, events, services, icons, templates.

mod auth_service;
mod dashboard_layout_service;
mod event_service;
mod event_template_service;
mod icon_service;
mod login_rate_limiter;
mod logo_service;
mod monitoring;
mod monitoring_task;
mod probe;
mod service_service;
mod settings_service;
mod update_check;

pub use auth_service::*;
pub use dashboard_layout_service::*;
pub use event_service::*;
pub use event_template_service::*;
pub use icon_service::*;
pub use login_rate_limiter::LoginRateLimiter;
pub use logo_service::*;
pub use monitoring::*;
pub use monitoring_task::{LastCheck, LastChecks, MonitorState, run_round, spawn_monitoring};
pub use probe::{CHECK_TIMEOUT, Finding, Prober, Probes, Report};
pub use service_service::*;
pub use settings_service::SettingsService;
pub use update_check::{CURRENT_VERSION, NewerRelease, UpdateStatus, spawn_update_check};
