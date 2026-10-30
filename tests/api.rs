//! Integration tests for the API's guards: a key in the `Authorization`
//! header is the only proof, every refusal is JSON, a session opens
//! nothing, and the API shares the rate limit of the pages.

mod common;

use chrono::Utc;
use reqwest::{Method, StatusCode};
use serde_json::Value;
use std::sync::Arc;

use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use common::TestApp;
use statup::models::{
    Category, ChannelInput, ChannelKind, CreateEventInput, Happening, Kind, Lifecycle, Role,
    Severity, User,
};
use statup::repositories::{
    ApiKeyRepository, EventRepository, NotificationRepository, ServiceRepository,
    SettingsRepository, UserRepository,
};
use statup::services::{ApiKeyService, EventService, NewApiKey};

const ADMIN_EMAIL: &str = "admin@test.com";
const ADMIN_PASSWORD: &str = "admin_password_12";
const SERVICES: &str = "/api/v1/services";
const EVENTS: &str = "/api/v1/events";

struct Answer {
    status: StatusCode,
    headers: reqwest::header::HeaderMap,
    body: Value,
}

async fn admin_key(app: &TestApp, name: &str) -> NewApiKey {
    let id = app
        .create_user(ADMIN_EMAIL, ADMIN_PASSWORD, "Admin", Role::Admin)
        .await;
    let creator = UserRepository::find_by_id(&app.pool, id)
        .await
        .unwrap()
        .expect("the admin exists");
    ApiKeyService::create(&app.pool, name, &creator)
        .await
        .expect("the key is made")
}

async fn call(app: &TestApp, path: &str, authorization: Option<&str>) -> Answer {
    let mut request = app.client.get(app.url(path));
    if let Some(value) = authorization {
        request = request.header("Authorization", value);
    }
    answer(request.send().await.expect("API request failed")).await
}

async fn answer(resp: reqwest::Response) -> Answer {
    let status = resp.status();
    let headers = resp.headers().clone();
    let text = resp.text().await.unwrap_or_default();
    let body = serde_json::from_str(&text).unwrap_or_else(|_| panic!("not JSON: {text}"));
    Answer {
        status,
        headers,
        body,
    }
}

fn bearer(key: &str) -> String {
    format!("Bearer {key}")
}

fn header<'a>(answer: &'a Answer, name: &str) -> Option<&'a str> {
    answer.headers.get(name).and_then(|v| v.to_str().ok())
}

fn error_code(answer: &Answer) -> &str {
    answer.body["error"]["code"].as_str().unwrap_or_default()
}

fn assert_refused(answer: &Answer) {
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(answer), "unauthorized");
    assert_eq!(header(answer, "www-authenticate"), Some("Bearer"));
    assert!(header(answer, "content-type").is_some_and(|v| v.starts_with("application/json")));
    assert_eq!(header(answer, "cache-control"), Some("no-store"));
}

#[tokio::test]
async fn a_key_in_use_reads_the_services_in_their_order() {
    let app = TestApp::spawn().await;
    let made = admin_key(&app, "Zabbix").await;
    ServiceRepository::create(&app.pool, "Mail", "mail", Some("Outlook"), None, None)
        .await
        .unwrap();
    ServiceRepository::create(&app.pool, "VPN", "vpn", None, None, None)
        .await
        .unwrap();

    let answer = call(&app, SERVICES, Some(&bearer(&made.key))).await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(header(&answer, "cache-control"), Some("no-store"));
    let services = answer.body["services"].as_array().expect("a list");
    let names: Vec<&str> = services.iter().filter_map(|s| s["name"].as_str()).collect();
    assert_eq!(names, ["Mail", "VPN"]);
    assert_eq!(services[0]["slug"], "mail");
    assert_eq!(services[0]["description"], "Outlook");
    assert_eq!(services[1]["description"], Value::Null);
    assert_eq!(services[0]["status"], "operational");
    assert_eq!(services[0]["manual_status"], "operational");
    assert_eq!(services[0]["monitored"], false);
    let updated_at = services[0]["updated_at"].as_str().unwrap_or_default();
    assert!(
        updated_at.ends_with('Z') && updated_at.len() == 20,
        "{updated_at}"
    );
}

async fn read(app: &TestApp, path: &str, key: &NewApiKey) -> Answer {
    call(app, path, Some(&bearer(&key.key))).await
}

async fn admin(app: &TestApp) -> User {
    UserRepository::find_by_email(&app.pool, ADMIN_EMAIL)
        .await
        .unwrap()
        .expect("the admin exists")
}

fn incident(title: &str, author: &User, service_ids: Vec<i64>) -> CreateEventInput {
    CreateEventInput {
        kind: Kind::Incident,
        severity: Some(Severity::Critical),
        planned: false,
        category: None,
        title: title.to_string(),
        description: "**Everything** is down".to_string(),
        planned_start: None,
        planned_end: None,
        started_at: None,
        opening_step: None,
        keeps_services_up: false,
        service_ids,
        follows_event_id: None,
        author_id: author.id,
        api_key_id: None,
        next_update: None,
    }
}

