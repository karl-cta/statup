//! CSRF protection middleware, session-based token generation and validation.
//!
//! Generates a random token per session and validates it on state-changing
//! requests (POST, PUT, DELETE). The token is checked from:
//! 1. `X-CSRF-Token` header (for HTMX / AJAX requests)
//! 2. `csrf_token` form field (for regular form submissions)

use async_trait::async_trait;
use axum::body::Body;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{Method, Request};
use axum::middleware::Next;
use axum::response::Response;
use rand::Rng;
use rand::distributions::Alphanumeric;
use tower_sessions::Session;

use crate::error::AppError;

/// Session key for the CSRF token.
const CSRF_SESSION_KEY: &str = "csrf_token";

/// Length of the generated CSRF token (alphanumeric characters).
const TOKEN_LENGTH: usize = 64;

/// Header name for CSRF token submission (used by HTMX/AJAX).
const CSRF_HEADER: &str = "x-csrf-token";

/// Form field name for CSRF token submission.
const CSRF_FORM_FIELD: &str = "csrf_token";

/// CSRF token injected into request extensions by the middleware.
///
/// Handlers extract this to include the token in templates.
#[derive(Clone, Debug)]
pub struct CsrfToken(pub String);

/// Axum extractor for the CSRF token.
///
/// Reads the token from request extensions (set by [`csrf_middleware`]).
/// Use this in GET handlers to pass the token to templates.
#[async_trait]
impl<S> FromRequestParts<S> for CsrfToken
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts.extensions.get::<CsrfToken>().cloned().ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "CsrfToken not found in extensions, is csrf_middleware installed?"
            ))
        })
    }
}

/// CSRF protection middleware.
///
/// On every request:
/// - Ensures a CSRF token exists in the session (generates one if absent).
/// - Injects the token into request extensions for handler access.
///
/// On state-changing methods (POST, PUT, DELETE):
/// - Validates the submitted token against the session token.
/// - Checks the `X-CSRF-Token` header first, then falls back to parsing
///   the `csrf_token` field from a `application/x-www-form-urlencoded` body.
/// - Returns 403 Forbidden if the token is missing or invalid.
///
/// # Errors
///
/// Returns `AppError::Forbidden` when the CSRF token is missing or invalid
/// on a state-changing request, or `AppError::Internal` on session errors.
pub async fn csrf_middleware(
    session: Session,
    request: Request<Body>,
    next: Next,
) -> Result<Response, AppError> {
    let token = ensure_token(&session).await?;
    let method = request.method().clone();

    if is_state_changing(&method) {
        let (parts, body, submitted) = extract_submitted_token(request).await?;
        validate_token(&token, submitted.as_deref())?;

        let mut request = Request::from_parts(parts, body);
        request.extensions_mut().insert(CsrfToken(token));
        Ok(next.run(request).await)
    } else {
        let mut request = request;
        request.extensions_mut().insert(CsrfToken(token));
        Ok(next.run(request).await)
    }
}

/// Ensure the session contains a CSRF token, creating one if absent.
async fn ensure_token(session: &Session) -> Result<String, AppError> {
    let existing: Option<String> = session
        .get(CSRF_SESSION_KEY)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("session read error: {e}")))?;

    if let Some(token) = existing {
        return Ok(token);
    }

    let token = generate_token();
    session
        .insert(CSRF_SESSION_KEY, &token)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("session write error: {e}")))?;

    Ok(token)
}

/// Generate a cryptographically random alphanumeric token.
fn generate_token() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(TOKEN_LENGTH)
        .map(char::from)
        .collect()
}

/// Returns `true` for methods that mutate server state.
fn is_state_changing(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::DELETE | Method::PATCH
    )
}

/// Extract the submitted CSRF token from the request.
///
/// Checks the `X-CSRF-Token` header first. If absent and the content type is
/// `application/x-www-form-urlencoded`, buffers the body and extracts the
/// `csrf_token` field. Returns the reconstructed request parts and body so
/// downstream handlers can still read the body.
async fn extract_submitted_token(
    request: Request<Body>,
) -> Result<(Parts, Body, Option<String>), AppError> {
    let (parts, body) = request.into_parts();

    // 1. Check header (preferred for HTMX/AJAX)
    let header_token = parts
        .headers
        .get(CSRF_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(ToOwned::to_owned);

    if header_token.is_some() {
        return Ok((parts, body, header_token));
    }

    // 2. For form-urlencoded bodies, parse the csrf_token field
    let is_form = parts
        .headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.starts_with("application/x-www-form-urlencoded"));

    if is_form {
        let bytes = axum::body::to_bytes(body, 1024 * 1024)
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!("failed to read request body: {e}")))?;

        let token = extract_field_from_form(&bytes);
        let body = Body::from(bytes);
        return Ok((parts, body, token));
    }

    // 3. Other content types, no token found
    Ok((parts, body, None))
}

/// Parse a `csrf_token` value from URL-encoded form bytes.
///
/// The token is alphanumeric, so no URL decoding is needed.
fn extract_field_from_form(bytes: &[u8]) -> Option<String> {
    let body = std::str::from_utf8(bytes).ok()?;
    body.split('&')
        .find_map(|pair| pair.strip_prefix(&format!("{CSRF_FORM_FIELD}=")))
        .map(ToOwned::to_owned)
}

/// Validate the submitted token against the session token.
///
/// Uses constant-time comparison to prevent timing side-channels.
fn validate_token(expected: &str, submitted: Option<&str>) -> Result<(), AppError> {
    let Some(submitted) = submitted else {
        tracing::warn!("CSRF token missing from request");
        return Err(AppError::Forbidden);
    };

    if !constant_time_eq(expected.as_bytes(), submitted.as_bytes()) {
        tracing::warn!("CSRF token mismatch");
        return Err(AppError::Forbidden);
    }

    Ok(())
}

/// Constant-time byte comparison to prevent timing attacks.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_token_has_correct_length() {
        let token = generate_token();
        assert_eq!(token.len(), TOKEN_LENGTH);
        assert!(token.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn constant_time_eq_works() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert!(!constant_time_eq(b"", b"a"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn extract_csrf_from_form_body() {
        let body = b"email=test%40example.com&csrf_token=abc123&password=secret";
        assert_eq!(extract_field_from_form(body), Some("abc123".to_owned()));
    }

    #[test]
    fn extract_csrf_missing_from_form_body() {
        let body = b"email=test%40example.com&password=secret";
        assert_eq!(extract_field_from_form(body), None);
    }

    #[test]
    fn state_changing_methods() {
        assert!(is_state_changing(&Method::POST));
        assert!(is_state_changing(&Method::PUT));
        assert!(is_state_changing(&Method::DELETE));
        assert!(is_state_changing(&Method::PATCH));
        assert!(!is_state_changing(&Method::GET));
        assert!(!is_state_changing(&Method::HEAD));
        assert!(!is_state_changing(&Method::OPTIONS));
    }
}
