//! Tests d'intégration du cycle de vie des événements.
//!
//! Valide le flux HTTP complet : création, transitions, updates, clôture.
//! Teste aussi le recalcul du statut des services.

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
use statup::models::{Role, ServiceStatus};
use statup::repositories::{ServiceRepository, UserRepository};
use statup::routes::create_router;
use statup::services::{AuthService, LoginRateLimiter};
use statup::state::AppState;

struct TestApp {
    addr: SocketAddr,
    client: reqwest::Client,
    pool: sqlx::SqlitePool,
}

impl TestApp {
    async fn spawn() -> Self {
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
            public_mode: Arc::new(AtomicBool::new(false)),
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

    /// POST via header X-CSRF-Token, pour les endpoints qui ne consomment pas
    /// le champ csrf_token du form mais passent quand même par le middleware CSRF.
    async fn post_form_with_header_csrf(
        &self,
        path: &str,
        fields: &[(&str, &str)],
    ) -> (StatusCode, String, Option<String>) {
        let (_, body) = self.get("/").await;
        let csrf = extract_csrf_token(&body);

        let resp = self
            .client
            .post(self.url(path))
            .header("x-csrf-token", &csrf)
            .form(fields)
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

    async fn setup_publisher(&self) {
        AuthService::register(
            &self.pool,
            "publisher@example.com",
            "publisher_pass_12",
            "Publisher",
        )
        .await
        .expect("failed to create user");

        let user = UserRepository::find_by_email(&self.pool, "publisher@example.com")
            .await
            .expect("db error")
            .expect("user not found");
        UserRepository::update_role(&self.pool, user.id, Role::Publisher)
            .await
            .expect("failed to update role");

        self.login("publisher@example.com", "publisher_pass_12")
            .await;
    }

    async fn login(&self, email: &str, password: &str) {
        let (_, body) = self.get("/login").await;
        let csrf = extract_csrf_token(&body);
        let (status, _body, _location) = self
            .post_form("/login", &csrf, &[("email", email), ("password", password)])
            .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "login should redirect");
    }

    async fn create_service(&self, name: &str) -> i64 {
        let slug = name.to_lowercase().replace(' ', "-");
        let service = ServiceRepository::create(&self.pool, name, &slug, None)
            .await
            .expect("failed to create service");
        service.id
    }

    async fn submit_create_event(
        &self,
        base_fields: Vec<(&str, String)>,
        service_ids: &[i64],
    ) -> String {
        let (_, body) = self.get("/events/new").await;
        let csrf = extract_csrf_token(&body);

        let mut fields = base_fields;
        for id in service_ids {
            fields.push(("service_ids", id.to_string()));
        }
        let fields_ref: Vec<(&str, &str)> = fields.iter().map(|(k, v)| (*k, v.as_str())).collect();

        let (status, _body, location) = self.post_form("/events/new", &csrf, &fields_ref).await;
        assert_eq!(
            status,
            StatusCode::SEE_OTHER,
            "event creation should redirect"
        );
        location.expect("should have Location header")
    }

    async fn create_incident(
        &self,
        title: &str,
        description: &str,
        severity: &str,
        service_ids: &[i64],
    ) -> String {
        self.submit_create_event(
            vec![
                ("title", title.to_string()),
                ("description", description.to_string()),
                ("kind", "incident".to_string()),
                ("severity", severity.to_string()),
            ],
            service_ids,
        )
        .await
    }

    async fn create_planned_maintenance(
        &self,
        title: &str,
        description: &str,
        severity: &str,
        service_ids: &[i64],
    ) -> String {
        self.submit_create_event(
            vec![
                ("title", title.to_string()),
                ("description", description.to_string()),
                ("kind", "maintenance".to_string()),
                ("severity", severity.to_string()),
                ("planned", "on".to_string()),
            ],
            service_ids,
        )
        .await
    }

    async fn create_publication(
        &self,
        title: &str,
        description: &str,
        category: &str,
        service_ids: &[i64],
    ) -> String {
        self.submit_create_event(
            vec![
                ("title", title.to_string()),
                ("description", description.to_string()),
                ("kind", "publication".to_string()),
                ("category", category.to_string()),
            ],
            service_ids,
        )
        .await
    }
}

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

fn event_id_from_path(path: &str) -> i64 {
    path.rsplit('/')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("could not parse event ID from path: {path}"))
}

#[tokio::test]
async fn create_incident_and_verify_detail() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;

    let path = app
        .create_incident(
            "Database outage",
            "The primary database is down",
            "critical",
            &[],
        )
        .await;

    let (status, body) = app.get(&path).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Database outage"), "should show event title");
    assert!(
        body.contains("Investigation") || body.contains("investigating"),
        "incident should start in Investigating lifecycle"
    );
}