fn announcement(title: &str, author: &User) -> CreateEventInput {
    CreateEventInput {
        kind: Kind::Publication,
        severity: None,
        category: Some(Category::Info),
        ..incident(title, author, Vec::new())
    }
}

fn titles(answer: &Answer) -> Vec<&str> {
    answer.body["events"]
        .as_array()
        .expect("a list")
        .iter()
        .filter_map(|event| event["title"].as_str())
        .collect()
}

#[tokio::test]
async fn one_service_is_read_by_its_id() {
    let app = TestApp::spawn().await;
    let made = admin_key(&app, "Zabbix").await;
    let mail = ServiceRepository::create(&app.pool, "Mail", "mail", Some("Outlook"), None, None)
        .await
        .unwrap();

    let answer = read(&app, &format!("{SERVICES}/{}", mail.id), &made).await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.body["id"], mail.id);
    assert_eq!(answer.body["name"], "Mail");
    assert_eq!(answer.body["description"], "Outlook");
    assert_eq!(answer.body["status"], "operational");
    assert_eq!(answer.body["manual_status"], "operational");
    assert_eq!(answer.body["monitored"], false);

    let missing = read(&app, &format!("{SERVICES}/{}", mail.id + 100), &made).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_eq!(error_code(&missing), "not_found");

    let malformed = read(&app, &format!("{SERVICES}/abc"), &made).await;
    assert_eq!(malformed.status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&malformed), "invalid_data");
}

#[tokio::test]
async fn an_open_incident_shows_on_the_status_and_not_on_the_manual_status() {
    let app = TestApp::spawn().await;
    let made = admin_key(&app, "Zabbix").await;
    let author = admin(&app).await;
    let mail = ServiceRepository::create(&app.pool, "Mail", "mail", None, None, None)
        .await
        .unwrap();
    EventService::create(&app.pool, incident("Mail is down", &author, vec![mail.id]))
        .await
        .unwrap();

    let answer = read(&app, &format!("{SERVICES}/{}", mail.id), &made).await;
    assert_eq!(answer.body["status"], "major_outage");
    assert_eq!(answer.body["manual_status"], "operational");
}

#[tokio::test]
async fn events_are_listed_newest_first_and_filtered() {
    let app = TestApp::spawn().await;
    let made = admin_key(&app, "Zabbix").await;
    let author = admin(&app).await;
    let mail = ServiceRepository::create(&app.pool, "Mail", "mail", None, None, None)
        .await
        .unwrap();
    let vpn = ServiceRepository::create(&app.pool, "VPN", "vpn", None, None, None)
        .await
        .unwrap();
    let over = EventService::create(&app.pool, incident("Old outage", &author, vec![vpn.id]))
        .await
        .unwrap();
    EventService::post_update(
        &app.pool,
        over.id,
        "",
        Some(Lifecycle::Resolved),
        None,
        &author,
        None,
    )
    .await
    .unwrap();
    EventService::create(&app.pool, incident("Mail outage", &author, vec![mail.id]))
        .await
        .unwrap();
    EventService::create(&app.pool, announcement("New release", &author))
        .await
        .unwrap();

    let all = read(&app, EVENTS, &made).await;
    assert_eq!(all.status, StatusCode::OK);
    assert_eq!(header(&all, "cache-control"), Some("no-store"));
    assert_eq!(titles(&all), ["New release", "Mail outage", "Old outage"]);
    assert_eq!(all.body["total"], 3);
    let outage = &all.body["events"][1];
    assert_eq!(outage["kind"], "incident");
    assert_eq!(outage["severity"], "critical");
    assert_eq!(outage["category"], Value::Null);
    assert_eq!(outage["state"], "investigating");
    assert_eq!(outage["services"][0]["id"], mail.id);
    assert_eq!(outage["services"][0]["name"], "Mail");
    assert_eq!(all.body["events"][0]["state"], Value::Null);
    assert_eq!(all.body["events"][0]["category"], "info");
    assert!(outage.get("updates").is_none());

    let incidents = read(&app, &format!("{EVENTS}?kind=incident"), &made).await;
    assert_eq!(titles(&incidents), ["Mail outage", "Old outage"]);
    let open = read(&app, &format!("{EVENTS}?open=true"), &made).await;
    assert_eq!(titles(&open), ["Mail outage"]);
    let closed = read(&app, &format!("{EVENTS}?open=false"), &made).await;
    assert_eq!(titles(&closed), ["Old outage"]);

    let page = read(&app, &format!("{EVENTS}?limit=1&offset=1"), &made).await;
    assert_eq!(titles(&page), ["Mail outage"]);
    assert_eq!(page.body["total"], 3);
}

