//! Integration tests for the instance time zone: taken from the first
//! administrator's browser, then changed in the settings. The zone lives in
//! process memory, so these tests have a binary of their own and run in
//! one sequence.

mod common;

use reqwest::StatusCode;

use common::{TestApp, extract_csrf_token};

const EMAIL: &str = "owner@example.com";
const PASSWORD: &str = "a long enough password";

async fn register_from(app: &TestApp, zone: &str) {
    let (_, body) = app.get("/register").await;
    let csrf = extract_csrf_token(&body);
    let (status, _, _) = app
        .post_form(
            "/register",
            &csrf,
            &[
                ("email", EMAIL),
                ("password", PASSWORD),
                ("password_confirm", PASSWORD),
                ("display_name", "Owner"),
                ("time_zone", zone),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "register should redirect");
}

async fn set_zone(app: &TestApp, zone: &str) -> (StatusCode, Option<String>) {
    let csrf = app.csrf_from("/admin/settings").await;
    let (status, _, location) = app
        .post_form("/admin/settings/time-zone", &csrf, &[("time_zone", zone)])
        .await;
    (status, location)
}

#[tokio::test]
async fn the_zone_comes_from_the_first_browser_then_from_the_settings() {
    let app = TestApp::spawn().await;
    register_from(&app, "Europe/Paris").await;

    let (_, body) = app.get("/admin/settings").await;
    assert!(
        body.contains(r#"<option value="Europe/Paris" selected>"#),
        "the first administrator's zone should be the instance's"
    );

    let (status, location) = set_zone(&app, "America/New_York").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/admin/settings?zone=1"));
    let (_, body) = app.get("/admin/settings?zone=1").await;
    assert!(
        body.contains("America/New York"),
        "the receipt names the zone"
    );
    assert!(body.contains(r#"<option value="America/New_York" selected>"#));

    let (status, _) = set_zone(&app, "Mars/Olympus").await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "an unknown zone is refused"
    );
    let (_, body) = app.get("/admin/settings").await;
    assert!(body.contains(r#"<option value="America/New_York" selected>"#));
}
