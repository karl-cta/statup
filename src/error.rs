//! Application error types and the middleware that turns them into pages.

use askama::Template;
use axum::extract::Request;
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Response};

use crate::i18n::{I18n, Locale};
use crate::middleware::headers::is_htmx;
use crate::middleware::rate_limit::{RateLimited, rate_limited_page};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Not found")]
    NotFound,

    #[error("Unauthorized")]
    Unauthorized,

    #[error("Forbidden")]
    Forbidden,

    /// A form sent on a session that holds no form token: it expired, or
    /// the page was never opened here.
    #[error("Session expired")]
    SessionExpired,

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Request body too large")]
    PayloadTooLarge,

    /// Too many passwords are being checked at once.
    #[error("Server busy")]
    Busy,

    #[error("Database error")]
    Database(#[from] sqlx::Error),

    #[error("Internal error")]
    Internal(#[from] anyhow::Error),
}

impl AppError {
    /// A refusal shown to the person, by the translation key of its message.
    pub fn validation(key: &str) -> Self {
        Self::Validation(key.to_string())
    }

    /// The status and the translation key of the message shown to the person.
    fn status_and_key(&self) -> (StatusCode, &str) {
        match self {
            Self::NotFound => (StatusCode::NOT_FOUND, "error.not_found"),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "error.unauthorized"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "error.forbidden"),
            Self::SessionExpired => (StatusCode::UNAUTHORIZED, "error.session_expired"),
            Self::Validation(key) => (StatusCode::BAD_REQUEST, key.as_str()),
            Self::PayloadTooLarge => (StatusCode::PAYLOAD_TOO_LARGE, "error.payload_too_large"),
            Self::Busy => (StatusCode::SERVICE_UNAVAILABLE, "error.busy"),
            Self::Database(err) if is_foreign_key_violation(err) => {
                (StatusCode::BAD_REQUEST, "error.invalid_data")
            }
            Self::Database(_) | Self::Internal(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "error.internal")
            }
        }
    }

    fn log(&self) {
        match self {
            Self::NotFound | Self::Validation(_) | Self::PayloadTooLarge | Self::Busy => {
                tracing::debug!("{self}");
            }
            Self::Unauthorized => {
                tracing::info!("Unauthorized access attempt");
            }
            Self::SessionExpired => {
                tracing::info!("Form sent without a session token");
            }
            Self::Forbidden => {
                tracing::warn!("Forbidden access attempt");
            }
            Self::Database(err) if is_foreign_key_violation(err) => {
                tracing::warn!("Request named a row that does not exist: {err}");
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

/// What an error response carries until `render_error_pages` turns it into
/// a page in the visitor's language. `IntoResponse` has no access to the
/// request, so the rendering happens one layer up.
#[derive(Clone)]
struct ErrorPayload {
    status: StatusCode,
    key: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        self.log();
        let (status, key) = self.status_and_key();
        let payload = ErrorPayload {
            status,
            key: key.to_string(),
        };
        let mut response = (status, I18n::default().t(key).to_string()).into_response();
        response.extensions_mut().insert(payload);
        response
    }
}

#[derive(Template)]
#[template(path = "error.html")]
struct ErrorTemplate {
    csrf_token: String,
    instance: String,
    powered_by: bool,
    code: u16,
    message: String,
    back_href: &'static str,
    back_label: String,
    i18n: I18n,
}

#[derive(Template)]
#[template(path = "components/error_fragment.html")]
struct ErrorFragment {
    message: String,
}

/// Middleware that renders error responses in the language of the request:
/// every `AppError`, the bare statuses that layers answer on their own, and
/// the rate limit page. htmx gets a form banner instead of a page.
pub async fn render_error_pages(
    Locale(i18n): Locale,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    let response = next.run(request).await;
    if let Some(limited) = response.extensions().get::<RateLimited>() {
        return rate_limited_page(&i18n, limited.retry_after_secs);
    }
    let Some((status, key)) = error_to_render(&response) else {
        return response;
    };
    let message = i18n.t(&key).to_string();
    let rendered = if is_htmx(&headers) {
        ErrorFragment { message }.render()
    } else {
        error_page(status, message, i18n).render()
    };
    match rendered {
        Ok(html) => (status, Html(html)).into_response(),
        Err(e) => {
            tracing::error!("error page render failed: {e}");
            response
        }
    }
}

fn error_to_render(response: &Response) -> Option<(StatusCode, String)> {
    if let Some(payload) = response.extensions().get::<ErrorPayload>() {
        return Some((payload.status, payload.key.clone()));
    }
    bare_status_key(response.status()).map(|key| (response.status(), key.to_string()))
}

/// A form that names a service or an icon that no longer exists: the
/// request is wrong, the server is fine.
fn is_foreign_key_violation(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(db) if db.is_foreign_key_violation())
}

/// Statuses the timeout and body limit layers answer without a page.
fn bare_status_key(status: StatusCode) -> Option<&'static str> {
    match status {
        StatusCode::BAD_REQUEST => Some("error.invalid_data"),
        StatusCode::REQUEST_TIMEOUT => Some("error.timeout"),
        StatusCode::PAYLOAD_TOO_LARGE => Some("error.payload_too_large"),
        _ => None,
    }
}

fn error_page(status: StatusCode, message: String, i18n: I18n) -> ErrorTemplate {
    let (back_href, back_label) = if status == StatusCode::UNAUTHORIZED {
        ("/login", i18n.t("auth.submit_login").to_string())
    } else {
        ("/", i18n.t("error.back_home").to_string())
    };
    ErrorTemplate {
        csrf_token: String::new(),
        instance: crate::brand_name(),
        powered_by: !crate::instance_name().is_empty(),
        code: status.as_u16(),
        message,
        back_href,
        back_label,
        i18n,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_layer_statuses_are_rendered_without_a_payload() {
        assert_eq!(
            bare_status_key(StatusCode::PAYLOAD_TOO_LARGE),
            Some("error.payload_too_large")
        );
        assert_eq!(
            bare_status_key(StatusCode::REQUEST_TIMEOUT),
            Some("error.timeout")
        );
        assert_eq!(bare_status_key(StatusCode::NOT_FOUND), None);
    }
}
