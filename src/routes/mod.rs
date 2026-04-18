//! HTTP route handlers - Axum handlers for all endpoints.
//!
//! Routes are grouped by permission level. Each handler uses the appropriate
//! extractor (`AuthUser`, `RequirePublisher`, `RequireAdmin`) to enforce access
//! control at the handler level, no separate middleware layer needed.
//!
//! Permission mapping:
//! - **Public**: `/login`, `/register`, `/health`, no auth required
//! - **Authenticated** (`AuthUser`): `/`, `/events`, `/history`, `/search`
//! - **Publisher** (`RequirePublisher`): `POST /events`, `POST /services`
//! - **Admin** (`RequireAdmin`): `/admin/*`

mod admin;
mod auth;
mod dashboard;
mod events;
mod health;
mod icons;
mod profile;
mod services;

use axum::Router;
use axum::http::HeaderValue;
use axum::http::header::HeaderName;
use axum::middleware;
use axum::routing::{get, post};
use tower_governor::GovernorLayer;
use tower_governor::governor::GovernorConfigBuilder;
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::{NotForContentType, Predicate, SizeAbove};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;

use crate::middleware::csrf::csrf_middleware;
use crate::state::AppState;

// Sub-modules are accessed via qualified paths (e.g. auth::login) in the router.
// No wildcard re-exports to avoid name collisions between modules.

/// Builds the application router with all routes grouped by permission level.
///
/// Permission enforcement is done via Axum extractors in each handler:
/// - `OptionalUser`, allows guests in public mode, redirects otherwise
/// - `AuthUser`, redirects to `/login` if not authenticated
/// - `RequirePublisher`, returns 403 if user lacks publisher/admin role
/// - `RequireAdmin`, returns 403 if user lacks admin role
pub fn create_router(state: AppState) -> Router {
    let upload_dir = state.upload_dir.clone();
    Router::new()
        // Public routes (no auth required)
        .route("/login", get(auth::login_form).post(auth::login))
        .route("/register", get(auth::register_form).post(auth::register))
        .route("/health", get(health::check))

        // Read-only routes (OptionalUser, public mode or authenticated)
        .route("/", get(dashboard::index))
        .route("/events", get(events::list))
        .route("/events/:id", get(events::detail))
        .route("/events/:id/panel", get(events::detail_panel))
        .route("/history", get(events::history))
        .route("/search", get(events::search))

        // Publisher routes (RequirePublisher extractor)
        .route("/events/new", get(events::new_form).post(events::create))
        .route("/events/:id/edit", get(events::edit_form).post(events::update))
        .route("/events/:id/lifecycle", post(events::update_lifecycle))
        .route("/events/:id/revert-lifecycle", post(events::revert_lifecycle))
        .route("/events/:id/delete", post(events::delete))
        .route("/events/:id/updates", post(events::add_update))
        // Event template routes (RequirePublisher extractor)
        .route("/events/templates/search", get(events::template_search))
        .route("/events/templates/:id", get(events::template_detail))
        .route("/events/templates/:id/delete", post(events::template_delete))
        .route("/services", get(services::list))
        .route("/services/new", get(services::new_form).post(services::create))
        .route("/services/:id/edit", get(services::edit_form).post(services::update))
        .route("/services/:id/status", post(services::update_status))
        .route("/services/:id/delete", post(services::delete))

        // Icon routes (RequirePublisher extractor)
        .route("/icons", get(icons::list))
        .route("/icons/picker", get(icons::picker))
        .route("/icons/upload", post(icons::upload))
        .route("/icons/:id/delete", post(icons::delete))

        // Admin routes (RequireAdmin extractor)
        .route("/admin/users", get(admin::users_list))
        .route("/admin/users/:id/role", post(admin::update_role))
        .route("/admin/users/:id/disable", post(admin::toggle_active))
        .route("/admin/settings/public-mode", post(admin::toggle_public_mode))

        // Profile routes (authenticated)
        .route("/profile", get(profile::edit_form).post(profile::update_profile))
        .route("/profile/password", post(profile::update_password))

        // Logout (authenticated)
        .route("/logout", post(auth::logout))

        // Static files, short TTL + ETag revalidation (no hash in filenames yet)
        .nest_service(
            "/static",
            tower::ServiceBuilder::new()
                .layer(SetResponseHeaderLayer::if_not_present(
                    axum::http::header::CACHE_CONTROL,
                    axum::http::HeaderValue::from_static(
                        "public, max-age=300, must-revalidate",
                    ),
                ))
                .service(ServeDir::new("static").precompressed_gzip()),
        )
        // User-uploaded files with shorter cache
        .nest_service(
            "/uploads",
            tower::ServiceBuilder::new()
                .layer(SetResponseHeaderLayer::if_not_present(
                    axum::http::header::CACHE_CONTROL,
                    axum::http::HeaderValue::from_static("public, max-age=86400"),
                ))
                .service(ServeDir::new(upload_dir)),
        )
        .layer(middleware::from_fn(csrf_middleware))
        // Security headers
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_XSS_PROTECTION,
            HeaderValue::from_static("1; mode=block"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("content-security-policy"),
            HeaderValue::from_static(
                "default-src 'none'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self'; font-src 'self'; connect-src 'self'; form-action 'self'; base-uri 'self'; frame-ancestors 'none'"
            ),
        ))
        // Request body size limit: 1 MB
        .layer(RequestBodyLimitLayer::new(1024 * 1024))
        // Global rate limiting: 100 req/min per IP with X-RateLimit-* headers
        .layer(GovernorLayer {
            config: std::sync::Arc::new(rate_limit_config()),
        })
        // Gzip compression for responses > 1 KB
        .layer(
            CompressionLayer::new()
                .gzip(true)
                .compress_when(
                    SizeAbove::new(1024)
                        .and(NotForContentType::GRPC)
                        .and(NotForContentType::IMAGES)
                        .and(NotForContentType::SSE),
                ),
        )
        .with_state(state)
}

/// Build governor config: 100 requests per minute per IP, with rate-limit headers.
fn rate_limit_config() -> tower_governor::governor::GovernorConfig<
    tower_governor::key_extractor::PeerIpKeyExtractor,
    governor::middleware::StateInformationMiddleware,
> {
    GovernorConfigBuilder::default()
        // 100 req/min → replenish 1 token every 600ms
        .per_millisecond(600)
        .burst_size(100)
        .use_headers()
        .finish()
        .expect("invalid rate limit configuration")
}