#[tokio::test]
async fn a_bad_filter_is_refused_and_a_page_out_of_range_is_clamped() {
    let app = TestApp::spawn().await;
    let made = admin_key(&app, "Zabbix").await;
    let author = admin(&app).await;
    EventService::create(&app.pool, announcement("New release", &author))
        .await
        .unwrap();

    let unknown = read(&app, &format!("{EVENTS}?kind=nonsense"), &made).await;
    assert_eq!(unknown.status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&unknown), "invalid_data");

    for query in ["limit=0", "limit=1000", "offset=-5", "limit=-1&offset=-1"] {
        let answer = read(&app, &format!("{EVENTS}?{query}"), &made).await;
        assert_eq!(answer.status, StatusCode::OK, "{query}");
        assert_eq!(titles(&answer), ["New release"], "{query}");
    }
}

#[tokio::test]
async fn one_event_comes_with_its_services_and_updates() {
    let app = TestApp::spawn().await;
    let made = admin_key(&app, "Zabbix").await;
    let author = admin(&app).await;
    let mail = ServiceRepository::create(&app.pool, "Mail", "mail", None, None, None)
        .await
        .unwrap();
    let vpn = ServiceRepository::create(&app.pool, "VPN", "vpn", None, None, None)
        .await
        .unwrap();
    let event = EventService::create(
        &app.pool,
        incident("Network outage", &author, vec![mail.id, vpn.id]),
    )
    .await
    .unwrap();
    EventRepository::add_update(
        &app.pool,
        event.id,
        "<p>Looking into it</p>",
        author.id,
        None,
    )
    .await
    .unwrap();
    EventRepository::add_update(
        &app.pool,
        event.id,
        "<p>Back <strong>up</strong></p>",
        author.id,
        None,
    )
    .await
    .unwrap();

    let answer = read(&app, &format!("{EVENTS}/{}", event.id), &made).await;
    assert_eq!(answer.status, StatusCode::OK);
    let body = &answer.body;
    assert_eq!(body["id"], event.id);
    assert_eq!(body["title"], "Network outage");
    assert_eq!(body["description"], "**Everything** is down");
    assert_eq!(body["kind"], "incident");
    assert_eq!(body["state"], "investigating");
    let mut services: Vec<(i64, &str)> = body["services"]
        .as_array()
        .expect("services")
        .iter()
        .map(|s| (s["id"].as_i64().unwrap(), s["name"].as_str().unwrap()))
        .collect();
    services.sort_unstable();
    assert_eq!(services, [(mail.id, "Mail"), (vpn.id, "VPN")]);
    assert!(
        body["started_at"]
            .as_str()
            .is_some_and(|d| d.ends_with('Z'))
    );
    assert_eq!(body["ended_at"], Value::Null);

    let updates = body["updates"].as_array().expect("updates");
    assert_eq!(updates.len(), 2);
    assert_eq!(updates[0]["message"], "Back up");
    assert_eq!(
        updates[0]["message_html"],
        "<p>Back <strong>up</strong></p>"
    );
    assert_eq!(updates[0]["author"], "Admin");
    assert_eq!(updates[1]["message"], "Looking into it");
    assert!(
        updates[0]["created_at"]
            .as_str()
            .is_some_and(|d| d.ends_with('Z'))
    );

    let missing = read(&app, &format!("{EVENTS}/{}", event.id + 100), &made).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_eq!(error_code(&missing), "not_found");
}

#[tokio::test]
async fn every_read_endpoint_wants_a_key() {
    let app = TestApp::spawn().await;
    for path in [
        format!("{SERVICES}/1"),
        EVENTS.to_string(),
        format!("{EVENTS}/1"),
    ] {
        assert_refused(&call(&app, &path, None).await);
    }
}

#[tokio::test]
async fn a_use_of_the_key_is_recorded() {
    let app = TestApp::spawn().await;
    let made = admin_key(&app, "Zabbix").await;
    let before = Utc::now() - chrono::Duration::seconds(1);

    call(&app, SERVICES, Some(&bearer(&made.key))).await;

    let holder = ApiKeyService::holder_of(&app.pool, &made.key)
        .await
        .unwrap()
        .expect("the key is in use");
    assert!(holder.last_used_at.is_some_and(|at| at >= before));
}

#[tokio::test]
async fn a_request_without_a_good_key_is_refused_in_json() {
    let app = TestApp::spawn().await;
    let made = admin_key(&app, "Zabbix").await;
    let unknown = format!("stp_{}", "a".repeat(40));

    for authorization in [
        None,
        Some(bearer(&unknown)),
        Some(bearer("not-a-key")),
        Some("Bearer".to_string()),
        Some(made.key.clone()),
        Some(format!("Basic {}", made.key)),
    ] {
        assert_refused(&call(&app, SERVICES, authorization.as_deref()).await);
    }
}

#[tokio::test]
async fn a_key_in_the_address_is_never_read() {
    let app = TestApp::spawn().await;
    let made = admin_key(&app, "Zabbix").await;

    for query in ["key", "api_key", "access_token", "token"] {
        let path = format!("{SERVICES}?{query}={}", made.key);
        assert_refused(&call(&app, &path, None).await);
    }
}

