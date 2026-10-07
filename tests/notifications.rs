//! Integration tests for the notifications: what the pages queue for the
//! destinations an admin set up.

mod common;

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};

use reqwest::StatusCode;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use common::TestApp;
use statup::models::{ChannelInput, ChannelKind, Happening, Role};
use statup::repositories::{NotificationRepository, SettingsRepository};

impl TestApp {
    async fn as_admin(&self) {
        self.create_user(
            "admin@example.com",
            "admin_password_12",
            "Admin",
            Role::Admin,
        )
        .await;
        self.login("admin@example.com", "admin_password_12").await;
    }

    async fn destinations(&self) -> Vec<(String, String)> {
        sqlx::query_as("SELECT name, target FROM notification_channels ORDER BY id")
            .fetch_all(&self.pool)
            .await
            .expect("destinations read")
    }

    async fn as_publisher(&self) {
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

    async fn add_destination(&self) {
        let input = ChannelInput {
            name: "IT team".to_string(),
            kind: ChannelKind::Slack,
            target: "https://hooks.slack.com/services/team".to_string(),
            locale: "en".to_string(),
            on_incidents: true,
            on_maintenances: true,
            on_publications: true,
            on_detected: false,
        };
        NotificationRepository::create_channel(&self.pool, &input)
            .await
            .expect("destination saved");
    }

    async fn queued(&self) -> Vec<Happening> {
        sqlx::query_scalar("SELECT happening FROM notification_deliveries ORDER BY id")
            .fetch_all(&self.pool)
            .await
            .expect("queue read")
    }
}

#[tokio::test]
async fn an_incident_declared_then_resolved_on_the_pages_queues_two_messages() {
    let app = TestApp::spawn().await;
    app.as_publisher().await;
    app.add_destination().await;

    let csrf = app.csrf_from("/events/new").await;
    let fields = [
        ("title", "Database outage"),
        ("description", "The primary database is down"),
        ("kind", "incident"),
        ("severity", "critical"),
    ];
    let (status, _, location) = app.post_form("/events/new", &csrf, &fields).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let path = location.expect("redirects to the event");
    let event_id = path
        .split('?')
        .next()
        .and_then(|p| p.rsplit('/').next())
        .expect("event path");

    let csrf = app.csrf_from(&path).await;
    let resolve = [("message", "Back up."), ("lifecycle", "resolved")];
    let (status, _, _) = app
        .post_form(&format!("/events/{event_id}/updates"), &csrf, &resolve)
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    assert_eq!(app.queued().await, [Happening::Opened, Happening::Closed]);
}

/// A tool that answers every post with `status` and keeps what it received.
async fn tool(status: &'static str) -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("address");
    let received = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&received);
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut buffer = vec![0u8; 65536];
            let read = stream.read(&mut buffer).await.unwrap_or(0);
            let request = String::from_utf8_lossy(&buffer[..read]).to_string();
            kept.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(request);
            let response =
                format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            let _ = stream.write_all(response.as_bytes()).await;
        }
    });
    (addr, received)
}

fn destination_fields<'a>(
    name: &'a str,
    kind: &'a str,
    target: &'a str,
) -> Vec<(&'a str, &'a str)> {
    vec![
        ("name", name),
        ("kind", kind),
        ("target", target),
        ("locale", "en"),
        ("on_incidents", "on"),
    ]
}

#[tokio::test]
async fn only_an_admin_reaches_the_destinations() {
    let app = TestApp::spawn().await;
    let (status, _) = app.get("/admin/notifications").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    app.as_publisher().await;
    let (status, _) = app.get("/admin/notifications").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let csrf = app.csrf_from("/").await;
    let fields = destination_fields("Team", "webhook", "https://example.com/hook");
    let (status, _, _) = app.post_form("/admin/notifications", &csrf, &fields).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(app.destinations().await.is_empty());
}

