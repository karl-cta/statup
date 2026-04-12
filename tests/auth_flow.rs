//! Integration tests for the authentication flow.
//!
//! Tests the full HTTP cycle: Register → Login → Access protected → Logout.
//! Also tests permission denial and error cases.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use reqwest::StatusCode;
use reqwest::redirect::Policy;
use time::Duration;
use tower_sessions::cookie::SameSite;
use tower_sessions::{Expiry, SessionManagerLayer};
use tower_sessions_sqlx_store::SqliteStore;

use statup::db;
use statup::models::Role;
use statup::repositories::UserRepository;
use statup::routes::create_router;
use statup::services::{AuthService, LoginRateLimiter};
use statup::state::AppState;

// ---------------------------------------------------------------------------
// Test application helper
// ---------------------------------------------------------------------------

struct TestApp {
    addr: SocketAddr,
    client: reqwest::Client,
    pool: sqlx::SqlitePool,
}

impl TestApp {
    /// Spawn the full Statup application on a random local port.
    async fn spawn() -> Self {
        Self::spawn_with_options(false).await
    }

    /// Spawn the application with public mode enabled.
    async fn spawn_public() -> Self {
        Self::spawn_with_options(true).await
    }

    async fn spawn_with_options(public_mode: bool) -> Self {
        let pool = db::create_pool("sqlite::memory:", 1)
            .await
            .expect("failed to create test pool");
        db::run_migrations(&pool)
            .await
            .expect("failed to run migrations");

        let session_store = SqliteStore::new(pool.clone());
        session_store
            .migrate()
            .await
            .expect("failed to migrate session store");

        let session_layer = SessionManagerLayer::new(session_store)
            .with_secure(false)
            .with_same_site(SameSite::Lax)
            .with_http_only(true)
            .with_expiry(Expiry::OnInactivity(Duration::seconds(3600)));

        let upload_dir = std::env::temp_dir()
            .join("statup-test-uploads")
            .to_string_lossy()
            .to_string();
        std::fs::create_dir_all(format!("{upload_dir}/icons")).ok();

        let state = AppState {
            pool: pool.clone(),
            login_limiter: Arc::new(LoginRateLimiter::default()),
            upload_dir,
            public_mode: Arc::new(AtomicBool::new(public_mode)),
        };

        let app = create_router(state).layer(session_layer);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("failed to bind test listener");
        let addr = listener.local_addr().expect("failed to get local addr");

        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .expect("server error");
        });

        let client = reqwest::Client::builder()
            .cookie_store(true)
            .redirect(Policy::none())
            .build()
            .expect("failed to build reqwest client");

        Self { addr, client, pool }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    /// GET a page and return (status, body text).
    async fn get(&self, path: &str) -> (StatusCode, String) {
        let resp = self
            .client
            .get(self.url(path))
            .send()
            .await
            .expect("GET request failed");
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        (status, body)
    }

    /// POST a form with CSRF token extracted from a prior GET response body.
    async fn post_form(
        &self,
        path: &str,
        csrf_token: &str,
        fields: &[(&str, &str)],
    ) -> (StatusCode, String, Option<String>) {
        let mut form: Vec<(&str, &str)> = vec![("csrf_token", csrf_token)];
        form.extend_from_slice(fields);

        let resp = self
            .client
            .post(self.url(path))
            .form(&form)
            .send()
            .await
            .expect("POST request failed");

        let status = resp.status();
        let location = resp
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .map(ToOwned::to_owned);
        let body = resp.text().await.unwrap_or_default();
        (status, body, location)
    }

    /// Register a user via the HTTP form flow. Returns the CSRF token for reuse.
    async fn register_user(&self, email: &str, password: &str, display_name: &str) -> String {
        let (status, body) = self.get("/register").await;
        assert_eq!(status, StatusCode::OK);
        let csrf = extract_csrf_token(&body);

        let (status, _body, location) = self
            .post_form(
                "/register",
                &csrf,
                &[
                    ("email", email),
                    ("password", password),
                    ("password_confirm", password),
                    ("display_name", display_name),
                ],
            )
            .await;

        assert_eq!(status, StatusCode::SEE_OTHER, "register should redirect");
        assert_eq!(location.as_deref(), Some("/login"));
        csrf
    }

    /// Login a user via the HTTP form flow. Panics on failure.
    async fn login_user(&self, email: &str, password: &str) {
        let (status, body) = self.get("/login").await;
        assert_eq!(status, StatusCode::OK);
        let csrf = extract_csrf_token(&body);

        let (status, _body, location) = self
            .post_form("/login", &csrf, &[("email", email), ("password", password)])
            .await;

        assert_eq!(status, StatusCode::SEE_OTHER, "login should redirect");
        assert_eq!(location.as_deref(), Some("/"));
    }
}

