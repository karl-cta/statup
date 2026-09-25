//! Integration tests for the authentication flow: first account, sign-in,
//! protected pages and sign-out, sessions, CSRF, and permission denials.

mod common;

use reqwest::StatusCode;

use common::{Options, TestApp, extract_csrf_token};
use statup::models::Role;
use statup::repositories::UserRepository;
use statup::services::AuthService;

const OWNER_EMAIL: &str = "owner@example.com";
const OWNER_PASSWORD: &str = "Owner_password_12";

/// Create the first account through the form. The person is signed in on
/// success. Returns the CSRF token of the form.
async fn create_first_account(app: &TestApp, email: &str, password: &str, name: &str) -> String {
    let (status, body) = app.get("/register").await;
    assert_eq!(status, StatusCode::OK);
    let csrf = extract_csrf_token(&body);

    let (status, _body, location) = app
        .post_form(
            "/register",
            &csrf,
            &[
                ("email", email),
                ("password", password),
                ("display_name", name),
            ],
        )
        .await;

    assert_eq!(status, StatusCode::SEE_OTHER, "register should redirect");
    assert_eq!(
        location.as_deref(),
        Some("/setup/page"),
        "the first launch goes on to the page step"
    );
    csrf
}

/// Seed the administrator directly, so the instance is no longer empty.
async fn seed_admin(app: &TestApp) -> i64 {
    app.create_user(OWNER_EMAIL, OWNER_PASSWORD, "Owner", Role::Admin)
        .await
}

async fn logout(app: &TestApp) {
    let csrf = app.csrf_from("/profile").await;
    let (status, _body, location) = app.post_form("/logout", &csrf, &[]).await;
    assert_eq!(status, StatusCode::SEE_OTHER, "logout should redirect");
    assert_eq!(location.as_deref(), Some("/login"));
}

#[tokio::test]
async fn first_account_then_login_then_protected_then_logout() {
    let app = TestApp::spawn().await;

    create_first_account(&app, "alice@example.com", "Secure_password_123", "Alice").await;
    logout(&app).await;

    app.login("alice@example.com", "Secure_password_123").await;

    let (status, body) = app.get("/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Alice"), "dashboard should show user info");

    logout(&app).await;

    let (status, _body) = app.get("/").await;
    assert_eq!(
        status,
        StatusCode::SEE_OTHER,
        "should redirect to login after logout"
    );
}

#[tokio::test]
async fn login_wrong_password() {
    let app = TestApp::spawn().await;
    seed_admin(&app).await;

    let csrf = app.csrf_from("/login").await;
    let (status, body, _location) = app
        .post_form(
            "/login",
            &csrf,
            &[("email", OWNER_EMAIL), ("password", "wrong_password_12")],
        )
        .await;

    assert_eq!(status, StatusCode::OK, "should re-render login form");
    assert!(
        body.contains("incorrect"),
        "should show invalid credentials error"
    );
}

#[tokio::test]
async fn an_empty_sign_in_field_is_refused_in_the_page() {
    let app = TestApp::spawn().await;
    seed_admin(&app).await;

    let csrf = app.csrf_from("/login").await;
    let (status, body, _location) = app
        .post_form("/login", &csrf, &[("email", OWNER_EMAIL)])
        .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(r#"id="form-error""#));
    assert!(body.contains(OWNER_EMAIL), "the email is kept in the field");
}

#[tokio::test]
async fn protected_route_without_auth_redirects() {
    let app = TestApp::spawn().await;

    for route in ["/", "/events", "/subscribe"] {
        let (status, _body) = app.get(route).await;
        assert_eq!(
            status,
            StatusCode::SEE_OTHER,
            "GET {route} should redirect to /login without auth"
        );
    }
}

#[tokio::test]
async fn reader_cannot_access_publisher_routes() {
    let app = TestApp::spawn().await;
    seed_admin(&app).await;
    app.create_user(
        "reader@example.com",
        "reader_password_12",
        "Reader",
        Role::Reader,
    )
    .await;
    app.login("reader@example.com", "reader_password_12").await;

    for route in ["/events/new", "/services", "/services/new"] {
        let (status, _body) = app.get(route).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "GET {route} as reader should be 403"
        );
    }
}

