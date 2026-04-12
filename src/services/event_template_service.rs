//! Event template service - business logic for event presets.

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::{EventTemplate, EventType, Impact};
use crate::repositories::EventTemplateRepository;

/// Business logic for event templates.
pub struct EventTemplateService;

impl EventTemplateService {
    /// Create a new event template.
    pub async fn create(
        pool: &DbPool,
        title: &str,
        description: &str,
        event_type: EventType,
        impact: Impact,
        icon_id: Option<i64>,
        created_by: i64,
    ) -> Result<EventTemplate, AppError> {
        let title = title.trim();
        if title.is_empty() {
            return Err(AppError::Validation(
                "validation.template_title_required".to_string(),
            ));
        }
        if title.len() > 200 {
            return Err(AppError::Validation(
                "validation.title_too_long".to_string(),
            ));
        }

        let tpl = EventTemplateRepository::create(
            pool,
            title,
            description.trim(),
            event_type,
            impact,
            icon_id,
            created_by,
        )
        .await?;

        Ok(tpl)
    }

    /// Record usage of a template (increment counter).
    pub async fn record_usage(pool: &DbPool, template_id: i64) -> Result<(), AppError> {
        EventTemplateRepository::increment_usage(pool, template_id).await?;
        Ok(())
    }

    /// Delete a template.
    pub async fn delete(pool: &DbPool, id: i64) -> Result<(), AppError> {
        EventTemplateRepository::find_by_id(pool, id)
            .await?
            .ok_or(AppError::NotFound)?;

        EventTemplateRepository::delete(pool, id).await?;
        Ok(())
    }
}
