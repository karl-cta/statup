//! CSRF protection: a random token per session, checked on every
//! state-changing request (POST, PUT, PATCH, DELETE).
//!
//! The token is read from the `X-CSRF-Token` header (htmx) or from the
//! `csrf_token` field of a URL-encoded or multipart body, so forms and file
//! uploads work without script. A visitor who is not signed in only gets a
//! session, and so a token, from a page that shows a form.

use async_trait::async_trait;
use axum::body::{Body, Bytes};
use axum::extract::FromRequestParts;
use axum::http::header::CONTENT_TYPE;
use axum::http::request::Parts;
use axum::http::{HeaderMap, Method, Request};
use axum::middleware::Next;
use axum::response::Response;
use rand::Rng;
use rand::distributions::Alphanumeric;
use tower_sessions::Session;

use super::body::buffer_body;
use crate::error::AppError;
use crate::session::{USER_ID_KEY, read_value, write_value};

/// Session key for the CSRF token.
const CSRF_SESSION_KEY: &str = "csrf_token";

/// Length of the generated CSRF token (alphanumeric characters).
const TOKEN_LENGTH: usize = 64;

/// Header name for CSRF token submission (used by HTMX/AJAX).
const CSRF_HEADER: &str = "x-csrf-token";

/// Form field name for CSRF token submission.
const CSRF_FORM_FIELD: &str = "csrf_token";

/// The session's CSRF token, for templates. Empty for a visitor who is not
/// signed in and has not been shown a form: extracting it never creates a
/// session for them. A signed-in session always gets one.
#[derive(Clone, Debug)]
pub struct CsrfToken(pub String);

/// The session's CSRF token, created with a session when missing. For the
/// pages that show a form to a visitor who is not signed in.
#[derive(Clone, Debug)]
pub struct FormCsrfToken(pub String);

#[async_trait]
impl<S> FromRequestParts<S> for CsrfToken
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        if let Some(token) = parts.extensions.get::<CsrfToken>() {
            return Ok(token.clone());
        }
        let session = request_session(parts, state).await?;
        if let Some(token) = stored_token(&session).await? {
            return Ok(Self(token));
        }
        if read_value::<i64>(&session, USER_ID_KEY).await?.is_some() {
            return renew_token(&session).await.map(Self);
        }
        Ok(Self(String::new()))
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for FormCsrfToken
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        if let Some(CsrfToken(token)) = parts.extensions.get::<CsrfToken>() {
            return Ok(Self(token.clone()));
        }
        let session = request_session(parts, state).await?;
        form_token(&session).await.map(Self)
    }
}

async fn request_session<S: Send + Sync>(
    parts: &mut Parts,
    state: &S,
) -> Result<Session, AppError> {
    Session::from_request_parts(parts, state)
        .await
        .map_err(|(_, message)| AppError::Internal(anyhow::anyhow!("no session layer: {message}")))
}

/// The session's token, created when missing.
///
/// # Errors
///
/// Returns `AppError::Internal` when the session store fails.
pub async fn form_token(session: &Session) -> Result<String, AppError> {
    match stored_token(session).await? {
        Some(token) => Ok(token),
        None => renew_token(session).await,
    }
}

/// Replaces the session's token, after a sign-in or a password change.
///
/// # Errors
///
/// Returns `AppError::Internal` when the session store fails.
pub async fn renew_token(session: &Session) -> Result<String, AppError> {
    let token = generate_token();
    write_value(session, CSRF_SESSION_KEY, &token).await?;
    Ok(token)
}

async fn stored_token(session: &Session) -> Result<Option<String>, AppError> {
    read_value(session, CSRF_SESSION_KEY).await
}

