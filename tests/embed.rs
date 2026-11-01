//! The status embed: framed by other sites, it shows what a visitor sees,
//! or what a member sees with the private link, and never a service
//! restricted to a group.

mod common;

use common::TestApp;
use statup::models::{CreateEventInput, Grants, Kind, Role, Severity, Visibility};
use statup::repositories::ServiceRepository;
use statup::services::{EmbedService, EventService};

/// Titles no other text of the embed could contain.
const OPEN_INCIDENT: &str = "Panne Quartz";
const MEMBERS_INCIDENT: &str = "Lenteur Opale";
const RESTRICTED_INCIDENT: &str = "Coffre Zephyr";

async fn service(app: &TestApp, name: &str, visibility: Visibility) -> i64 {
    let id = ServiceRepository::create(&app.pool, name, &name.to_lowercase(), None, None, None)
        .await
        .unwrap()
        .id;
    ServiceRepository::set_visibility(&app.pool, id, visibility, Grants::groups(&[]))
        .await
        .unwrap();
    id
}

async fn incident(app: &TestApp, title: &str, severity: Severity, service_id: i64) {
    let author_id = app
        .create_user(
            &format!("{service_id}@acme.fr"),
            "a long enough passphrase",
            "Ops",
            Role::Admin,
        )
        .await;
    let input = CreateEventInput {
        kind: Kind::Incident,
        severity: Some(severity),
        planned: false,
        category: None,
        title: title.to_string(),
        description: String::new(),
        planned_start: None,
        planned_end: None,
        started_at: None,
        opening_step: None,
        keeps_services_up: false,
        service_ids: vec![service_id],
        follows_event_id: None,
        author_id,
        api_key_id: None,
        next_update: None,
    };
    EventService::create(&app.pool, input).await.unwrap();
}

/// A page with one service and one incident per visibility: the worst,
/// critical one is on the service restricted to a group.
async fn seeded(app: &TestApp) {
    let open = service(app, "Wiki", Visibility::Everyone).await;
    let members = service(app, "Annuaire", Visibility::Members).await;
    let restricted = service(app, "Paie", Visibility::Groups).await;
    incident(app, OPEN_INCIDENT, Severity::Minor, open).await;
    incident(app, MEMBERS_INCIDENT, Severity::Minor, members).await;
    incident(app, RESTRICTED_INCIDENT, Severity::Critical, restricted).await;
}

async fn embed(app: &TestApp, query: &str) -> (reqwest::header::HeaderMap, String) {
    let resp = app
        .client
        .get(app.url(&format!("/embed{query}")))
        .header("accept-language", "fr")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let headers = resp.headers().clone();
    (headers, resp.text().await.unwrap())
}

#[tokio::test]
async fn any_site_may_frame_the_embed_and_only_the_embed() {
    let app = TestApp::spawn_public().await;
    let (headers, _) = embed(&app, "").await;
    assert!(headers.get("x-frame-options").is_none());
    let policy = headers["content-security-policy"].to_str().unwrap();
    assert!(policy.contains("frame-ancestors *"), "{policy}");
    assert!(policy.contains("form-action 'none'"), "{policy}");
    assert_eq!(headers["x-content-type-options"], "nosniff");

    let page = app.get_response("/").await;
    assert_eq!(page.headers()["x-frame-options"], "DENY");
    let policy = page.headers()["content-security-policy"].to_str().unwrap();
    assert!(policy.contains("frame-ancestors 'none'"), "{policy}");
}

#[tokio::test]
async fn a_members_only_page_shows_no_state_without_its_link() {
    let app = TestApp::spawn().await;
    seeded(&app).await;
    for query in ["", "?key=", "?key=0123456789abcdefghijABCDEFGHIJ01"] {
        let (_, body) = embed(&app, query).await;
        for title in [OPEN_INCIDENT, MEMBERS_INCIDENT, RESTRICTED_INCIDENT] {
            assert!(!body.contains(title), "{query}: {title}");
        }
        assert!(
            body.contains("Lien de l&#39;encart à recopier depuis Statup"),
            "{query}"
        );
    }
}

#[tokio::test]
async fn the_private_link_shows_what_a_member_sees_and_never_a_group() {
    let app = TestApp::spawn().await;
    seeded(&app).await;
    let key = EmbedService::key(&app.pool).await.unwrap();
    let (headers, body) = embed(&app, &format!("?key={key}")).await;
    assert!(!body.contains(RESTRICTED_INCIDENT));
    assert!(body.contains("embed-alert"));
    assert!(
        body.contains(OPEN_INCIDENT) || body.contains(MEMBERS_INCIDENT),
        "the worst visible incident leads"
    );
    assert!(
        body.contains("1\u{a0}autre"),
        "the other visible incident is counted, not the hidden one"
    );
    assert_eq!(headers["cache-control"], "private, no-cache");

    EmbedService::regenerate(&app.pool).await.unwrap();
    let (_, body) = embed(&app, &format!("?key={key}")).await;
    assert!(!body.contains(OPEN_INCIDENT), "an old link stops at once");
}

#[tokio::test]
async fn a_public_page_shows_a_visitor_what_the_page_does() {
    let app = TestApp::spawn_public().await;
    seeded(&app).await;
    let (headers, body) = embed(&app, "").await;
    assert!(body.contains(OPEN_INCIDENT));
    assert!(!body.contains(MEMBERS_INCIDENT));
    assert!(!body.contains(RESTRICTED_INCIDENT));
    assert!(
        !body.contains("autre"),
        "nothing else is visible to a visitor"
    );
    assert_eq!(headers["cache-control"], "no-cache");
    assert_eq!(headers["vary"], "Accept-Language");
}

#[tokio::test]
async fn a_calm_page_says_so_and_links_to_the_page() {
    let app = TestApp::spawn_public().await;
    service(&app, "Wiki", Visibility::Everyone).await;
    let (_, body) = embed(&app, "").await;
    assert!(body.contains("Tous les services fonctionnent normalement"));
    assert!(!body.contains("embed-alert"));
    assert!(body.contains(r#"href="/" target="_blank" rel="noopener""#));
    assert!(body.contains(r#"http-equiv="refresh" content="60""#));
}

#[tokio::test]
async fn the_theme_follows_the_link_and_the_words_the_reader() {
    let app = TestApp::spawn_public().await;
    service(&app, "Wiki", Visibility::Everyone).await;

    let (_, light) = embed(&app, "").await;
    assert!(!light.contains(r#"class="dark""#));
    assert!(!light.contains("embed-theme.js"));
    let (_, dark) = embed(&app, "?theme=dark").await;
    assert!(dark.contains(r#"class="dark""#));
    let (_, device) = embed(&app, "?theme=auto").await;
    assert!(!device.contains(r#"class="dark""#));
    assert!(device.contains("embed-theme.js"));

    let english = app
        .client
        .get(app.url("/embed"))
        .header("accept-language", "en")
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(english.contains("View<svg"), "{english}");
}
