//! Form extractor: `application/x-www-form-urlencoded` bodies decoded with
//! `serde_html_form`, which turns repeated keys into a `Vec`.

use async_trait::async_trait;
use axum::extract::{FromRequest, Request};
use serde::de::DeserializeOwned;

use super::body::buffer_body;
use crate::error::AppError;

/// Axum extractor that decodes a form body without validating it. A body
/// that does not decode is a 400 with a generic message: handlers that
/// re-render a form give their fields `#[serde(default)]` and check them.
pub struct HtmlForm<T>(pub T);

#[async_trait]
impl<S, T> FromRequest<S> for HtmlForm<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = AppError;

    async fn from_request(req: Request, _state: &S) -> Result<Self, Self::Rejection> {
        decode_form(req).await.map(Self)
    }
}

async fn decode_form<T: DeserializeOwned>(req: Request) -> Result<T, AppError> {
    let bytes = buffer_body(req.into_body()).await?;
    serde_html_form::from_bytes(&bytes)
        .map_err(|_| AppError::validation("validation.invalid_form_data"))
}
