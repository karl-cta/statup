//! Integration tests for the event lifecycle.
//!
//! Validates the full HTTP flow: creation, transitions, updates, closure.
//! Also exercises service status recalculation.

mod common;

use reqwest::StatusCode;

use chrono::Utc;
use common::{TestApp, extract_csrf_token};
use statup::models::{CheckKind, Lifecycle, Role, ServiceCheck, ServiceStatus};
use statup::repositories::{EventRepository, ServiceRepository};
use statup::services::ServiceService;

impl TestApp {
    /// POST via the `X-CSRF-Token` header, the way htmx sends it.
    async fn post_form_with_header_csrf(
        &self,
        path: &str,
        fields: &[(&str, &str)],
    ) -> (StatusCode, String, Option<String>) {
        let csrf = self.csrf_from("/").await;

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
        self.create_user(
            "publisher@example.com",
            "publisher_pass_12",
            "Publisher",
            Role::Publisher,
        )
        .await;
        self.login("publisher@example.com", "publisher_pass_12")
            .await;
    }

    async fn create_service(&self, name: &str) -> i64 {
        let slug = name.to_lowercase().replace(' ', "-");
        let service = ServiceRepository::create(&self.pool, name, &slug, None, None, None)
            .await
            .expect("failed to create service");
        service.id
    }

    async fn set_state_by_hand(&self, service_id: i64, status: &str) {
        let (code, _, _) = self
            .post_form_with_header_csrf(
                &format!("/services/{service_id}/status"),
                &[("status", status)],
            )
            .await;
        assert_eq!(code, StatusCode::SEE_OTHER);
    }

    async fn service(&self, id: i64) -> statup::models::Service {
        ServiceRepository::find_by_id(&self.pool, id)
            .await
            .expect("db error")
            .expect("service not found")
    }

