//! Integration tests for the first launch: the account, then the page's
//! name and audience, the services it follows, and its address.

mod common;

use reqwest::StatusCode;

use common::{TestApp, extract_csrf_token};
use statup::repositories::{ServiceRepository, SettingsRepository};

async fn register(app: &TestApp) {
    let (_, body) = app.get("/register").await;
    let csrf = extract_csrf_token(&body);
    let (status, _, location) = app
        .post_form(
            "/register",
            &csrf,
            &[
                ("email", "owner@example.com"),
                ("password", "Owner_password_12"),
                ("display_name", "Owner"),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/setup/page"));
}

/// The page step sends its fields as multipart, the logo among them.
fn page_body(csrf: &str, name: &str, access: &str) -> Vec<u8> {
    let field = |key: &str, value: &str| {
        format!("--statup\r\nContent-Disposition: form-data; name=\"{key}\"\r\n\r\n{value}\r\n")
    };
    format!(
        "{}{}{}--statup\r\nContent-Disposition: form-data; name=\"file\"; filename=\"\"\r\n\
         Content-Type: application/octet-stream\r\n\r\n\r\n--statup--\r\n",
        field("csrf_token", csrf),
        field("instance_name", name),
        field("access", access),
    )
    .into_bytes()
}

async fn save_page(app: &TestApp, name: &str, access: &str) -> reqwest::Response {
    let csrf = app.csrf_from("/setup/page").await;
    app.client
        .post(app.url("/setup/page"))
        .header("content-type", "multipart/form-data; boundary=statup")
        .body(page_body(&csrf, name, access))
        .send()
        .await
        .expect("page step failed")
}

#[tokio::test]
async fn the_first_launch_sets_up_the_page_and_its_services() {
    let app = TestApp::spawn().await;
    register(&app).await;

    let response = save_page(&app, "Acme IT", "everyone").await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        response
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok()),
        Some("/setup/services")
    );
    let public = SettingsRepository::get(&app.pool, "public_mode")
        .await
        .expect("db error");
    assert_eq!(public.as_deref(), Some("true"));

    let csrf = app.csrf_from("/setup/services").await;
    let (status, _, location) = app
        .post_form(
            "/setup/services",
            &csrf,
            &[
                ("services", "Messagerie"),
                ("services", "VPN"),
                ("custom", "Sage paie"),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/setup/done"));

    app.post_form("/setup/services", &csrf, &[("services", "messagerie")])
        .await;
    let services = ServiceRepository::list_all(&app.pool)
        .await
        .expect("db error");
    let names: Vec<&str> = services.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        ["Messagerie", "Sage paie", "VPN"],
        "each service once"
    );
    let mail = services
        .iter()
        .find(|s| s.name == "Messagerie")
        .expect("mail");
    assert_eq!(
        mail.icon_name.as_deref(),
        Some("envelope"),
        "a suggestion keeps its icon"
    );

    let (status, done) = app.get("/setup/done").await;
    assert_eq!(status, StatusCode::OK);
    assert!(done.contains("Votre page est prête"));
    assert!(
        done.contains(&format!("value=\"{}\"", app.url("").trim_end_matches('/'))),
        "{done}"
    );
    assert!(
        done.contains("Acme IT"),
        "the preview shows the page's name"
    );
}

#[tokio::test]
async fn a_name_too_long_is_refused_on_the_page_step() {
    let app = TestApp::spawn().await;
    register(&app).await;
    let response = save_page(&app, &"a".repeat(41), "members").await;
    assert_eq!(response.status(), StatusCode::OK, "refused in the page");
    let body = response.text().await.unwrap_or_default();
    assert!(body.contains("form-error"), "{body}");
}

#[tokio::test]
async fn the_setup_steps_are_for_administrators() {
    let app = TestApp::spawn().await;
    for path in ["/setup/page", "/setup/services", "/setup/done"] {
        let (status, _) = app.redirect_of(path).await;
        assert!(
            status.is_client_error() || status.is_redirection(),
            "{path}: {status}"
        );
    }
}

/// The built-in icon of a service, by name.
async fn icon_of(app: &TestApp, name: &str) -> Option<String> {
    ServiceRepository::list_all(&app.pool)
        .await
        .expect("db error")
        .into_iter()
        .find(|s| s.name == name)
        .and_then(|s| s.icon_name)
}

/// The monitored services of an instance, by name, with their target.
async fn checked(app: &TestApp) -> Vec<(String, String)> {
    let services = ServiceRepository::list_all(&app.pool)
        .await
        .expect("db error");
    let mut checked: Vec<(String, String)> = ServiceRepository::list_checked(&app.pool)
        .await
        .expect("db error")
        .into_iter()
        .filter_map(|check| {
            let service = services.iter().find(|s| s.id == check.id)?;
            Some((service.name.clone(), check.target))
        })
        .collect();
    checked.sort();
    checked
}

#[tokio::test]
async fn a_name_typed_without_the_script_keeps_its_monitoring() {
    let app = TestApp::spawn().await;
    register(&app).await;
    let csrf = app.csrf_from("/setup/services").await;

    let (status, _, location) = app
        .post_form(
            "/setup/services",
            &csrf,
            &[
                ("services", "Messagerie"),
                ("custom", "Syspirit"),
                ("check_kind", "http"),
                ("check_url", "https://syspirit.example"),
            ],
        )
        .await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/setup/done"));
    assert_eq!(
        checked(&app).await,
        [(
            "Syspirit".to_string(),
            "https://syspirit.example".to_string()
        )]
    );
    assert_eq!(
        icon_of(&app, "Syspirit").await.as_deref(),
        Some("globe"),
        "a monitored name gets the icon of its kind"
    );
    assert_eq!(
        icon_of(&app, "Messagerie").await.as_deref(),
        Some("envelope"),
        "a suggestion keeps its own"
    );
}

#[tokio::test]
async fn chips_added_with_the_script_carry_their_monitoring() {
    let app = TestApp::spawn().await;
    register(&app).await;
    let csrf = app.csrf_from("/setup/services").await;

    let (status, _, _) = app
        .post_form(
            "/setup/services",
            &csrf,
            &[
                ("services", "Messagerie"),
                ("services", "VPS"),
                ("checks", "VPS"),
                ("check_kinds", "tcp"),
                ("check_urls", ""),
                ("check_hosts", "203.0.113.7"),
                ("check_ports", "22"),
                ("check_certs", "0"),
            ],
        )
        .await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        checked(&app).await,
        [("VPS".to_string(), "203.0.113.7:22".to_string())]
    );
    assert_eq!(icon_of(&app, "VPS").await.as_deref(), Some("server-stack"));
}

