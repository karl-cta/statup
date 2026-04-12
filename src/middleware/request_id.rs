//! Request ID middleware, generates a unique ID per request for traceability.

use std::fmt::Write;

use axum::body::Body;
use axum::http::Request;
use axum::http::header::HeaderValue;
use axum::middleware::Next;
use axum::response::Response;
use rand::Rng;

/// Key used to store the request ID in request extensions.
#[derive(Clone, Debug)]
pub struct RequestId(pub String);

/// Middleware that generates a request ID and injects it into both
/// the request extensions and the `tracing` span.
///
pub async fn request_id_middleware(mut req: Request<Body>, next: Next) -> Response {
    let id = generate_request_id();
    req.extensions_mut().insert(RequestId(id.clone()));

    let span = tracing::info_span!("request", request_id = %id);
    let _guard = span.enter();

    let mut response = next.run(req).await;
    // Hex strings are always valid ASCII, so `from_str` cannot fail here.
    if let Ok(val) = HeaderValue::from_str(&id) {
        response.headers_mut().insert("x-request-id", val);
    }
    response
}

/// Generate a short hex request ID (16 hex chars = 8 random bytes).
fn generate_request_id() -> String {
    let bytes: [u8; 8] = rand::thread_rng().r#gen();
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}
