//! `ValidatedForm` extractor, deserializes `application/x-www-form-urlencoded`
//! bodies using `serde_html_form` (which supports repeated keys → `Vec`) and
//! runs `validator::Validate` before handing the value to the handler.

use async_trait::async_trait;
use axum::extract::{FromRequest, Request};
use serde::de::DeserializeOwned;
use validator::Validate;

use crate::error::AppError;

/// Maximum form body size (1 MB).
const MAX_FORM_BODY: usize = 1024 * 1024;

/// Axum extractor that deserializes a form body **and** validates it.
///
/// Uses `serde_html_form` instead of `serde_urlencoded` so that repeated
/// field names (e.g. checkboxes with the same `name`) correctly deserialize
/// into `Vec<T>`.
pub struct ValidatedForm<T>(pub T);

#[async_trait]
impl<S, T> FromRequest<S> for ValidatedForm<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Validate,
{
    type Rejection = AppError;

    async fn from_request(req: Request, _state: &S) -> Result<Self, Self::Rejection> {
        let bytes = axum::body::to_bytes(req.into_body(), MAX_FORM_BODY)
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!("failed to read request body: {e}")))?;

        let value: T = serde_html_form::from_bytes(&bytes)
            .map_err(|_e| AppError::Validation("validation.invalid_form_data".to_string()))?;

        value.validate()?;
        Ok(Self(value))
    }
}