/// Extract the CSRF token from an HTML response body.
fn extract_csrf_token(html: &str) -> String {
    let marker = r#"name="csrf_token" value=""#;
    let start = html
        .find(marker)
        .unwrap_or_else(|| panic!("csrf_token not found in HTML"))
        + marker.len();
    let end = html[start..]
        .find('"')
        .unwrap_or_else(|| panic!("closing quote for csrf_token not found"))
        + start;
    html[start..end].to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn register_then_login_then_protected_then_logout() {
    let app = TestApp::spawn().await;

    // 1. Register
    app.register_user("alice@example.com", "secure_password_123", "Alice")
        .await;

    // 2. Login
    app.login_user("alice@example.com", "secure_password_123")
        .await;

    // 3. Access protected route (dashboard)
    let (status, body) = app.get("/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("Alice") || body.contains("alice"),
        "dashboard should show user info"
    );

    // 4. Logout
    let (_, csrf_body) = app.get("/").await;
    let csrf = extract_csrf_token(&csrf_body);
    let (status, _body, location) = app.post_form("/logout", &csrf, &[]).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/login"));

    // 5. After logout, protected route should redirect to /login
    let (status, _body) = app.get("/").await;
    assert_eq!(
        status,
        StatusCode::SEE_OTHER,
        "should redirect to login after logout"
    );
}

#[tokio::test]
async fn register_password_mismatch() {
    let app = TestApp::spawn().await;

    let (_, body) = app.get("/register").await;
    let csrf = extract_csrf_token(&body);

    let (status, body, location) = app
        .post_form(
            "/register",
            &csrf,
            &[
                ("email", "bob@example.com"),
                ("password", "secure_password_123"),
                ("password_confirm", "different_password_456"),
                ("display_name", "Bob"),
            ],
        )
        .await;

    assert_eq!(status, StatusCode::OK, "should re-render form on error");
    assert!(location.is_none());
    assert!(
        body.contains("ne correspondent pas"),
        "should show password mismatch error"
    );
}

#[tokio::test]
async fn login_wrong_password() {
    let app = TestApp::spawn().await;

    // Register first
    app.register_user("charlie@example.com", "correct_password_12", "Charlie")
        .await;

    // Try login with wrong password
    let (_, body) = app.get("/login").await;
    let csrf = extract_csrf_token(&body);

    let (status, body, _location) = app
        .post_form(
            "/login",
            &csrf,
            &[
                ("email", "charlie@example.com"),
                ("password", "wrong_password_12"),
            ],
        )
        .await;

    assert_eq!(status, StatusCode::OK, "should re-render login form");
    assert!(
        body.contains("incorrect"),
        "should show invalid credentials error"
    );
}

#[tokio::test]
async fn protected_route_without_auth_redirects() {
    let app = TestApp::spawn().await;

    // All these routes should redirect unauthenticated users to /login
    let routes = ["/", "/events", "/history", "/search"];
    for route in routes {
        let (status, _body) = app.get(route).await;
        assert_eq!(
            status,
            StatusCode::SEE_OTHER,
            "GET {route} should redirect to /login without auth"
        );
    }
}

#[tokio::test]
async fn reader_cannot_access_publisher_routes() {
    let app = TestApp::spawn().await;

    // Register and login as a reader (default role)
    app.register_user("reader@example.com", "reader_password_12", "Reader")
        .await;
    app.login_user("reader@example.com", "reader_password_12")
        .await;

    // GET publisher routes should return 401 (extractor chain: AuthUser OK, RequirePublisher fails → Unauthorized)
    let publisher_get_routes = ["/events/new", "/services", "/services/new"];
    for route in publisher_get_routes {
        let (status, _body) = app.get(route).await;
        assert!(
            status == StatusCode::FORBIDDEN || status == StatusCode::UNAUTHORIZED,
            "GET {route} as reader should be 401 or 403, got {status}"
        );
    }
}

#[tokio::test]
async fn reader_cannot_access_admin_routes() {
    let app = TestApp::spawn().await;

    app.register_user("viewer@example.com", "viewer_password_12", "Viewer")
        .await;
    app.login_user("viewer@example.com", "viewer_password_12")
        .await;

    let (status, _body) = app.get("/admin/users").await;
    assert!(
        status == StatusCode::FORBIDDEN || status == StatusCode::UNAUTHORIZED,
        "GET /admin/users as reader should be 401 or 403, got {status}"
    );
}

#[tokio::test]
async fn publisher_can_access_publisher_routes() {
    let app = TestApp::spawn().await;

    // Register, then promote to publisher via DB
    app.register_user("pub@example.com", "publisher_pass_12", "Publisher")
        .await;
    let user = UserRepository::find_by_email(&app.pool, "pub@example.com")
        .await
        .expect("db error")
        .expect("user not found");
    UserRepository::update_role(&app.pool, user.id, Role::Publisher)
        .await
        .expect("failed to update role");

    app.login_user("pub@example.com", "publisher_pass_12").await;

    let (status, _body) = app.get("/events/new").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "publisher should access /events/new"
    );
}

