//! Request body buffering within the limit the router set for the route.

use axum::body::{Body, Bytes};
use axum::extract::rejection::{BytesRejection, FailedToBufferBody};
use axum::extract::{FromRequest, Request};

use crate::error::AppError;

/// Reads a whole body. A body past the route limit is a 413, any other
/// read failure (a client that hangs up) a 400.
pub(crate) async fn buffer_body(body: Body) -> Result<Bytes, AppError> {
    Bytes::from_request(Request::new(body), &())
        .await
        .map_err(body_error)
}

fn body_error(rejection: BytesRejection) -> AppError {
    match rejection {
        BytesRejection::FailedToBufferBody(FailedToBufferBody::LengthLimitError(_)) => {
            AppError::PayloadTooLarge
        }
        other => {
            tracing::debug!(error = %other, "Request body could not be read");
            AppError::Validation("error.invalid_data".to_string())
        }
    }
}
