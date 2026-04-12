//! Application error types and conversions.

use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Not found")]
    NotFound,

    #[error("Unauthorized")]
    Unauthorized,

    #[error("Forbidden")]
    Forbidden,

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Database error")]
    Database(#[from] sqlx::Error),

    #[error("Internal error")]
    Internal(#[from] anyhow::Error),
}

impl From<validator::ValidationErrors> for AppError {
    fn from(errors: validator::ValidationErrors) -> Self {
        let message = errors
            .field_errors()
            .iter()
            .find_map(|(field, errs)| {
                errs.first().map(|e| {
                    e.message
                        .as_ref()
                        .map_or_else(|| format!("{field}: invalid"), ToString::to_string)
                })
            })
            .unwrap_or_else(|| "error.invalid_data".to_string());
        Self::Validation(message)
    }
}

impl AppError {
    fn status_and_message(&self) -> (StatusCode, &str) {
        match self {
            Self::NotFound => (StatusCode::NOT_FOUND, "Resource not found"),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "Please log in"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "Permission denied"),
            Self::Validation(msg) => (StatusCode::BAD_REQUEST, msg.as_str()),
            Self::Database(_) | Self::Internal(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
            }
        }
    }

    fn log(&self) {
        match self {
            Self::NotFound | Self::Validation(_) => {
                tracing::debug!("{self}");
            }
            Self::Unauthorized => {
                tracing::info!("Unauthorized access attempt");
            }
            Self::Forbidden => {
                tracing::warn!("Forbidden access attempt");
            }
            Self::Database(err) => {
                tracing::error!("Database error: {err:?}");
            }
            Self::Internal(err) => {
                tracing::error!("Internal error: {err:?}");
            }
        }
    }
}

/// Check if the request originates from HTMX (partial request).
fn is_htmx_request(headers: &HeaderMap) -> bool {
    headers.contains_key("hx-request")
}

/// Render an HTMX error fragment.
fn htmx_error_response(status: StatusCode, message: &str) -> Response {
    let html = format!(
        r#"<div id="error-message" class="alert alert-error" role="alert">{message}</div>"#
    );
    (status, Html(html)).into_response()
}

/// Render a full HTML error page.
fn html_error_response(status: StatusCode, message: &str) -> Response {
    let code = status.as_u16();
    let i18n = crate::i18n::I18n::default();
    let translated_message = i18n.t(message);
    let title = i18n.t("error.title");
    let back = i18n.t("error.back_home");
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="{lang}">
<head><meta charset="utf-8"><title>{title} {code}</title></head>
<body>
<div class="error-page">
    <h1>{code}</h1>
    <p>{translated_message}</p>
    <a href="/">{back}</a>
</div>
</body>
</html>"#,
        lang = i18n.locale(),
    );
    (status, Html(html)).into_response()
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        self.log();
        let (status, message) = self.status_and_message();

        // We don't have access to the request headers in IntoResponse directly.
        // Return a plain HTML error page for now; HTMX detection will be handled
        // once we have a middleware or extractor that captures the HX-Request header.
        html_error_response(status, message)
    }
}

/// Convert an `AppError` into a response, with HTMX-aware formatting.
///
/// Use this in handlers where you have access to request headers:
/// ```ignore
/// let response = error.into_response_for(&headers);
/// ```
impl AppError {
    pub fn into_response_for(self, headers: &HeaderMap) -> Response {
        self.log();
        let (status, message) = self.status_and_message();

        if is_htmx_request(headers) {
            htmx_error_response(status, message)
        } else {
            html_error_response(status, message)
        }
    }
}
