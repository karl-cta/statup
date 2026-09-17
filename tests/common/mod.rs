//! Shared harness for the integration tests: the full application, with its
//! production router and session layer, on a random local port and an
//! in-memory database.

#![allow(dead_code)]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::cookie::{CookieStore, Jar};
use reqwest::redirect::Policy;

use statup::db;
use statup::middleware::rate_limit::RateLimit;
use statup::models::Role;
use statup::routes::create_router;
use statup::services::{AuthService, LoginRateLimiter, NewAccount};
use statup::session;
use statup::state::AppState;

/// How the application under test is configured.
#[derive(Clone, Copy, Default)]
pub struct Options {
    pub public_mode: bool,
    pub trust_proxy_headers: bool,
    pub public_url: Option<&'static str>,
}

pub struct TestApp {
    pub addr: SocketAddr,
    pub client: reqwest::Client,
    pub pool: sqlx::SqlitePool,
    jar: Arc<Jar>,
    upload_dir: PathBuf,
}

impl TestApp {
    /// A members-only instance.
    pub async fn spawn() -> Self {
        Self::spawn_with(Options::default()).await
    }

    /// An instance anyone can read.
    pub async fn spawn_public() -> Self {
        Self::spawn_with(Options {
            public_mode: true,
            ..Options::default()
        })
        .await
    }

    pub async fn spawn_with(options: Options) -> Self {
        let pool = db::create_pool("sqlite::memory:", 1)
            .await
            .expect("failed to create test pool");
        db::run_migrations(&pool)
            .await
            .expect("failed to run migrations");
        let store = session::create_session_store(&pool)
            .await
            .expect("failed to create the session store");

        let upload_dir = std::env::temp_dir().join(format!("statup-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(upload_dir.join("icons")).expect("failed to create upload dir");

        let state = AppState {
            pool: pool.clone(),
            login_limiter: Arc::new(LoginRateLimiter::default()),
            upload_dir: upload_dir.to_string_lossy().to_string(),
            public_mode: Arc::new(AtomicBool::new(options.public_mode)),
            trust_proxy_headers: options.trust_proxy_headers,
            public_url: options.public_url.map(ToOwned::to_owned),
        };
        // A small budget, so a test can exhaust it with a short burst.
        let rate_limit = RateLimit::with_quota(100, options.trust_proxy_headers)
            .expect("invalid rate limit quota");
        let secure = state.serves_https();
        let sessions = session::session_layer(store, Duration::from_secs(3600), secure);
        let app = create_router(state, &rate_limit).layer(sessions);

        let addr = serve(app).await;
        let jar = Arc::new(Jar::default());
        let client = reqwest::Client::builder()
            .cookie_provider(Arc::clone(&jar))
            .redirect(Policy::none())
            .build()
            .expect("failed to build reqwest client");

        Self {
            addr,
            client,
            pool,
            jar,
            upload_dir,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    /// GET a page and return (status, body text).
    pub async fn get(&self, path: &str) -> (StatusCode, String) {
        let resp = self.get_response(path).await;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        (status, body)
    }

    pub async fn get_response(&self, path: &str) -> reqwest::Response {
        self.client
            .get(self.url(path))
            .send()
            .await
            .expect("GET request failed")
    }

    /// Status and `Location` of a GET, for pages expected to redirect.
    pub async fn redirect_of(&self, path: &str) -> (StatusCode, Option<String>) {
        let resp = self.get_response(path).await;
        (resp.status(), location(&resp))
    }

    /// POST a form with a CSRF token taken from a page this client loaded.
    pub async fn post_form(
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
        let location = location(&resp);
        let body = resp.text().await.unwrap_or_default();
        (status, body, location)
    }

    /// The CSRF token of a page.
    pub async fn csrf_from(&self, path: &str) -> String {
        let (_, body) = self.get(path).await;
        extract_csrf_token(&body)
    }

    /// Sign in through the form. Panics unless the sign-in redirects home.
    pub async fn login(&self, email: &str, password: &str) {
        let csrf = self.csrf_from("/login").await;
        let (status, _body, location) = self
            .post_form("/login", &csrf, &[("email", email), ("password", password)])
            .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "login should redirect");
        assert_eq!(location.as_deref(), Some("/"));
    }

    /// Create an account directly, with a password its owner chose.
    pub async fn create_user(&self, email: &str, password: &str, name: &str, role: Role) -> i64 {
        let account = NewAccount {
            email,
            password,
            display_name: name,
            role,
            must_change_password: false,
        };
        AuthService::create_account(&self.pool, &account)
            .await
            .expect("failed to create user")
            .id
    }

    /// The session id the client holds, if any.
    pub fn session_cookie(&self) -> Option<String> {
        let url = self.url("/").parse().expect("valid url");
        let cookies = self.jar.cookies(&url)?;
        cookies
            .to_str()
            .ok()?
            .split(';')
            .map(str::trim)
            .find_map(|pair| pair.strip_prefix("id="))
            .map(ToOwned::to_owned)
    }

    /// Rows in the session table.
    pub async fn session_rows(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM tower_sessions")
            .fetch_one(&self.pool)
            .await
            .expect("failed to count sessions")
    }
}

impl Drop for TestApp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.upload_dir);
    }
}

async fn serve(app: axum::Router) -> SocketAddr {
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
    addr
}

fn location(resp: &reqwest::Response) -> Option<String> {
    resp.headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .map(ToOwned::to_owned)
}

/// Extract the CSRF token from an HTML response body.
pub fn extract_csrf_token(html: &str) -> String {
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