#[tokio::test]
async fn a_revoked_or_replaced_key_stops_at_once() {
    let app = TestApp::spawn().await;
    let revoked = admin_key(&app, "Zabbix").await;
    ApiKeyRepository::revoke(&app.pool, revoked.id, Utc::now())
        .await
        .unwrap();
    assert_refused(&call(&app, SERVICES, Some(&bearer(&revoked.key))).await);

    let creator = UserRepository::find_by_email(&app.pool, ADMIN_EMAIL)
        .await
        .unwrap()
        .expect("the admin exists");
    let old = ApiKeyService::create(&app.pool, "Grafana", &creator)
        .await
        .unwrap();
    let new = ApiKeyService::regenerate(&app.pool, old.id).await.unwrap();
    assert_refused(&call(&app, SERVICES, Some(&bearer(&old.key))).await);
    let answer = call(&app, SERVICES, Some(&bearer(&new.key))).await;
    assert_eq!(answer.status, StatusCode::OK);
}

/// A key belongs to the instance: disabling the admin who made it does not
/// cut a monitoring tool off without a word.
#[tokio::test]
async fn a_disabled_creator_leaves_the_key_working() {
    let app = TestApp::spawn().await;
    let made = admin_key(&app, "Zabbix").await;
    sqlx::query("UPDATE users SET is_active = 0")
        .execute(&app.pool)
        .await
        .unwrap();

    let answer = call(&app, SERVICES, Some(&bearer(&made.key))).await;
    assert_eq!(answer.status, StatusCode::OK);
}

#[tokio::test]
async fn a_session_opens_nothing_and_the_api_sets_no_cookie() {
    let app = TestApp::spawn().await;
    let made = admin_key(&app, "Zabbix").await;
    app.login(ADMIN_EMAIL, ADMIN_PASSWORD).await;
    assert!(app.session_cookie().is_some(), "the admin is signed in");
    let sessions = app.session_rows().await;

    let refused = call(&app, SERVICES, None).await;
    assert_refused(&refused);
    let allowed = call(&app, SERVICES, Some(&bearer(&made.key))).await;
    assert_eq!(allowed.status, StatusCode::OK);

    for answer in [&refused, &allowed] {
        assert!(answer.headers.get("set-cookie").is_none());
    }
    assert_eq!(app.session_rows().await, sessions);
}

