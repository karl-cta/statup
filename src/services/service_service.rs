//! Service rules: creation, edits, deletion and the status that open work
//! gives them.

use chrono::Utc;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::{CheckKind, Service, ServiceCheck, ServiceStatus, derive_status};
use crate::repositories::{EventRepository, ServiceRepository};

const MAX_NAME_CHARS: usize = 100;
const MAX_DESCRIPTION_CHARS: usize = 500;
const MAX_CHECK_TARGET_CHARS: usize = 2000;

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
            return Err(AppError::validation(key));
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
            return Err(AppError::validation(key));
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
            return Err(AppError::validation("validation.service_has_events"));
        }
        ServiceRepository::delete(pool, id).await?;
        Ok(())
    }

    /// Shows the worst of the state set by hand, the ones open work implies
    /// and the one the checks found, so closing an event falls back to what
    /// the team declared.
    pub async fn recalculate_status(pool: &DbPool, service_id: i64) -> Result<(), AppError> {
        let service = ServiceRepository::find_by_id(pool, service_id)
            .await?
            .ok_or(AppError::NotFound)?;
        let drivers = EventRepository::status_drivers(pool, service_id).await?;
        let worst = drivers
            .into_iter()
            .filter_map(|(kind, severity)| derive_status(kind, severity))
            .chain([service.manual_status])
            .chain(service.detected_status)
            .max_by_key(|status| status.priority())
            .unwrap_or(service.manual_status);
        ServiceRepository::update_status(pool, service_id, worst).await?;
        Ok(())
    }

    /// Records the state set by hand and shows what follows from it.
    pub async fn set_manual_status(
        pool: &DbPool,
        service_id: i64,
        status: ServiceStatus,
    ) -> Result<(), AppError> {
        ServiceRepository::update_manual_status(pool, service_id, status).await?;
        Self::recalculate_status(pool, service_id).await
    }

    /// Saves what to check; a changed check drops what the old one found,
    /// and the status follows.
    pub async fn set_check(
        pool: &DbPool,
        service_id: i64,
        check: Option<&ServiceCheck>,
    ) -> Result<(), AppError> {
        if ServiceRepository::set_check(pool, service_id, check, Utc::now()).await? {
            Self::recalculate_status(pool, service_id).await?;
        }
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

/// The one gate for what a check may reach, as a message key when refused.
/// A hosted instance will add its own limits here, such as private
/// addresses, and the probes stay as they are.
pub fn target_allowed(kind: CheckKind, target: &str) -> Result<(), &'static str> {
    match kind {
        CheckKind::Http => web_target_error(target),
        CheckKind::Tcp => port_target_error(target),
    }
    .map_or(Ok(()), Err)
}

/// A full `http` or `https` address, without credentials: Statup stores none.
fn web_target_error(target: &str) -> Option<&'static str> {
    if target.is_empty() {
        return Some("validation.check_url_required");
    }
    let Ok(url) = reqwest::Url::parse(target) else {
        return Some("validation.check_url_invalid");
    };
    if target.chars().count() > MAX_CHECK_TARGET_CHARS
        || !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
    {
        return Some("validation.check_url_invalid");
    }
    (!url.username().is_empty() || url.password().is_some())
        .then_some("validation.check_url_credentials")
}

/// A host and its port, an IPv6 address in brackets: `10.0.0.1:443`,
/// `nas.local:5000`, `[fd00::1]:22`.
fn port_target_error(target: &str) -> Option<&'static str> {
    if target.is_empty() {
        return Some("validation.check_address_required");
    }
    let valid = target.rsplit_once(':').is_some_and(|(host, port)| {
        let bracketed = host.starts_with('[') && host.ends_with(']');
        port.parse::<u16>().is_ok_and(|port| port > 0)
            && !host.is_empty()
            && (bracketed || !host.contains(':'))
            && !host.contains(|c: char| c.is_whitespace() || "/@?#".contains(c))
            && target.chars().count() <= MAX_CHECK_TARGET_CHARS
    });
    (!valid).then_some("validation.check_address_invalid")
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::test_pool;

    #[test]
    fn field_rules_count_characters() {
        assert!(service_field_error(&"é".repeat(100), None).is_none());
        assert!(service_field_error(&"a".repeat(101), None).is_some());
        assert!(service_field_error("  ", None).is_some());
        assert!(service_field_error("API", Some(&"x".repeat(501))).is_some());
    }

    #[test]
    fn web_checks_need_a_full_address_without_credentials() {
        let web = |target| target_allowed(CheckKind::Http, target);
        assert_eq!(web("https://intranet.example.com/health"), Ok(()));
        assert_eq!(web("http://10.0.0.5:8080"), Ok(()));
        assert_eq!(web(""), Err("validation.check_url_required"));
        for refused in [
            "intranet.example.com",
            "ftp://files.example.com",
            "https://",
            "not a url",
        ] {
            assert_eq!(
                web(refused),
                Err("validation.check_url_invalid"),
                "{refused}"
            );
        }
        assert_eq!(
            web("https://admin:secret@router.example.com"),
            Err("validation.check_url_credentials")
        );
        let long = format!("https://example.com/{}", "a".repeat(2000));
        assert_eq!(web(&long), Err("validation.check_url_invalid"));
    }

    #[test]
    fn port_checks_need_a_host_and_its_port() {
        let port = |target| target_allowed(CheckKind::Tcp, target);
        for allowed in ["192.168.1.1:443", "nas.local:5000", "[fd00::1]:22"] {
            assert_eq!(port(allowed), Ok(()), "{allowed}");
        }
        assert_eq!(port(""), Err("validation.check_address_required"));
        for refused in [
            "router",
            "router:",
            ":443",
            "router:0",
            "router:70000",
            "fd00::1:22",
            "my router:22",
            "https://router:443",
        ] {
            assert_eq!(
                port(refused),
                Err("validation.check_address_invalid"),
                "{refused}"
            );
        }
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
