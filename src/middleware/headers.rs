//! Headers: whether htmx sent a request, caching of static and uploaded
//! files, and the policy that confines uploaded files.

use axum::extract::Request;
use axum::http::header::{CACHE_CONTROL, CONTENT_SECURITY_POLICY};
use axum::http::{HeaderMap, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;

/// For files whose address changes with their content.
const IMMUTABLE: &str = "public, max-age=31536000, immutable";

/// For files served under a fixed address.
const REVALIDATE: &str = "public, max-age=300, must-revalidate";

/// An uploaded file opened on its own renders inert: no script, no request
/// to anywhere, only its inline styles.
const UPLOAD_CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; sandbox";

/// Pages carry a CSRF token and personal data, so no shared cache may
/// keep them and browsers ask again before reuse.
pub const DYNAMIC_CACHE_CONTROL: &str = "no-cache, private";

/// A request sent by htmx, which swaps the answer into the page it came
/// from instead of loading a new one.
pub fn is_htmx(headers: &HeaderMap) -> bool {
    headers.contains_key("hx-request")
}

/// Static files: cached for a year when the page links them with a version
/// (`?v=`), revalidated after five minutes otherwise. Errors are not cached.
pub async fn static_cache_control(request: Request, next: Next) -> Response {
    let versioned = request
        .uri()
        .query()
        .is_some_and(|query| query.split('&').any(|pair| pair.starts_with("v=")));
    let mut response = next.run(request).await;
    if response.status().is_success() {
        let policy = if versioned { IMMUTABLE } else { REVALIDATE };
        response
            .headers_mut()
            .insert(CACHE_CONTROL, HeaderValue::from_static(policy));
    }
    response
}

/// Uploaded files: sandboxed, and cached for a year since every upload gets
/// a new random name. Errors are not cached.
pub async fn uploaded_file_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let success = response.status().is_success();
    let headers = response.headers_mut();
    headers.insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(UPLOAD_CSP),
    );
    if success {
        headers.insert(CACHE_CONTROL, HeaderValue::from_static(IMMUTABLE));
    } else {
        headers.remove(CACHE_CONTROL);
    }
    response
}

/// No-store for a response that shows a secret or a password form.
pub fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
