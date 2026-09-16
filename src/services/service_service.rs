//! Service rules: creation, edits, deletion and the status that open work
//! gives them.

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::{Kind, Service, ServiceStatus, Severity};
use crate::repositories::{EventRepository, ServiceRepository};

const MAX_NAME_CHARS: usize = 100;
const MAX_DESCRIPTION_CHARS: usize = 500;

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
        if let Some(key) = service_field_error(name, description) {
            return Err(AppError::Validation(key.to_string()));
        }
        let slug = unique_slug(pool, name).await?;
        let description = clean_description(description);
        Ok(ServiceRepository::create(pool, name, &slug, description, icon_id, icon_name).await?)
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
        if let Some(key) = service_field_error(name, description) {
            return Err(AppError::Validation(key.to_string()));
        }
        ServiceRepository::find_by_id(pool, id)
            .await?
            .ok_or(AppError::NotFound)?;
        let description = clean_description(description);
        ServiceRepository::update(pool, id, name, description, icon_id, icon_name).await?;
        Ok(())
    }

    /// A service with history cannot be deleted: its past incidents would
    /// lose their subject.
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

    /// Sets the service to the worst status its open work implies, or back
    /// to operational when nothing is open.
    pub async fn recalculate_status(pool: &DbPool, service_id: i64) -> Result<(), AppError> {
        let drivers = EventRepository::status_drivers(pool, service_id).await?;
        let worst = drivers
            .into_iter()
            .filter_map(|(kind, severity)| derive_status(kind, severity))
            .max_by_key(|status| status.priority())
            .unwrap_or(ServiceStatus::Operational);
        ServiceRepository::update_status(pool, service_id, worst).await?;
        Ok(())
    }

    pub async fn recalculate_many(pool: &DbPool, service_ids: &[i64]) -> Result<(), AppError> {
        for id in service_ids {
            Self::recalculate_status(pool, *id).await?;
        }
        Ok(())
    }
}

/// Name and description rules as a message key, so a route can re-render
/// the form with what the author typed.
pub fn service_field_error(name: &str, description: Option<&str>) -> Option<&'static str> {
    let name = name.trim();
    if name.is_empty() {
        return Some("validation.service_name_required");
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Some("validation.service_name_too_long");
    }
    if description.is_some_and(|d| d.trim().chars().count() > MAX_DESCRIPTION_CHARS) {
        return Some("validation.description_max_length");
    }
    None
}

fn clean_description(description: Option<&str>) -> Option<&str> {
    description.map(str::trim).filter(|d| !d.is_empty())
}

/// A URL-safe slug that no other service uses: `-2`, `-3`... are appended
/// when the plain one is taken.
async fn unique_slug(pool: &DbPool, name: &str) -> Result<String, AppError> {
    let base = match slug::slugify(name) {
        s if s.is_empty() => "service".to_string(),
        s => s,
    };
    if !ServiceRepository::slug_exists(pool, &base).await? {
        return Ok(base);
    }
    for suffix in 2..=10_000u32 {
        let candidate = format!("{base}-{suffix}");
        if !ServiceRepository::slug_exists(pool, &candidate).await? {
            return Ok(candidate);
        }
    }
    Err(AppError::Internal(anyhow::anyhow!(
        "no free slug for {base}"
    )))
}

/// Status an open event gives its services. Maintenance under way sets
/// Maintenance; an incident follows its severity, minor when none was given.
fn derive_status(kind: Kind, severity: Option<Severity>) -> Option<ServiceStatus> {
    match kind {
        Kind::Incident => Some(match severity {
            Some(Severity::Critical) => ServiceStatus::MajorOutage,
            Some(Severity::Major) => ServiceStatus::PartialOutage,
            Some(Severity::Minor) | None => ServiceStatus::Degraded,
        }),
        Kind::Maintenance => Some(ServiceStatus::Maintenance),
        Kind::Publication => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::test_pool;

    #[test]
    fn incidents_follow_their_severity() {
        assert_eq!(
            derive_status(Kind::Incident, Some(Severity::Critical)),
            Some(ServiceStatus::MajorOutage)
        );
        assert_eq!(
            derive_status(Kind::Incident, Some(Severity::Major)),
            Some(ServiceStatus::PartialOutage)
        );
        assert_eq!(
            derive_status(Kind::Incident, Some(Severity::Minor)),
            Some(ServiceStatus::Degraded)
        );
    }

    #[test]
    fn an_incident_without_severity_still_counts() {
        assert_eq!(
            derive_status(Kind::Incident, None),
            Some(ServiceStatus::Degraded)
        );
    }

    #[test]
    fn maintenance_and_announcements() {
        assert_eq!(
            derive_status(Kind::Maintenance, Some(Severity::Critical)),
            Some(ServiceStatus::Maintenance)
        );
        assert_eq!(derive_status(Kind::Publication, None), None);
    }

    #[test]
    fn field_rules_count_characters() {
        assert!(service_field_error(&"é".repeat(100), None).is_none());
        assert!(service_field_error(&"a".repeat(101), None).is_some());
        assert!(service_field_error("  ", None).is_some());
        assert!(service_field_error("API", Some(&"x".repeat(501))).is_some());
    }

    #[tokio::test]
    async fn slugs_stay_unique() {
        let pool = test_pool().await;
        let first = ServiceService::create(&pool, "Paie", None, None, None)
            .await
            .unwrap();
        let second = ServiceService::create(&pool, "Paie", None, None, None)
            .await
            .unwrap();
        assert_eq!(first.slug, "paie");
        assert_eq!(second.slug, "paie-2");
        let symbols = ServiceService::create(&pool, "***", None, None, None)
            .await
            .unwrap();
        assert_eq!(symbols.slug, "service");
    }
}