#[tokio::test]
async fn an_address_or_a_method_the_api_does_not_have_answers_in_json() {
    let app = TestApp::spawn().await;
    let made = admin_key(&app, "Zabbix").await;
    let key = bearer(&made.key);

    for path in ["/api/v1/nothing", "/api/v2/services", "/api"] {
        let answer = call(&app, path, Some(&key)).await;
        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{path}");
        assert_eq!(error_code(&answer), "not_found");
    }

    let resp = app
        .client
        .delete(app.url(SERVICES))
        .header("Authorization", &key)
        .send()
        .await
        .unwrap();
    let answer = answer(resp).await;
    assert_eq!(answer.status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(error_code(&answer), "method_not_allowed");
}

#[tokio::test]
async fn a_body_past_the_limit_is_refused_in_json() {
    let app = TestApp::spawn().await;
    let made = admin_key(&app, "Zabbix").await;

    let sent = app
        .client
        .get(app.url(SERVICES))
        .header("Authorization", bearer(&made.key))
        .body(vec![b'a'; 65 * 1024])
        .send()
        .await;

    // The limit answers before the body is read: Windows may then reset the
    // connection under the upload, and the answer is lost.
    let resp = match sent {
        Err(e) if cfg!(windows) => {
            assert!(e.is_request(), "{e}");
            return;
        }
        sent => sent.unwrap(),
    };
    let answer = answer(resp).await;
    assert_eq!(answer.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(error_code(&answer), "payload_too_large");
}

#[tokio::test]
async fn the_api_shares_the_rate_limit_of_the_pages() {
    let app = TestApp::spawn().await;
    let made = admin_key(&app, "Zabbix").await;
    let key = bearer(&made.key);

    // Together, 16 at a time: the budget refills every 600 ms, so a slow
    // sequence would never run out, and 130 connections at once overflow
    // the listen queue.
    let in_flight = Arc::new(Semaphore::new(16));
    let mut requests = JoinSet::new();
    for _ in 0..130 {
        let request = app
            .client
            .get(app.url(SERVICES))
            .header("Authorization", &key);
        let in_flight = Arc::clone(&in_flight);
        requests.spawn(async move {
            let _slot = in_flight.acquire_owned().await.expect("semaphore closed");
            answer(request.send().await.expect("request failed")).await
        });
    }
    let mut limited = Vec::new();
    while let Some(answer) = requests.join_next().await {
        let answer = answer.expect("request task failed");
        if answer.status == StatusCode::TOO_MANY_REQUESTS {
            limited.push(answer);
        }
    }

    let answer = limited
        .first()
        .expect("a burst past 100 requests is limited");
    assert_eq!(error_code(answer), "rate_limited");
    assert!(header(answer, "retry-after").is_some());
    let (status, _) = app.get("/login").await;
    assert_eq!(
        status,
        StatusCode::TOO_MANY_REQUESTS,
        "one budget per address"
    );
}

async fn send(app: &TestApp, method: Method, path: &str, key: &NewApiKey, body: &str) -> Answer {
    let resp = app
        .client
        .request(method, app.url(path))
        .header("Authorization", bearer(&key.key))
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .expect("API request failed");
    answer(resp).await
}

fn english(key: &str) -> String {
    statup::i18n::I18n::new("en").t(key).to_string()
}

fn assert_refusal(answer: &Answer, status: StatusCode, code: &str, key: &str) {
    assert_eq!(answer.status, status);
    assert_eq!(error_code(answer), code);
    assert_eq!(answer.body["error"]["message"], english(key));
}

async fn service_count(app: &TestApp) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM services")
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

/// Kind, actor name, actor id, subject and detail of each Journal line.
type Line = (
    String,
    Option<String>,
    Option<i64>,
    Option<String>,
    Option<String>,
);

async fn journal_lines(app: &TestApp) -> Vec<Line> {
    sqlx::query_as(
        "SELECT kind, actor_name, actor_id, subject, detail FROM journal_entries ORDER BY id",
    )
    .fetch_all(&app.pool)
    .await
    .unwrap()
}

fn line(kind: &str, subject: &str, detail: Option<&str>) -> Line {
    (
        kind.to_string(),
        Some("Zabbix".to_string()),
        None,
        Some(subject.to_string()),
        detail.map(str::to_string),
    )
}

#[tokio::test]
async fn a_key_creates_a_public_unmonitored_service_and_signs_the_journal() {
    let app = TestApp::spawn().await;
    let made = admin_key(&app, "Zabbix").await;

    let body = r#"{"name": "  Mail ", "description": "Outlook"}"#;
    let answer = send(&app, Method::POST, SERVICES, &made, body).await;
    assert_eq!(answer.status, StatusCode::CREATED);
    assert_eq!(answer.body["name"], "Mail");
    assert_eq!(answer.body["slug"], "mail");
    assert_eq!(answer.body["description"], "Outlook");
    assert_eq!(answer.body["status"], "operational");
    assert_eq!(answer.body["manual_status"], "operational");
    assert_eq!(answer.body["monitored"], false);

    let (visibility, folder): (String, Option<i64>) =
        sqlx::query_as("SELECT visibility, folder_id FROM services")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(visibility, "everyone");
    assert_eq!(folder, None);
    assert_eq!(
        journal_lines(&app).await,
        [line("service_created", "Mail", None)]
    );

    let bare = send(&app, Method::POST, SERVICES, &made, r#"{"name": "VPN"}"#).await;
    assert_eq!(bare.status, StatusCode::CREATED);
    assert_eq!(bare.body["description"], Value::Null);
}

#[tokio::test]
async fn a_service_the_rules_refuse_is_not_created() {
    let app = TestApp::spawn().await;
    let made = admin_key(&app, "Zabbix").await;
    let long_name = format!(r#"{{"name": "{}"}}"#, "n".repeat(101));
    let long_description = format!(
        r#"{{"name": "Mail", "description": "{}"}}"#,
        "d".repeat(501)
    );

    for (body, code) in [
        ("{}", "service_name_required"),
        (r#"{"name": "   "}"#, "service_name_required"),
        (long_name.as_str(), "service_name_too_long"),
        (long_description.as_str(), "description_max_length"),
    ] {
        let answer = send(&app, Method::POST, SERVICES, &made, body).await;
        assert_refusal(
            &answer,
            StatusCode::BAD_REQUEST,
            code,
            &format!("validation.{code}"),
        );
    }
    assert_eq!(service_count(&app).await, 0);
    assert!(journal_lines(&app).await.is_empty());
}

#[tokio::test]
async fn a_body_the_api_cannot_read_is_refused_and_creates_nothing() {
    let app = TestApp::spawn().await;
    let made = admin_key(&app, "Zabbix").await;

    let unknown = r#"{"name": "Mail", "visibility": "members"}"#;
    let extra = send(&app, Method::POST, SERVICES, &made, unknown).await;
    assert_eq!(extra.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error_code(&extra), "invalid_data");

    let malformed = send(&app, Method::POST, SERVICES, &made, r#"{"name": "#).await;
    assert_eq!(malformed.status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&malformed), "invalid_data");

    let resp = app
        .client
        .post(app.url(SERVICES))
        .header("Authorization", bearer(&made.key))
        .header("Content-Type", "text/plain")
        .body(r#"{"name": "Mail"}"#)
        .send()
        .await
        .unwrap();
    let plain = answer(resp).await;
    assert_eq!(plain.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(error_code(&plain), "unsupported_media_type");
    assert_eq!(
        plain.body["error"]["message"],
        "Send the body as JSON, with Content-Type: application/json."
    );
    assert_eq!(service_count(&app).await, 0);
}

#[tokio::test]
async fn a_patch_changes_what_it_names_and_keeps_the_rest() {
    let app = TestApp::spawn().await;
    let made = admin_key(&app, "Zabbix").await;
    let mail = ServiceRepository::create(
        &app.pool,
        "Mail",
        "mail",
        Some("Outlook"),
        None,
        Some("server"),
    )
    .await
    .unwrap();
    let path = format!("{SERVICES}/{}", mail.id);

    let renamed = send(&app, Method::PATCH, &path, &made, r#"{"name": "Mailbox"}"#).await;
    assert_eq!(renamed.status, StatusCode::OK);
    assert_eq!(renamed.body["name"], "Mailbox");
    assert_eq!(renamed.body["description"], "Outlook");
    let kept = ServiceRepository::find_by_id(&app.pool, mail.id)
        .await
        .unwrap()
        .expect("the service exists");
    assert_eq!(kept.icon_name.as_deref(), Some("server"));

    let nothing = send(&app, Method::PATCH, &path, &made, "{}").await;
    assert_eq!(nothing.status, StatusCode::OK);
    assert_eq!(nothing.body["name"], "Mailbox");
    assert_eq!(nothing.body["description"], "Outlook");

    let cleared = send(&app, Method::PATCH, &path, &made, r#"{"description": ""}"#).await;
    assert_eq!(cleared.status, StatusCode::OK);
    assert_eq!(cleared.body["name"], "Mailbox");
    assert_eq!(cleared.body["description"], Value::Null);

    let blank = send(&app, Method::PATCH, &path, &made, r#"{"name": " "}"#).await;
    assert_refusal(
        &blank,
        StatusCode::BAD_REQUEST,
        "service_name_required",
        "validation.service_name_required",
    );

    let missing = send(&app, Method::PATCH, &format!("{SERVICES}/999"), &made, "{}").await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_eq!(error_code(&missing), "not_found");

    let lines = journal_lines(&app).await;
    assert_eq!(lines.len(), 3, "the refusals leave nothing");
    assert_eq!(lines[0], line("service_updated", "Mailbox", None));
}

#[tokio::test]
async fn a_status_set_by_a_key_shows_and_is_journaled_with_detail() {
    let app = TestApp::spawn().await;
    let made = admin_key(&app, "Zabbix").await;
    SettingsRepository::set(&app.pool, "journal_detailed", "true")
        .await
        .unwrap();
    let mail = ServiceRepository::create(&app.pool, "Mail", "mail", None, None, None)
        .await
        .unwrap();
    let path = format!("{SERVICES}/{}/status", mail.id);

    let answer = send(
        &app,
        Method::PUT,
        &path,
        &made,
        r#"{"status": "major_outage"}"#,
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.body["status"], "major_outage");
    assert_eq!(answer.body["manual_status"], "major_outage");
    assert_eq!(
        journal_lines(&app).await,
        [line("service_status_set", "Mail", Some("major_outage"))]
    );

    for body in [r#"{"status": "nonsense"}"#, "{}"] {
        let refused = send(&app, Method::PUT, &path, &made, body).await;
        assert_eq!(refused.status, StatusCode::BAD_REQUEST);
        assert_eq!(error_code(&refused), "invalid_data");
    }
    let still = read(&app, &format!("{SERVICES}/{}", mail.id), &made).await;
    assert_eq!(still.body["manual_status"], "major_outage");
    assert_eq!(journal_lines(&app).await.len(), 1);

    let missing = send(
        &app,
        Method::PUT,
        &format!("{SERVICES}/999/status"),
        &made,
        r#"{"status": "degraded"}"#,
    )
    .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn every_write_endpoint_wants_a_key() {
    let app = TestApp::spawn().await;
    for (method, path) in [
        (Method::POST, SERVICES.to_string()),
        (Method::PATCH, format!("{SERVICES}/1")),
        (Method::PUT, format!("{SERVICES}/1/status")),
    ] {
        let resp = app
            .client
            .request(method, app.url(&path))
            .header("Content-Type", "application/json")
            .body(r#"{"name": "Mail"}"#)
            .send()
            .await
            .unwrap();
        assert_refused(&answer(resp).await);
    }
    assert_eq!(service_count(&app).await, 0);
}

const EVENT_JSON: &str = "application/json";

/// The Slack channel an admin would add, taking every kind of event.
async fn add_channel(app: &TestApp) {
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
    NotificationRepository::create_channel(&app.pool, &input)
        .await
        .unwrap();
}

async fn queued(app: &TestApp) -> Vec<Happening> {
    sqlx::query_scalar("SELECT happening FROM notification_deliveries ORDER BY id")
        .fetch_all(&app.pool)
        .await
        .unwrap()
}

async fn event_count(app: &TestApp) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM events")
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

/// Publishes an incident on Mail through the API and returns its id.
async fn publish_incident(app: &TestApp, made: &NewApiKey, mail: i64) -> i64 {
    let body = format!(
        r#"{{"kind": "incident", "title": "Mail down", "severity": "critical", "services": [{mail}]}}"#
    );
    let answer = send(app, Method::POST, EVENTS, made, &body).await;
    assert_eq!(answer.status, StatusCode::CREATED, "{}", answer.body);
    answer.body["id"].as_i64().expect("an id")
}

#[tokio::test]
async fn a_key_publishes_an_incident_as_the_screen_does_signed_with_its_name() {
    let app = TestApp::spawn().await;
    let made = admin_key(&app, "Zabbix").await;
    let creator = admin(&app).await;
    SettingsRepository::set(&app.pool, "journal_detailed", "true")
        .await
        .unwrap();
    add_channel(&app).await;
    let mail = ServiceRepository::create(&app.pool, "Mail", "mail", None, None, None)
        .await
        .unwrap();

    let body = format!(
        r#"{{"kind": "incident", "title": " Mail down ", "description": "Looking.",
            "severity": "critical", "services": [{}], "next_update": "30 min"}}"#,
        mail.id
    );
    let created = send(&app, Method::POST, EVENTS, &made, &body).await;
    assert_eq!(created.status, StatusCode::CREATED);
    assert_eq!(header(&created, "content-type"), Some(EVENT_JSON));
    assert_eq!(created.body["title"], "Mail down");
    assert_eq!(created.body["state"], "investigating");
    assert_eq!(created.body["services"][0]["name"], "Mail");
    assert!(created.body["next_update_at"].is_string());
    let id = created.body["id"].as_i64().expect("an id");
    let signature: (i64, Option<i64>) =
        sqlx::query_as("SELECT author_id, api_key_id FROM events WHERE id = ?")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(signature, (creator.id, Some(made.id)));
    let service = ServiceRepository::find_by_id(&app.pool, mail.id)
        .await
        .unwrap()
        .expect("the service");
    assert_eq!(service.status.as_str(), "major_outage");

    let resolve = r#"{"message": "Back **up**.", "state": "resolved"}"#;
    let path = format!("{EVENTS}/{id}/updates");
    let updated = send(&app, Method::POST, &path, &made, resolve).await;
    assert_eq!(updated.status, StatusCode::CREATED);
    assert_eq!(updated.body["state"], "resolved");
    assert_eq!(updated.body["updates"][0]["author"], "Zabbix");
    assert_eq!(updated.body["updates"][0]["message"], "Back up.");

    assert_eq!(queued(&app).await, [Happening::Opened, Happening::Closed]);
    let events: Vec<Line> = journal_lines(&app)
        .await
        .into_iter()
        .filter(|line| line.0.starts_with("event_"))
        .collect();
    assert_eq!(
        events,
        [
            line("event_created", "Mail down", None),
            line("event_update_posted", "Mail down", Some("resolved")),
        ]
    );
}

#[tokio::test]
async fn the_event_page_names_the_key_not_the_admin_who_made_it() {
    let app = TestApp::spawn().await;
    let made = admin_key(&app, "Zabbix").await;
    let mail = ServiceRepository::create(&app.pool, "Mail", "mail", None, None, None)
        .await
        .unwrap();
    let id = publish_incident(&app, &made, mail.id).await;
    let path = format!("{EVENTS}/{id}/updates");
    let posted = send(&app, Method::POST, &path, &made, r#"{"message": "On it."}"#).await;
    assert_eq!(posted.status, StatusCode::CREATED);

    app.login(ADMIN_EMAIL, ADMIN_PASSWORD).await;
    let (status, page) = app.get(&format!("/events/{id}")).await;
    assert_eq!(status, StatusCode::OK);
    // The opening of the incident and the update, each signed.
    let signed = r#"<span class="moment-author">Zabbix</span>"#;
    assert_eq!(page.matches(signed).count(), 2, "{page}");
    assert!(!page.contains(r#"<span class="moment-author">Admin</span>"#));
}

/// A key acts as an editor even when an admin made it: a closed event stays
/// closed to it.
#[tokio::test]
async fn a_key_cannot_change_a_closed_event_even_when_an_admin_made_it() {
    let app = TestApp::spawn().await;
    let made = admin_key(&app, "Zabbix").await;
    let mail = ServiceRepository::create(&app.pool, "Mail", "mail", None, None, None)
        .await
        .unwrap();
    let id = publish_incident(&app, &made, mail.id).await;
    let path = format!("{EVENTS}/{id}/updates");
    let resolved = send(&app, Method::POST, &path, &made, r#"{"state": "resolved"}"#).await;
    assert_eq!(resolved.status, StatusCode::CREATED);

    let refused = send(
        &app,
        Method::POST,
        &path,
        &made,
        r#"{"message": "One more."}"#,
    )
    .await;
    assert_refusal(
        &refused,
        StatusCode::FORBIDDEN,
        "event_closed_admin_only",
        "validation.event_closed_admin_only",
    );
}

#[tokio::test]
async fn each_kind_keeps_the_fields_that_fit_it() {
    let app = TestApp::spawn().await;
    let made = admin_key(&app, "Zabbix").await;

    let maintenance = r#"{"kind": "maintenance", "title": "Upgrade", "severity": "critical",
        "planned_start": "2040-01-01T09:00:00Z", "planned_end": "2040-01-01T10:00:00Z"}"#;
    let answer = send(&app, Method::POST, EVENTS, &made, maintenance).await;
    assert_eq!(answer.status, StatusCode::CREATED);
    assert_eq!(answer.body["state"], "scheduled");
    assert_eq!(answer.body["severity"], Value::Null);
    assert_eq!(answer.body["planned_start"], "2040-01-01T09:00:00Z");
    assert_eq!(answer.body["planned_end"], "2040-01-01T10:00:00Z");

    let urgent = r#"{"kind": "maintenance", "title": "Reboot"}"#;
    let answer = send(&app, Method::POST, EVENTS, &made, urgent).await;
    assert_eq!(answer.body["state"], "in_progress");

    let note = r#"{"kind": "publication", "title": "New release", "state": "monitoring"}"#;
    let answer = send(&app, Method::POST, EVENTS, &made, note).await;
    assert_eq!(answer.status, StatusCode::CREATED);
    assert_eq!(answer.body["category"], "info");
    assert_eq!(answer.body["state"], Value::Null);

    let further = r#"{"kind": "incident", "title": "Slow", "severity": "minor",
        "state": "monitoring"}"#;
    let answer = send(&app, Method::POST, EVENTS, &made, further).await;
    assert_eq!(answer.body["state"], "monitoring");
}

#[tokio::test]
async fn a_new_event_is_refused_with_the_rule_it_breaks_and_nothing_is_saved() {
    let app = TestApp::spawn().await;
    let made = admin_key(&app, "Zabbix").await;

    let refusals = [
        (
            r#"{"kind": "incident", "severity": "minor"}"#,
            "title_empty",
            "validation.title_empty",
        ),
        (
            r#"{"kind": "incident", "title": "Down"}"#,
            "severity_required",
            "validation.severity_required",
        ),
        (
            r#"{"kind": "incident", "title": "Down", "severity": "minor", "next_update": "soon"}"#,
            "next_update",
            "validation.next_update",
        ),
        (
            r#"{"kind": "incident", "title": "Down", "severity": "minor", "state": "resolved"}"#,
            "invalid_transition",
            "validation.invalid_transition",
        ),
    ];
    for (body, code, key) in refusals {
        let answer = send(&app, Method::POST, EVENTS, &made, body).await;
        assert_refusal(&answer, StatusCode::BAD_REQUEST, code, key);
    }

    let unknown_service = r#"{"kind": "incident", "title": "Down", "severity": "minor",
        "services": [999]}"#;
    let answer = send(&app, Method::POST, EVENTS, &made, unknown_service).await;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&answer), "invalid_data");

    for body in [
        r#"{"kind": "outage", "title": "Down"}"#,
        r#"{"title": "Down"}"#,
        r#"{"kind": "publication", "title": "Note", "author": "Someone"}"#,
    ] {
        let answer = send(&app, Method::POST, EVENTS, &made, body).await;
        assert_eq!(answer.status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(error_code(&answer), "invalid_data");
    }
    assert_eq!(event_count(&app).await, 0);
}

#[tokio::test]
async fn an_update_is_refused_with_the_rule_it_breaks() {
    let app = TestApp::spawn().await;
    let made = admin_key(&app, "Zabbix").await;
    let mail = ServiceRepository::create(&app.pool, "Mail", "mail", None, None, None)
        .await
        .unwrap();
    let incident = publish_incident(&app, &made, mail.id).await;
    let note = send(
        &app,
        Method::POST,
        EVENTS,
        &made,
        r#"{"kind": "publication", "title": "Note"}"#,
    )
    .await;
    let note = note.body["id"].as_i64().expect("an id");

    let refusals = [
        (incident, "{}", "update_required"),
        (incident, r#"{"state": "completed"}"#, "invalid_transition"),
        (note, r#"{"state": "resolved"}"#, "event_has_no_lifecycle"),
    ];
    for (id, body, code) in refusals {
        let path = format!("{EVENTS}/{id}/updates");
        let answer = send(&app, Method::POST, &path, &made, body).await;
        let key = format!("validation.{code}");
        assert_refusal(&answer, StatusCode::BAD_REQUEST, code, &key);
    }

    let missing = format!("{EVENTS}/{}/updates", note + 100);
    let answer = send(&app, Method::POST, &missing, &made, r#"{"message": "Hi"}"#).await;
    assert_eq!(answer.status, StatusCode::NOT_FOUND);
    assert_eq!(error_code(&answer), "not_found");
}

#[tokio::test]
async fn publishing_wants_a_key() {
    let app = TestApp::spawn().await;
    for path in [EVENTS.to_string(), format!("{EVENTS}/1/updates")] {
        let resp = app
            .client
            .post(app.url(&path))
            .header("Content-Type", "application/json")
            .body(r#"{"kind": "publication", "title": "Note"}"#)
            .send()
            .await
            .unwrap();
        assert_refused(&answer(resp).await);
    }
}