#[tokio::test]
async fn post_without_csrf_token_is_rejected() {
    let app = TestApp::spawn().await;

    // POST /login without CSRF token should be rejected
    let resp = app
        .client
        .post(app.url("/login"))
        .form(&[("email", "a@b.com"), ("password", "test")])
        .send()
        .await
        .expect("request failed");

    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "POST without CSRF should be 403"
    );
}

#[tokio::test]
async fn register_duplicate_email() {
    let app = TestApp::spawn().await;

    app.register_user("dup@example.com", "password_12345678", "First")
        .await;

    // Try to register again with the same email
    let (_, body) = app.get("/register").await;
    let csrf = extract_csrf_token(&body);

    let (status, body, _location) = app
        .post_form(
            "/register",
            &csrf,
            &[
                ("email", "dup@example.com"),
                ("password", "password_12345678"),
                ("password_confirm", "password_12345678"),
                ("display_name", "Second"),
            ],
        )
        .await;

    assert_eq!(
        status,
        StatusCode::OK,
        "should re-render form on duplicate email"
    );
    assert!(
        body.contains("déjà utilisée"),
        "should show duplicate email error"
    );
}

#[tokio::test]
async fn register_password_too_short() {
    let app = TestApp::spawn().await;

    let (_, body) = app.get("/register").await;
    let csrf = extract_csrf_token(&body);

    let (status, body, _location) = app
        .post_form(
            "/register",
            &csrf,
            &[
                ("email", "short@example.com"),
                ("password", "short"),
                ("password_confirm", "short"),
                ("display_name", "Short"),
            ],
        )
        .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body.contains("12 caractères") || body.contains("12 caract"),
        "should show password length error, got: {body}"
    );
}

#[tokio::test]
async fn health_check_is_public() {
    let app = TestApp::spawn().await;

    let (status, body) = app.get("/health").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("ok") || body.contains("healthy"));
}

