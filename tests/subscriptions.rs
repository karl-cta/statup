//! Integration tests for email subscriptions: the "Get notified" page and its
//! email card, the links the emails carry, and who may leave when.

mod common;

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use reqwest::StatusCode;
use reqwest::cookie::Jar;
use reqwest::header::{ACCEPT_LANGUAGE, HeaderMap, HeaderValue};
use reqwest::redirect::Policy;

use common::{MailSink, Options, TestApp, extract_csrf_token};
use statup::config::{SmtpConfig, SmtpSecurity};
use statup::i18n::I18n;
use statup::models::{Choices, CreateEventInput, Kind, Role, Severity, Subscriber};
use statup::services::{EventService, Notifier, SubscriptionService, deliver_to_subscribers};

const PUBLIC_URL: &str = "http://status.test";
const EMAIL: &str = "ana@example.com";
const PASSWORD: &str = "a passphrase that is long enough";
const VISITOR_FORM: &str = r#"action="/subscribe/email""#;
const MEMBER_FORM: &str = r#"action="/subscribe/me""#;
const INVALID: &str = r#"aria-invalid="true""#;

type Answer = (StatusCode, String, Option<String>);

/// A browser of its own, reading the page in English. Without cookies it
/// stands for a mail client or a link opened on another device.
struct Browser<'a> {
    app: &'a TestApp,
    client: reqwest::Client,
}

impl<'a> Browser<'a> {
    fn new(app: &'a TestApp) -> Self {
        Self::build(app, true)
    }

    fn without_cookies(app: &'a TestApp) -> Self {
        Self::build(app, false)
    }

    fn build(app: &'a TestApp, cookies: bool) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en"));
        let builder = reqwest::Client::builder()
            .default_headers(headers)
            .redirect(Policy::none());
        let builder = if cookies {
            builder.cookie_provider(Arc::new(Jar::default()))
        } else {
            builder
        };
        Self {
            app,
            client: builder.build().expect("client"),
        }
    }

    async fn get(&self, path: &str) -> Answer {
        let response = self
            .client
            .get(self.app.url(path))
            .send()
            .await
            .expect("GET");
        answer(response).await
    }

    /// Submits a form with the token of a page this browser loaded first.
    async fn submit(&self, page: &str, action: &str, fields: &[(&str, &str)]) -> Answer {
        let (_, html, _) = self.get(page).await;
        let csrf = extract_csrf_token(&html);
        let mut form = vec![("csrf_token", csrf.as_str())];
        form.extend_from_slice(fields);
        let response = self
            .client
            .post(self.app.url(action))
            .form(&form)
            .send()
            .await
            .expect("POST");
        answer(response).await
    }

    /// The visitor form of the page, with the boxes named ticked.
    async fn ask(&self, email: &str, boxes: &[&str]) -> Answer {
        let mut fields = vec![("email", email)];
        fields.extend(boxes.iter().map(|name| (*name, "on")));
        self.submit("/subscribe", "/subscribe/email", &fields).await
    }

    /// A bare POST, as a link page or a mail client sends it: no CSRF token.
    async fn post_link(&self, path: &str) -> Answer {
        let response = self
            .client
            .post(self.app.url(path))
            .form(&[("List-Unsubscribe", "One-Click")])
            .send()
            .await
            .expect("POST");
        answer(response).await
    }

    async fn post_multipart(&self, path: &str) -> Answer {
        let body = "--statup\r\nContent-Disposition: form-data; name=\"List-Unsubscribe\"\r\n\r\n\
                    One-Click\r\n--statup--\r\n";
        let response = self
            .client
            .post(self.app.url(path))
            .header("content-type", "multipart/form-data; boundary=statup")
            .body(body)
            .send()
            .await
            .expect("POST");
        answer(response).await
    }
}

async fn answer(response: reqwest::Response) -> Answer {
    let status = response.status();
    let location = response
        .headers()
        .get("location")
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    let body = response.text().await.unwrap_or_default();
    (status, body, location)
}

