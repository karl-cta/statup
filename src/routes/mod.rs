//! HTTP routes. Access is enforced by each handler's extractor:
//! `OptionalUser` (public mode or signed in), `AuthUser`, `RequirePublisher`
//! or `RequireAdmin`. Every dynamic route goes through the rate limit, the
//! error pages, the CSRF check and the signed-in user lookup; files and the
//! health check skip them.

mod admin;
mod auth;
mod dashboard;
mod dashboard_layout;
mod events;
mod feed;
mod health;
mod icons;
mod locale;
mod page;
mod password;
mod profile;
mod services;
mod timeline;

use std::any::Any;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::header::{
    CACHE_CONTROL, CONTENT_SECURITY_POLICY, REFERRER_POLICY, STRICT_TRANSPORT_SECURITY,
    X_CONTENT_TYPE_OPTIONS, X_FRAME_OPTIONS,
};
use axum::http::{HeaderName, HeaderValue, Request};
use axum::middleware;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::classify::{ServerErrorsAsFailures, SharedClassifier};
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::{NotForContentType, Predicate, SizeAbove};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::{DefaultOnResponse, TraceLayer};
use tracing::{Level, Span};

use crate::error::{AppError, render_error_pages};
use crate::middleware::csrf::csrf_middleware;
use crate::middleware::headers::{
    DYNAMIC_CACHE_CONTROL, static_cache_control, uploaded_file_headers,
};
use crate::middleware::load_session_user;
use crate::middleware::rate_limit::RateLimit;
use crate::state::AppState;

pub(crate) use page::{Frame, members_only, render};

/// Forms carry text only.
const FORM_BODY_LIMIT: usize = 64 * 1024;

/// An icon file (256 KB after checks) plus the multipart envelope, with room
/// for the handler to say the file is too large.
const UPLOAD_BODY_LIMIT: usize = 2 * 1024 * 1024;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// No inline script, handler or style anywhere: behavior lives in
/// `static/js`, presentation in the stylesheet.
const APP_CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; font-src 'self'; connect-src 'self'; form-action 'self'; base-uri 'self'; frame-ancestors 'none'";

const PERMISSIONS_POLICY: &str = "camera=(), microphone=(), geolocation=(), interest-cohort=()";

/// Builds the application router. The session layer is added by the caller.
pub fn create_router(state: AppState, rate_limit: &RateLimit) -> Router {
    let serves_https = state.serves_https();
    let files = static_files()
        .layer(overriding(CONTENT_SECURITY_POLICY, APP_CSP))
        .merge(uploaded_files(&state.upload_dir));
    let app = dynamic_routes(&state, rate_limit)
        .layer(overriding(CONTENT_SECURITY_POLICY, APP_CSP))
        .merge(files)
        .layer(access_log())
        .route("/health", get(health::check));
    with_security_headers(app, serves_https)
        .layer(compression())
        .with_state(state)
}

/// Pages and form posts, behind the rate limit and the request guards.
fn dynamic_routes(state: &AppState, rate_limit: &RateLimit) -> Router<AppState> {
    let pages = with_request_guards(page_routes(), state)
        .layer(RequestBodyLimitLayer::new(FORM_BODY_LIMIT));
    let uploads = with_request_guards(upload_routes(), state)
        .layer(RequestBodyLimitLayer::new(UPLOAD_BODY_LIMIT));
    pages
        .merge(uploads)
        .fallback(not_found)
        .layer(TimeoutLayer::new(REQUEST_TIMEOUT))
        .layer(CatchPanicLayer::custom(|panic: Box<dyn Any + Send>| {
            panic_response(&*panic)
        }))
        .layer(rate_limit.layer())
        .layer(middleware::from_fn(render_error_pages))
        .layer(SetResponseHeaderLayer::if_not_present(
            CACHE_CONTROL,
            HeaderValue::from_static(DYNAMIC_CACHE_CONTROL),
        ))
}

/// Inside the body limit: the CSRF check reads form bodies, then the
/// signed-in user is loaded once for the handler.
fn with_request_guards(routes: Router<AppState>, state: &AppState) -> Router<AppState> {
    routes
        .layer(middleware::from_fn_with_state(
            state.pool.clone(),
            load_session_user,
        ))
        .layer(middleware::from_fn(csrf_middleware))
}

fn page_routes() -> Router<AppState> {
    public_routes()
        .merge(publisher_routes())
        .merge(admin_routes())
        .merge(account_routes())
}

/// Reachable without an account; the read pages check the public mode.
fn public_routes() -> Router<AppState> {
    Router::new()
        .route("/login", get(auth::login_form).post(auth::login))
        .route("/register", get(auth::register_form).post(auth::register))
        .route("/logout", post(auth::logout))
        .route("/i18n", get(locale::switch))
        .route("/", get(dashboard::index))
        .route("/events", get(events::list))
        .route("/events/:id", get(events::detail))
        .route("/events/:id/drawer", get(events::drawer_content))
        .route("/services/:id/drawer", get(services::drawer_content))
        .route("/history", get(|| async { Redirect::permanent("/events") }))
        .route("/search", get(events::search))
        .route("/feed", get(feed::atom))
        .route("/subscribe", get(dashboard::subscribe))
}