#[tokio::test]
async fn reader_cannot_access_admin_routes() {
    let app = TestApp::spawn().await;
    seed_admin(&app).await;
    app.create_user(
        "viewer@example.com",
        "viewer_password_12",
        "Viewer",
        Role::Reader,
    )
    .await;
    app.login("viewer@example.com", "viewer_password_12").await;

    let (status, _body) = app.get("/admin/users").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn publisher_can_access_publisher_routes() {
    let app = TestApp::spawn().await;
    seed_admin(&app).await;
    app.create_user(
        "pub@example.com",
        "publisher_pass_12",
        "Publisher",
        Role::Publisher,
    )
    .await;
    app.login("pub@example.com", "publisher_pass_12").await;

    let (status, _body) = app.get("/events/new").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "publisher should access /events/new"
    );
}

#[tokio::test]
async fn post_without_csrf_token_is_rejected() {
    let app = TestApp::spawn().await;
    seed_admin(&app).await;

    let resp = app
        .client
        .post(app.url("/login"))
        .form(&[("email", "a@b.com"), ("password", "test")])
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "a form sent without a session asks to sign in again"
    );
    let body = resp.text().await.expect("body");
    assert!(body.contains("ouverte depuis trop longtemps"), "{body}");
    assert!(body.contains(r#"href="/login""#), "{body}");

    let (status, _body) = app.get("/login").await;
    assert_eq!(status, StatusCode::OK);
    let resp = app
        .client
        .post(app.url("/login"))
        .form(&[("email", "a@b.com"), ("password", "test")])
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "POST with a session but no token should be 403"
    );

    let (status, _body, _) = app
        .post_form("/login", "not-the-token", &[("email", "a@b.com")])
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a wrong token is refused");
}