#[tokio::test]
async fn full_incident_lifecycle_with_service_status() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;

    let service_id = app.create_service("API Gateway").await;

    let svc = ServiceRepository::find_by_id(&app.pool, service_id)
        .await
        .expect("db error")
        .expect("service not found");
    assert_eq!(svc.status, ServiceStatus::Operational);

    let path = app
        .create_incident(
            "API Gateway down",
            "The API gateway is not responding",
            "critical",
            &[service_id],
        )
        .await;
    let event_id = event_id_from_path(&path);

    let svc = ServiceRepository::find_by_id(&app.pool, service_id)
        .await
        .expect("db error")
        .expect("service not found");
    assert_eq!(
        svc.status,
        ServiceStatus::MajorOutage,
        "critical incident should cause MajorOutage"
    );

    let (status, _, _) = app
        .post_form_with_header_csrf(
            &format!("/events/{event_id}/lifecycle"),
            &[("lifecycle", "in_progress")],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let (_, detail_body) = app.get(&path).await;
    let csrf = extract_csrf_token(&detail_body);
    let (status, _, _) = app
        .post_form(
            &format!("/events/{event_id}/updates"),
            &csrf,
            &[("message", "Root cause identified: disk full")],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let (_, body) = app.get(&path).await;
    assert!(
        body.contains("Root cause identified") || body.contains("disk full"),
        "update should appear on event detail"
    );

    let (status, _, _) = app
        .post_form_with_header_csrf(
            &format!("/events/{event_id}/lifecycle"),
            &[("lifecycle", "monitoring")],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let (status, _, _) = app
        .post_form_with_header_csrf(
            &format!("/events/{event_id}/lifecycle"),
            &[
                ("lifecycle", "resolved"),
                ("resolution_comment", "Problème résolu"),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let (_, body) = app.get(&path).await;
    assert!(
        body.contains("Resolved") || body.contains("resolved") || body.contains("Résolu"),
        "event should be resolved"
    );

    let svc = ServiceRepository::find_by_id(&app.pool, service_id)
        .await
        .expect("db error")
        .expect("service not found");
    assert_eq!(
        svc.status,
        ServiceStatus::Operational,
        "service should return to operational after incident resolved"
    );
}

#[tokio::test]
async fn scheduled_maintenance_lifecycle() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;

    let service_id = app.create_service("Auth Service").await;

    let path = app
        .create_planned_maintenance(
            "Planned DB migration",
            "Migrating to new schema",
            "major",
            &[service_id],
        )
        .await;
    let event_id = event_id_from_path(&path);

    let svc = ServiceRepository::find_by_id(&app.pool, service_id)
        .await
        .expect("db error")
        .expect("service not found");
    assert_eq!(
        svc.status,
        ServiceStatus::Maintenance,
        "planned maintenance should put service in Maintenance"
    );

    let (_, body) = app.get(&path).await;
    assert!(
        body.contains("Scheduled") || body.contains("scheduled") || body.contains("Planifié"),
        "planned maintenance should start in Scheduled lifecycle"
    );

    let (status, _, _) = app
        .post_form_with_header_csrf(
            &format!("/events/{event_id}/lifecycle"),
            &[("lifecycle", "in_progress")],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let (status, _, _) = app
        .post_form_with_header_csrf(
            &format!("/events/{event_id}/lifecycle"),
            &[
                ("lifecycle", "completed"),
                ("resolution_comment", "Maintenance terminée"),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let svc = ServiceRepository::find_by_id(&app.pool, service_id)
        .await
        .expect("db error")
        .expect("service not found");
    assert_eq!(svc.status, ServiceStatus::Operational);
}

#[tokio::test]
async fn invalid_lifecycle_transition_is_rejected() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;

    let path = app
        .create_incident(
            "Test transition",
            "Testing invalid transitions",
            "minor",
            &[],
        )
        .await;
    let event_id = event_id_from_path(&path);

    let (status, body, _) = app
        .post_form_with_header_csrf(
            &format!("/events/{event_id}/lifecycle"),
            &[("lifecycle", "scheduled")],
        )
        .await;

    assert!(
        status == StatusCode::BAD_REQUEST || body.contains("autorisée"),
        "invalid transition should be rejected, got status {status}"
    );
}

#[tokio::test]
async fn reader_cannot_create_events() {
    let app = TestApp::spawn().await;

    AuthService::register(
        &app.pool,
        "reader@example.com",
        "reader_pass_1234",
        "Reader",
    )
    .await
    .expect("failed to create user");
    app.login("reader@example.com", "reader_pass_1234").await;

    let (status, _) = app.get("/events/new").await;
    assert!(
        status == StatusCode::FORBIDDEN || status == StatusCode::UNAUTHORIZED,
        "reader should not access /events/new, got {status}"
    );
}

#[tokio::test]
async fn multiple_events_worst_status_wins() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;

    let service_id = app.create_service("Payment Service").await;

    let minor_path = app
        .create_incident(
            "Slow payments",
            "Payment processing is slow",
            "minor",
            &[service_id],
        )
        .await;

    let svc = ServiceRepository::find_by_id(&app.pool, service_id)
        .await
        .expect("db error")
        .expect("service not found");
    assert_eq!(svc.status, ServiceStatus::Degraded);

    app.create_incident(
        "Payment gateway down",
        "Gateway unreachable",
        "critical",
        &[service_id],
    )
    .await;

    let svc = ServiceRepository::find_by_id(&app.pool, service_id)
        .await
        .expect("db error")
        .expect("service not found");
    assert_eq!(
        svc.status,
        ServiceStatus::MajorOutage,
        "worst status should win"
    );

    let minor_id = event_id_from_path(&minor_path);
    let (status, _, _) = app
        .post_form_with_header_csrf(
            &format!("/events/{minor_id}/lifecycle"),
            &[
                ("lifecycle", "resolved"),
                ("resolution_comment", "Problème résolu"),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let svc = ServiceRepository::find_by_id(&app.pool, service_id)
        .await
        .expect("db error")
        .expect("service not found");
    assert_eq!(
        svc.status,
        ServiceStatus::MajorOutage,
        "should stay MajorOutage while critical incident is active"
    );
}

#[tokio::test]
async fn publication_does_not_affect_service_status() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;

    let service_id = app.create_service("Publication Service").await;

    app.create_publication(
        "New feature shipped",
        "We shipped dark mode",
        "changelog",
        &[service_id],
    )
    .await;

    let svc = ServiceRepository::find_by_id(&app.pool, service_id)
        .await
        .expect("db error")
        .expect("service not found");
    assert_eq!(
        svc.status,
        ServiceStatus::Operational,
        "publication should not affect service status"
    );
}
