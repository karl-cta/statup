//! Integration tests for "Forgot password?": the link by email, its single
//! use, and what each page says.

mod common;

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::cookie::Jar;
use reqwest::redirect::Policy;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

use common::{Options, TestApp, extract_csrf_token};
use statup::models::Role;

const PUBLIC_URL: &str = "http://status.test";
const EMAIL: &str = "ana@example.com";
const OLD_PASSWORD: &str = "the old passphrase, long enough";
const NEW_PASSWORD: &str = "a brand new passphrase to keep";
const FORM: &str = r#"action="/password/forgot""#;
const RESET_FORM: &str = r#"action="/password/reset""#;
const TOKEN_INPUT: &str = r#"name="token""#;

type Mailbox = Arc<Mutex<Vec<String>>>;

/// A mail server that accepts everything and keeps each message it receives.
struct MailSink {
    port: u16,
    messages: Mailbox,
}

impl MailSink {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("address").port();
        let messages = Mailbox::default();
        let kept = Arc::clone(&messages);
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(converse(stream, Arc::clone(&kept)));
            }
        });
        Self { port, messages }
    }

    fn received(&self) -> Vec<String> {
        self.messages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Waits up to five seconds for `count` messages and returns what arrived.
    async fn wait_for(&self, count: usize) -> Vec<String> {
        for _ in 0..100 {
            if self.received().len() >= count {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        self.received()
    }
}

async fn converse(stream: TcpStream, messages: Mailbox) {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader);
    if writer.write_all(b"220 sink ready\r\n").await.is_err() {
        return;
    }
    let mut line = String::new();
    loop {
        line.clear();
        match lines.read_line(&mut line).await {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let command = line.trim_end().to_ascii_uppercase();
        let reply: &[u8] = if command.starts_with("QUIT") {
            let _ = writer.write_all(b"221 bye\r\n").await;
            return;
        } else if command.starts_with("DATA") {
            let _ = writer.write_all(b"354 go ahead\r\n").await;
            let message = read_message(&mut lines).await;
            messages
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(message);
            b"250 queued\r\n"
        } else {
            b"250 ok\r\n"
        };
        if writer.write_all(reply).await.is_err() {
            return;
        }
    }
}

async fn read_message<R: AsyncBufReadExt + Unpin>(lines: &mut R) -> String {
    let mut message = String::new();
    let mut line = String::new();
    loop {
        line.clear();
        match lines.read_line(&mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) if line == ".\r\n" => break,
            Ok(_) => message.push_str(&line),
        }
    }
    message
}

/// The link of a message, once quoted-printable soft breaks and `=3D` are undone.
fn link_in(message: &str) -> String {
    let decoded = message.replace("=\r\n", "").replace("=3D", "=");
    let prefix = format!("{PUBLIC_URL}/password/reset?token=");
    let start = decoded
        .find(&prefix)
        .unwrap_or_else(|| panic!("no reset link in {decoded}"));
    let token: String = decoded[start + prefix.len()..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '.')
        .collect();
    format!("/password/reset?token={token}")
}

/// A browser of its own: cookies are not shared with the app's client.
struct Visitor<'a> {
    app: &'a TestApp,
    client: reqwest::Client,
}

impl<'a> Visitor<'a> {
    fn new(app: &'a TestApp) -> Self {
        let client = reqwest::Client::builder()
            .cookie_provider(Arc::new(Jar::default()))
            .redirect(Policy::none())
            .build()
            .expect("client");
        Self { app, client }
    }

    async fn get(&self, path: &str) -> (StatusCode, String, Option<String>) {
        let response = self
            .client
            .get(self.app.url(path))
            .send()
            .await
            .expect("GET");
        answer(response).await
    }

