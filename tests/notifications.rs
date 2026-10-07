//! Integration tests for the notifications: what the pages queue for the
//! destinations an admin set up.

mod common;

use reqwest::StatusCode;

use common::TestApp;
use statup::models::{ChannelInput, ChannelKind, Happening, Role};
use statup::repositories::NotificationRepository;

impl TestApp {
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
