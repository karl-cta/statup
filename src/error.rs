//! Application error types and conversions.

use askama::Template;
use axum::extract::Request;
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Response};

use crate::i18n::{I18n, Locale};

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
    /// The status and the translation key of the message shown to the person.
    fn status_and_key(&self) -> (StatusCode, &str) {
        match self {
            Self::NotFound => (StatusCode::NOT_FOUND, "error.not_found"),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "error.unauthorized"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "error.forbidden"),
            Self::Validation(key) => (StatusCode::BAD_REQUEST, key.as_str()),
            Self::Database(_) | Self::Internal(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "error.internal")
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

/// Middleware that renders every `AppError` as a page in the language of
/// the request, or as a form banner when htmx asked for a fragment.
pub async fn render_error_pages(
    Locale(i18n): Locale,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    let response = next.run(request).await;
    let Some(payload) = response.extensions().get::<ErrorPayload>().cloned() else {
        return response;
    };
    let message = i18n.t(&payload.key).to_string();
    let rendered = if headers.contains_key("hx-request") {
        ErrorFragment { message }.render()
    } else {
        error_page(payload.status, message, i18n).render()
    };
    match rendered {
        Ok(html) => (payload.status, Html(html)).into_response(),
        Err(e) => {
            tracing::error!("error page render failed: {e}");
            response
        }
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