    /// Submits a form with the token of the sign-in page: the token belongs
    /// to the session, and not every page shows a form.
    async fn submit(
        &self,
        action: &str,
        fields: &[(&str, &str)],
    ) -> (StatusCode, String, Option<String>) {
        let (_, html, _) = self.get("/login").await;
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

    async fn ask_for_link(&self, email: &str) -> String {
        let (status, body, _) = self.submit("/password/forgot", &[("email", email)]).await;
        assert_eq!(status, StatusCode::OK);
        body
    }

    async fn choose_password(
        &self,
        link: &str,
        password: &str,
        confirmation: &str,
    ) -> (StatusCode, String, Option<String>) {
        let token = link.split_once("token=").map_or("", |(_, token)| token);
        let fields = [
            ("token", token),
            ("password", password),
            ("password_confirm", confirmation),
        ];
        self.submit("/password/reset", &fields).await
    }

    async fn sign_in(&self, email: &str, password: &str) -> (StatusCode, Option<String>) {
        let fields = [("email", email), ("password", password)];
        let (status, _, location) = self.submit("/login", &fields).await;
        (status, location)
    }
}

async fn answer(response: reqwest::Response) -> (StatusCode, String, Option<String>) {
    let status = response.status();
    let location = response
        .headers()
        .get("location")
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    let body = response.text().await.unwrap_or_default();
    (status, body, location)
}

async fn app_with_mail() -> (TestApp, MailSink) {
    let sink = MailSink::start().await;
    let app = TestApp::spawn_with(Options {
        public_url: Some(PUBLIC_URL),
        mail_port: Some(sink.port),
        ..Options::default()
    })
    .await;
    app.create_user(EMAIL, OLD_PASSWORD, "Ana", Role::Reader)
        .await;
    (app, sink)
}

async fn stored_links(app: &TestApp) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM password_resets")
        .fetch_one(&app.pool)
        .await
        .expect("links counted")
}

/// Asks for a link through the form and reads it from the email.
async fn emailed_link(visitor: &Visitor<'_>, sink: &MailSink, sent_before: usize) -> String {
    visitor.ask_for_link(EMAIL).await;
    let messages = sink.wait_for(sent_before + 1).await;
    assert_eq!(messages.len(), sent_before + 1);
    link_in(&messages[sent_before])
}

#[tokio::test]
async fn without_a_mail_server_the_page_explains_and_offers_no_form() {
    let app = TestApp::spawn_with(Options {
        public_url: Some(PUBLIC_URL),
        ..Options::default()
    })
    .await;
    app.create_user(EMAIL, OLD_PASSWORD, "Ana", Role::Reader)
        .await;
    let visitor = Visitor::new(&app);

    let (_, sign_in, _) = visitor.get("/login").await;
    assert!(sign_in.contains(r#"href="/password/forgot""#));
    let (status, body, _) = visitor.get("/password/forgot").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains(FORM));

    let (status, body, _) = visitor
        .submit("/password/forgot", &[("email", EMAIL)])
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains(FORM));
    assert_eq!(stored_links(&app).await, 0);
}

#[tokio::test]
async fn the_answer_is_the_same_for_a_known_and_an_unknown_address() {
    let (app, sink) = app_with_mail().await;
    let visitor = Visitor::new(&app);

    let known = visitor.ask_for_link(EMAIL).await;
    let unknown = visitor.ask_for_link("nobody@example.com").await;

    assert_eq!(known, unknown);
    let messages = sink.wait_for(1).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(sink.received().len(), 1);
    let recipient = messages[0].lines().find(|line| line.starts_with("To:"));
    assert!(recipient.is_some_and(|line| line.contains(EMAIL)));
    assert_eq!(stored_links(&app).await, 1);
}

