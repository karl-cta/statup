//! Integration tests for the event lifecycle.
//!
//! Validates the full HTTP flow: creation, transitions, updates, closure.
//! Also exercises service status recalculation.

mod common;

use reqwest::StatusCode;

use common::{TestApp, extract_csrf_token};
use statup::models::{Role, ServiceStatus};
use statup::repositories::ServiceRepository;

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
    (chrono::Local::now() + chrono::Duration::days(2))
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
        body.contains("&lt;strong&gt;declined&lt;/strong&gt;"),
        "the description should be rendered from Markdown and escaped for XML"
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
        Some(format!("/events/{event_id}").as_str())
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
        body.contains(&format!(r#"<option value="{event_id}" selected>"#)),
        "the maintenance is preselected: {body}"
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
        list.contains("un événement est en cours") || list.contains("an event is open"),
        "the list says why the page shows more than the state set by hand"
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
