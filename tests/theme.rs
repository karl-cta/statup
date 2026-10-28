//! Integration tests for the accent colour and the ambiance chosen in the
//! settings. Both live in process memory, so every assertion that reads the
//! saved theme sits in one test, run in sequence; the other tests here only
//! check refusals, which save nothing.

mod common;

use reqwest::StatusCode;

use common::TestApp;
use statup::i18n::I18n;
use statup::models::Role;

const EMAIL: &str = "owner@example.com";
const PASSWORD: &str = "a long enough password";

async fn spawn_with_admin() -> TestApp {
    let app = TestApp::spawn().await;
    app.create_user(EMAIL, PASSWORD, "Owner", Role::Admin).await;
    app.login(EMAIL, PASSWORD).await;
    app
}

/// The opening `<html …>` tag of a page.
fn html_tag(body: &str) -> &str {
    let start = body.find("<html").expect("page has an html tag");
    let end = body[start..].find('>').expect("html tag is closed");
    &body[start..start + end]
}

/// The refusal text as a page prints it, apostrophes escaped.
fn refusal_text() -> String {
    I18n::default()
        .t("validation.unknown_theme")
        .replace('\'', "&#39;")
}

async fn assert_theme_on_pages(app: &TestApp, accent: Option<&str>, ambiance: Option<&str>) {
    for path in ["/", "/admin/settings"] {
        let (status, body) = app.get(path).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        let tag = html_tag(&body);
        for (name, value) in [("data-accent", accent), ("data-ambiance", ambiance)] {
            match value {
                Some(value) => assert!(
                    tag.contains(&format!(r#" {name}="{value}""#)),
                    "{path}: {tag}"
                ),
                None => assert!(!tag.contains(name), "{path}: {tag}"),
            }
        }
    }
}

async fn assert_checked_radios(app: &TestApp, accent: &str, ambiance: &str) {
    let (_, body) = app.get("/admin/settings").await;
    assert!(
        body.contains(&format!(
            r#"name="accent" value="{accent}" class="sr-only" checked"#
        )),
        "the {accent} accent should be checked"
    );
    assert!(
        body.contains(&format!(
            r#"name="ambiance" value="{ambiance}" class="sr-only" checked"#
        )),
        "the {ambiance} ambiance should be checked"
    );
    assert_eq!(body.matches(r#"name="accent""#).count(), 12);
    assert_eq!(body.matches(r#"name="ambiance""#).count(), 4);
}

async fn journal_details(app: &TestApp) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT detail FROM journal_entries WHERE kind = 'settings_saved' ORDER BY id",
    )
    .fetch_all(&app.pool)
    .await
    .expect("failed to read the journal")
}

/// The setup's page step, which sends no theme fields.
async fn save_setup_page(app: &TestApp) -> StatusCode {
    let csrf = app.csrf_from("/setup/page").await;
    let field = |key: &str, value: &str| {
        format!("--statup\r\nContent-Disposition: form-data; name=\"{key}\"\r\n\r\n{value}\r\n")
    };
    let body = format!(
        "{}{}{}--statup--\r\n",
        field("csrf_token", &csrf),
        field("instance_name", "Acme"),
        field("access", "members"),
    );
    app.client
        .post(app.url("/setup/page"))
        .header("content-type", "multipart/form-data; boundary=statup")
        .body(body.into_bytes())
        .send()
        .await
        .expect("page step failed")
        .status()
}

#[tokio::test]
async fn the_theme_is_chosen_in_the_settings_and_kept_until_changed() {
    let app = spawn_with_admin().await;

    assert_theme_on_pages(&app, None, None).await;
    assert_checked_radios(&app, "ink", "sand").await;

    let (status, _, location) = app
        .save_settings(&[("accent", "navy"), ("ambiance", "pebble")], None)
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/admin/settings?saved=1"));
    assert_theme_on_pages(&app, Some("navy"), Some("pebble")).await;
    assert_checked_radios(&app, "navy", "pebble").await;
    assert_eq!(
        journal_details(&app).await,
        ["accent, ambiance".to_string()],
        "the journal names the two fields"
    );

    let (status, body, _) = app.save_settings(&[("accent", "neon")], None).await;
    assert_eq!(status, StatusCode::OK, "an unknown accent is refused");
    assert!(body.contains(&refusal_text()), "{body}");
    assert_theme_on_pages(&app, Some("navy"), Some("pebble")).await;

    assert_eq!(save_setup_page(&app).await, StatusCode::SEE_OTHER);
    assert_theme_on_pages(&app, Some("navy"), Some("pebble")).await;

    let (status, _, _) = app.save_settings(&[("accent", "cobalt")], None).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_theme_on_pages(&app, Some("cobalt"), Some("pebble")).await;

    let (status, _, _) = app
        .save_settings(&[("accent", "ink"), ("ambiance", "sand")], None)
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_theme_on_pages(&app, None, None).await;
    assert_checked_radios(&app, "ink", "sand").await;
}

#[tokio::test]
async fn an_unknown_ambiance_is_refused_in_the_page() {
    let app = spawn_with_admin().await;

    let (status, body, location) = app.save_settings(&[("ambiance", "neon")], None).await;

    assert_eq!((status, location), (StatusCode::OK, None));
    assert!(body.contains(&refusal_text()), "{body}");
}

#[tokio::test]
async fn a_refused_save_keeps_the_colour_and_ambiance_it_sent() {
    let app = spawn_with_admin().await;
    let long_name = "A".repeat(41);

    let (status, body, _) = app
        .save_settings(
            &[
                ("instance_name", long_name.as_str()),
                ("accent", "navy"),
                ("ambiance", "pebble"),
            ],
            None,
        )
        .await;

    assert_eq!(status, StatusCode::OK, "the long name is refused");
    assert!(body.contains(r#"name="accent" value="navy" class="sr-only" checked"#));
    assert!(body.contains(r#"name="ambiance" value="pebble" class="sr-only" checked"#));
}