#[tokio::test]
async fn a_link_signs_the_member_in_and_ends_the_other_sessions() {
    let (app, sink) = app_with_mail().await;
    app.login(EMAIL, OLD_PASSWORD).await;
    assert_eq!(app.get("/profile").await.0, StatusCode::OK);
    let visitor = Visitor::new(&app);
    let link = emailed_link(&visitor, &sink, 0).await;

    let (status, body, _) = visitor.get(&link).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(RESET_FORM) && body.contains(TOKEN_INPUT));
    let (status, _, location) = visitor
        .choose_password(&link, NEW_PASSWORD, NEW_PASSWORD)
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/"));

    let (status, _, _) = visitor.get("/profile").await;
    assert_eq!(status, StatusCode::OK);
    let (status, location) = app.redirect_of("/profile").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(location.is_some_and(|path| path.starts_with("/login")));
    let (status, location) = Visitor::new(&app).sign_in(EMAIL, OLD_PASSWORD).await;
    assert!(!(status == StatusCode::SEE_OTHER && location.as_deref() == Some("/")));
    let (status, location) = Visitor::new(&app).sign_in(EMAIL, NEW_PASSWORD).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/"));
}

#[tokio::test]
async fn a_link_works_once() {
    let (app, sink) = app_with_mail().await;
    let visitor = Visitor::new(&app);
    let link = emailed_link(&visitor, &sink, 0).await;
    let (status, _, _) = visitor
        .choose_password(&link, NEW_PASSWORD, NEW_PASSWORD)
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let other = Visitor::new(&app);
    let (status, body, _) = other.get(&link).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains(TOKEN_INPUT));
    assert!(body.contains("/password/forgot"));
    let (status, body, _) = other
        .choose_password(
            &link,
            "another passphrase to keep",
            "another passphrase to keep",
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains(TOKEN_INPUT));
    let (status, location) = Visitor::new(&app)
        .sign_in(EMAIL, "another passphrase to keep")
        .await;
    assert!(!(status == StatusCode::SEE_OTHER && location.as_deref() == Some("/")));
    let (status, location) = Visitor::new(&app).sign_in(EMAIL, NEW_PASSWORD).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/"));
}

#[tokio::test]
async fn a_refused_password_leaves_the_link_working() {
    let (app, sink) = app_with_mail().await;
    let visitor = Visitor::new(&app);
    let link = emailed_link(&visitor, &sink, 0).await;

    for (password, confirmation) in [
        ("short", "short"),
        (NEW_PASSWORD, "something else entirely"),
    ] {
        let (status, body, _) = visitor.choose_password(&link, password, confirmation).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains(TOKEN_INPUT));
        assert!(body.contains("field-error"));
    }

    let (status, _, location) = visitor
        .choose_password(&link, NEW_PASSWORD, NEW_PASSWORD)
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/"));
}

#[tokio::test]
async fn an_account_is_sent_three_links_an_hour() {
    let (app, sink) = app_with_mail().await;
    let visitor = Visitor::new(&app);

    for _ in 0..4 {
        visitor.ask_for_link(EMAIL).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    assert_eq!(sink.wait_for(3).await.len(), 3);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(sink.received().len(), 3);
    assert_eq!(stored_links(&app).await, 3);
}

#[tokio::test]
async fn a_malformed_or_unknown_token_shows_the_expired_link_page() {
    let (app, _sink) = app_with_mail().await;
    let visitor = Visitor::new(&app);

    for token in ["", "abc", "999.AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"] {
        let (status, body, _) = visitor.get(&format!("/password/reset?token={token}")).await;
        assert_eq!(status, StatusCode::OK, "{token:?}");
        assert!(!body.contains(TOKEN_INPUT), "{token:?}");
        assert!(body.contains("/password/forgot"), "{token:?}");
    }
}

#[tokio::test]
async fn asking_for_links_in_someone_s_name_never_locks_them_out() {
    let (app, _sink) = app_with_mail().await;
    let stranger = Visitor::new(&app);

    for _ in 0..6 {
        stranger.ask_for_link(EMAIL).await;
    }

    let (status, location) = Visitor::new(&app).sign_in(EMAIL, OLD_PASSWORD).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/"));
}
