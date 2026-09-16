//! Services and their status.

use std::str::FromStr;

use super::Tone;

/// Current operational status of a service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
pub enum ServiceStatus {
    Operational,
    Degraded,
    PartialOutage,
    MajorOutage,
    Maintenance,
}

impl FromStr for ServiceStatus {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|status| status.as_str() == s)
            .ok_or(())
    }
}

impl ServiceStatus {
    /// Every status, in the order the status menu lists them.
    pub const ALL: [Self; 5] = [
        Self::Operational,
        Self::Degraded,
        Self::PartialOutage,
        Self::MajorOutage,
        Self::Maintenance,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Operational => "operational",
            Self::Degraded => "degraded",
            Self::PartialOutage => "partial_outage",
            Self::MajorOutage => "major_outage",
            Self::Maintenance => "maintenance",
        }
    }

    pub fn i18n_key(self) -> &'static str {
        match self {
            Self::Operational => "status.service.operational",
            Self::Degraded => "status.service.degraded",
            Self::PartialOutage => "status.service.partial_outage",
            Self::MajorOutage => "status.service.major_outage",
            Self::Maintenance => "status.service.maintenance",
        }
    }

    pub fn tone(self) -> Tone {
        match self {
            Self::Operational => Tone::Ok,
            Self::Degraded => Tone::Minor,
            Self::PartialOutage => Tone::Major,
            Self::MajorOutage => Tone::Crit,
            Self::Maintenance => Tone::Info,
        }
    }

    pub fn is_operational(self) -> bool {
        self == Self::Operational
    }

    /// A tool that works badly or not at all, maintenance aside.
    pub fn is_disruption(self) -> bool {
        matches!(
            self,
            Self::Degraded | Self::PartialOutage | Self::MajorOutage
        )
    }

    /// Asks for a confirmation before it reaches every visitor.
    pub fn is_outage(self) -> bool {
        matches!(self, Self::PartialOutage | Self::MajorOutage)
    }

    /// Rank used to pick the worst status (higher is worse).
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
    /// File name of the uploaded icon, when the query joins it.
    #[sqlx(default)]
    pub icon_filename: Option<String>,
    /// Whether an event ever named this service, when the query asks.
    #[sqlx(default)]
    pub has_history: bool,
}

impl Service {
    pub fn icon_url(&self) -> Option<String> {
        self.icon_filename
            .as_ref()
            .map(|f| format!("/uploads/icons/{f}"))
    }

    /// SVG path data of the built-in icon, `|||` between paths.
    pub fn builtin_icon_paths(&self) -> Option<&'static str> {
        self.icon_name
            .as_deref()
            .and_then(super::find_builtin_icon)
            .map(|i| i.paths)
    }

    /// Fragment identifier of the service on the status page, stable across
    /// renames, so a link can point at one service.
    pub fn anchor(&self) -> String {
        format!("service-{}", self.slug)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_round_trip() {
        for status in ServiceStatus::ALL {
            assert_eq!(status.as_str().parse::<ServiceStatus>(), Ok(status));
        }
        assert!("broken".parse::<ServiceStatus>().is_err());
    }

    #[test]
    fn priority_ordering_matches_impact() {
        let order = [
            ServiceStatus::Operational,
            ServiceStatus::Maintenance,
            ServiceStatus::Degraded,
            ServiceStatus::PartialOutage,
            ServiceStatus::MajorOutage,
        ];
        assert!(order.windows(2).all(|w| w[0].priority() < w[1].priority()));
    }

    #[test]
    fn only_outages_ask_first() {
        assert!(ServiceStatus::MajorOutage.is_outage());
        assert!(ServiceStatus::PartialOutage.is_outage());
        assert!(!ServiceStatus::Degraded.is_outage());
    }
}
