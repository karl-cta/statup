//! Health check endpoint for monitoring and orchestration (Docker, load balancers).

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use crate::state::AppState;

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

///
/// No authentication required. Designed for Docker HEALTHCHECK, load balancer
/// probes, and uptime monitors.
pub async fn check(State(state): State<AppState>) -> Response {
    let db_ok = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.pool)
        .await
        .is_ok();

    let (status_code, body) = if db_ok {
        (StatusCode::OK, HealthResponse { status: "ok" })
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            HealthResponse { status: "degraded" },
        )
    };

    (status_code, axum::Json(body)).into_response()
}