#[tokio::test]
async fn login_form_is_public() {
    let app = TestApp::spawn().await;

    let (status, body) = app.get("/login").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("csrf_token"),
        "login form should contain CSRF token"
    );
    assert!(
        body.contains("Connexion"),
        "login form should contain login title"
    );
}

#[tokio::test]
async fn admin_can_access_admin_routes() {
    let app = TestApp::spawn().await;

    // Register, promote to admin, login
    app.register_user("admin@example.com", "admin_password_12", "Admin")
        .await;
    let user = UserRepository::find_by_email(&app.pool, "admin@example.com")
        .await
        .expect("db error")
        .expect("user not found");
    UserRepository::update_role(&app.pool, user.id, Role::Admin)
        .await
        .expect("failed to update role");

    app.login_user("admin@example.com", "admin_password_12")
        .await;

    let (status, _body) = app.get("/admin/users").await;
    assert_eq!(status, StatusCode::OK, "admin should access /admin/users");
}

#[tokio::test]
async fn disabled_user_session_is_rejected() {
    let app = TestApp::spawn().await;

    app.register_user("disabled@example.com", "disabled_pass_12", "Disabled")
        .await;
    app.login_user("disabled@example.com", "disabled_pass_12")
        .await;

    // Verify access works before disabling
    let (status, _body) = app.get("/").await;
    assert_eq!(status, StatusCode::OK);

    // Disable the user via DB while they have an active session
    let user = UserRepository::find_by_email(&app.pool, "disabled@example.com")
        .await
        .expect("db error")
        .expect("user not found");
    UserRepository::set_active(&app.pool, user.id, false)
        .await
        .expect("failed to disable user");

    // The AuthUser extractor checks is_active, disabled user should be rejected
    let (status, _body) = app.get("/").await;
    assert_eq!(
        status,
        StatusCode::SEE_OTHER,
        "disabled user should be redirected to login"
    );
}

#[tokio::test]
async fn public_mode_register_requires_admin() {
    let app = TestApp::spawn_public().await;

    // Unauthenticated user should not be able to access /register
    let (status, _body) = app.get("/register").await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "GET /register in public mode without auth should be 401"
    );
}

#[tokio::test]
async fn public_mode_reader_cannot_register() {
    let app = TestApp::spawn_public().await;

    // Create a reader user directly in DB
    AuthService::register(
        &app.pool,
        "reader@example.com",
        "reader_pass_1234",
        "Reader",
    )
    .await
    .expect("failed to create user");

    app.login_user("reader@example.com", "reader_pass_1234")
        .await;

    // Reader should be forbidden from /register in public mode
    let (status, _body) = app.get("/register").await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "GET /register in public mode as reader should be 403"
    );
}

#[tokio::test]
async fn public_mode_admin_can_register_users() {
    let app = TestApp::spawn_public().await;

    // Create an admin user directly
    AuthService::register(&app.pool, "admin@example.com", "admin_pass_12345", "Admin")
        .await
        .expect("failed to create user");
    let user = UserRepository::find_by_email(&app.pool, "admin@example.com")
        .await
        .expect("db error")
        .expect("user not found");
    UserRepository::update_role(&app.pool, user.id, Role::Admin)
        .await
        .expect("failed to update role");

    app.login_user("admin@example.com", "admin_pass_12345")
        .await;

    // Admin should be able to access /register in public mode
    let (status, body) = app.get("/register").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "GET /register in public mode as admin should be 200"
    );
    assert!(
        body.contains("csrf_token"),
        "register form should contain CSRF token"
    );
}

#[tokio::test]
async fn public_mode_read_routes_accessible_without_auth() {
    let app = TestApp::spawn_public().await;

    // In public mode, read-only routes should be accessible
    let read_routes = ["/", "/events", "/history", "/search"];
    for route in read_routes {
        let (status, _body) = app.get(route).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "GET {route} in public mode should be accessible without auth"
        );
    }
}
