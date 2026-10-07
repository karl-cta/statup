//! Notification destinations and the messages queued for them.

use chrono::{DateTime, Utc};

use super::{Kind, Lifecycle};

/// The tool a destination posts to, which decides the message format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
pub enum ChannelKind {
    Teams,
    Slack,
    GoogleChat,
    Discord,
    Mattermost,
    Ntfy,
    Email,
    /// Any other tool that takes a JSON webhook.
    Webhook,
}

/// The part of the page a message belongs to, and so the box a destination
/// ticks to receive it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Audience {
    Incidents,
    Maintenances,
    Publications,
    DetectedOutages,
}

impl Audience {
    pub fn of(kind: Kind) -> Self {
        match kind {
            Kind::Incident => Self::Incidents,
            Kind::Maintenance => Self::Maintenances,
            Kind::Publication => Self::Publications,
        }
    }
}

// One box of the form per flag, one column each.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Channel {
    pub id: i64,
    pub name: String,
    pub kind: ChannelKind,
    /// The webhook address, or the addresses of an email destination.
    pub target: String,
    /// The language the messages are written in.
    pub locale: String,
    pub on_incidents: bool,
    pub on_maintenances: bool,
    pub on_publications: bool,
    pub on_detected: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A destination as its form sets it.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone)]
pub struct ChannelInput {
    pub name: String,
    pub kind: ChannelKind,
    pub target: String,
    pub locale: String,
    pub on_incidents: bool,
    pub on_maintenances: bool,
    pub on_publications: bool,
    pub on_detected: bool,
}

/// What a message says happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
pub enum Happening {
    /// An incident declared, or a maintenance announced ahead.
    Opened,
    /// A message posted, a state changed or a state change undone.
    Updated,
    /// The window of an announced maintenance moved.
    Rescheduled,
    /// A maintenance began.
    Started,
    /// An incident resolved or cancelled, a maintenance completed or
    /// cancelled.
    Closed,
    /// An announcement published.
    Published,
    /// The checks found a service down.
    ServiceDown,
    /// A service the checks found down answers again.
    ServiceUp,
    /// A test sent from the destination's settings.
    Test,
}

impl Happening {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Opened => "opened",
            Self::Updated => "updated",
            Self::Rescheduled => "rescheduled",
            Self::Started => "started",
            Self::Closed => "closed",
            Self::Published => "published",
            Self::ServiceDown => "service_down",
            Self::ServiceUp => "service_up",
            Self::Test => "test",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
pub enum DeliveryStatus {
    Pending,
    Sent,
    Failed,
}

/// Something to tell the destinations that receive its part of the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Notification {
    pub happening: Happening,
    pub audience: Audience,
    pub event_id: Option<i64>,
    pub update_id: Option<i64>,
    /// The state the event moved to, when it moved.
    pub lifecycle: Option<Lifecycle>,
    pub service_id: Option<i64>,
}

/// One message for one destination.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Delivery {
    pub id: i64,
    pub channel_id: i64,
    pub happening: Happening,
    pub event_id: Option<i64>,
    pub update_id: Option<i64>,
    pub lifecycle: Option<Lifecycle>,
    pub service_id: Option<i64>,
    pub status: DeliveryStatus,
    pub attempts: i64,
    pub next_attempt_at: DateTime<Utc>,
    /// Why the last attempt failed, as a code the page words.
    pub failure: Option<String>,
    pub created_at: DateTime<Utc>,
    pub sent_at: Option<DateTime<Utc>>,
}
