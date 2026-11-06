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
    ServiceRepository::set_visibility(&app.pool, id, visibility, Grants::default())
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

/// Two page requests a minute, so twenty for the embed, given back one every
/// three seconds: the burst outlasts nothing, even on a slow machine.
#[tokio::test]
async fn the_embed_has_a_larger_limit_of_its_own() {
    let app = TestApp::spawn_with(common::Options {
        public_mode: true,
        requests_per_minute: Some(2),
        ..common::Options::default()
    })
    .await;
    let mut served = 0;
    let mut refused = 0;
    for _ in 0..30 {
        match app.get_response("/embed").await.status() {
            reqwest::StatusCode::OK => served += 1,
            reqwest::StatusCode::TOO_MANY_REQUESTS => refused += 1,
            other => panic!("unexpected {other}"),
        }
    }
    assert!(served >= 20, "{served} served, past the pages' budget");
    assert!(refused > 0, "a flood is limited all the same");
    assert_eq!(
        app.get_response("/").await.status(),
        reqwest::StatusCode::OK,
        "the pages keep their own budget"
    );
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
            body.contains("Lien du widget à recopier depuis Statup"),
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
    assert_eq!(headers["cache-control"], "no-cache, private");

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
async fn the_theme_and_the_language_follow_the_link_then_the_reader() {
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

    let (_, chosen) = embed(&app, "?lang=en").await;
    assert!(
        chosen.contains("View<svg"),
        "the address's language wins over the reader's"
    );
    let (_, unknown) = embed(&app, "?lang=xx").await;
    assert!(
        unknown.contains("Voir<svg"),
        "an unknown language falls back to the reader's"
    );
}

async fn admin(app: &TestApp) {
    app.create_user(
        "karl@acme.fr",
        "a long enough passphrase",
        "Karl",
        Role::Admin,
    )
    .await;
    app.login("karl@acme.fr", "a long enough passphrase").await;
}

#[tokio::test]
async fn the_settings_give_the_address_and_the_code_with_the_private_link() {
    let app = TestApp::spawn_with(common::Options {
        public_url: Some("https://status.acme.fr"),
        ..common::Options::default()
    })
    .await;
    admin(&app).await;
    let (status, settings) = app.get("/admin/settings").await;
    assert_eq!(status, reqwest::StatusCode::OK);
    assert!(settings.contains(r#"href="/admin/embed""#));

    let resp = app.get_response("/admin/embed").await;
    assert_eq!(resp.headers()["cache-control"], "no-store");
    let page = resp.text().await.unwrap();
    let key = EmbedService::key(&app.pool).await.unwrap();
    let address = format!("https://status.acme.fr/embed?key={key}&#38;lang=fr");
    assert!(page.contains(&format!(r#"value="{address}""#)), "{page}");
    assert!(
        page.contains("height=&#34;68&#34;"),
        "the code says the height"
    );
    assert!(
        page.contains("&#38;amp;lang=fr"),
        "the code keeps the entity an administrator reads"
    );
    assert!(
        page.contains(&format!(
            r#"<iframe class="embed-frame" src="/embed?key={key}&#38;lang=fr""#
        )),
        "the preview is the embed itself"
    );
    assert!(page.contains(r#"aria-current="true">Clair<"#));

    let (_, dark) = app.get("/admin/embed?theme=dark").await;
    assert!(
        dark.contains(&format!(r#"value="{address}&#38;theme=dark""#)),
        "{dark}"
    );
    assert!(dark.contains(&format!(
        r#"src="/embed?key={key}&#38;lang=fr&#38;theme=dark""#
    )));
}

#[tokio::test]
async fn only_an_administrator_opens_the_embed_settings() {
    let app = TestApp::spawn().await;
    app.create_user(
        "lea@acme.fr",
        "a long enough passphrase",
        "Léa",
        Role::Publisher,
    )
    .await;
    app.login("lea@acme.fr", "a long enough passphrase").await;
    let resp = app.get_response("/admin/embed").await;
    assert_ne!(resp.status(), reqwest::StatusCode::OK);
}

#[tokio::test]
async fn changing_the_link_stops_the_old_one_and_is_journaled() {
    let app = TestApp::spawn().await;
    admin(&app).await;
    let csrf = app.csrf_from("/admin/embed").await;
    let old = EmbedService::key(&app.pool).await.unwrap();

    let (status, _, location) = app.post_form("/admin/embed/link", &csrf, &[]).await;
    assert_eq!(status, reqwest::StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/admin/embed?changed=1"));
    assert!(!EmbedService::grants(&app.pool, &old).await.unwrap());
    let (_, page) = app.get("/admin/embed?changed=1").await;
    assert!(page.contains("Lien changé."));

    let journaled: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM journal_entries WHERE kind = 'embed_link_changed'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(journaled, 1);
}