fn options(public_mode: bool, mail: Option<&MailSink>) -> Options {
    Options {
        public_mode,
        public_url: Some(PUBLIC_URL),
        mail_port: mail.map(|sink| sink.port),
        ..Options::default()
    }
}

/// A public page that sends email and takes visitors' subscriptions.
async fn open_page() -> (TestApp, MailSink) {
    let sink = MailSink::start().await;
    let app = TestApp::spawn_with(options(true, Some(&sink))).await;
    SubscriptionService::set_visitors_open(&app.pool, true)
        .await
        .expect("visitors welcome");
    (app, sink)
}

/// The journal keeps subscriptions in its detailed capture only.
async fn capture_everything(app: &TestApp) {
    statup::journal::save_settings(&app.pool, true, 30)
        .await
        .expect("detailed capture");
}

/// A confirmation link as the page would have emailed it, without the email.
async fn pending_token(app: &TestApp, email: &str, choices: Choices) -> String {
    match SubscriptionService::request(&app.pool, email, "en", choices).await {
        Ok(Some(token)) => token,
        _ => panic!("no link issued for {email}"),
    }
}

async fn subscribe_visitor(app: &TestApp, email: &str) -> Subscriber {
    let token = pending_token(app, email, Choices::EVERYTHING).await;
    SubscriptionService::confirm(&app.pool, &token)
        .await
        .expect("confirmed")
        .expect("a subscriber")
}

/// The path of a link of a message, once quoted-printable soft breaks and
/// `=3D` are undone.
fn link_in(message: &str, path: &str) -> String {
    let decoded = readable(message);
    let prefix = format!("{PUBLIC_URL}{path}?token=");
    let start = decoded
        .find(&prefix)
        .unwrap_or_else(|| panic!("no {path} link in {decoded}"));
    let token: String = decoded[start + prefix.len()..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '.')
        .collect();
    format!("{path}?token={token}")
}

/// A raw message with its quoted-printable encoding and header folding undone.
fn readable(message: &str) -> String {
    message
        .replace("=\r\n", "")
        .replace("=3D", "=")
        .replace("\r\n ", " ")
}

async fn subscriber_rows(app: &TestApp) -> Vec<(Option<String>, i64, i64, i64)> {
    sqlx::query_as(
        "SELECT email, on_incidents, on_maintenances, on_publications FROM subscribers ORDER BY id",
    )
    .fetch_all(&app.pool)
    .await
    .expect("subscribers read")
}

async fn subscriber_count(app: &TestApp) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM subscribers")
        .fetch_one(&app.pool)
        .await
        .expect("subscribers counted")
}

async fn request_count(app: &TestApp, email: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM subscription_requests WHERE email = ?")
        .bind(email)
        .fetch_one(&app.pool)
        .await
        .expect("requests counted")
}

async fn journal_subjects(app: &TestApp, kind: &str) -> Vec<Option<String>> {
    sqlx::query_scalar("SELECT subject FROM journal_entries WHERE kind = ? ORDER BY id")
        .bind(kind)
        .fetch_all(&app.pool)
        .await
        .expect("journal read")
}

async fn publish_incident(app: &TestApp, author_id: i64) {
    let input = CreateEventInput {
        kind: Kind::Incident,
        severity: Some(Severity::Critical),
        planned: false,
        category: None,
        title: "Mail is down".to_string(),
        description: "Messages are not leaving since 9 am.".to_string(),
        planned_start: None,
        planned_end: None,
        started_at: None,
        opening_step: None,
        keeps_services_up: false,
        service_ids: Vec::new(),
        follows_event_id: None,
        author_id,
        api_key_id: None,
        next_update: None,
    };
    EventService::create(&app.pool, input)
        .await
        .expect("incident created");
}

fn notifier_for(sink: &MailSink) -> Arc<Notifier> {
    let smtp = SmtpConfig {
        host: "127.0.0.1".into(),
        port: sink.port,
        security: SmtpSecurity::None,
        credentials: None,
        from: "Statup <statup@status.test>".into(),
    };
    Arc::new(Notifier::new(Duration::from_secs(5), Some(&smtp)).expect("notifier"))
}

