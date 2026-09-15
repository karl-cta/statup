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
            trust_proxy_headers: false,
            public_url: None,
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

    /// Create an account through the form. The person is signed in on
    /// success, so the helper lands on the dashboard. Returns the CSRF token.
    async fn create_account(&self, email: &str, password: &str, display_name: &str) -> String {
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
        assert_eq!(location.as_deref(), Some("/"));
        csrf
    }

    /// Create an account, then sign out, for tests that exercise the
    /// sign-in form themselves.
    async fn register_user(&self, email: &str, password: &str, display_name: &str) -> String {
        let csrf = self.create_account(email, password, display_name).await;
        let (status, _body, location) = self.post_form("/logout", &csrf, &[]).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "logout should redirect");
        assert_eq!(location.as_deref(), Some("/login"));
        csrf
    }

    /// Seed an administrator directly, so that the next account created
    /// through the form is an ordinary reader rather than the first account.
    async fn seed_admin(&self) {
        AuthService::register(
            &self.pool,
            "owner@example.com",
            "owner_password_12",
            "Owner",
            Role::Admin,
        )
        .await
        .expect("failed to seed admin");
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
    let routes = ["/", "/events", "/search"];
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

    // Register and login as a reader (the role every account after the first gets)
    app.seed_admin().await;
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

    app.seed_admin().await;
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

    assert_eq!(status, StatusCode::OK, "should re-render the form");
    assert!(
        body.contains(r#"id="password-error""#) && body.contains("12 caract"),
        "should show the password length error under the field"
    );
    assert!(
        body.contains("short@example.com") && body.contains(r#"value="Short""#),
        "should keep the other fields filled in"
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
async fn public_mode_register_is_closed_for_visitors() {
    let app = TestApp::spawn_public().await;
    app.seed_admin().await;

    let resp = app
        .client
        .get(app.url("/register"))
        .send()
        .await
        .expect("GET /register failed");
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        resp.headers().get("location").and_then(|v| v.to_str().ok()),
        Some("/login")
    );

    let (_, body) = app.get("/login").await;
    assert!(
        !body.contains(r#"href="/register""#),
        "the sign-in page must not offer a closed door"
    );
}

#[tokio::test]
async fn public_mode_fresh_instance_offers_the_first_account() {
    let app = TestApp::spawn_public().await;

    let (status, body) = app.get("/register").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("administrateur"));

    let (_, body) = app.get("/login").await;
    assert!(body.contains(r#"href="/register""#));
}

#[tokio::test]
async fn members_instance_offers_sign_up_once_an_account_exists() {
    let app = TestApp::spawn().await;
    app.seed_admin().await;

    let (status, body) = app.get("/register").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Rejoindre") && !body.contains("administrateur"));

    let (_, body) = app.get("/login").await;
    assert!(body.contains(r#"href="/register""#));
}

#[tokio::test]
async fn first_account_is_admin_and_signed_in() {
    let app = TestApp::spawn().await;

    app.create_account("first@example.com", "first_password_12", "First")
        .await;

    let user = UserRepository::find_by_email(&app.pool, "first@example.com")
        .await
        .expect("db error")
        .expect("user not found");
    assert_eq!(user.role, Role::Admin);

    let (status, _body) = app.get("/admin/users").await;
    assert_eq!(status, StatusCode::OK, "signed in straight after creation");
}

/// Status and `Location` of a GET, for pages expected to redirect.
async fn redirect_of(app: &TestApp, path: &str) -> (StatusCode, Option<String>) {
    let resp = app
        .client
        .get(app.url(path))
        .send()
        .await
        .expect("GET request failed");
    let location = resp
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .map(ToOwned::to_owned);
    (resp.status(), location)
}

#[tokio::test]
async fn temporary_password_must_be_replaced_before_anything_else() {
    let app = TestApp::spawn().await;
    app.seed_admin().await;
    let member = AuthService::register(
        &app.pool,
        "member@example.com",
        "temporary_pass_1",
        "Member",
        Role::Reader,
    )
    .await
    .expect("failed to create member");
    UserRepository::require_password_change(&app.pool, member.id)
        .await
        .expect("failed to flag member");

    let (_, body) = app.get("/login").await;
    let csrf = extract_csrf_token(&body);
    let (status, _body, location) = app
        .post_form(
            "/login",
            &csrf,
            &[
                ("email", "member@example.com"),
                ("password", "temporary_pass_1"),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/password/new"));

    let (status, location) = redirect_of(&app, "/events").await;
    assert_eq!(
        status,
        StatusCode::SEE_OTHER,
        "every other page is held back"
    );
    assert_eq!(location.as_deref(), Some("/password/new"));

    let (status, body) = app.get("/password/new").await;
    assert_eq!(status, StatusCode::OK);
    let csrf = extract_csrf_token(&body);

    let (status, body, _) = app
        .post_form(
            "/password/new",
            &csrf,
            &[
                ("password", "temporary_pass_1"),
                ("password_confirm", "temporary_pass_1"),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains(r#"id="password-error""#),
        "the temporary password cannot be kept"
    );

    let (status, body, _) = app
        .post_form(
            "/password/new",
            &csrf,
            &[
                ("password", "my_own_password_42"),
                ("password_confirm", "my_own_password_43"),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(r#"id="confirm-error""#));

    let (status, _body, location) = app
        .post_form(
            "/password/new",
            &csrf,
            &[
                ("password", "my_own_password_42"),
                ("password_confirm", "my_own_password_42"),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/"));

    let (status, _body) = app.get("/events").await;
    assert_eq!(status, StatusCode::OK, "the instance opens once replaced");

    let user = UserRepository::find_by_id(&app.pool, member.id)
        .await
        .expect("db error")
        .expect("user not found");
    assert!(!user.must_change_password);
    assert!(
        AuthService::verify_password("my_own_password_42", &user.password_hash)
            .expect("hash error")
    );
}

#[tokio::test]
async fn account_created_by_its_owner_is_not_asked_for_a_new_password() {
    let app = TestApp::spawn().await;
    app.create_account("owner@example.com", "owner_password_12", "Owner")
        .await;

    let (status, location) = redirect_of(&app, "/password/new").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/"));
}

#[tokio::test]
async fn host_reset_signs_the_account_out_and_asks_for_a_new_password() {
    let app = TestApp::spawn().await;
    app.create_account("reset@example.com", "forgotten_password_1", "Reset")
        .await;
    let (status, _body) = app.get("/profile").await;
    assert_eq!(status, StatusCode::OK);

    let temporary = AuthService::reset_password(&app.pool, "reset@example.com")
        .await
        .expect("reset failed");

    let (status, location) = redirect_of(&app, "/profile").await;
    assert_eq!(
        status,
        StatusCode::SEE_OTHER,
        "the open session no longer matches the password"
    );
    assert_eq!(location.as_deref(), Some("/login"));

    let (_, body) = app.get("/login").await;
    let csrf = extract_csrf_token(&body);
    let (status, _body, location) = app
        .post_form(
            "/login",
            &csrf,
            &[
                ("email", "reset@example.com"),
                ("password", temporary.as_str()),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/password/new"));
}

#[tokio::test]
async fn resetting_an_unknown_account_is_refused() {
    let app = TestApp::spawn().await;
    let result = AuthService::reset_password(&app.pool, "nobody@example.com").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn changing_the_password_from_the_profile_keeps_this_session() {
    let app = TestApp::spawn().await;
    app.create_account("profile@example.com", "first_password_12", "Profile")
        .await;

    let (_, body) = app.get("/profile").await;
    let csrf = extract_csrf_token(&body);
    let (status, _body, _) = app
        .post_form(
            "/profile/password",
            &csrf,
            &[
                ("current_password", "first_password_12"),
                ("new_password", "second_password_34"),
                ("new_password_confirm", "second_password_34"),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _body) = app.get("/profile").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the session that changed the password stays open"
    );
}

#[tokio::test]
async fn signed_in_user_is_sent_home_from_login() {
    let app = TestApp::spawn().await;
    app.create_account("home@example.com", "home_password_123", "Home")
        .await;

    let resp = app
        .client
        .get(app.url("/login"))
        .send()
        .await
        .expect("GET /login failed");
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        resp.headers().get("location").and_then(|v| v.to_str().ok()),
        Some("/")
    );
}

#[tokio::test]
async fn public_mode_reader_is_sent_home_from_register() {
    let app = TestApp::spawn_public().await;
    app.seed_admin().await;

    AuthService::register(
        &app.pool,
        "reader@example.com",
        "reader_pass_1234",
        "Reader",
        statup::models::Role::Reader,
    )
    .await
    .expect("failed to create user");

    app.login_user("reader@example.com", "reader_pass_1234")
        .await;

    let resp = app
        .client
        .get(app.url("/register"))
        .send()
        .await
        .expect("GET /register failed");
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        resp.headers().get("location").and_then(|v| v.to_str().ok()),
        Some("/")
    );
}

#[tokio::test]
async fn public_mode_admin_is_sent_to_the_team_page_from_register() {
    let app = TestApp::spawn_public().await;
    app.seed_admin().await;
    app.login_user("owner@example.com", "owner_password_12")
        .await;

    let resp = app
        .client
        .get(app.url("/register"))
        .send()
        .await
        .expect("GET /register failed");
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        resp.headers().get("location").and_then(|v| v.to_str().ok()),
        Some("/admin/users")
    );
}

#[tokio::test]
async fn public_mode_read_routes_accessible_without_auth() {
    let app = TestApp::spawn_public().await;

    // In public mode, read-only routes should be accessible
    let read_routes = ["/", "/events", "/search"];
    for route in read_routes {
        let (status, _body) = app.get(route).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "GET {route} in public mode should be accessible without auth"
        );
    }
}

#[tokio::test]
async fn history_bookmarks_land_on_the_events_list() {
    let app = TestApp::spawn_public().await;

    let resp = app
        .client
        .get(app.url("/history"))
        .send()
        .await
        .expect("GET /history failed");
    assert_eq!(resp.status(), StatusCode::PERMANENT_REDIRECT);
    assert_eq!(
        resp.headers().get("location").and_then(|v| v.to_str().ok()),
        Some("/events")
    );
}

#[tokio::test]
async fn missing_event_renders_a_styled_error_page_in_the_request_language() {
    let app = TestApp::spawn_public().await;

    let (status, body) = app.get("/events/999999").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("Page non trouvée") && body.contains("style.css"));
    assert!(body.contains(r#"href="/""#), "offers a way back");

    let resp = app
        .client
        .get(app.url("/events/999999"))
        .header("Cookie", "lang=en")
        .send()
        .await
        .expect("GET failed");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = resp.text().await.unwrap_or_default();
    assert!(body.contains("Page not found"));
}

#[tokio::test]
async fn htmx_request_gets_an_error_fragment_instead_of_a_page() {
    let app = TestApp::spawn_public().await;

    let resp = app
        .client
        .get(app.url("/events/999999"))
        .header("HX-Request", "true")
        .send()
        .await
        .expect("GET failed");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = resp.text().await.unwrap_or_default();
    assert!(body.contains(r#"class="form-error""#) && body.contains("Page non trouvée"));
    assert!(!body.contains("<html"), "a fragment, not a page");
}
