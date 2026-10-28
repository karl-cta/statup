//! Shared harness for the integration tests: the full application, with its
//! production router and session layer, on a random local port and an
//! in-memory database.

#![allow(dead_code)]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::cookie::{CookieStore, Jar};
use reqwest::redirect::Policy;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

use statup::config::{SmtpConfig, SmtpSecurity};
use statup::db;
use statup::middleware::client_ip::{ClientIpSource, X_FORWARDED_FOR};
use statup::middleware::rate_limit::RateLimit;
use statup::models::Role;
use statup::routes::create_router;
use statup::services::{
    AuthService, DashboardLayoutService, LastChecks, LoginRateLimiter, NewAccount, Notifier,
    UpdateStatus,
};
use statup::session;
use statup::state::AppState;

/// How the application under test is configured.
#[derive(Clone, Copy, Default)]
pub struct Options {
    pub public_mode: bool,
    pub trust_proxy_headers: bool,
    pub public_url: Option<&'static str>,
    /// The tag the daily update check found, as if it had run. The check
    /// itself never runs in tests.
    pub latest_release: Option<&'static str>,
    /// The port of a mail server on this machine, as if `SMTP_*` was set.
    pub mail_port: Option<u16>,
}

pub struct TestApp {
    pub addr: SocketAddr,
    pub client: reqwest::Client,
    pub pool: sqlx::SqlitePool,
    /// The last checks the services page reads, as the monitoring shares them.
    pub checks: Arc<LastChecks>,
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
        // As at start: every module the binary ships has its layout row.
        DashboardLayoutService::reconcile(&pool)
            .await
            .expect("failed to reconcile the layout");
        let store = session::create_session_store(&pool)
            .await
            .expect("failed to create the session store");

