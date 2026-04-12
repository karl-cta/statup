//! Event template model - reusable presets for recurring events.

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::{EventType, Impact};

/// A saved event template for quick event creation.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct EventTemplate {
    pub id: i64,
    pub title: String,
    pub description: String,
    pub event_type: EventType,
    pub impact: Impact,
    pub icon_id: Option<i64>,
    pub created_by: i64,
    pub usage_count: i64,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}
