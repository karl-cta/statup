//! Service management - CRUD operations, status recalculation.

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::{self, EventType, Impact, Service, ServiceStatus};
use crate::repositories::{EventRepository, ServiceRepository};

/// Business logic for service CRUD and status computation.
pub struct ServiceService;

impl ServiceService {
    /// Create a new service with a unique slug.
    pub async fn create(
        pool: &DbPool,
        name: &str,
        description: Option<&str>,
        icon_id: Option<i64>,
        icon_name: Option<&str>,
    ) -> Result<Service, AppError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(AppError::Validation(
                "validation.service_name_required".to_string(),
            ));
        }
        if name.len() > 100 {
            return Err(AppError::Validation(
                "validation.service_name_too_long".to_string(),
            ));
        }

        let description = description.map(str::trim).filter(|d| !d.is_empty());

        let slug = models::generate_unique_slug(pool, name).await?;

        let service =
            ServiceRepository::create_with_icon(pool, name, &slug, description, icon_id, icon_name)
                .await?;

        Ok(service)
    }

    /// Update an existing service's name, description, and icon.
    pub async fn update(
        pool: &DbPool,
        id: i64,
        name: &str,
        description: Option<&str>,
        icon_id: Option<i64>,
        icon_name: Option<&str>,
    ) -> Result<(), AppError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(AppError::Validation(
                "validation.service_name_required".to_string(),
            ));
        }
        if name.len() > 100 {
            return Err(AppError::Validation(
                "validation.service_name_too_long".to_string(),
            ));
        }

        let description = description.map(str::trim).filter(|d| !d.is_empty());

        ServiceRepository::find_by_id(pool, id)
            .await?
            .ok_or(AppError::NotFound)?;

        ServiceRepository::update(pool, id, name, description, icon_id, icon_name).await?;

        Ok(())
    }

    /// Delete a service, refusing if it has associated events.
    pub async fn delete(pool: &DbPool, id: i64) -> Result<(), AppError> {
        ServiceRepository::find_by_id(pool, id)
            .await?
            .ok_or(AppError::NotFound)?;

        if ServiceRepository::has_events(pool, id).await? {
            return Err(AppError::Validation(
                "validation.service_has_events".to_string(),
            ));
        }

        ServiceRepository::delete(pool, id).await?;
        Ok(())
    }

    /// Recalculate a service's status from its active events.
    ///
    /// Determines the worst status by mapping each active event's impact
    /// and type to a `ServiceStatus`, then keeping the one with the highest
    /// priority. If no active events remain, the service becomes Operational.
    pub async fn recalculate_status(
        pool: &DbPool,
        service_id: i64,
    ) -> Result<ServiceStatus, AppError> {
        let active_events = EventRepository::list_active_for_service(pool, service_id).await?;

        let worst = active_events
            .iter()
            .filter_map(|event| derive_service_status(event.event_type, event.impact))
            .max_by_key(|s| s.priority())
            .unwrap_or(ServiceStatus::Operational);

        ServiceRepository::update_status(pool, service_id, worst).await?;

        Ok(worst)
    }
}

/// Map an event type + impact to the corresponding service status.
///
/// Returns `None` for event types that don't affect service status
/// (changelog, info).
fn derive_service_status(event_type: EventType, impact: Impact) -> Option<ServiceStatus> {
    match event_type {
        EventType::Incident => match impact {
            Impact::Critical => Some(ServiceStatus::MajorOutage),
            Impact::Major => Some(ServiceStatus::PartialOutage),
            Impact::Minor => Some(ServiceStatus::Degraded),
            Impact::None => None,
        },
        EventType::MaintenanceScheduled | EventType::MaintenanceUrgent => {
            Some(ServiceStatus::Maintenance)
        }
        EventType::Changelog | EventType::Info => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn critical_incident_maps_to_major_outage() {
        assert_eq!(
            derive_service_status(EventType::Incident, Impact::Critical),
            Some(ServiceStatus::MajorOutage)
        );
    }

    #[test]
    fn major_incident_maps_to_partial_outage() {
        assert_eq!(
            derive_service_status(EventType::Incident, Impact::Major),
            Some(ServiceStatus::PartialOutage)
        );
    }

    #[test]
    fn minor_incident_maps_to_degraded() {
        assert_eq!(
            derive_service_status(EventType::Incident, Impact::Minor),
            Some(ServiceStatus::Degraded)
        );
    }

    #[test]
    fn no_impact_incident_returns_none() {
        assert_eq!(
            derive_service_status(EventType::Incident, Impact::None),
            None
        );
    }

    #[test]
    fn scheduled_maintenance_maps_to_maintenance() {
        assert_eq!(
            derive_service_status(EventType::MaintenanceScheduled, Impact::None),
            Some(ServiceStatus::Maintenance)
        );
    }

    #[test]
    fn urgent_maintenance_maps_to_maintenance() {
        assert_eq!(
            derive_service_status(EventType::MaintenanceUrgent, Impact::Critical),
            Some(ServiceStatus::Maintenance)
        );
    }

    #[test]
    fn changelog_does_not_affect_status() {
        assert_eq!(
            derive_service_status(EventType::Changelog, Impact::Major),
            None
        );
    }

    #[test]
    fn info_does_not_affect_status() {
        assert_eq!(derive_service_status(EventType::Info, Impact::Minor), None);
    }
}