#[tokio::test]
async fn a_refused_address_names_its_service_and_creates_nothing() {
    let app = TestApp::spawn().await;
    register(&app).await;
    let csrf = app.csrf_from("/setup/services").await;

    let (status, body, _) = app
        .post_form(
            "/setup/services",
            &csrf,
            &[
                ("services", "Messagerie"),
                ("custom", "NAS"),
                ("check_kind", "tcp"),
                ("check_host", "nas.local"),
                ("check_port", ""),
            ],
        )
        .await;

    assert_eq!(status, StatusCode::OK, "the step is shown again");
    assert!(body.contains("NAS\u{a0}: Saisissez un port entre 1 et 65535"));
    assert!(body.contains(r#"value="NAS""#), "the typed name stays");
    assert!(body.contains(r#"value="nas.local""#), "its host stays");
    assert!(
        ServiceRepository::list_all(&app.pool)
            .await
            .expect("db error")
            .is_empty(),
        "nothing is created"
    );
}

#[tokio::test]
async fn monitoring_lists_of_different_lengths_are_refused() {
    let app = TestApp::spawn().await;
    register(&app).await;
    let csrf = app.csrf_from("/setup/services").await;

    let (status, body, _) = app
        .post_form(
            "/setup/services",
            &csrf,
            &[
                ("services", "VPS"),
                ("checks", "VPS"),
                ("check_kinds", "tcp"),
            ],
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("pas pu être traitée"));
    assert!(
        ServiceRepository::list_all(&app.pool)
            .await
            .expect("db error")
            .is_empty()
    );
}
