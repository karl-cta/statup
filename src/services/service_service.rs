//! Service management, CRUD et recalcul du statut à partir des événements actifs.

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::{self, Kind, Service, ServiceStatus, Severity};
use crate::repositories::{EventRepository, ServiceRepository};

pub struct ServiceService;

impl ServiceService {
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

    /// Recalcule le statut d'un service à partir de ses événements actifs. On
    /// prend le pire statut projeté (priorité max). Sans événement actif, le
    /// service redevient opérationnel.
    pub async fn recalculate_status(
        pool: &DbPool,
        service_id: i64,
    ) -> Result<ServiceStatus, AppError> {
        let active_events = EventRepository::list_active_for_service(pool, service_id).await?;

        let worst = active_events
            .iter()
            .filter_map(|event| derive_service_status(event.kind, event.severity))
            .max_by_key(|s| s.priority())
            .unwrap_or(ServiceStatus::Operational);

        ServiceRepository::update_status(pool, service_id, worst).await?;

        Ok(worst)
    }
}

/// Projette un événement actif sur un statut de service. Les publications
/// n'affectent pas le statut. Une maintenance (planifiée ou subie) force
/// `Maintenance`. Un incident est projeté selon sa sévérité, ou ignoré si
/// aucune n'est renseignée.
fn derive_service_status(kind: Kind, severity: Option<Severity>) -> Option<ServiceStatus> {
    match kind {
        Kind::Incident => severity.map(|s| match s {
            Severity::Critical => ServiceStatus::MajorOutage,
            Severity::Major => ServiceStatus::PartialOutage,
            Severity::Minor => ServiceStatus::Degraded,
        }),
        Kind::Maintenance => Some(ServiceStatus::Maintenance),
        Kind::Publication => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn critical_incident_maps_to_major_outage() {
        assert_eq!(
            derive_service_status(Kind::Incident, Some(Severity::Critical)),
            Some(ServiceStatus::MajorOutage)
        );
    }

    #[test]
    fn major_incident_maps_to_partial_outage() {
        assert_eq!(
            derive_service_status(Kind::Incident, Some(Severity::Major)),
            Some(ServiceStatus::PartialOutage)
        );
    }

    #[test]
    fn minor_incident_maps_to_degraded() {
        assert_eq!(
            derive_service_status(Kind::Incident, Some(Severity::Minor)),
            Some(ServiceStatus::Degraded)
        );
    }

    #[test]
    fn incident_without_severity_returns_none() {
        assert_eq!(derive_service_status(Kind::Incident, None), None);
    }

    #[test]
    fn maintenance_maps_to_maintenance_status() {
        assert_eq!(
            derive_service_status(Kind::Maintenance, None),
            Some(ServiceStatus::Maintenance)
        );
        assert_eq!(
            derive_service_status(Kind::Maintenance, Some(Severity::Critical)),
            Some(ServiceStatus::Maintenance)
        );
    }

    #[test]
    fn publication_does_not_affect_status() {
        assert_eq!(
            derive_service_status(Kind::Publication, Some(Severity::Major)),
            None
        );
        assert_eq!(derive_service_status(Kind::Publication, None), None);
    }
}
