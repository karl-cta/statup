//! Event template service: business logic for event presets.

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::{Category, EventTemplate, Kind, Severity};
use crate::repositories::{CreateTemplateInput, EventTemplateRepository};

pub struct EventTemplateService;

pub struct CreateTemplateParams<'a> {
    pub title: &'a str,
    pub description: &'a str,
    pub kind: Kind,
    pub severity: Option<Severity>,
    pub planned: bool,
    pub category: Option<Category>,
    pub icon_id: Option<i64>,
    pub created_by: i64,
}

impl EventTemplateService {
    pub async fn create(
        pool: &DbPool,
        params: CreateTemplateParams<'_>,
    ) -> Result<EventTemplate, AppError> {
        let title = params.title.trim();
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
            CreateTemplateInput {
                title,
                description: params.description.trim(),
                kind: params.kind,
                severity: params.severity,
                planned: params.planned,
                category: params.category,
                icon_id: params.icon_id,
                created_by: params.created_by,
            },
        )
        .await?;

        Ok(tpl)
    }

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