/// CSRF protection middleware.
///
/// A state-changing request is refused (403) unless the session holds a
/// token and the request carries the same one. The token, when the session
/// has one, is put in the request extensions for [`CsrfToken`]. Nothing is
/// written to the session here.
///
/// # Errors
///
/// Returns `AppError::Forbidden` when the token is missing or wrong,
/// `AppError::PayloadTooLarge` for a body past the route limit, and
/// `AppError::Internal` on session errors.
pub async fn csrf_middleware(
    session: Session,
    mut request: Request<Body>,
    next: Next,
) -> Result<Response, AppError> {
    let stored = stored_token(&session).await?;
    if !is_state_changing(request.method()) {
        if let Some(token) = stored {
            request.extensions_mut().insert(CsrfToken(token));
        }
        return Ok(next.run(request).await);
    }

    let Some(expected) = stored else {
        tracing::warn!("CSRF check failed: the session holds no token");
        return Err(AppError::Forbidden);
    };
    let (parts, body, submitted) = extract_submitted_token(request).await?;
    validate_token(&expected, submitted.as_deref())?;

    let mut request = Request::from_parts(parts, body);
    request.extensions_mut().insert(CsrfToken(expected));
    Ok(next.run(request).await)
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

/// How a form body carries its fields.
enum FormBody {
    UrlEncoded,
    Multipart(String),
}

impl FormBody {
    fn of(headers: &HeaderMap) -> Option<Self> {
        let content_type = headers.get(CONTENT_TYPE)?.to_str().ok()?;
        if content_type.starts_with("application/x-www-form-urlencoded") {
            return Some(Self::UrlEncoded);
        }
        multer::parse_boundary(content_type)
            .ok()
            .map(Self::Multipart)
    }
}

/// The submitted token: the header first, then the form field. A form body
/// is buffered to be read, then handed back so the handler can read it too.
async fn extract_submitted_token(
    request: Request<Body>,
) -> Result<(Parts, Body, Option<String>), AppError> {
    let (parts, body) = request.into_parts();
    if let Some(token) = header_token(&parts.headers) {
        return Ok((parts, body, Some(token)));
    }
    let Some(form) = FormBody::of(&parts.headers) else {
        return Ok((parts, body, None));
    };

    let bytes = buffer_body(body).await?;
    let token = match form {
        FormBody::UrlEncoded => extract_field_from_form(&bytes),
        FormBody::Multipart(boundary) => {
            extract_field_from_multipart(bytes.clone(), boundary).await
        }
    };
    Ok((parts, Body::from(bytes), token))
}

fn header_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(CSRF_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(ToOwned::to_owned)
}

/// Find the `csrf_token` part of a buffered multipart body.
async fn extract_field_from_multipart(bytes: Bytes, boundary: String) -> Option<String> {
    let stream = Body::from(bytes).into_data_stream();
    let mut multipart = multer::Multipart::new(stream, boundary);
    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some(CSRF_FORM_FIELD) {
            return field.text().await.ok();
        }
    }
    None
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
    use std::sync::Arc;

    use super::*;

    fn session() -> Session {
        Session::new(None, Arc::new(tower_sessions::MemoryStore::default()), None)
    }

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
    fn an_empty_session_token_never_matches() {
        assert!(validate_token("abc", Some("")).is_err());
        assert!(validate_token("abc", None).is_err());
        assert!(validate_token("abc", Some("abc")).is_ok());
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

    #[tokio::test]
    async fn extract_csrf_from_multipart_body() {
        let body = "--b\r\nContent-Disposition: form-data; name=\"csrf_token\"\r\n\r\nabc123\r\n\
                    --b\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.png\"\r\n\
                    Content-Type: image/png\r\n\r\nPNG\r\n--b--\r\n";
        let token = extract_field_from_multipart(Bytes::from(body), "b".to_owned()).await;
        assert_eq!(token, Some("abc123".to_owned()));
    }

    #[tokio::test]
    async fn extract_csrf_missing_from_multipart_body() {
        let body = "--b\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.png\"\r\n\
                    Content-Type: image/png\r\n\r\nPNG\r\n--b--\r\n";
        let token = extract_field_from_multipart(Bytes::from(body), "b".to_owned()).await;
        assert_eq!(token, None);
    }

    #[tokio::test]
    async fn a_form_token_is_kept_until_renewed() {
        let session = session();
        let first = form_token(&session).await.unwrap();
        assert_eq!(form_token(&session).await.unwrap(), first);

        let renewed = renew_token(&session).await.unwrap();
        assert_ne!(renewed, first);
        assert_eq!(form_token(&session).await.unwrap(), renewed);
    }

    #[tokio::test]
    async fn reading_the_token_does_not_touch_the_session() {
        let session = session();
        assert_eq!(stored_token(&session).await.unwrap(), None);
        assert!(!session.is_modified());
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