#[tokio::test]
async fn a_visitor_subscribes_through_the_emailed_link() {
    let (app, sink) = open_page().await;
    capture_everything(&app).await;
    let visitor = Browser::new(&app);

    let (status, page, _) = visitor.get("/subscribe").await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains(VISITOR_FORM));
    let (status, body, _) = visitor
        .ask(EMAIL, &["on_incidents", "on_publications"])
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(EMAIL));
    assert!(!body.contains(VISITOR_FORM));

    let messages = sink.wait_for(1).await;
    assert_eq!(messages.len(), 1);
    assert!(readable(&messages[0]).contains(EMAIL));
    let link = link_in(&messages[0], "/subscribe/confirm");
    let (status, page, _) = visitor.get(&link).await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains(&format!(r#"action="{link}""#)));
    assert_eq!(subscriber_count(&app).await, 0);

    let (status, _, _) = Browser::without_cookies(&app).post_link(&link).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        subscriber_rows(&app).await,
        [(Some(EMAIL.to_string()), 1, 0, 1)]
    );
    assert_eq!(
        journal_subjects(&app, "subscription_confirmed").await,
        [Some(EMAIL.to_string())]
    );
}

#[tokio::test]
async fn the_first_news_reaches_the_new_subscriber_with_a_way_out() {
    let (app, sink) = open_page().await;
    let author = app
        .create_user("karl@example.com", PASSWORD, "Karl", Role::Admin)
        .await;
    let subscriber = subscribe_visitor(&app, EMAIL).await;
    publish_incident(&app, author).await;

    let round = deliver_to_subscribers(
        &app.pool,
        &notifier_for(&sink),
        Some(PUBLIC_URL),
        true,
        Utc::now(),
    )
    .await
    .expect("round");

    assert_eq!(round.sent, 1);
    let messages = sink.wait_for(1).await;
    assert_eq!(messages.len(), 1);
    let message = readable(&messages[0]);
    assert!(message.contains(EMAIL));
    let way_out = format!(
        "{PUBLIC_URL}/unsubscribe?token={}",
        SubscriptionService::unsubscribe_token(subscriber.id, &subscriber.unsubscribe_secret)
    );
    assert!(message.contains(&format!("List-Unsubscribe: <{way_out}>")));
    assert!(!message.contains("List-Unsubscribe-Post"));
    let body = message.split_once("\r\n\r\n").map_or("", |(_, body)| body);
    assert!(body.contains(&way_out));
}

#[tokio::test]
async fn the_answer_is_the_same_whatever_the_state_of_the_address() {
    let (app, sink) = open_page().await;
    let visitor = Browser::new(&app);
    let mut bodies = Vec::new();

    // New, then pending twice: each is sent a link. The fourth is not.
    for sent in 1..=3 {
        let (status, body, _) = visitor.ask(EMAIL, &["on_incidents"]).await;
        assert_eq!(status, StatusCode::OK);
        bodies.push(body);
        assert_eq!(sink.wait_for(sent).await.len(), sent);
    }
    let (status, body, _) = visitor.ask(EMAIL, &["on_incidents"]).await;
    assert_eq!(status, StatusCode::OK);
    bodies.push(body);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(sink.received().len(), 3);

    let link = link_in(&sink.received()[2], "/subscribe/confirm");
    let (status, _, _) = Browser::without_cookies(&app).post_link(&link).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(subscriber_count(&app).await, 1);
    let (status, body, _) = visitor.ask(EMAIL, &["on_incidents"]).await;
    assert_eq!(status, StatusCode::OK);
    bodies.push(body);

    let answers: Vec<&str> = bodies.iter().map(|body| main_of(body)).collect();
    assert!(answers.iter().all(|answer| *answer == answers[0]));
}

/// The content of the page, without the masthead, whose clock can turn
/// between two answers.
fn main_of(page: &str) -> &str {
    let start = page.find("<main").expect("a page with a main");
    let end = page.find("</main>").expect("a page with a main");
    &page[start..end]
}

