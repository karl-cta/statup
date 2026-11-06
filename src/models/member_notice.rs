//! What the app tells a publisher or an administrator under the bell of the
//! masthead.

use chrono::{DateTime, Utc};

/// What happened, which decides the words and the page a notice leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
pub enum NoticeKind {
    /// The update a member announced is late.
    UpdateOverdue,
    /// The checks found a service down.
    OutageDetected,
    /// A message to a destination could not be sent.
    DeliveryFailed,
}

impl NoticeKind {
    /// The hue of its square: a late update is the team's own, an outage
    /// and a refused message are problems.
    pub fn tone(self) -> &'static str {
        match self {
            Self::UpdateOverdue => "ink",
            Self::OutageDetected => "crit",
            Self::DeliveryFailed => "minor",
        }
    }
}

/// What a notice is about; only the one its kind needs is set.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoticeSubject {
    pub event_id: Option<i64>,
    pub service_id: Option<i64>,
    pub channel_id: Option<i64>,
    pub due_at: Option<DateTime<Utc>>,
    /// The services that fell with the one named.
    pub others: i64,
}

/// A notice as the bell lists it, with the names of what it is about.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MemberNotice {
    pub kind: NoticeKind,
    pub event_id: Option<i64>,
    pub service_id: Option<i64>,
    pub due_at: Option<DateTime<Utc>>,
    pub others: i64,
    pub created_at: DateTime<Utc>,
    pub read_at: Option<DateTime<Utc>>,
    pub event_title: Option<String>,
    pub service_name: Option<String>,
    pub channel_name: Option<String>,
}

impl MemberNotice {
    pub fn is_unread(&self) -> bool {
        self.read_at.is_none()
    }
}