#[tokio::test]
async fn register_password_too_short() {
    let app = TestApp::spawn().await;

    let csrf = app.csrf_from("/register").await;
    let (status, body, _location) = app
        .post_form(
            "/register",
            &csrf,
            &[
                ("email", "short@example.com"),
                ("password", "short"),
                ("display_name", "Short"),
            ],
        )
        .await;

    assert_eq!(status, StatusCode::OK, "should re-render the form");
    assert!(
        body.contains(r#"id="password-error""#) && body.contains("12 caract"),
        "should show the password length error under the field"
    );
    assert!(
        body.contains("short@example.com") && body.contains(r#"value="Short""#),
        "should keep the other fields filled in"
    );
}

#[tokio::test]
async fn health_check_is_public_sessionless_and_not_rate_limited() {
    let app = TestApp::spawn().await;

    for _ in 0..150 {
        let resp = app.get_response("/health").await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers().get("set-cookie").is_none());
        let body = resp.text().await.unwrap_or_default();
        assert!(body.contains("ok"));
    }
    assert_eq!(app.session_rows().await, 0);
}

/// What a burst keeps of each response.
struct Answer {
    status: StatusCode,
    retry_after: Option<String>,
    body: String,
}

/// Sends one page request per entry, 16 at a time, each with that entry as
/// its `X-Forwarded-For` header when there is one. Together, because the
/// budget refills every 600 ms and a slow sequence would never run out.
async fn burst(app: &TestApp, forwarded_for: Vec<Option<String>>) -> Vec<Answer> {
    let in_flight = std::sync::Arc::new(tokio::sync::Semaphore::new(16));
    let mut requests = tokio::task::JoinSet::new();
    for header in forwarded_for {
        let mut request = app.client.get(app.url("/i18n?locale=xx"));
        if let Some(value) = header {
            request = request.header("x-forwarded-for", value);
        }
        let in_flight = std::sync::Arc::clone(&in_flight);
        requests.spawn(async move {
            let _slot = in_flight.acquire_owned().await.expect("semaphore closed");
            let resp = request.send().await.expect("GET failed");
            let retry_after = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .map(ToOwned::to_owned);
            Answer {
                status: resp.status(),
                retry_after,
                body: resp.text().await.unwrap_or_default(),
            }
        });
    }
    let mut answers = Vec::new();
    while let Some(answer) = requests.join_next().await {
        answers.push(answer.expect("request task failed"));
    }
    answers
}

fn limited(answers: &[Answer]) -> Vec<&Answer> {
    answers
        .iter()
        .filter(|answer| answer.status == StatusCode::TOO_MANY_REQUESTS)
        .collect()
}

#[tokio::test]
async fn pages_are_rate_limited_per_client() {
    let app = TestApp::spawn_public().await;

    let answers = burst(&app, vec![None; 130]).await;
    let refused = limited(&answers);
    let answer = refused
        .first()
        .expect("a burst past 100 requests is limited");
    assert!(answer.retry_after.is_some());
    assert!(
        answer.body.contains(r#"<html lang="fr">"#) && answer.body.contains("Trop de requêtes")
    );
}

#[tokio::test]
async fn a_proxy_header_only_counts_when_the_proxy_is_trusted() {
    let app = TestApp::spawn_with(Options {
        public_mode: true,
        trust_proxy_headers: true,
        ..Options::default()
    })
    .await;

    let two_clients = (0..140)
        .map(|i| Some(format!("1.2.3.4, 198.51.100.{}", i % 2)))
        .collect();
    assert!(
        limited(&burst(&app, two_clients).await).is_empty(),
        "two clients behind the proxy share nothing"
    );

    let one_client = (0..130)
        .map(|i| Some(format!("10.9.{}.{}, 198.51.100.9", i / 200, i % 200)))
        .collect();
    assert!(
        !limited(&burst(&app, one_client).await).is_empty(),
        "a client cannot escape the limit by rotating the leftmost address"
    );
}

#[tokio::test]
async fn proxy_headers_are_ignored_when_not_trusted() {
    let app = TestApp::spawn_public().await;

    let spoofed = (0..130)
        .map(|i| Some(format!("203.0.{}.{}", i / 200, i % 200)))
        .collect();
    assert!(
        !limited(&burst(&app, spoofed).await).is_empty(),
        "every request counts against the connection's address"
    );
}

#[tokio::test]
async fn login_form_is_public() {
    let app = TestApp::spawn().await;
    seed_admin(&app).await;

    let (status, body) = app.get("/login").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !extract_csrf_token(&body).is_empty(),
        "login form should carry a CSRF token"
    );
    assert!(
        body.contains("Connexion"),
        "login form should contain login title"
    );
}

#[tokio::test]
async fn admin_can_access_admin_routes() {
    let app = TestApp::spawn().await;
    seed_admin(&app).await;
    app.login(OWNER_EMAIL, OWNER_PASSWORD).await;

    let (status, _body) = app.get("/admin/users").await;
    assert_eq!(status, StatusCode::OK, "admin should access /admin/users");
}

#[tokio::test]
async fn disabled_user_session_is_rejected() {
    let app = TestApp::spawn().await;
    seed_admin(&app).await;
    let user_id = app
        .create_user(
            "disabled@example.com",
            "disabled_pass_12",
            "Disabled",
            Role::Reader,
        )
        .await;
    app.login("disabled@example.com", "disabled_pass_12").await;

    let (status, _body) = app.get("/").await;
    assert_eq!(status, StatusCode::OK);

    assert!(
        UserRepository::set_active(&app.pool, user_id, false)
            .await
            .expect("failed to disable user")
    );

    let (status, _body) = app.get("/").await;
    assert_eq!(
        status,
        StatusCode::SEE_OTHER,
        "disabled user should be redirected to login"
    );
}

#[tokio::test]
async fn registration_is_closed_once_an_account_exists() {
    for options in [
        Options::default(),
        Options {
            public_mode: true,
            ..Options::default()
        },
    ] {
        let app = TestApp::spawn_with(options).await;
        seed_admin(&app).await;

        let (status, location) = app.redirect_of("/register").await;
        assert_eq!(status, StatusCode::SEE_OTHER);
        assert_eq!(location.as_deref(), Some("/login"));
        assert_eq!(
            app.session_rows().await,
            0,
            "a closed door opens no session"
        );

        let (_, body) = app.get("/login").await;
        assert!(
            !body.contains(r#"href="/register""#),
            "the sign-in page must not offer a closed door"
        );

        let csrf = extract_csrf_token(&body);
        let (status, _body, location) = app
            .post_form(
                "/register",
                &csrf,
                &[
                    ("email", "intruder@example.com"),
                    ("password", "intruder_password"),
                    ("display_name", "Intruder"),
                ],
            )
            .await;
        assert_eq!(status, StatusCode::SEE_OTHER);
        assert_eq!(location.as_deref(), Some("/login"));
        assert_eq!(UserRepository::count_all(&app.pool).await.unwrap(), 1);
    }
}

#[tokio::test]
async fn fresh_instance_offers_the_first_account() {
    let app = TestApp::spawn_public().await;

    let (status, body) = app.get("/register").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("administrateur"));

    let (status, location) = app.redirect_of("/login").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/register"));
}

#[tokio::test]
async fn first_account_is_admin_and_signed_in() {
    let app = TestApp::spawn().await;

    create_first_account(&app, "First@Example.com", "First_password_12", "  First  ").await;

    let user = UserRepository::find_by_email(&app.pool, "first@example.com")
        .await
        .expect("db error")
        .expect("user not found");
    assert_eq!(user.role, Role::Admin);
    assert_eq!(
        user.email, "first@example.com",
        "emails are stored lowercase"
    );
    assert_eq!(user.display_name, "First", "names are trimmed");

    let (status, _body) = app.get("/admin/users").await;
    assert_eq!(status, StatusCode::OK, "signed in straight after creation");

    let (_, body) = app.get("/").await;
    assert!(
        body.contains(r#"class="card first-steps""#),
        "the empty dashboard walks the administrator through the first steps"
    );
    assert!(body.contains(&app.url("/").trim_end_matches('/').to_string()));
}

#[tokio::test]
async fn signing_in_gives_a_new_session_id_and_token() {
    let app = TestApp::spawn().await;
    seed_admin(&app).await;

    let (_, body) = app.get("/login").await;
    let form_token = extract_csrf_token(&body);
    let before = app.session_cookie().expect("the form opened a session");

    app.login(OWNER_EMAIL, OWNER_PASSWORD).await;

    let after = app.session_cookie().expect("signed in");
    assert_ne!(before, after, "the session id is rotated at sign-in");
    let token = app.csrf_from("/profile").await;
    assert!(!token.is_empty());
    assert_ne!(token, form_token, "the form token is replaced at sign-in");

    let (status, _, _) = app.post_form("/logout", &form_token, &[]).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a token from before the sign-in no longer works"
    );
    let (status, _body) = app.get("/profile").await;
    assert_eq!(status, StatusCode::OK, "still signed in");
}

/// The `Max-Age` of the session cookie a response sets, if it sets one.
fn session_max_age(resp: &reqwest::Response) -> Option<String> {
    resp.headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find(|cookie| cookie.starts_with("id="))
        .and_then(|cookie| {
            cookie
                .split(';')
                .map(str::trim)
                .find_map(|part| part.strip_prefix("Max-Age="))
                .map(ToOwned::to_owned)
        })
}

async fn sign_in_response(app: &TestApp, remember: bool) -> reqwest::Response {
    let csrf = app.csrf_from("/login").await;
    let mut form = vec![
        ("csrf_token", csrf.as_str()),
        ("email", OWNER_EMAIL),
        ("password", OWNER_PASSWORD),
    ];
    if remember {
        form.push(("remember_me", "on"));
    }
    app.client
        .post(app.url("/login"))
        .form(&form)
        .send()
        .await
        .expect("POST /login failed")
}

#[tokio::test]
async fn signed_in_sessions_keep_their_own_lifetime() {
    let app = TestApp::spawn().await;
    seed_admin(&app).await;

    let resp = sign_in_response(&app, false).await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    assert_eq!(session_max_age(&resp).as_deref(), Some("86400"));
    logout(&app).await;

    let resp = sign_in_response(&app, true).await;
    assert_eq!(session_max_age(&resp).as_deref(), Some("2592000"));

    let resp = app.get_response("/admin/users").await;
    assert_eq!(
        session_max_age(&resp),
        None,
        "a page view within the hour writes nothing"
    );

    let csrf = extract_csrf_token(&resp.text().await.unwrap_or_default());
    let resp = app
        .client
        .post(app.url("/profile/password"))
        .form(&[
            ("csrf_token", csrf.as_str()),
            ("current_password", OWNER_PASSWORD),
            ("new_password", "Another-owner-pass-42"),
            ("new_password_confirm", "Another-owner-pass-42"),
        ])
        .send()
        .await
        .expect("POST failed");
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        session_max_age(&resp).as_deref(),
        Some("2592000"),
        "a later session write keeps the lifetime chosen at sign-in"
    );
}