#[tokio::test]
async fn an_admin_adds_a_destination_and_finds_it_listed() {
    let app = TestApp::spawn().await;
    app.as_admin().await;

    let csrf = app.csrf_from("/admin/notifications").await;
    let fields = destination_fields(" IT team ", "slack", " https://hooks.slack.com/services/a ");
    let (status, _, location) = app.post_form("/admin/notifications", &csrf, &fields).await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(location.is_some_and(|l| l.starts_with("/admin/notifications?added=")));
    assert_eq!(
        app.destinations().await,
        [(
            "IT team".to_string(),
            "https://hooks.slack.com/services/a".to_string()
        )]
    );
    let (_, body) = app.get("/admin/notifications").await;
    assert!(body.contains("IT team"));
    let (_, settings) = app.get("/admin/settings").await;
    assert!(settings.contains("/admin/notifications"));
    let address = SettingsRepository::get(&app.pool, "page_address")
        .await
        .expect("read");
    assert_eq!(address.as_deref(), Some(app.url("").trim_end_matches('/')));
}

#[tokio::test]
async fn a_refused_address_is_said_and_the_form_kept() {
    let app = TestApp::spawn().await;
    app.as_admin().await;

    let csrf = app.csrf_from("/admin/notifications").await;
    let fields = destination_fields("Sales", "discord", "http://discord.com/api/webhooks/1");
    let (status, body, _) = app.post_form("/admin/notifications", &csrf, &fields).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("target-error"));
    assert!(body.contains("value=\"Sales\""));
    assert!(app.destinations().await.is_empty());
}

#[tokio::test]
async fn the_test_button_posts_to_the_tool_and_says_how_it_went() {
    let app = TestApp::spawn().await;
    app.as_admin().await;
    let (working, received) = tool("200 OK").await;
    let (missing, _) = tool("404 Not Found").await;

    let csrf = app.csrf_from("/admin/notifications").await;
    let target = format!("http://{working}/hook");
    let fields = destination_fields("", "webhook", &target);
    let (status, sent, _) = app
        .post_form("/admin/notifications/test", &csrf, &fields)
        .await;
    let target = format!("http://{missing}/hook");
    let fields = destination_fields("", "webhook", &target);
    let (_, refused, _) = app
        .post_form("/admin/notifications/test", &csrf, &fields)
        .await;

    assert_eq!(status, StatusCode::OK);
    assert!(sent.contains("data-tone=\"ok\""));
    assert!(refused.contains("data-tone=\"crit\""));
    assert!(refused.contains("404"));
    let requests = received
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /hook"));
    assert!(requests[0].contains("\"happening\":\"test\""));
    assert!(app.destinations().await.is_empty(), "a test saves nothing");
}

#[tokio::test]
async fn a_destination_is_changed_then_deleted() {
    let app = TestApp::spawn().await;
    app.as_admin().await;
    app.add_destination().await;
    let id: i64 = sqlx::query_scalar("SELECT id FROM notification_channels")
        .fetch_one(&app.pool)
        .await
        .expect("destination id");

    let path = format!("/admin/notifications/{id}");
    let csrf = app.csrf_from(&path).await;
    let fields = destination_fields("Help desk", "mattermost", "http://chat.local/hooks/x");
    let (status, _, location) = app.post_form(&path, &csrf, &fields).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        location.as_deref(),
        Some(format!("/admin/notifications?saved={id}").as_str())
    );
    assert_eq!(app.destinations().await[0].0, "Help desk");

    let (status, _, _) = app.post_form(&format!("{path}/delete"), &csrf, &[]).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(app.destinations().await.is_empty());
    let (status, _) = app.get(&path).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_page_address_is_saved_or_refused() {
    let app = TestApp::spawn().await;
    app.as_admin().await;

    let csrf = app.csrf_from("/admin/notifications").await;
    let (status, body, _) = app
        .post_form(
            "/admin/notifications/page-address",
            &csrf,
            &[("page_address", "status.example.com")],
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("address-error"));

    let (status, _, _) = app
        .post_form(
            "/admin/notifications/page-address",
            &csrf,
            &[("page_address", "https://status.example.com/")],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let address = SettingsRepository::get(&app.pool, "page_address")
        .await
        .expect("read");
    assert_eq!(address.as_deref(), Some("https://status.example.com"));
}
