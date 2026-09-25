//! Event template rules.

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::EventTemplate;
use crate::repositories::{CreateTemplateInput, EventTemplateRepository};
use crate::services::event_field_error;

pub struct EventTemplateService;

impl EventTemplateService {
    pub async fn create(
        pool: &DbPool,
        input: CreateTemplateInput<'_>,
    ) -> Result<EventTemplate, AppError> {
        let title = input.title.trim();
        if let Some(key) = event_field_error(title) {
            return Err(AppError::validation(key));
        }
        let input = CreateTemplateInput {
            title,
            description: input.description.trim(),
            ..input
        };
        Ok(EventTemplateRepository::create(pool, input).await?)
    }

    /// Counts a template once an event was published from it.
    pub async fn record_usage(pool: &DbPool, template_id: i64) -> Result<(), AppError> {
        EventTemplateRepository::increment_usage(pool, template_id).await?;
        Ok(())
    }

    pub async fn delete(pool: &DbPool, id: i64) -> Result<(), AppError> {
        EventTemplateRepository::find_by_id(pool, id)
            .await?
            .ok_or(AppError::NotFound)?;
        EventTemplateRepository::delete(pool, id).await?;
        Ok(())
    }
}
