//! "Add to my calendar" on a maintenance still to come: the file Outlook and
//! Apple Calendar open, the way to Google Calendar, and who may have them.

mod common;

use chrono::{Duration, Utc};
use reqwest::StatusCode;

use common::TestApp;
use statup::i18n::{I18n, LANGUAGES};
use statup::models::{CreateEventInput, Grants, Kind, Role, Severity, Visibility};
use statup::repositories::{GroupRepository, ServiceRepository};
use statup::services::EventService;

async fn author(app: &TestApp) -> i64 {
    app.create_user(
        "editor@example.com",
        "editor_password_12",
        "Editor",
        Role::Publisher,
    )
    .await
}

async fn service(app: &TestApp, name: &str) -> i64 {
    ServiceRepository::create(&app.pool, name, &name.to_lowercase(), None, None, None)
        .await
        .unwrap()
        .id
}

fn input(kind: Kind, author_id: i64, service_ids: Vec<i64>) -> CreateEventInput {
    let start = Utc::now() + Duration::days(3);
    let maintenance = kind == Kind::Maintenance;
    CreateEventInput {
        kind,
        severity: (!maintenance).then_some(Severity::Minor),
        planned: maintenance,
        category: None,
        title: "Mail, upgrade; v2".to_string(),
        description: "Mail stops for **two hours**.".to_string(),
        planned_start: maintenance.then_some(start),
        planned_end: maintenance.then(|| start + Duration::hours(2)),
        started_at: None,
        opening_step: None,
        keeps_services_up: false,
        service_ids,
        follows_event_id: None,
        author_id,
        api_key_id: None,
        next_update: None,
    }
}

async fn publish(app: &TestApp, input: CreateEventInput) -> i64 {
    EventService::create(&app.pool, input).await.unwrap().id
}

async fn scheduled_maintenance(app: &TestApp) -> i64 {
    let author_id = author(app).await;
    let mail = service(app, "Mail").await;
    publish(app, input(Kind::Maintenance, author_id, vec![mail])).await
}

#[tokio::test]
async fn a_maintenance_to_come_offers_its_calendar_menu_on_its_page_and_panel() {
    let app = TestApp::spawn_public().await;
    let id = scheduled_maintenance(&app).await;

    for path in [format!("/events/{id}"), format!("/events/{id}/drawer")] {
        let (status, body) = app.get(&path).await;
        assert_eq!(status, StatusCode::OK);
        let file = format!("href=\"/events/{id}/calendar.ics\"");
        assert_eq!(body.matches(&file).count(), 2, "Outlook and Apple");
        assert!(body.contains(&format!("href=\"/events/{id}/calendar/google\"")));
    }
}

#[tokio::test]
async fn the_file_holds_the_maintenance_and_is_kept_by_no_cache() {
    let app = TestApp::spawn_public().await;
    let id = scheduled_maintenance(&app).await;

    let response = app
        .get_response(&format!("/events/{id}/calendar.ics"))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string()
    };
    assert_eq!(header("content-type"), "text/calendar; charset=utf-8");
    assert_eq!(
        header("content-disposition"),
        format!("inline; filename=\"maintenance-{id}.ics\"")
    );
    assert_eq!(header("cache-control"), "private, no-store");
    let body = response.text().await.unwrap();
    let unfolded = body.replace("\r\n ", "");
    let i18n = I18n::new(LANGUAGES[0].code);
    let summary = i18n.tf(
        "events.calendar_title",
        &[("title", "Mail\\, upgrade\\; v2")],
    );
    assert!(unfolded.contains(&format!("SUMMARY:{summary}\r\n")));
    let services = i18n.tf("events.calendar_services", &[("names", "Mail")]);
    let description = format!("DESCRIPTION:Mail stops for two hours.\\n\\n{services}\\n\\n");
    assert!(unfolded.contains(&description));
    assert!(unfolded.contains(&format!("/events/{id}\r\n")));
    assert!(unfolded.contains(&format!("UID:event-{id}@")));
    assert!(body.contains("\r\nDTSTART:"));
    assert!(body.contains("\r\nDTEND:"));
}

#[tokio::test]
async fn google_calendar_is_reached_through_a_redirect() {
    let app = TestApp::spawn_public().await;
    let id = scheduled_maintenance(&app).await;

    let (status, location) = app
        .redirect_of(&format!("/events/{id}/calendar/google"))
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let location = location.unwrap();
    assert!(
        location.starts_with("https://calendar.google.com/calendar/render?action=TEMPLATE&text=")
    );
    assert!(location.contains("&dates="));
}

#[tokio::test]
async fn an_incident_and_a_maintenance_already_over_offer_no_calendar() {
    let app = TestApp::spawn_public().await;
    let author_id = author(&app).await;
    let mail = service(&app, "Mail").await;
    let incident = publish(&app, input(Kind::Incident, author_id, vec![mail])).await;
    let mut over = Vec::new();
    for lifecycle in ["in_progress", "completed", "cancelled"] {
        let id = publish(&app, input(Kind::Maintenance, author_id, vec![mail])).await;
        sqlx::query("UPDATE events SET lifecycle = ? WHERE id = ?")
            .bind(lifecycle)
            .bind(id)
            .execute(&app.pool)
            .await
            .unwrap();
        over.push(id);
    }

    for id in std::iter::once(incident).chain(over) {
        let (_, page) = app.get(&format!("/events/{id}")).await;
        assert!(!page.contains("calendar.ics"));
        let (status, _) = app.get(&format!("/events/{id}/calendar.ics")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = app.get(&format!("/events/{id}/calendar/google")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}

#[tokio::test]
async fn a_maintenance_of_a_restricted_service_stays_hidden_from_visitors() {
    let app = TestApp::spawn_public().await;
    let author_id = author(&app).await;
    let vault = service(&app, "Vault").await;
    let group = GroupRepository::create(&app.pool, "Ops").await.unwrap();
    ServiceRepository::set_visibility(
        &app.pool,
        vault,
        Visibility::Groups,
        Grants::groups(&[group]),
    )
    .await
    .unwrap();
    let id = publish(&app, input(Kind::Maintenance, author_id, vec![vault])).await;

    let (status, _) = app.get(&format!("/events/{id}/calendar.ics")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = app.get(&format!("/events/{id}/calendar/google")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_members_only_page_sends_a_visitor_to_sign_in() {
    let app = TestApp::spawn().await;
    let id = scheduled_maintenance(&app).await;

    for path in [
        format!("/events/{id}/calendar.ics"),
        format!("/events/{id}/calendar/google"),
    ] {
        let (_, location) = app.redirect_of(&path).await;
        assert_eq!(location.as_deref(), Some("/login"));
    }
}