        let upload_dir = std::env::temp_dir().join(format!("statup-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(upload_dir.join("icons")).expect("failed to create upload dir");

        let smtp = options.mail_port.map(local_mail_server);
        let state = AppState {
            pool: pool.clone(),
            login_limiter: Arc::new(LoginRateLimiter::default()),
            upload_dir: upload_dir.to_string_lossy().to_string(),
            public_mode: Arc::new(AtomicBool::new(options.public_mode)),
            trust_proxy_headers: options.trust_proxy_headers,
            client_ip_source: client_ip_source(options.trust_proxy_headers),
            public_url: options.public_url.map(ToOwned::to_owned),
            update: update_status(options.latest_release),
            checks: Arc::default(),
            notifier: Arc::new(
                Notifier::new(Duration::from_secs(5), smtp.as_ref())
                    .expect("failed to build the notifier"),
            ),
        };
        let checks = Arc::clone(&state.checks);
        // A small budget, so a test can exhaust it with a short burst.
        let rate_limit = RateLimit::with_quota(100, client_ip_source(options.trust_proxy_headers))
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
            checks,
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

    /// GET as htmx does for a refresh: same route, the fragment answer.
    pub async fn get_htmx(&self, path: &str) -> (StatusCode, String) {
        let resp = self
            .client
            .get(self.url(path))
            .header("HX-Request", "true")
            .send()
            .await
            .expect("GET request failed");
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

    /// The general settings card, sent at once as its page sends it: the
    /// access already chosen and an empty name, with `changes` over them,
    /// and a logo when given. The zone stays unless `changes` names one.
    pub async fn save_settings(
        &self,
        changes: &[(&str, &str)],
        logo: Option<(&str, &str, &[u8])>,
    ) -> (StatusCode, String, Option<String>) {
        let (_, page) = self.get("/admin/settings").await;
        let public = page.contains(r#"value="everyone" class="sr-only" checked"#);
        let mut fields: Vec<(&str, String)> = vec![
            ("csrf_token", extract_csrf_token(&page)),
            ("instance_name", String::new()),
            (
                "access",
                if public { "everyone" } else { "members" }.to_string(),
            ),
        ];
        for (name, value) in changes {
            match fields.iter_mut().find(|(field, _)| field == name) {
                Some(field) => field.1 = (*value).to_string(),
                None => fields.push((name, (*value).to_string())),
            }
        }
        let mut body = Vec::new();
        for (name, value) in fields {
            body.extend(
                format!(
                    "--statup\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
                )
                .into_bytes(),
            );
        }
        if let Some((filename, content_type, data)) = logo {
            body.extend(
                format!(
                    "--statup\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\n\
                     Content-Type: {content_type}\r\n\r\n"
                )
                .into_bytes(),
            );
            body.extend_from_slice(data);
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(b"--statup--\r\n");
        let resp = self
            .client
            .post(self.url("/admin/settings/general"))
            .header("content-type", "multipart/form-data; boundary=statup")
            .body(body)
            .send()
            .await
            .expect("POST request failed");
        let status = resp.status();
        let location = location(&resp);
        let body = resp.text().await.unwrap_or_default();
        (status, body, location)
    }

    /// The CSRF token of a page.
    /// The page step of the first launch, sent as multipart with an empty
    /// logo, as the browser sends it.
    pub async fn save_setup_page(&self, name: &str, access: &str) -> reqwest::Response {
        let csrf = self.csrf_from("/setup/page").await;
        let field = |key: &str, value: &str| {
            format!("--statup\r\nContent-Disposition: form-data; name=\"{key}\"\r\n\r\n{value}\r\n")
        };
        let body = format!(
            "{}{}{}--statup\r\nContent-Disposition: form-data; name=\"file\"; filename=\"\"\r\n\
             Content-Type: application/octet-stream\r\n\r\n\r\n--statup--\r\n",
            field("csrf_token", &csrf),
            field("instance_name", name),
            field("access", access),
        );
        self.client
            .post(self.url("/setup/page"))
            .header("content-type", "multipart/form-data; boundary=statup")
            .body(body.into_bytes())
            .send()
            .await
            .expect("page step failed")
    }

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

fn local_mail_server(port: u16) -> SmtpConfig {
    SmtpConfig {
        host: "127.0.0.1".into(),
        port,
        security: SmtpSecurity::None,
        credentials: None,
        from: "Statup <status@example.com>".into(),
    }
}

fn update_status(latest_release: Option<&str>) -> Arc<UpdateStatus> {
    let status = UpdateStatus::default();
    if let Some(tag) = latest_release {
        status.record_latest(tag);
    }
    Arc::new(status)
}

/// Behind the test's pretend proxy, the one that appends `X-Forwarded-For`.
fn client_ip_source(trust_proxy_headers: bool) -> ClientIpSource {
    if trust_proxy_headers {
        ClientIpSource::Header(X_FORWARDED_FOR)
    } else {
        ClientIpSource::Peer
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

pub type Mailbox = Arc<Mutex<Vec<String>>>;

/// A mail server that accepts everything and keeps each message it receives.
pub struct MailSink {
    pub port: u16,
    messages: Mailbox,
}

impl MailSink {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("address").port();
        let messages = Mailbox::default();
        let kept = Arc::clone(&messages);
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(converse(stream, Arc::clone(&kept)));
            }
        });
        Self { port, messages }
    }

    pub fn received(&self) -> Vec<String> {
        self.messages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Waits up to five seconds for `count` messages and returns what arrived.
    pub async fn wait_for(&self, count: usize) -> Vec<String> {
        for _ in 0..100 {
            if self.received().len() >= count {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        self.received()
    }
}

async fn converse(stream: TcpStream, messages: Mailbox) {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader);
    if writer.write_all(b"220 sink ready\r\n").await.is_err() {
        return;
    }
    let mut line = String::new();
    loop {
        line.clear();
        match lines.read_line(&mut line).await {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let command = line.trim_end().to_ascii_uppercase();
        let reply: &[u8] = if command.starts_with("QUIT") {
            let _ = writer.write_all(b"221 bye\r\n").await;
            return;
        } else if command.starts_with("DATA") {
            let _ = writer.write_all(b"354 go ahead\r\n").await;
            let message = read_message(&mut lines).await;
            messages
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(message);
            b"250 queued\r\n"
        } else {
            b"250 ok\r\n"
        };
        if writer.write_all(reply).await.is_err() {
            return;
        }
    }
}

async fn read_message<R: AsyncBufReadExt + Unpin>(lines: &mut R) -> String {
    let mut message = String::new();
    let mut line = String::new();
    loop {
        line.clear();
        match lines.read_line(&mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) if line == ".\r\n" => break,
            Ok(_) => message.push_str(&line),
        }
    }
    message
}