#[tokio::test]
async fn too_many_requests_from_one_client_get_the_same_answer_and_no_email() {
    let (app, sink) = open_page().await;
    let visitor = Browser::new(&app);
    let mut bodies = Vec::new();

    for number in 0..11 {
        let address = format!("reader{number}@example.com");
        let (status, body, _) = visitor.ask(&address, &["on_incidents"]).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains(&address));
        bodies.push(body.replace(&address, "ADDRESS"));
    }

    assert!(bodies.iter().all(|body| *body == bodies[0]));
    sink.wait_for(10).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(sink.received().len(), 10);
}

#[tokio::test]
async fn opening_a_link_changes_nothing() {
    let (app, _sink) = open_page().await;
    let token = pending_token(&app, EMAIL, Choices::EVERYTHING).await;
    let link = format!("/subscribe/confirm?token={token}");
    let browser = Browser::new(&app);

    for _ in 0..2 {
        let (status, page, _) = browser.get(&link).await;
        assert_eq!(status, StatusCode::OK);
        assert!(page.contains(&format!(r#"action="{link}""#)));
    }
    assert_eq!(subscriber_count(&app).await, 0);
    let (status, _, _) = browser.post_link(&link).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(subscriber_count(&app).await, 1);

    let subscriber = subscribe_visitor(&app, "ben@example.com").await;
    let leave = format!(
        "/unsubscribe?token={}",
        SubscriptionService::unsubscribe_token(subscriber.id, &subscriber.unsubscribe_secret)
    );
    let (status, page, _) = browser.get(&leave).await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains(&format!(r#"action="{leave}""#)));
    assert_eq!(subscriber_count(&app).await, 2);
}

#[tokio::test]
async fn a_mail_client_unsubscribes_in_one_click_without_a_session() {
    let (app, _sink) = open_page().await;
    capture_everything(&app).await;
    let ana = subscribe_visitor(&app, EMAIL).await;
    let ben = subscribe_visitor(&app, "ben@example.com").await;
    let leave = |subscriber: &Subscriber| {
        format!(
            "/unsubscribe?token={}",
            SubscriptionService::unsubscribe_token(subscriber.id, &subscriber.unsubscribe_secret)
        )
    };
    let client = Browser::without_cookies(&app);

    let (status, real, _) = client.post_link(&leave(&ana)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(subscriber_count(&app).await, 1);
    let (status, _, _) = client.post_multipart(&leave(&ben)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(subscriber_count(&app).await, 0);

    assert_eq!(
        journal_subjects(&app, "subscription_ended").await,
        [Some(EMAIL.to_string()), Some("ben@example.com".to_string())]
    );
    let unknown = format!("/unsubscribe?token=999.{}", "a".repeat(32));
    let (status, nobody, _) = client.post_link(&unknown).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(nobody, real);
}

#[tokio::test]
async fn a_members_only_page_keeps_visitors_out_but_lets_them_leave() {
    let sink = MailSink::start().await;
    let app = TestApp::spawn_with(options(false, Some(&sink))).await;
    SubscriptionService::set_visitors_open(&app.pool, true)
        .await
        .expect("visitors welcome");
    app.create_user("karl@example.com", PASSWORD, "Karl", Role::Admin)
        .await;
    let ana = subscribe_visitor(&app, EMAIL).await;
    let visitor = Browser::new(&app);

    let (status, _, location) = visitor.get("/subscribe").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/login"));
    let fields = [("email", "ben@example.com"), ("on_incidents", "on")];
    let (status, _, location) = visitor.submit("/login", "/subscribe/email", &fields).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/subscribe"));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(request_count(&app, "ben@example.com").await, 0);
    assert!(sink.received().is_empty());

    let leave = format!(
        "/unsubscribe?token={}",
        SubscriptionService::unsubscribe_token(ana.id, &ana.unsubscribe_secret)
    );
    let (status, _, _) = Browser::without_cookies(&app).post_link(&leave).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(subscriber_count(&app).await, 0);
}

#[tokio::test]
async fn closed_subscriptions_show_no_form_and_confirm_nothing() {
    let sink = MailSink::start().await;
    let app = TestApp::spawn_with(options(true, Some(&sink))).await;
    let browser = Browser::new(&app);
    let (status, page, _) = browser.get("/subscribe").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!page.contains(VISITOR_FORM));

    SubscriptionService::set_visitors_open(&app.pool, true)
        .await
        .expect("open");
    let token = pending_token(&app, EMAIL, Choices::EVERYTHING).await;
    SubscriptionService::set_visitors_open(&app.pool, false)
        .await
        .expect("closed");

    let link = format!("/subscribe/confirm?token={token}");
    let closed = I18n::new("en").t("subscription.closed_title").to_string();
    let (status, page, _) = browser.get(&link).await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains(&closed));
    let (status, page, _) = browser.post_link(&link).await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains(&closed));
    assert_eq!(subscriber_count(&app).await, 0);
}

#[tokio::test]
async fn a_member_subscribes_changes_and_stops_from_the_page() {
    let sink = MailSink::start().await;
    let app = TestApp::spawn_with(options(false, Some(&sink))).await;
    let member = app.create_user(EMAIL, PASSWORD, "Ana", Role::Reader).await;
    app.login(EMAIL, PASSWORD).await;

    let (status, page) = app.get("/subscribe").await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains(MEMBER_FORM) && page.contains(EMAIL));
    assert!(!page.contains(VISITOR_FORM));
    let csrf = extract_csrf_token(&page);

    let save = [("action", "save"), ("on_incidents", "on")];
    let (status, _, location) = app.post_form("/subscribe/me", &csrf, &save).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/subscribe?done=saved"));
    let stored: Vec<(Option<i64>, i64, i64, i64)> = sqlx::query_as(
        "SELECT user_id, on_incidents, on_maintenances, on_publications FROM subscribers",
    )
    .fetch_all(&app.pool)
    .await
    .expect("subscribers read");
    assert_eq!(stored, [(Some(member), 1, 0, 0)]);

    let stop = [("action", "stop")];
    let (status, _, location) = app.post_form("/subscribe/me", &csrf, &stop).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/subscribe?done=stopped"));
    assert_eq!(subscriber_count(&app).await, 0);

    app.post_form("/subscribe/me", &csrf, &save).await;
    assert_eq!(subscriber_count(&app).await, 1);
    let (status, _, location) = app
        .post_form("/subscribe/me", &csrf, &[("action", "save")])
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/subscribe?done=stopped"));
    assert_eq!(subscriber_count(&app).await, 0);
}

#[tokio::test]
async fn without_a_mail_server_there_is_no_email_card() {
    let app = TestApp::spawn_with(options(true, None)).await;
    SubscriptionService::set_visitors_open(&app.pool, true)
        .await
        .expect("visitors welcome");
    app.create_user(EMAIL, PASSWORD, "Ana", Role::Reader).await;

    let (status, page) = app.get("/subscribe").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!page.contains(VISITOR_FORM) && !page.contains(MEMBER_FORM));

    app.login(EMAIL, PASSWORD).await;
    let (status, page) = app.get("/subscribe").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!page.contains(VISITOR_FORM) && !page.contains(MEMBER_FORM));
}

#[tokio::test]
async fn a_wrong_address_or_no_box_is_said_on_the_form_and_nothing_is_sent() {
    let (app, sink) = open_page().await;
    let visitor = Browser::new(&app);

    let (status, body, _) = visitor.ask("not an address", &["on_incidents"]).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(VISITOR_FORM));
    assert!(body.contains(INVALID));

    let (status, body, _) = visitor.ask(EMAIL, &[]).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(VISITOR_FORM));
    assert!(body.contains(EMAIL));
    assert!(!body.contains(INVALID));

    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(sink.received().is_empty());
    assert_eq!(request_count(&app, EMAIL).await, 0);
}
