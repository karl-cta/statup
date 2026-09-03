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
mod dashboard_layout;
mod events;
mod health;
mod icons;
mod locale;
mod profile;
mod services;

use axum::Router;
use axum::http::header::HeaderName;
use axum::http::{HeaderValue, Response, StatusCode};
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
#[allow(clippy::too_many_lines)]
pub fn create_router(state: AppState) -> Router {
    let upload_dir = state.upload_dir.clone();
    let trust_proxy_headers = state.trust_proxy_headers;

    // Assets are served outside the rate limiter. One page view pulls six
    // requests (page, stylesheet, two scripts, font, texture), so counting
    // them left a budget of about sixteen page views per minute per IP.
    let assets = Router::new()
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
        );

    let routes = Router::new()
        // Public routes (no auth required)
        .route("/login", get(auth::login_form).post(auth::login))
        .route("/register", get(auth::register_form).post(auth::register))
        .route("/health", get(health::check))
        .route("/i18n", post(locale::switch))

        // Read-only routes (OptionalUser, public mode or authenticated)
        .route("/", get(dashboard::index))
        .route("/events", get(events::list))
        .route("/events/:id", get(events::detail))
        .route("/events/:id/drawer", get(events::drawer_content))
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
        .route("/icons/upload", post(icons::upload))
        .route("/icons/upload-picker", post(icons::upload_picker))
        .route("/icons/:id/delete", post(icons::delete))

        // Admin routes (RequireAdmin extractor)
        .route("/admin/settings", get(admin::settings_page))
        .route("/admin/settings/public-mode", post(admin::toggle_public_mode))
        .route("/admin/users", get(admin::users_list))
        .route("/admin/users/:id/role", post(admin::update_role))
        .route("/admin/users/:id/disable", post(admin::toggle_active))
        .route(
            "/admin/dashboard/:context/layout",
            get(dashboard_layout::layout_editor),
        )
        .route(
            "/admin/dashboard/:context/layout/order",
            post(dashboard_layout::save_order),
        )
        .route(
            "/admin/dashboard/:context/layout/:module_id/toggle",
            post(dashboard_layout::toggle_module),
        )

        // Profile routes (authenticated)
        .route("/profile", get(profile::edit_form).post(profile::update_profile))
        .route("/profile/password", post(profile::update_password))

        // Logout (authenticated)
        .route("/logout", post(auth::logout))

        .layer(middleware::from_fn(csrf_middleware));

    // 100 requests per minute per client, dynamic routes only. Behind a reverse
    // proxy every visitor shares the proxy address, which would turn a per
    // client limit into a site wide one, so the key extractor is configurable.
    let routes = if trust_proxy_headers {
        routes.layer(GovernorLayer {
            config: std::sync::Arc::new(proxied_rate_limit_config()),
        })
    } else {
        routes.layer(GovernorLayer {
            config: std::sync::Arc::new(direct_rate_limit_config()),
        })
    };

    routes
        .merge(assets)
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

/// 100 requests per minute, keyed on the address of the direct connection.
/// The default: correct when the app faces clients itself.
fn direct_rate_limit_config() -> tower_governor::governor::GovernorConfig<
    tower_governor::key_extractor::PeerIpKeyExtractor,
    governor::middleware::StateInformationMiddleware,
> {
    GovernorConfigBuilder::default()
        // 100 req/min → replenish 1 token every 600ms
        .per_millisecond(600)
        .burst_size(100)
        .use_headers()
        .error_handler(rate_limited_response)
        .finish()
        .expect("invalid rate limit configuration")
}

/// Same budget, keyed on the client address advertised by the proxy. Enabled by
/// `TRUST_PROXY_HEADERS`: without a proxy in front, those headers are attacker
/// controlled and every request would land in a different bucket.
fn proxied_rate_limit_config() -> tower_governor::governor::GovernorConfig<
    tower_governor::key_extractor::SmartIpKeyExtractor,
    governor::middleware::StateInformationMiddleware,
> {
    GovernorConfigBuilder::default()
        .per_millisecond(600)
        .burst_size(100)
        .use_headers()
        .key_extractor(tower_governor::key_extractor::SmartIpKeyExtractor)
        .error_handler(rate_limited_response)
        .finish()
        .expect("invalid rate limit configuration")
}

/// Response served when a client runs out of budget. The crate default is a bare
/// string that reads "Wait for 0s", because a slot frees in under a second and
/// the delay is printed as whole seconds. This says something true instead, and
/// stays self contained so it renders even if nothing else loads.
fn rate_limited_response(
    error: tower_governor::errors::GovernorError,
) -> Response<axum::body::Body> {
    let (status, wait_time, headers) = match error {
        tower_governor::errors::GovernorError::TooManyRequests { wait_time, headers } => {
            (StatusCode::TOO_MANY_REQUESTS, wait_time.max(1), headers)
        }
        tower_governor::errors::GovernorError::UnableToExtractKey => {
            (StatusCode::INTERNAL_SERVER_ERROR, 1, None)
        }
        tower_governor::errors::GovernorError::Other { code, headers, .. } => (code, 1, headers),
    };

    let body = format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="refresh" content="{wait_time}">
<title>Slow down | Statup</title>
<style>
  :root {{ color-scheme: light dark; }}
  body {{ margin: 0; min-height: 100vh; display: grid; place-items: center;
         background: #FFFFFF; color: #1C1B18; text-align: center; padding: 24px;
         font: 400 14px/1.6 -apple-system, BlinkMacSystemFont, system-ui, sans-serif; }}
  h1 {{ font-size: 20px; font-weight: 600; letter-spacing: -0.01em; margin: 0 0 8px; }}
  p {{ margin: 0; color: #5F5C52; max-width: 44ch; }}
  @media (prefers-color-scheme: dark) {{
    body {{ background: #1C1B18; color: #FAF8F2; }}
    p {{ color: #9C9689; }}
  }}
</style></head>
<body><main>
  <h1>Too many requests</h1>
  <p>This page is rate limited. It reloads on its own in {wait_time} second(s).</p>
</main></body></html>"#
    );

    let mut response = Response::new(axum::body::Body::from(body));
    *response.status_mut() = status;
    if let Some(headers) = headers {
        *response.headers_mut() = headers;
    }
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response
}