/// Events, templates, services and icons, for publishers and admins.
fn publisher_routes() -> Router<AppState> {
    Router::new()
        .route("/events/new", get(events::new_form).post(events::create))
        .route(
            "/events/:id/edit",
            get(events::edit_form).post(events::update),
        )
        .route(
            "/events/:id/revert-lifecycle",
            post(events::revert_lifecycle),
        )
        .route("/events/:id/delete", post(events::delete))
        .route("/events/:id/updates", post(events::add_update))
        .route(
            "/events/:id/panel-updates",
            post(events::add_update_in_panel),
        )
        .route(
            "/events/:id/updates/:update_id/delete",
            post(events::delete_update),
        )
        .route("/events/templates/search", get(events::template_search))
        .route("/events/templates/:id", get(events::template_detail))
        .route(
            "/events/templates/:id/delete",
            post(events::template_delete),
        )
        .route("/services", get(services::list))
        .route(
            "/services/new",
            get(services::new_form).post(services::create),
        )
        .route(
            "/services/:id/edit",
            get(services::edit_form).post(services::update),
        )
        .route("/services/:id/status", post(services::update_status))
        .route("/services/:id/delete", post(services::delete))
        .route("/icons", get(icons::list))
        .route("/icons/:id/delete", post(icons::delete))
}

/// The only routes that accept a file.
fn upload_routes() -> Router<AppState> {
    Router::new()
        .route("/icons/upload", post(icons::upload))
        .route("/icons/upload-picker", post(icons::upload_picker))
        .route("/admin/settings/logo", post(admin::update_logo))
}

fn admin_routes() -> Router<AppState> {
    Router::new()
        .route("/admin/settings", get(admin::settings_page))
        .route(
            "/admin/settings/public-mode",
            post(admin::set_public_access),
        )
        .route(
            "/admin/settings/instance-name",
            post(admin::update_instance_name),
        )
        .route("/admin/settings/time-zone", post(admin::update_time_zone))
        .route("/admin/settings/logo/remove", post(admin::remove_logo))
        .route("/admin/users", get(admin::users_list))
        .route("/admin/users/new", post(admin::add_member))
        .route("/admin/users/:id/role", post(admin::update_role))
        .route("/admin/users/:id/disable", post(admin::toggle_active))
        .route(
            "/admin/users/:id/reset-password",
            post(admin::reset_member_password),
        )
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
        .route(
            "/admin/dashboard/:context/layout/:module_id/width",
            post(dashboard_layout::set_width),
        )
}

/// A signed-in person's own account.
fn account_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/profile",
            get(profile::edit_form).post(profile::update_profile),
        )
        .route("/profile/password", post(profile::update_password))
        .route("/password/new", get(password::form).post(password::update))
}

/// Built files, outside the rate limit: one page view pulls about six.
fn static_files() -> Router<AppState> {
    Router::new()
        .nest_service("/static", ServeDir::new("static"))
        .layer(middleware::from_fn(static_cache_control))
}

/// Uploaded icons and the logo only, never the rest of the upload
/// directory, which may sit next to the database.
fn uploaded_files(upload_dir: &str) -> Router<AppState> {
    let icons_dir = std::path::Path::new(upload_dir).join("icons");
    Router::new()
        .nest_service("/uploads/icons", ServeDir::new(icons_dir))
        .nest_service(
            "/uploads/brand",
            ServeDir::new(crate::services::logo_dir(upload_dir)),
        )
        .layer(middleware::from_fn(uploaded_file_headers))
}

async fn not_found() -> AppError {
    AppError::NotFound
}

/// A panicking handler costs its own request, with the usual error page.
fn panic_response(panic: &(dyn Any + Send)) -> Response {
    let message = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .unwrap_or("no message");
    AppError::Internal(anyhow::anyhow!("handler panicked: {message}")).into_response()
}

type RequestSpan = fn(&Request<Body>) -> Span;
type AccessLog =
    TraceLayer<SharedClassifier<ServerErrorsAsFailures>, RequestSpan, (), DefaultOnResponse>;

/// One line per request with method, path, status and latency. The query
/// string is left out: it can hold what a visitor searched for.
fn access_log() -> AccessLog {
    TraceLayer::new_for_http()
        .make_span_with(request_span as RequestSpan)
        .on_request(())
        .on_response(DefaultOnResponse::new().level(Level::INFO))
}

fn request_span(request: &Request<Body>) -> Span {
    tracing::info_span!(
        "request",
        method = %request.method(),
        path = %request.uri().path(),
    )
}

fn with_security_headers(router: Router<AppState>, serves_https: bool) -> Router<AppState> {
    let router = router
        .layer(overriding(X_CONTENT_TYPE_OPTIONS, "nosniff"))
        .layer(overriding(X_FRAME_OPTIONS, "DENY"))
        .layer(overriding(
            REFERRER_POLICY,
            "strict-origin-when-cross-origin",
        ))
        .layer(overriding(
            HeaderName::from_static("permissions-policy"),
            PERMISSIONS_POLICY,
        ))
        .layer(overriding(
            HeaderName::from_static("cross-origin-opener-policy"),
            "same-origin",
        ));
    if serves_https {
        router.layer(overriding(STRICT_TRANSPORT_SECURITY, "max-age=31536000"))
    } else {
        router
    }
}

fn overriding(name: HeaderName, value: &'static str) -> SetResponseHeaderLayer<HeaderValue> {
    SetResponseHeaderLayer::overriding(name, HeaderValue::from_static(value))
}

/// Gzip for text. Fonts and raster images are compressed already.
fn compression() -> CompressionLayer<impl Predicate> {
    CompressionLayer::new().gzip(true).compress_when(
        SizeAbove::new(1024)
            .and(NotForContentType::GRPC)
            .and(NotForContentType::IMAGES)
            .and(NotForContentType::SSE)
            .and(NotForContentType::const_new("font/")),
    )
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use super::*;

    #[test]
    fn a_panic_becomes_an_error_page() {
        for panic in [
            Box::new("boom") as Box<dyn Any + Send>,
            Box::new(String::from("boom")),
            Box::new(42),
        ] {
            let response = panic_response(&*panic);
            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }
    }
}