    async fn submit_create_event(
        &self,
        base_fields: Vec<(&str, String)>,
        service_ids: &[i64],
    ) -> String {
        let csrf = self.csrf_from("/events/new").await;

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
                ("planned_start", in_two_days()),
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

/// A start the maintenance schedule will not reach during the test, in the
/// form a `datetime-local` field sends.
fn in_two_days() -> String {
    (statup::clock::local(&chrono::Utc::now()) + chrono::Duration::days(2))
        .format("%Y-%m-%dT%H:%M")
        .to_string()
}

fn event_id_from_path(path: &str) -> i64 {
    path.split('?')
        .next()
        .and_then(|p| p.rsplit('/').next())
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
        body.contains("En analyse") || body.contains("Investigating"),
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
            &format!("/events/{event_id}/updates"),
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
            &format!("/events/{event_id}/updates"),
            &[("lifecycle", "monitoring")],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let (status, _, _) = app
        .post_form_with_header_csrf(
            &format!("/events/{event_id}/updates"),
            &[("lifecycle", "resolved"), ("message", "Problème résolu")],
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
            "minor",
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
        ServiceStatus::Operational,
        "a maintenance still to come leaves the service as it is"
    );

    let (_, body) = app.get(&path).await;
    assert!(
        body.contains("Programmée") || body.contains("Scheduled"),
        "planned maintenance should start in Scheduled lifecycle"
    );

    let (status, _, _) = app
        .post_form_with_header_csrf(
            &format!("/events/{event_id}/updates"),
            &[("lifecycle", "in_progress")],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let svc = ServiceRepository::find_by_id(&app.pool, service_id)
        .await
        .expect("db error")
        .expect("service not found");
    assert_eq!(
        svc.status,
        ServiceStatus::Maintenance,
        "work under way puts the service in maintenance"
    );

    let (status, _, _) = app
        .post_form_with_header_csrf(
            &format!("/events/{event_id}/updates"),
            &[
                ("lifecycle", "completed"),
                ("message", "Maintenance terminée"),
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
            &format!("/events/{event_id}/updates"),
            &[("lifecycle", "scheduled")],
        )
        .await;

    assert_eq!(
        status,
        StatusCode::OK,
        "the page comes back with the refusal"
    );
    assert!(
        body.contains("pas possible"),
        "invalid transition should be refused in the page"
    );
}

#[tokio::test]
async fn reader_cannot_create_events() {
    let app = TestApp::spawn().await;

    app.create_user(
        "reader@example.com",
        "reader_pass_1234",
        "Reader",
        Role::Reader,
    )
    .await;
    app.login("reader@example.com", "reader_pass_1234").await;

    let (status, _) = app.get("/events/new").await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "reader should not access /events/new"
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
            &format!("/events/{minor_id}/updates"),
            &[("lifecycle", "resolved"), ("message", "Problème résolu")],
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

/// An incident its author already works on opens at the step they chose;
/// one already under watch leaves its service as it is.
#[tokio::test]
async fn an_incident_opens_at_the_chosen_step() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;

    for (step, expected, service_status) in [
        (
            "in_progress",
            Lifecycle::InProgress,
            ServiceStatus::MajorOutage,
        ),
        (
            "monitoring",
            Lifecycle::Monitoring,
            ServiceStatus::Operational,
        ),
    ] {
        let service_id = app.create_service(&format!("Service {step}")).await;
        let location = app
            .submit_create_event(
                vec![
                    ("title", format!("Declared at {step}")),
                    ("kind", "incident".to_string()),
                    ("severity", "critical".to_string()),
                    ("opening_step", step.to_string()),
                ],
                &[service_id],
            )
            .await;
        let id: i64 = location
            .trim_start_matches("/events/")
            .split('?')
            .next()
            .and_then(|id| id.parse().ok())
            .expect("event id in the location");
        let event = EventRepository::find_by_id(&app.pool, id)
            .await
            .expect("db error")
            .expect("event not found");
        assert_eq!(event.lifecycle, Some(expected), "opening step {step}");
        assert_eq!(app.service(service_id).await.status, service_status);
    }
}

/// Maintenance without downtime leaves its services up while it runs;
/// ordinary maintenance puts them under maintenance.
#[tokio::test]
async fn maintenance_without_downtime_keeps_its_services_up() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;

    for (keeps_up, expected) in [
        (true, ServiceStatus::Operational),
        (false, ServiceStatus::Maintenance),
    ] {
        let service_id = app.create_service(&format!("Service {keeps_up}")).await;
        let mut fields = vec![
            ("title", format!("Work, keeps up: {keeps_up}")),
            ("kind", "maintenance".to_string()),
        ];
        if keeps_up {
            fields.push(("keeps_services_up", "on".to_string()));
        }
        app.submit_create_event(fields, &[service_id]).await;
        assert_eq!(
            app.service(service_id).await.status,
            expected,
            "keeps up: {keeps_up}"
        );
    }
}

/// The services page names the event that holds a service in its state,
/// in place of the control for the state set by hand.
#[tokio::test]
async fn services_page_names_the_event_holding_a_service() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;
    let service_id = app.create_service("Network").await;
    let path = app
        .submit_create_event(
            vec![
                ("title", "Router swap".to_string()),
                ("kind", "maintenance".to_string()),
            ],
            &[service_id],
        )
        .await;
    let event_path = path.split('?').next().unwrap_or_default().to_string();

    let (_, page) = app.get("/services").await;
    assert!(
        page.contains(&format!(r#"href="{event_path}">Router swap</a>"#)),
        "the event is named and linked: {page}"
    );
    assert!(
        !page.contains(&format!(r#"id="status-{service_id}""#)),
        "the control waits for the end of the event"
    );
}

/// A refused creation re-renders the form with everything the author typed.
#[tokio::test]
async fn rejected_event_creation_gives_the_input_back() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;
    let service_id = app.create_service("Payment gateway").await;

    let (_, form) = app.get("/events/new").await;
    let csrf = extract_csrf_token(&form);
    let service_id_str = service_id.to_string();

    let (status, body, _) = app
        .post_form(
            "/events/new",
            &csrf,
            &[
                ("title", "   "),
                ("description", "Checkout has been failing since 02:14 UTC."),
                ("kind", "incident"),
                ("severity", "critical"),
                ("planned_start", "2026-09-07T02:14"),
                ("service_ids", &service_id_str),
            ],
        )
        .await;

    assert_eq!(status, StatusCode::OK, "should re-render the form");
    assert!(
        body.contains("Checkout has been failing since 02:14 UTC."),
        "description should survive the rejection"
    );
    assert!(
        body.contains("2026-09-07T02:14"),
        "planned start should survive the rejection"
    );
    let critical = body
        .find(r#"value="critical""#)
        .expect("severity choice rendered");
    let tag_end = critical + body[critical..].find('>').expect("tag closes");
    assert!(
        body[critical..tag_end].contains("checked"),
        "severity should stay checked"
    );
    assert!(
        body.contains(r#"action="/events/new""#),
        "the form should still post to the creation route"
    );
}

/// The same rule on the service form.
#[tokio::test]
async fn rejected_service_creation_gives_the_input_back() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;

    let (_, form) = app.get("/services/new").await;
    let csrf = extract_csrf_token(&form);

    let (status, body, _) = app
        .post_form(
            "/services/new",
            &csrf,
            &[
                ("name", "  "),
                ("description", "Handles checkout and refunds."),
            ],
        )
        .await;

    assert_eq!(status, StatusCode::OK, "should re-render the form");
    assert!(
        body.contains("Handles checkout and refunds."),
        "description should survive the rejection"
    );
    assert!(
        body.contains(r#"action="/services/new""#),
        "the form should still post to the creation route"
    );
    assert!(
        body.contains(r#"id="name-error""#) && !body.contains(r#"id="form-error""#),
        "a refused name is said under the name field, not above the form"
    );
}

#[tokio::test]
async fn a_service_saved_with_a_check_is_watched() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;
    let csrf = app.csrf_from("/services/new").await;

    let (status, _, _) = app
        .post_form(
            "/services/new",
            &csrf,
            &[
                ("name", "Intranet"),
                ("check_kind", "http"),
                ("check_url", " https://intranet.example.com "),
                ("check_address", "left over from the other kind"),
                ("check_internal_cert", "on"),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let checked = ServiceRepository::list_checked(&app.pool).await.unwrap();
    assert_eq!(checked.len(), 1);
    assert_eq!(checked[0].kind, CheckKind::Http);
    assert_eq!(checked[0].target, "https://intranet.example.com");
    assert!(checked[0].internal_cert);

    let (_, form) = app.get(&format!("/services/{}/edit", checked[0].id)).await;
    assert!(form.contains(r#"value="https://intranet.example.com""#));
    assert!(form.contains(r#"value="http" class="sr-only" checked"#));
}

#[tokio::test]
async fn a_refused_check_saves_nothing_and_is_said_under_its_field() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;
    let csrf = app.csrf_from("/services/new").await;

    let (status, body, _) = app
        .post_form(
            "/services/new",
            &csrf,
            &[
                ("name", "NAS"),
                ("check_kind", "tcp"),
                ("check_address", "nas"),
            ],
        )
        .await;

    assert_eq!(status, StatusCode::OK, "should re-render the form");
    assert!(body.contains(r#"id="check-error""#) && !body.contains(r#"id="form-error""#));
    assert!(
        body.contains(r#"value="nas""#),
        "the typed address should survive"
    );
    assert!(
        ServiceRepository::list_all(&app.pool)
            .await
            .unwrap()
            .is_empty(),
        "a refused check should save no service"
    );
}

#[tokio::test]
async fn removing_the_check_clears_a_detected_outage() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;
    let id = app.create_service("Paie").await;
    let check = ServiceCheck {
        kind: CheckKind::Tcp,
        target: "10.0.0.9:443".to_string(),
        internal_cert: false,
    };
    ServiceRepository::set_check(&app.pool, id, Some(&check))
        .await
        .unwrap();
    ServiceRepository::mark_detected_down(&app.pool, id, Utc::now())
        .await
        .unwrap();
    ServiceService::recalculate_status(&app.pool, id)
        .await
        .unwrap();
    assert_eq!(app.service(id).await.status, ServiceStatus::MajorOutage);

    let csrf = app.csrf_from(&format!("/services/{id}/edit")).await;
    let (status, _, _) = app
        .post_form(
            &format!("/services/{id}/edit"),
            &csrf,
            &[("name", "Paie"), ("check_kind", "none")],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let service = app.service(id).await;
    assert_eq!(service.status, ServiceStatus::Operational);
    assert_eq!(service.detected_status, None);
    assert!(
        ServiceRepository::list_checked(&app.pool)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn testing_a_check_says_why_it_fails_and_saves_nothing() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;
    let closed = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .to_string();
    let csrf = app.csrf_from("/services/new").await;

    let (status, body, _) = app
        .post_form(
            "/services/check-test",
            &csrf,
            &[
                ("name", ""),
                ("check_kind", "tcp"),
                ("check_address", &closed),
            ],
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(r#"data-tone="crit""#), "a failure is red");
    assert!(body.contains("Port fermé"), "the reason is said in words");
    assert!(body.contains("Détail technique"), "the raw error follows");
    assert!(
        ServiceRepository::list_all(&app.pool)
            .await
            .unwrap()
            .is_empty(),
        "a test saves nothing"
    );
}

#[tokio::test]
async fn testing_an_open_port_says_it_answers() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;
    let csrf = app.csrf_from("/services/new").await;

    let (status, body, _) = app
        .post_form(
            "/services/check-test",
            &csrf,
            &[
                ("check_kind", "tcp"),
                ("check_address", &app.addr.to_string()),
            ],
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(r#"data-tone="ok""#));
    assert!(body.contains("Le port accepte la connexion"));
}

#[tokio::test]
async fn testing_a_misspelt_address_says_what_is_wrong() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;
    let csrf = app.csrf_from("/services/new").await;

    let (status, body, _) = app
        .post_form(
            "/services/check-test",
            &csrf,
            &[("check_kind", "tcp"), ("check_address", "nas")],
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(r#"class="field-error""#));
    assert!(body.contains("suivie de son port"));
}

#[tokio::test]
async fn readers_cannot_test_a_check() {
    let app = TestApp::spawn().await;
    app.create_user(
        "reader@example.com",
        "reader_password_12",
        "Reader",
        Role::Reader,
    )
    .await;
    app.login("reader@example.com", "reader_password_12").await;
    let csrf = app.csrf_from("/").await;

    let (status, _, _) = app
        .post_form(
            "/services/check-test",
            &csrf,
            &[
                ("check_kind", "tcp"),
                ("check_address", &app.addr.to_string()),
            ],
        )
        .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn feed_is_private_when_public_mode_is_off() {
    let app = TestApp::spawn().await;

    let (status, _) = app.get("/feed").await;
    assert_eq!(
        status,
        StatusCode::SEE_OTHER,
        "anonymous feed should redirect"
    );
}

#[tokio::test]
async fn feed_lists_events_with_their_updates() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;
    let service_id = app.create_service("Checkout API").await;

    let path = app
        .create_incident(
            "Payments are failing",
            "Cards are **declined** at checkout",
            "critical",
            &[service_id],
        )
        .await;
    let event_id = event_id_from_path(&path);

    let (_, detail_body) = app.get(&path).await;
    let csrf = extract_csrf_token(&detail_body);
    let (status, _, _) = app
        .post_form(
            &format!("/events/{event_id}/updates"),
            &csrf,
            &[("message", "Provider confirmed the outage")],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let resp = app
        .client
        .get(app.url("/feed"))
        .send()
        .await
        .expect("GET /feed failed");
    assert_eq!(resp.status(), StatusCode::OK);
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(
        content_type.starts_with("application/atom+xml"),
        "feed should be served as Atom, got {content_type}"
    );

    let body = resp.text().await.expect("feed body");
    assert!(body.starts_with("<?xml"), "feed should be an XML document");
    assert!(
        body.contains("<title>Payments are failing</title>"),
        "the incident should be an entry"
    );
    assert!(
        body.contains(&format!("http://{}/events/{event_id}", app.addr)),
        "entry links should be absolute and point at the event page"
    );
    assert!(
        body.contains("Checkout API"),
        "the affected service should be named in the entry"
    );
    assert!(
        body.contains("&#60;strong&#62;declined&#60;/strong&#62;"),
        "the description should be rendered from Markdown and escaped for XML"
    );
    assert!(
        !body.contains("<strong>declined"),
        "rendered Markdown must never reach the feed unescaped"
    );
    assert!(
        body.contains("Provider confirmed the outage"),
        "posted updates should be part of the entry"
    );

    let (_, home) = app.get("/").await;
    assert!(
        home.contains(r#"<link rel="alternate" type="application/atom+xml" href="/feed""#),
        "pages should advertise the feed for auto discovery"
    );
    assert!(
        home.contains(r#"href="/subscribe""#),
        "the banner should offer the subscription page"
    );
}

#[tokio::test]
async fn detail_page_says_an_incident_is_still_ongoing() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;
    let service_id = app.create_service("Search").await;

    let path = app
        .create_incident(
            "Search is slow",
            "Latency above 2s",
            "critical",
            &[service_id],
        )
        .await;
    let event_id = event_id_from_path(&path);

    let (_, body) = app.get(&path).await;
    assert!(
        body.contains(r#"class="fact-state" data-tone="crit""#),
        "an open incident should carry its tone on its own page"
    );
    assert!(
        body.contains("ouvert depuis"),
        "the time since the incident opened should be shown"
    );
    assert!(
        body.contains(r#"name="lifecycle""#) && body.contains(r#"<option value="">"#),
        "the state list should open on keeping the current state"
    );

    let (status, body, _) = app
        .post_form_with_header_csrf(
            &format!("/events/{event_id}/updates"),
            &[("lifecycle", ""), ("message", "")],
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "an update with nothing in it is refused in the page"
    );
    assert!(body.contains("Écrivez un message"));

    let (status, _, location) = app
        .post_form_with_header_csrf(
            &format!("/events/{event_id}/updates"),
            &[("lifecycle", "resolved"), ("message", "Index rebuilt")],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        location.as_deref(),
        Some(format!("/events/{event_id}?posted=1").as_str()),
        "back on the event, where the update lights up once"
    );

    let (_, body) = app.get(&path).await;
    assert!(
        !body.contains(r#"class="fact-state" data-tone="crit""#),
        "a closed incident should not claim to be ongoing"
    );
    assert!(
        body.contains("Index rebuilt"),
        "the closing message is shown"
    );
}

#[tokio::test]
async fn a_finished_maintenance_offers_the_announcement_of_what_is_new() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;
    let service_id = app.create_service("Payroll").await;
    let path = app
        .create_planned_maintenance("Payroll update", "New version", "minor", &[service_id])
        .await;
    let event_id = event_id_from_path(&path);

    let (_, body) = app.get(&path).await;
    assert!(
        !body.contains(&format!("after={event_id}")),
        "nothing to announce before the work is done"
    );

    for lifecycle in ["in_progress", "completed"] {
        let (status, _, _) = app
            .post_form_with_header_csrf(
                &format!("/events/{event_id}/updates"),
                &[("lifecycle", lifecycle), ("message", "Done")],
            )
            .await;
        assert_eq!(status, StatusCode::SEE_OTHER);
    }

    let (_, body) = app.get(&path).await;
    assert!(
        body.contains(&format!(
            "/events/new?kind=publication&amp;after={event_id}"
        )),
        "a finished maintenance offers the announcement: {body}"
    );

    let (status, body) = app
        .get(&format!("/events/new?kind=publication&after={event_id}"))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("Payroll update"),
        "the title names the maintenance"
    );
    assert!(
        body.contains(&format!(
            "<option value=\"{event_id}\" selected>Payroll update\u{a0}· "
        )),
        "the maintenance is preselected, with the day it ended: {body}"
    );
    assert!(
        body.contains(r#"value="changelog" class="sr-only" checked"#)
            || body.contains(r#"value="changelog" class="sr-only" data-required checked"#),
        "the announcement is a changelog: {body}"
    );
}

#[tokio::test]
async fn an_announcement_names_the_maintenance_it_follows() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;
    let service_id = app.create_service("Payroll").await;
    let path = app
        .create_planned_maintenance("Payroll update", "New version", "minor", &[service_id])
        .await;
    let maintenance_id = event_id_from_path(&path);
    for lifecycle in ["in_progress", "completed"] {
        app.post_form_with_header_csrf(
            &format!("/events/{maintenance_id}/updates"),
            &[("lifecycle", lifecycle), ("message", "Done")],
        )
        .await;
    }

    let announcement = app
        .submit_create_event(
            vec![
                ("title", "What is new".to_string()),
                ("description", "Faster payslips".to_string()),
                ("kind", "publication".to_string()),
                ("category", "changelog".to_string()),
                ("follows_event_id", maintenance_id.to_string()),
            ],
            &[service_id],
        )
        .await;
    let (status, body) = app.get(&announcement).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains(&format!(r#"href="/events/{maintenance_id}""#))
            && body.contains("Payroll update"),
        "the announcement links to the maintenance it follows: {body}"
    );
}

#[tokio::test]
async fn a_state_set_by_hand_outlasts_the_events_on_its_service() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;
    let service_id = app.create_service("HR portal").await;
    app.set_state_by_hand(service_id, "major_outage").await;
    assert_eq!(
        app.service(service_id).await.status,
        ServiceStatus::MajorOutage
    );

    let incident = app
        .create_incident("HR portal slow", "Pages take long", "minor", &[service_id])
        .await;
    let incident_id = event_id_from_path(&incident);
    assert_eq!(
        app.service(service_id).await.status,
        ServiceStatus::MajorOutage,
        "a milder incident does not lower what the team declared"
    );
    app.post_form_with_header_csrf(
        &format!("/events/{incident_id}/updates"),
        &[("lifecycle", "resolved"), ("message", "Back to normal")],
    )
    .await;
    assert_eq!(
        app.service(service_id).await.status,
        ServiceStatus::MajorOutage,
        "closing an incident falls back to the state set by hand"
    );

    app.set_state_by_hand(service_id, "degraded").await;
    let path = app
        .create_planned_maintenance("Upgrade", "New version", "minor", &[service_id])
        .await;
    let maintenance_id = event_id_from_path(&path);
    app.post_form_with_header_csrf(
        &format!("/events/{maintenance_id}/updates"),
        &[("lifecycle", "in_progress")],
    )
    .await;
    assert_eq!(
        app.service(service_id).await.status,
        ServiceStatus::Degraded
    );
    app.post_form_with_header_csrf(
        &format!("/events/{maintenance_id}/updates"),
        &[("lifecycle", "completed"), ("message", "Done")],
    )
    .await;
    assert_eq!(
        app.service(service_id).await.status,
        ServiceStatus::Degraded,
        "a finished maintenance leaves the state set by hand"
    );

    app.set_state_by_hand(service_id, "operational").await;
    app.create_incident("HR portal down", "No page loads", "critical", &[service_id])
        .await;
    let service = app.service(service_id).await;
    assert_eq!(service.manual_status, ServiceStatus::Operational);
    assert_eq!(service.status, ServiceStatus::MajorOutage);
    let (_, list) = app.get("/services").await;
    assert!(
        list.contains(">HR portal down</a>")
            && !list.contains(&format!(r#"id="status-{service_id}""#)),
        "the list names the event that sets the state, instead of the control"
    );
}

#[tokio::test]
async fn availability_ends_when_the_service_came_back() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;
    let service_id = app.create_service("Payroll").await;
    let path = app
        .create_incident("Payroll down", "No access", "critical", &[service_id])
        .await;
    let event_id = event_id_from_path(&path);
    app.post_form_with_header_csrf(
        &format!("/events/{event_id}/updates"),
        &[("lifecycle", "monitoring")],
    )
    .await;

    let since = chrono::Utc::now() - chrono::Duration::days(30);
    let spans = statup::repositories::EventRepository::incident_spans(&app.pool, since)
        .await
        .expect("db error");
    let span = &spans[&service_id][0];
    assert!(
        span.end.is_some(),
        "an incident under watch no longer counts as down time"
    );
}

#[tokio::test]
async fn a_maintenance_without_a_start_begins_right_away() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;
    let service_id = app.create_service("File server").await;
    app.submit_create_event(
        vec![
            ("title", "Disk replacement".to_string()),
            ("kind", "maintenance".to_string()),
            ("planned_start", String::new()),
        ],
        &[service_id],
    )
    .await;
    assert_eq!(
        app.service(service_id).await.status,
        ServiceStatus::Maintenance
    );
}

#[tokio::test]
async fn a_maintenance_begun_right_away_keeps_its_end() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;
    let service_id = app.create_service("File server").await;
    let end = chrono::Utc::now() + chrono::TimeDelta::hours(1);
    let location = app
        .submit_create_event(
            vec![
                ("title", "Disk replacement".to_string()),
                ("kind", "maintenance".to_string()),
                ("planned_start", String::new()),
                ("planned_end", statup::clock::format_input(&end)),
            ],
            &[service_id],
        )
        .await;
    let id: i64 = location
        .trim_start_matches("/events/")
        .split('?')
        .next()
        .and_then(|id| id.parse().ok())
        .expect("event id in the redirect");
    let event = statup::repositories::EventRepository::find_by_id(&app.pool, id)
        .await
        .expect("db error")
        .expect("event not found");
    assert!(!event.planned);
    assert_eq!(
        event
            .planned_end
            .map(|end| statup::clock::format_input(&end)),
        Some(statup::clock::format_input(&end))
    );
}

#[tokio::test]
async fn an_incident_declared_late_keeps_when_it_began() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;
    let service_id = app.create_service("Mail").await;
    let began = chrono::Utc::now() - chrono::TimeDelta::minutes(40);
    let location = app
        .submit_create_event(
            vec![
                ("title", "Mail down".to_string()),
                ("kind", "incident".to_string()),
                ("severity", "critical".to_string()),
                ("started_at", statup::clock::format_input(&began)),
            ],
            &[service_id],
        )
        .await;
    let event =
        statup::repositories::EventRepository::find_by_id(&app.pool, event_id_from_path(&location))
            .await
            .expect("db error")
            .expect("event not found");
    assert_eq!(
        event.started_at.map(|at| statup::clock::format_input(&at)),
        Some(statup::clock::format_input(&began))
    );

    let later = chrono::Utc::now() + chrono::TimeDelta::hours(2);
    let csrf = app.csrf_from("/events/new").await;
    let started_at = statup::clock::format_input(&later);
    let (status, body, _) = app
        .post_form(
            "/events/new",
            &csrf,
            &[
                ("title", "Mail slow"),
                ("kind", "incident"),
                ("severity", "minor"),
                ("started_at", started_at.as_str()),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::OK, "refused in the form");
    assert!(body.contains("form-error"));
}

#[tokio::test]
async fn an_update_posted_from_the_side_panel_redraws_it() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;
    let service_id = app.create_service("VPN").await;
    let location = app
        .create_incident("VPN down", "", "critical", &[service_id])
        .await;
    let id = event_id_from_path(&location);
    let path = format!("/events/{id}/panel-updates");

    let csrf = app.csrf_from(&format!("/events/{id}")).await;
    let (status, body, _) = app
        .post_form(
            &path,
            &csrf,
            &[("message", "Provider called"), ("lifecycle", "in_progress")],
        )
        .await;
    assert_eq!(status, StatusCode::OK, "the panel is drawn in place");
    assert!(body.contains("drawer-title"));
    assert!(body.contains("Provider called"));

    let (status, body, _) = app.post_form(&path, &csrf, &[("message", "")]).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("form-error") && body.contains("composer is-open"),
        "a refused update keeps the composer open with its reason"
    );
}

#[tokio::test]
async fn a_service_panel_tells_its_story() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;
    let service_id = app.create_service("Payroll").await;
    app.create_incident("Payroll slow", "", "minor", &[service_id])
        .await;

    let (status, body) = app.get(&format!("/services/{service_id}/drawer")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("drawer-title") && body.contains("Payroll"));
    assert!(body.contains("Payroll slow"), "its last events are listed");
    assert!(body.contains(&format!("/events?service_id={service_id}")));

    let printers = app.create_service("Printers").await;
    app.create_incident("Printers jammed", "", "minor", &[printers])
        .await;
    let (status, body) = app.get(&format!("/events?service_id={service_id}")).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the link to its events opens the list"
    );
    assert!(body.contains("Payroll slow"));
    assert!(!body.contains("Printers jammed"), "only its own events");
}

async fn lifecycle_of(app: &TestApp, event_id: i64) -> Option<Lifecycle> {
    EventRepository::find_by_id(&app.pool, event_id)
        .await
        .expect("db error")
        .expect("event not found")
        .lifecycle
}

#[tokio::test]
async fn deleting_an_ongoing_incident_gives_its_services_back() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;
    let service_id = app.create_service("Mail").await;
    let path = app
        .create_incident("Mail down", "No mail goes out", "critical", &[service_id])
        .await;
    let event_id = event_id_from_path(&path);
    assert_eq!(
        app.service(service_id).await.status,
        ServiceStatus::MajorOutage
    );

    let (status, _, location) = app
        .post_form_with_header_csrf(&format!("/events/{event_id}/delete"), &[])
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/events"));
    assert_eq!(
        app.service(service_id).await.status,
        ServiceStatus::Operational
    );
    let (status, _) = app.get(&path).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn undoing_a_step_puts_its_services_back_once() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;
    let service_id = app.create_service("VPN").await;
    let path = app
        .create_incident("VPN drops", "Sessions drop", "critical", &[service_id])
        .await;
    let event_id = event_id_from_path(&path);
    let (status, _, _) = app
        .post_form_with_header_csrf(
            &format!("/events/{event_id}/updates"),
            &[("lifecycle", "monitoring")],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        app.service(service_id).await.status,
        ServiceStatus::Operational,
        "an incident under watch leaves its services up"
    );

    let revert = format!("/events/{event_id}/revert-lifecycle");
    let (status, _, _) = app.post_form_with_header_csrf(&revert, &[]).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        lifecycle_of(&app, event_id).await,
        Some(Lifecycle::Investigating)
    );
    assert_eq!(
        app.service(service_id).await.status,
        ServiceStatus::MajorOutage
    );

    let (status, body, _) = app.post_form_with_header_csrf(&revert, &[]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "one step back only");
    assert!(body.contains("état précédent à rétablir"), "{body}");
}

#[tokio::test]
async fn a_closed_event_belongs_to_administrators() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;
    let service_id = app.create_service("Intranet").await;
    let path = app
        .create_incident("Intranet slow", "Pages are slow", "minor", &[service_id])
        .await;
    let event_id = event_id_from_path(&path);
    let (status, _, _) = app
        .post_form_with_header_csrf(
            &format!("/events/{event_id}/updates"),
            &[("lifecycle", "resolved")],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let csrf = app.csrf_from("/").await;
    let (status, body, _) = app
        .post_form(
            &format!("/events/{event_id}/edit"),
            &csrf,
            &[
                ("title", "Rewritten"),
                ("description", "Rewritten"),
                ("severity", "critical"),
                ("service_ids", &service_id.to_string()),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "edit");
    assert!(body.contains("seul un administrateur"), "{body}");
    for action in ["delete", "revert-lifecycle"] {
        let (status, _, _) = app
            .post_form_with_header_csrf(&format!("/events/{event_id}/{action}"), &[])
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{action}");
    }
    let (status, _, _) = app
        .post_form_with_header_csrf(
            &format!("/events/{event_id}/updates"),
            &[("message", "One more thing")],
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "update");
    let (status, _) = app.get(&format!("/events/{event_id}/edit")).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "edit form");

    let event = EventRepository::find_by_id(&app.pool, event_id)
        .await
        .expect("db error")
        .expect("the event is still there");
    assert_eq!(event.title, "Intranet slow");
    assert_eq!(event.lifecycle, Some(Lifecycle::Resolved));
    assert_eq!(
        app.service(service_id).await.status,
        ServiceStatus::Operational
    );
}

#[tokio::test]
async fn an_administrator_can_still_edit_a_closed_event() {
    let app = TestApp::spawn().await;
    app.create_user("admin@example.com", "admin_pass_1234", "Admin", Role::Admin)
        .await;
    app.login("admin@example.com", "admin_pass_1234").await;
    let service_id = app.create_service("Intranet").await;
    let path = app
        .create_incident("Intranet slow", "Pages are slow", "minor", &[service_id])
        .await;
    let event_id = event_id_from_path(&path);
    let (status, _, _) = app
        .post_form_with_header_csrf(
            &format!("/events/{event_id}/updates"),
            &[("lifecycle", "resolved")],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let csrf = app.csrf_from("/").await;
    let (status, _, location) = app
        .post_form(
            &format!("/events/{event_id}/edit"),
            &csrf,
            &[
                ("title", "Rewritten"),
                ("description", "Rewritten"),
                ("severity", "minor"),
                ("service_ids", &service_id.to_string()),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        location.as_deref(),
        Some(format!("/events/{event_id}").as_str())
    );

    let event = EventRepository::find_by_id(&app.pool, event_id)
        .await
        .expect("db error")
        .expect("the event is still there");
    assert_eq!(event.title, "Rewritten");
}

#[tokio::test]
async fn a_publisher_edits_a_service() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;
    let service_id = app.create_service("Billing").await;

    let (status, form) = app.get(&format!("/services/{service_id}/edit")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(form.contains("Billing"), "the form starts from the name");
    let (status, _) = app.get("/services/9999/edit").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "unknown service");

    let (status, _, location) = app
        .post_form(
            &format!("/services/{service_id}/edit"),
            &extract_csrf_token(&form),
            &[
                ("name", "Billing API"),
                ("description", "Handles invoices."),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        location.as_deref(),
        Some(format!("/services?saved={service_id}").as_str())
    );
    let service = app.service(service_id).await;
    assert_eq!(service.name, "Billing API");
    assert_eq!(service.description.as_deref(), Some("Handles invoices."));
}

#[tokio::test]
async fn an_empty_name_is_refused_on_the_service_edit_form() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;
    let service_id = app.create_service("Billing").await;
    let csrf = app.csrf_from(&format!("/services/{service_id}/edit")).await;

    let (status, body, _) = app
        .post_form(
            &format!("/services/{service_id}/edit"),
            &csrf,
            &[("name", "  "), ("description", "Handles invoices.")],
        )
        .await;

    assert_eq!(status, StatusCode::OK, "should re-render the form");
    assert!(
        body.contains("Handles invoices."),
        "description should survive the rejection"
    );
    assert!(
        body.contains(r#"id="name-error""#),
        "the refusal is said under the name field"
    );
    let service = app.service(service_id).await;
    assert_eq!(service.name, "Billing");
    assert_eq!(service.description, None);
}

#[tokio::test]
async fn a_service_is_deleted_unless_events_cite_it() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;
    let idle_id = app.create_service("Idle").await;
    let cited_id = app.create_service("Intranet").await;
    app.create_incident("Intranet slow", "Pages are slow", "minor", &[cited_id])
        .await;

    let (status, body, _) = app
        .post_form_with_header_csrf(&format!("/services/{cited_id}/delete"), &[])
        .await;
    assert_eq!(status, StatusCode::OK, "the list is redrawn");
    assert!(
        body.contains("Ce service est cité par des événements\u{a0}: il est conservé."),
        "the refusal is said in the list"
    );
    let kept = ServiceRepository::find_by_id(&app.pool, cited_id)
        .await
        .expect("db error");
    assert!(kept.is_some(), "a service with history stays");

    let (status, _, location) = app
        .post_form_with_header_csrf(&format!("/services/{idle_id}/delete"), &[])
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(
        location
            .as_deref()
            .is_some_and(|path| path.starts_with("/services?deleted=")),
        "{location:?}"
    );
    let gone = ServiceRepository::find_by_id(&app.pool, idle_id)
        .await
        .expect("db error");
    assert!(gone.is_none(), "a service without history is gone");
}

#[tokio::test]
async fn a_reader_cannot_edit_or_delete_a_service() {
    let app = TestApp::spawn().await;
    let service_id = app.create_service("Billing").await;
    app.create_user(
        "reader@example.com",
        "reader_pass_1234",
        "Reader",
        Role::Reader,
    )
    .await;
    app.login("reader@example.com", "reader_pass_1234").await;

    let (status, _) = app.get(&format!("/services/{service_id}/edit")).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "edit form");
    for (action, fields) in [
        ("edit", &[("name", "Renamed"), ("description", "")][..]),
        ("delete", &[][..]),
    ] {
        let (status, _, _) = app
            .post_form_with_header_csrf(&format!("/services/{service_id}/{action}"), fields)
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{action}");
    }

    let service = app.service(service_id).await;
    assert_eq!(service.name, "Billing");
}

async fn template_id_by_title(app: &TestApp, title: &str) -> Option<i64> {
    sqlx::query_scalar("SELECT id FROM event_templates WHERE title = ?")
        .bind(title)
        .fetch_optional(&app.pool)
        .await
        .expect("template lookup failed")
}

async fn publish_incident_as_template(app: &TestApp, title: &str) -> i64 {
    app.submit_create_event(
        vec![
            ("title", title.to_string()),
            ("description", "Card payments are failing".to_string()),
            ("kind", "incident".to_string()),
            ("severity", "critical".to_string()),
            ("save_as_template", "on".to_string()),
        ],
        &[],
    )
    .await;
    template_id_by_title(app, title)
        .await
        .expect("the template should have been saved")
}

#[tokio::test]
async fn an_incident_saved_as_a_template_can_be_searched_and_read() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;
    let id = publish_incident_as_template(&app, "Payment gateway down").await;

    let (status, body) = app.get("/events/templates/search?q=Payment").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Payment gateway down"), "match on 2+ chars");
    let (status, body) = app.get("/events/templates/search?q=P").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !body.contains("Payment gateway down"),
        "one char is too few"
    );

    let (status, body) = app.get(&format!("/events/templates/{id}")).await;
    assert_eq!(status, StatusCode::OK);
    let detail: serde_json::Value = serde_json::from_str(&body).expect("detail is JSON");
    assert_eq!(detail["title"], "Payment gateway down");
    assert_eq!(detail["description"], "Card payments are failing");
    assert_eq!(detail["kind"], "incident");
    assert_eq!(detail["severity"], "critical");

    let (status, _) = app.get("/events/templates/999999").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn publishing_from_a_template_counts_its_usage() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;
    let id = publish_incident_as_template(&app, "Payment gateway down").await;
    let usage = || async {
        sqlx::query_scalar::<_, i64>("SELECT usage_count FROM event_templates WHERE id = ?")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .expect("usage lookup failed")
    };
    assert_eq!(usage().await, 0, "saving a template is not using it");

    app.submit_create_event(
        vec![
            ("title", "Payment gateway down again".to_string()),
            ("description", "Same failure".to_string()),
            ("kind", "incident".to_string()),
            ("severity", "critical".to_string()),
            ("template_id", id.to_string()),
        ],
        &[],
    )
    .await;

    assert_eq!(usage().await, 1);
}

#[tokio::test]
async fn deleting_a_template_goes_back_to_the_form_or_answers_htmx() {
    let app = TestApp::spawn().await;
    app.setup_publisher().await;
    let first = publish_incident_as_template(&app, "Payment gateway down").await;
    let second = publish_incident_as_template(&app, "Mail relay down").await;

    let (status, _, location) = app
        .post_form_with_header_csrf(&format!("/events/templates/{first}/delete"), &[])
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/events/new"));
    assert!(
        template_id_by_title(&app, "Payment gateway down")
            .await
            .is_none()
    );

    let csrf = app.csrf_from("/").await;
    let resp = app
        .client
        .post(app.url(&format!("/events/templates/{second}/delete")))
        .header("x-csrf-token", &csrf)
        .header("hx-request", "true")
        .send()
        .await
        .expect("POST request failed");
    assert_eq!(resp.status(), StatusCode::OK, "htmx removes the suggestion");
    assert!(
        template_id_by_title(&app, "Mail relay down")
            .await
            .is_none()
    );

    let (status, _, _) = app
        .post_form_with_header_csrf(&format!("/events/templates/{first}/delete"), &[])
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "already gone");
}

#[tokio::test]
async fn a_reader_cannot_search_read_or_delete_templates() {
    let app = TestApp::spawn().await;
    let reader_id = app
        .create_user(
            "reader@example.com",
            "reader_pass_1234",
            "Reader",
            Role::Reader,
        )
        .await;
    let template = statup::repositories::EventTemplateRepository::create(
        &app.pool,
        statup::repositories::CreateTemplateInput {
            title: "Payment gateway down",
            description: "Card payments are failing",
            kind: statup::models::Kind::Incident,
            severity: Some(statup::models::Severity::Critical),
            planned: false,
            category: None,
            created_by: reader_id,
        },
    )
    .await
    .expect("failed to create template");
    app.login("reader@example.com", "reader_pass_1234").await;

    let (status, _) = app.get("/events/templates/search?q=Payment").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "search");
    let (status, _) = app.get(&format!("/events/templates/{}", template.id)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "detail");
    let (status, _, _) = app
        .post_form_with_header_csrf(&format!("/events/templates/{}/delete", template.id), &[])
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "delete");

    assert!(
        template_id_by_title(&app, "Payment gateway down")
            .await
            .is_some()
    );
}