#[tokio::test]
async fn background_requests_leave_a_members_only_page() {
    let app = TestApp::spawn().await;
    seed_admin(&app).await;

    for path in ["/", "/events", "/events/1/drawer"] {
        let resp = app
            .client
            .get(app.url(path))
            .header("HX-Request", "true")
            .send()
            .await
            .expect("request failed");
        assert_eq!(resp.status(), StatusCode::OK, "{path}");
        let target = resp
            .headers()
            .get("hx-redirect")
            .and_then(|v| v.to_str().ok());
        assert_eq!(
            target,
            Some("/login"),
            "{path} sends the page to the sign-in form"
        );
    }
}

#[tokio::test]
async fn anonymous_pages_open_no_session() {
    let app = TestApp::spawn_public().await;
    seed_admin(&app).await;

    for path in ["/", "/events", "/feed", "/health", "/wp-login.php"] {
        let resp = app.get_response(path).await;
        assert!(
            resp.headers().get("set-cookie").is_none(),
            "GET {path} should not set a cookie"
        );
    }
    assert_eq!(app.session_rows().await, 0);

    let (status, _body) = app.get("/login").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(app.session_rows().await, 1, "a form needs a session");
}

#[tokio::test]
async fn temporary_password_must_be_replaced_before_anything_else() {
    let app = TestApp::spawn().await;
    seed_admin(&app).await;
    let (member, temporary) =
        AuthService::add_member(&app.pool, "member@example.com", "Member", Role::Reader)
            .await
            .expect("failed to create member");

    let csrf = app.csrf_from("/login").await;
    let (status, _body, location) = app
        .post_form(
            "/login",
            &csrf,
            &[
                ("email", "member@example.com"),
                ("password", temporary.as_str()),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/password/new"));

    let (status, location) = app.redirect_of("/events").await;
    assert_eq!(
        status,
        StatusCode::SEE_OTHER,
        "every other page is held back"
    );
    assert_eq!(location.as_deref(), Some("/password/new"));

    let resp = app.get_response("/password/new").await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get("cache-control")
            .and_then(|v| v.to_str().ok()),
        Some("no-store")
    );
    let csrf = extract_csrf_token(&resp.text().await.unwrap_or_default());

    let (status, body, _) = app
        .post_form(
            "/password/new",
            &csrf,
            &[
                ("password", temporary.as_str()),
                ("password_confirm", temporary.as_str()),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains(r#"id="password-error""#),
        "the temporary password cannot be kept"
    );

    let (status, body, _) = app
        .post_form(
            "/password/new",
            &csrf,
            &[
                ("password", "My_own_password_42"),
                ("password_confirm", "My_own_password_43"),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(r#"id="confirm-error""#));

    let before = app.session_cookie();
    let (status, _body, location) = app
        .post_form(
            "/password/new",
            &csrf,
            &[
                ("password", "My_own_password_42"),
                ("password_confirm", "My_own_password_42"),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/"));
    assert_ne!(
        before,
        app.session_cookie(),
        "a new password, a new session id"
    );

    let (status, _body) = app.get("/events").await;
    assert_eq!(status, StatusCode::OK, "the instance opens once replaced");

    let user = UserRepository::find_by_id(&app.pool, member.id)
        .await
        .expect("db error")
        .expect("user not found");
    assert!(!user.must_change_password);
    assert!(
        AuthService::verify_password("My_own_password_42", &user.password_hash)
            .await
            .expect("hash error")
    );
}

#[tokio::test]
async fn account_created_by_its_owner_is_not_asked_for_a_new_password() {
    let app = TestApp::spawn().await;
    create_first_account(&app, OWNER_EMAIL, OWNER_PASSWORD, "Owner").await;

    let (status, location) = app.redirect_of("/password/new").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/"));
}

#[tokio::test]
async fn host_reset_signs_the_account_out_and_asks_for_a_new_password() {
    let app = TestApp::spawn().await;
    create_first_account(&app, "reset@example.com", "Forgotten_password_1", "Reset").await;
    let (status, _body) = app.get("/profile").await;
    assert_eq!(status, StatusCode::OK);

    let temporary = AuthService::reset_password(&app.pool, "reset@example.com")
        .await
        .expect("reset failed");

    let (status, location) = app.redirect_of("/profile").await;
    assert_eq!(
        status,
        StatusCode::SEE_OTHER,
        "the open session no longer matches the password"
    );
    assert_eq!(location.as_deref(), Some("/login"));

    let csrf = app.csrf_from("/login").await;
    let (status, _body, location) = app
        .post_form(
            "/login",
            &csrf,
            &[
                ("email", "reset@example.com"),
                ("password", temporary.as_str()),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/password/new"));
}

#[tokio::test]
async fn resetting_an_unknown_account_is_refused() {
    let app = TestApp::spawn().await;
    let result = AuthService::reset_password(&app.pool, "nobody@example.com").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn changing_the_password_from_the_profile_keeps_this_session() {
    let app = TestApp::spawn().await;
    create_first_account(&app, "profile@example.com", "First_password_12", "Profile").await;

    let resp = app.get_response("/profile").await;
    assert_eq!(
        resp.headers()
            .get("cache-control")
            .and_then(|v| v.to_str().ok()),
        Some("no-store")
    );
    let csrf = extract_csrf_token(&resp.text().await.unwrap_or_default());
    let before = app.session_cookie();

    let (status, body, _) = app
        .post_form(
            "/profile/password",
            &csrf,
            &[
                ("current_password", "First_password_12"),
                ("new_password", "Second_password_34"),
                ("new_password_confirm", "Second_password_34"),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_ne!(before, app.session_cookie(), "the session id is rotated");

    let (status, _body, _) = app
        .post_form(
            "/profile",
            &extract_csrf_token(&body),
            &[
                ("email", "profile@example.com"),
                ("display_name", "Profile"),
            ],
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the page rendered after the change carries the new token"
    );

    let (status, _body) = app.get("/profile").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the session that changed the password stays open"
    );
}

#[tokio::test]
async fn a_short_new_password_is_refused_in_the_profile_page() {
    let app = TestApp::spawn().await;
    create_first_account(&app, "short@example.com", "First_password_12", "Short").await;

    let csrf = app.csrf_from("/profile").await;
    let (status, body, _) = app
        .post_form(
            "/profile/password",
            &csrf,
            &[
                ("current_password", "First_password_12"),
                ("new_password", "short"),
                ("new_password_confirm", "short"),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(r#"id="password-form-error""#));
}

#[tokio::test]
async fn the_current_password_cannot_be_guessed_from_an_open_session() {
    let app = TestApp::spawn().await;
    create_first_account(&app, "guess@example.com", "First_password_12", "Guess").await;

    let csrf = app.csrf_from("/profile").await;
    let change = |current: &'static str| {
        [
            ("current_password", current),
            ("new_password", "Second_password_34"),
            ("new_password_confirm", "Second_password_34"),
        ]
    };
    for _ in 0..5 {
        let (_, body, _) = app
            .post_form("/profile/password", &csrf, &change("Wrong_guess_123"))
            .await;
        assert!(body.contains("Le mot de passe actuel est incorrect"));
    }
    let (_, body, _) = app
        .post_form("/profile/password", &csrf, &change("First_password_12"))
        .await;
    assert!(
        body.contains("Trop de tentatives"),
        "even the right one waits: {body}"
    );
}

#[tokio::test]
async fn a_blank_display_name_is_refused_in_the_profile_page() {
    let app = TestApp::spawn().await;
    create_first_account(&app, "blank@example.com", "First_password_12", "Blank").await;

    let csrf = app.csrf_from("/profile").await;
    let (status, body, _) = app
        .post_form(
            "/profile",
            &csrf,
            &[("email", "blank@example.com"), ("display_name", "   ")],
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(r#"id="profile-error""#));
    let user = UserRepository::find_by_email(&app.pool, "blank@example.com")
        .await
        .expect("db error")
        .expect("user not found");
    assert_eq!(user.display_name, "Blank");
}

#[tokio::test]
async fn signed_in_user_is_sent_home_from_login() {
    let app = TestApp::spawn().await;
    create_first_account(&app, "home@example.com", "Home_password_123", "Home").await;

    let (status, location) = app.redirect_of("/login").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/"));
}

#[tokio::test]
async fn public_mode_reader_is_sent_home_from_register() {
    let app = TestApp::spawn_public().await;
    seed_admin(&app).await;
    app.create_user(
        "reader@example.com",
        "reader_pass_1234",
        "Reader",
        Role::Reader,
    )
    .await;
    app.login("reader@example.com", "reader_pass_1234").await;

    let (status, location) = app.redirect_of("/register").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/"));
}

#[tokio::test]
async fn admin_is_sent_to_the_team_page_from_register() {
    let app = TestApp::spawn_public().await;
    seed_admin(&app).await;
    app.login(OWNER_EMAIL, OWNER_PASSWORD).await;

    let (status, location) = app.redirect_of("/register").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/admin/users"));
}

#[tokio::test]
async fn public_mode_read_routes_accessible_without_auth() {
    let app = TestApp::spawn_public().await;

    for route in ["/", "/events", "/subscribe"] {
        let (status, _body) = app.get(route).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "GET {route} in public mode should be accessible without auth"
        );
    }
}

#[tokio::test]
async fn search_links_land_on_the_events_list() {
    let app = TestApp::spawn_public().await;

    let (status, location) = app.redirect_of("/search?q=disk").await;
    assert_eq!(status, StatusCode::PERMANENT_REDIRECT);
    assert_eq!(location.as_deref(), Some("/events?q=disk"));
}

#[tokio::test]
async fn history_bookmarks_land_on_the_events_list() {
    let app = TestApp::spawn_public().await;

    let (status, location) = app.redirect_of("/history").await;
    assert_eq!(status, StatusCode::PERMANENT_REDIRECT);
    assert_eq!(location.as_deref(), Some("/events"));
}

#[tokio::test]
async fn missing_event_renders_a_styled_error_page_in_the_request_language() {
    let app = TestApp::spawn_public().await;

    let (status, body) = app.get("/events/999999").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("existe pas") && body.contains("style.css"));
    assert!(body.contains(r#"href="/""#), "offers a way back");

    let resp = app
        .client
        .get(app.url("/events/999999"))
        .header("Cookie", "lang=en")
        .send()
        .await
        .expect("GET failed");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = resp.text().await.unwrap_or_default();
    assert!(body.contains("does not exist"));
}

#[tokio::test]
async fn unknown_paths_render_the_error_page() {
    let app = TestApp::spawn().await;

    let resp = app.get_response("/wp-login.php").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        resp.headers()
            .get("cache-control")
            .and_then(|v| v.to_str().ok()),
        Some("no-cache, private")
    );
    let body = resp.text().await.unwrap_or_default();
    assert!(body.contains("existe pas") && body.contains("style.css"));

    let (status, _body) = app.get("/uploads/statup.db").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "only icons are served");
}

#[tokio::test]
async fn htmx_request_gets_an_error_fragment_instead_of_a_page() {
    let app = TestApp::spawn_public().await;

    let resp = app
        .client
        .get(app.url("/events/999999"))
        .header("HX-Request", "true")
        .send()
        .await
        .expect("GET failed");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = resp.text().await.unwrap_or_default();
    assert!(body.contains(r#"class="form-error""#) && body.contains("existe pas"));
    assert!(!body.contains("<html"), "a fragment, not a page");
}

#[tokio::test]
async fn an_oversized_form_gets_the_error_page() {
    let app = TestApp::spawn().await;
    seed_admin(&app).await;
    let csrf = app.csrf_from("/login").await;
    let padding = "a".repeat(70 * 1024);

    let (status, body, _) = app
        .post_form("/login", &csrf, &[("email", padding.as_str())])
        .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(body.contains("trop volumineux"), "{body}");
}

#[tokio::test]
async fn security_headers_are_sent() {
    let app = TestApp::spawn().await;

    let resp = app.get_response("/login").await;
    let header = |name| {
        resp.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(ToOwned::to_owned)
    };
    assert_eq!(header("x-frame-options").as_deref(), Some("DENY"));
    assert_eq!(header("x-content-type-options").as_deref(), Some("nosniff"));
    assert_eq!(
        header("referrer-policy").as_deref(),
        Some("strict-origin-when-cross-origin")
    );
    assert_eq!(
        header("cross-origin-opener-policy").as_deref(),
        Some("same-origin")
    );
    assert!(header("permissions-policy").is_some());
    assert!(header("content-security-policy").is_some());
    assert!(header("x-xss-protection").is_none());
    assert!(
        header("strict-transport-security").is_none(),
        "no HSTS without an https address"
    );
}

#[tokio::test]
async fn an_https_instance_asks_for_https_and_secure_cookies() {
    let app = TestApp::spawn_with(Options {
        public_url: Some("https://status.example.com"),
        ..Options::default()
    })
    .await;
    seed_admin(&app).await;

    let resp = app.get_response("/login").await;
    let header = |name| {
        resp.headers()
            .get_all(name)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>()
    };
    assert_eq!(header("strict-transport-security"), ["max-age=31536000"]);
    let session_cookie = header("set-cookie")
        .into_iter()
        .find(|cookie| cookie.starts_with("id="))
        .expect("the form opens a session");
    assert!(session_cookie.contains("Secure"), "{session_cookie}");

    let resp = app.get_response("/i18n?locale=en").await;
    let lang_cookie = resp
        .headers()
        .get("set-cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert!(lang_cookie.ends_with("; Secure"), "{lang_cookie}");

    let resp = app.get_response("/health").await;
    assert!(resp.headers().get("strict-transport-security").is_some());
}

/// Status, `Location`, `Set-Cookie` and body of a language switch sent
/// from `referer`.
async fn switch_language(
    app: &TestApp,
    query: &str,
    referer: Option<&str>,
) -> (StatusCode, Option<String>, Option<String>, String) {
    let mut request = app.client.get(app.url(&format!("/i18n{query}")));
    if let Some(referer) = referer {
        request = request.header("referer", referer);
    }
    let resp = request.send().await.expect("GET /i18n failed");
    let header = |name| {
        resp.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(ToOwned::to_owned)
    };
    let (location, cookie) = (header("location"), header("set-cookie"));
    let status = resp.status();
    (
        status,
        location,
        cookie,
        resp.text().await.unwrap_or_default(),
    )
}

#[tokio::test]
async fn language_switch_returns_to_a_page_of_this_site() {
    let app = TestApp::spawn_public().await;
    let origin = app.url("");

    let (status, location, cookie, _) = switch_language(
        &app,
        "?locale=en",
        Some(&format!("{origin}/events?kind=incident")),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/events?kind=incident"));
    let cookie = cookie.expect("the choice is kept in a cookie");
    assert!(cookie.starts_with("lang=en;") && !cookie.contains("Secure"));
    assert_eq!(app.session_rows().await, 0, "a visitor gets no session");

    let referer = format!("{origin}/admin/users/3/role");
    let (_, location, _, _) = switch_language(&app, "?locale=fr", Some(&referer)).await;
    assert_eq!(
        location.as_deref(),
        Some("/admin/users"),
        "action paths map to their page"
    );

    for foreign in ["https://evil.example/events", "not a url"] {
        let (_, location, _, _) = switch_language(&app, "?locale=fr", Some(foreign)).await;
        assert_eq!(location.as_deref(), Some("/"), "{foreign}");
    }
    let (_, location, _, _) = switch_language(&app, "?locale=fr", None).await;
    assert_eq!(location.as_deref(), Some("/"));

    let (status, _, cookie, body) = switch_language(&app, "?locale=de", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(cookie.is_none());
    assert!(body.contains("Langue non prise en charge"), "{body}");

    let (status, _, _, _) = switch_language(&app, "", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn language_switch_is_saved_on_the_account() {
    let app = TestApp::spawn().await;
    let admin_id = seed_admin(&app).await;
    app.login(OWNER_EMAIL, OWNER_PASSWORD).await;

    let (status, _, _, _) = switch_language(&app, "?locale=en", None).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let user = UserRepository::find_by_id(&app.pool, admin_id)
        .await
        .expect("db error")
        .expect("user not found");
    assert_eq!(user.preferred_locale.as_deref(), Some("en"));
}

/// On a page open to everyone, signing out leaves the reader on it rather
/// than at a sign-in form they no longer need.
#[tokio::test]
async fn signing_out_of_an_open_page_lands_on_it() {
    let app = TestApp::spawn_public().await;
    seed_admin(&app).await;
    app.login(OWNER_EMAIL, OWNER_PASSWORD).await;

    let csrf = app.csrf_from("/profile").await;
    let (status, _body, location) = app.post_form("/logout", &csrf, &[]).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/"));
}
