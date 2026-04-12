//! Service model, `ServiceStatus` enum, and slug generation.

use std::str::FromStr;

use crate::db::DbPool;
use crate::repositories::ServiceRepository;
use serde::Serialize;

/// Current operational status of a service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type, Serialize)]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
pub enum ServiceStatus {
    Operational,
    Degraded,
    PartialOutage,
    MajorOutage,
    Maintenance,
}

impl FromStr for ServiceStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "operational" => Ok(Self::Operational),
            "degraded" => Ok(Self::Degraded),
            "partial_outage" => Ok(Self::PartialOutage),
            "major_outage" => Ok(Self::MajorOutage),
            "maintenance" => Ok(Self::Maintenance),
            other => Err(format!("unknown service status: {other}")),
        }
    }
}

impl ServiceStatus {
    /// All possible statuses, for building UI selectors.
    pub const ALL: [Self; 5] = [
        Self::Operational,
        Self::Degraded,
        Self::PartialOutage,
        Self::MajorOutage,
        Self::Maintenance,
    ];

    /// `snake_case` string for form values and DB storage.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Operational => "operational",
            Self::Degraded => "degraded",
            Self::PartialOutage => "partial_outage",
            Self::MajorOutage => "major_outage",
            Self::Maintenance => "maintenance",
        }
    }

    /// Tailwind CSS class for the status color.
    pub fn css_class(self) -> &'static str {
        match self {
            Self::Operational => "text-status-operational",
            Self::Degraded => "text-status-degraded",
            Self::PartialOutage => "text-status-partial",
            Self::MajorOutage => "text-status-major",
            Self::Maintenance => "text-status-maintenance",
        }
    }

    /// Icon identifier for the status.
    pub fn icon(self) -> &'static str {
        match self {
            Self::Operational => "check-circle",
            Self::Degraded => "minus-circle",
            Self::PartialOutage => "exclamation-circle",
            Self::MajorOutage => "x-circle",
            Self::Maintenance => "wrench",
        }
    }

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Operational => "Opérationnel",
            Self::Degraded => "Dégradé",
            Self::PartialOutage => "Panne partielle",
            Self::MajorOutage => "Panne majeure",
            Self::Maintenance => "Maintenance",
        }
    }

    /// Translation key for i18n.
    pub fn i18n_key(self) -> &'static str {
        match self {
            Self::Operational => "status.service.operational",
            Self::Degraded => "status.service.degraded",
            Self::PartialOutage => "status.service.partial_outage",
            Self::MajorOutage => "status.service.major_outage",
            Self::Maintenance => "status.service.maintenance",
        }
    }

    /// Tailwind CSS background class for a small status dot.
    pub fn dot_class(self) -> &'static str {
        match self {
            Self::Operational => "bg-emerald-500",
            Self::Degraded => "bg-yellow-500",
            Self::PartialOutage => "bg-orange-500",
            Self::MajorOutage => "bg-red-500",
            Self::Maintenance => "bg-blue-500",
        }
    }

    /// CSS class for the left-side status strip on cards.
    pub fn strip_class(self) -> &'static str {
        match self {
            Self::Operational => "strip-operational",
            Self::Degraded => "strip-degraded",
            Self::PartialOutage => "strip-partial",
            Self::MajorOutage => "strip-major",
            Self::Maintenance => "strip-maintenance",
        }
    }

    /// Priority for determining the worst status (higher = worse).
    pub fn priority(self) -> u8 {
        match self {
            Self::Operational => 0,
            Self::Maintenance => 1,
            Self::Degraded => 2,
            Self::PartialOutage => 3,
            Self::MajorOutage => 4,
        }
    }
}

/// A monitored service.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Service {
    pub id: i64,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub status: ServiceStatus,
    pub icon_id: Option<i64>,
    pub icon_name: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// Icon filename from JOIN with icons table (not always populated).
    #[sqlx(default)]
    pub icon_filename: Option<String>,
}

impl Service {
    /// URL to the uploaded service icon (if any).
    pub fn icon_url(&self) -> Option<String> {
        self.icon_filename
            .as_ref()
            .map(|f| format!("/uploads/icons/{f}"))
    }

    /// SVG path data for the built-in icon (if `icon_name` is set and valid).
    /// Returns `|||`-separated paths when the icon has multiple `<path>` elements.
    pub fn builtin_icon_paths(&self) -> Option<&'static str> {
        self.icon_name
            .as_deref()
            .and_then(super::find_builtin_icon)
            .map(|i| i.paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operational_has_lowest_priority() {
        assert_eq!(ServiceStatus::Operational.priority(), 0);
    }

    #[test]
    fn major_outage_has_highest_priority() {
        let max = [
            ServiceStatus::Operational,
            ServiceStatus::Degraded,
            ServiceStatus::PartialOutage,
            ServiceStatus::MajorOutage,
            ServiceStatus::Maintenance,
        ]
        .iter()
        .map(|s| s.priority())
        .max()
        .unwrap();
        assert_eq!(max, ServiceStatus::MajorOutage.priority());
    }

    #[test]
    fn priority_ordering_matches_severity() {
        assert!(ServiceStatus::Operational.priority() < ServiceStatus::Degraded.priority());
        assert!(ServiceStatus::Degraded.priority() < ServiceStatus::PartialOutage.priority());
        assert!(ServiceStatus::PartialOutage.priority() < ServiceStatus::MajorOutage.priority());
    }

    #[test]
    fn all_statuses_have_non_empty_labels() {
        let statuses = [
            ServiceStatus::Operational,
            ServiceStatus::Degraded,
            ServiceStatus::PartialOutage,
            ServiceStatus::MajorOutage,
            ServiceStatus::Maintenance,
        ];
        for s in statuses {
            assert!(!s.label().is_empty());
            assert!(!s.css_class().is_empty());
            assert!(!s.icon().is_empty());
        }
    }
}

/// Generate a unique slug from a service name.
///
/// Uses the `slug` crate to create a URL-friendly string, then checks
/// the database for uniqueness. If the slug already exists, appends
/// `-2`, `-3`, etc. until a unique value is found.
///
/// # Errors
///
/// Returns `sqlx::Error` if a database query fails.
pub async fn generate_unique_slug(pool: &DbPool, name: &str) -> Result<String, sqlx::Error> {
    let base = slug::slugify(name);
    if ServiceRepository::find_by_slug(pool, &base)
        .await?
        .is_none()
    {
        return Ok(base);
    }

    let mut suffix = 2u32;
    loop {
        let candidate = format!("{base}-{suffix}");
        if ServiceRepository::find_by_slug(pool, &candidate)
            .await?
            .is_none()
        {
            return Ok(candidate);
        }
        suffix += 1;
    }
}
