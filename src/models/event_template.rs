//! Event template: reusable preset for quick event creation.

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::{Category, Kind, Severity};

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct EventTemplate {
    pub id: i64,
    pub title: String,
    pub description: String,
    pub kind: Kind,
    pub severity: Option<Severity>,
    pub planned: bool,
    pub category: Option<Category>,
    pub icon_id: Option<i64>,
    pub created_by: i64,
    pub usage_count: i64,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}
