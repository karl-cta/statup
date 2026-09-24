//! Integration tests for admin operations: role changes, account
//! activation, members added with a temporary password, instance settings,
//! uploaded icons, and permission enforcement on admin-only endpoints.

mod common;

use reqwest::StatusCode;

use common::{TestApp, extract_csrf_token};
use statup::models::Role;
use statup::repositories::UserRepository;

/// The instance name lives in process memory: the tests that set it or read
/// the default brand run one at a time.
static BRAND: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const ADMIN_EMAIL: &str = "admin@test.com";
const ADMIN_PASSWORD: &str = "admin_password_12";

impl TestApp {
    async fn reader(&self, email: &str, name: &str) -> i64 {
        self.create_user(email, "reader_password_12", name, Role::Reader)
            .await
    }

    /// Promote a user directly in the database.
    async fn promote(&self, user_id: i64, role: Role) {
        assert!(
            UserRepository::update_role(&self.pool, user_id, role)
                .await
                .expect("failed to update role")
        );
    }

    /// A CSRF token from the team page.
    async fn csrf(&self) -> String {
        self.csrf_from("/admin/users").await
    }

    async fn role_of(&self, user_id: i64) -> Role {
        UserRepository::find_by_id(&self.pool, user_id)
            .await
            .expect("db error")
            .expect("user not found")
            .role
    }

    async fn is_active(&self, user_id: i64) -> bool {
        UserRepository::find_by_id(&self.pool, user_id)
            .await
            .expect("db error")
            .expect("user not found")
            .is_active
    }

    async fn add_member(&self, fields: &[(&str, &str)]) -> (StatusCode, String, Option<String>) {
        let csrf = self.csrf().await;
        self.post_form("/admin/users/new", &csrf, fields).await
    }
}

/// Spawn the app with a signed-in administrator, and return its id.
async fn spawn_with_admin() -> (TestApp, i64) {
    let app = TestApp::spawn().await;
    let admin_id = app
        .create_user(ADMIN_EMAIL, ADMIN_PASSWORD, "Admin", Role::Admin)
        .await;
    app.login(ADMIN_EMAIL, ADMIN_PASSWORD).await;
    (app, admin_id)
}

fn is_team_page_redirect(location: Option<&str>) -> bool {
    location.is_some_and(|l| l.starts_with("/admin/users?"))
}

/// Whether the input with this `value` carries `checked`.
fn is_checked(body: &str, value: &str) -> bool {
    let marker = format!(r#"value="{value}""#);
    let Some(start) = body.find(&marker) else {
        return false;
    };
    let end = start + body[start..].find('>').unwrap_or(0);
    body[start..end].contains("checked")
}

#[tokio::test]
async fn admin_can_change_user_role_to_publisher() {
    let (app, _admin_id) = spawn_with_admin().await;
    let reader_id = app.reader("reader@test.com", "Reader").await;

    let csrf = app.csrf().await;
    let path = format!("/admin/users/{reader_id}/role");
    let (status, _body, location) = app.post_form(&path, &csrf, &[("role", "publisher")]).await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(is_team_page_redirect(location.as_deref()));
    assert_eq!(app.role_of(reader_id).await, Role::Publisher);
}

#[tokio::test]
async fn admin_can_change_user_role_to_admin() {
    let (app, _admin_id) = spawn_with_admin().await;
    let user_id = app.reader("user@test.com", "User").await;

    let csrf = app.csrf().await;
    let path = format!("/admin/users/{user_id}/role");
    let (status, _body, location) = app.post_form(&path, &csrf, &[("role", "admin")]).await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(is_team_page_redirect(location.as_deref()));
    assert_eq!(app.role_of(user_id).await, Role::Admin);
}

#[tokio::test]
async fn admin_cannot_change_own_role() {
    let (app, admin_id) = spawn_with_admin().await;

    let csrf = app.csrf().await;
    let path = format!("/admin/users/{admin_id}/role");
    let (status, body, _location) = app.post_form(&path, &csrf, &[("role", "reader")]).await;

    assert_eq!(status, StatusCode::OK, "refused in the page");
    assert!(
        body.contains("propre rôle"),
        "should mention cannot change own role, got: {body}"
    );
    assert_eq!(app.role_of(admin_id).await, Role::Admin);
}

#[tokio::test]
async fn demoting_other_admins_always_leaves_one() {
    let (app, admin_id) = spawn_with_admin().await;
    let other_id = app.reader("other@test.com", "Other").await;
    let third_id = app.reader("third@test.com", "Third").await;
    app.promote(other_id, Role::Admin).await;
    app.promote(third_id, Role::Admin).await;

    for target in [other_id, third_id] {
        let csrf = app.csrf().await;
        let path = format!("/admin/users/{target}/role");
        let (status, _body, location) = app.post_form(&path, &csrf, &[("role", "reader")]).await;
        assert_eq!(status, StatusCode::SEE_OTHER);
        assert!(is_team_page_redirect(location.as_deref()));
        assert_eq!(app.role_of(target).await, Role::Reader);
    }

    assert_eq!(app.role_of(admin_id).await, Role::Admin);
    let admin_count = UserRepository::count_admins(&app.pool)
        .await
        .expect("db error");
    assert_eq!(admin_count, 1, "only one admin should remain");
}

#[tokio::test]
async fn admin_can_disable_user() {
    let (app, _admin_id) = spawn_with_admin().await;
    let user_id = app.reader("target@test.com", "Target").await;

    let csrf = app.csrf().await;
    let path = format!("/admin/users/{user_id}/disable");
    let (status, _body, location) = app.post_form(&path, &csrf, &[]).await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(is_team_page_redirect(location.as_deref()));
    assert!(!app.is_active(user_id).await, "user should be disabled");
}

#[tokio::test]
async fn admin_can_reenable_user() {
    let (app, _admin_id) = spawn_with_admin().await;
    let user_id = app.reader("target@test.com", "Target").await;
    assert!(
        UserRepository::set_active(&app.pool, user_id, false)
            .await
            .expect("failed to disable")
    );

    let csrf = app.csrf().await;
    let path = format!("/admin/users/{user_id}/disable");
    let (status, _body, location) = app.post_form(&path, &csrf, &[]).await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(is_team_page_redirect(location.as_deref()));
    assert!(app.is_active(user_id).await, "user should be re-enabled");
}

#[tokio::test]
async fn admin_cannot_disable_self() {
    let (app, admin_id) = spawn_with_admin().await;

    let csrf = app.csrf().await;
    let path = format!("/admin/users/{admin_id}/disable");
    let (status, body, _location) = app.post_form(&path, &csrf, &[]).await;

    assert_eq!(status, StatusCode::OK, "refused in the page");
    assert!(
        body.contains("désactiver vous-même"),
        "should mention cannot disable self, got: {body}"
    );
    assert!(
        app.is_active(admin_id).await,
        "admin should still be active"
    );
}

#[tokio::test]
async fn another_admin_can_be_disabled_while_one_stays_active() {
    let (app, _admin_id) = spawn_with_admin().await;
    let other_id = app.reader("other@test.com", "Other").await;
    app.promote(other_id, Role::Admin).await;

    let csrf = app.csrf().await;
    let path = format!("/admin/users/{other_id}/disable");
    let (status, _body, location) = app.post_form(&path, &csrf, &[]).await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(is_team_page_redirect(location.as_deref()));
    assert!(
        !app.is_active(other_id).await,
        "other admin should be disabled"
    );
}

#[tokio::test]
async fn reader_cannot_post_admin_role_change() {
    let app = TestApp::spawn().await;
    app.reader("reader@test.com", "Reader").await;
    app.login("reader@test.com", "reader_password_12").await;
    let target_id = app.reader("target@test.com", "Target").await;

    let csrf = app.csrf_from("/").await;
    let path = format!("/admin/users/{target_id}/role");
    let (status, _body, _location) = app.post_form(&path, &csrf, &[("role", "admin")]).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(app.role_of(target_id).await, Role::Reader);
}

#[tokio::test]
async fn reader_cannot_post_admin_disable() {
    let app = TestApp::spawn().await;
    app.reader("reader@test.com", "Reader").await;
    app.login("reader@test.com", "reader_password_12").await;
    let target_id = app.reader("target@test.com", "Target").await;

    let csrf = app.csrf_from("/").await;
    let path = format!("/admin/users/{target_id}/disable");
    let (status, _body, _location) = app.post_form(&path, &csrf, &[]).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(app.is_active(target_id).await);
}

#[tokio::test]
async fn publisher_cannot_access_admin_operations() {
    let app = TestApp::spawn().await;
    app.create_user(
        "pub@test.com",
        "publisher_pass_12",
        "Publisher",
        Role::Publisher,
    )
    .await;
    app.login("pub@test.com", "publisher_pass_12").await;

    let (status, _body) = app.get("/admin/users").await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let target_id = app.reader("target@test.com", "Target").await;
    let csrf = app.csrf_from("/").await;
    let path = format!("/admin/users/{target_id}/role");
    let (status, _body, _location) = app.post_form(&path, &csrf, &[("role", "publisher")]).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn unauthenticated_cannot_access_admin_operations() {
    let app = TestApp::spawn().await;

    let (status, _body) = app.get("/admin/users").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn admin_users_page_lists_all_users() {
    let (app, _admin_id) = spawn_with_admin().await;
    app.reader("alice@test.com", "Alice").await;
    app.reader("bob@test.com", "Bob").await;

    let (status, body) = app.get("/admin/users").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Alice"), "should list Alice");
    assert!(body.contains("Bob"), "should list Bob");
    assert!(body.contains("Admin"), "should list the admin user");
}

#[tokio::test]
async fn a_new_member_is_a_reader_unless_chosen_otherwise() {
    let (app, _admin_id) = spawn_with_admin().await;

    let (_, body) = app.get("/admin/users").await;
    assert!(
        is_checked(&body, "reader"),
        "the reader role is preselected"
    );
    assert!(!is_checked(&body, "publisher"));
}

#[tokio::test]
async fn the_temporary_password_is_shown_once_after_the_redirect() {
    let (app, _admin_id) = spawn_with_admin().await;

    let (status, body, location) = app
        .add_member(&[
            ("display_name", "  Paul  "),
            ("email", "Paul@Test.com"),
            ("role", "publisher"),
        ])
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/admin/users"));
    assert!(!body.contains("temp-password"));

    let member = UserRepository::find_by_email(&app.pool, "paul@test.com")
        .await
        .expect("db error")
        .expect("member not created");
    assert!(member.must_change_password);
    assert_eq!(member.role, Role::Publisher);
    assert_eq!(member.display_name, "Paul");

    let resp = app.get_response("/admin/users").await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get("cache-control")
            .and_then(|v| v.to_str().ok()),
        Some("no-store"),
        "a page showing a password is not stored"
    );
    let body = resp.text().await.unwrap_or_default();
    assert!(body.contains("temp-password"), "shown after the redirect");
    let password = body
        .split(r#"<code class="temp-password">"#)
        .nth(1)
        .and_then(|rest| rest.split("</code>").next())
        .expect("password rendered")
        .to_string();
    assert!(
        statup::services::AuthService::verify_password(&password, &member.password_hash)
            .await
            .expect("hash error")
    );

    let (_, body) = app.get("/admin/users").await;
    assert!(!body.contains("temp-password"), "and never again");
}

#[tokio::test]
async fn a_refused_member_stays_in_the_form() {
    let (app, _admin_id) = spawn_with_admin().await;

    let (status, body, location) = app
        .add_member(&[
            ("display_name", "   "),
            ("email", "blank@test.com"),
            ("role", "reader"),
        ])
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(location.is_none());
    assert!(body.contains("form-error"));
    assert!(body.contains(r#"value="blank@test.com""#));

    let (status, body, _) = app
        .add_member(&[
            ("display_name", "Role"),
            ("email", "role@test.com"),
            ("role", "superadmin"),
        ])
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("form-error"));

    let (status, body, _) = app
        .add_member(&[("display_name", "Mail"), ("email", "not-an-email")])
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("form-error"));
    assert_eq!(UserRepository::count_all(&app.pool).await.unwrap(), 1);
}

#[tokio::test]
async fn a_disabled_accounts_email_is_still_taken() {
    let (app, _admin_id) = spawn_with_admin().await;
    let gone_id = app.reader("gone@test.com", "Gone").await;
    assert!(
        UserRepository::set_active(&app.pool, gone_id, false)
            .await
            .expect("failed to disable")
    );

    let (status, body, _) = app
        .add_member(&[
            ("display_name", "Back"),
            ("email", "GONE@test.com"),
            ("role", "reader"),
        ])
        .await;

    assert_eq!(status, StatusCode::OK, "a message, not a server error");
    assert!(body.contains("déjà utilisée"), "{body}");
    assert_eq!(UserRepository::count_all(&app.pool).await.unwrap(), 2);
}

#[tokio::test]
async fn modules_are_reordered_by_their_handle_alone() {
    let (app, _admin_id) = spawn_with_admin().await;

    let (status, _body) = app.get("/admin/dashboard/public/layout").await;
    assert_eq!(
        status,
        StatusCode::SEE_OTHER,
        "the old page leads to the dashboard"
    );

    let (status, body) = app.get("/?view=public&arrange=1").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("data-drag-handle"), "each block has a handle");
    assert!(
        body.contains("data-arrange-start"),
        "opened in arrange mode"
    );
    assert!(
        body.contains(r#"data-size="wide""#),
        "each block offers its widths"
    );
    assert!(
        !body.contains("data-move="),
        "the arrow buttons that duplicated the handle are gone"
    );

    let csrf = app.csrf().await;
    let (status, _body, _location) = app
        .post_form(
            "/admin/dashboard/public/layout/services/width",
            &csrf,
            &[("width", "wide")],
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_status, body) = app.get("/?view=public").await;
    assert!(
        body.contains(r#"data-width="wide" data-module-id="services""#),
        "the chosen width is what the page draws: {body}"
    );
}

#[tokio::test]
async fn role_change_with_invalid_role_is_rejected() {
    let (app, _admin_id) = spawn_with_admin().await;
    let user_id = app.reader("user@test.com", "User").await;

    let csrf = app.csrf().await;
    let path = format!("/admin/users/{user_id}/role");
    let (status, body, _location) = app.post_form(&path, &csrf, &[("role", "superadmin")]).await;

    assert_eq!(status, StatusCode::OK, "refused in the page");
    assert!(body.contains("form-error"));
    assert_eq!(app.role_of(user_id).await, Role::Reader);
}

/// A self-hosted page should carry the host's identity, not ours: the name
/// set in the settings replaces the wordmark and the tab title, and the
/// product only keeps a credit in the footer.
#[tokio::test]
async fn instance_name_replaces_the_brand_in_masthead_and_title() {
    let _brand = BRAND.lock().await;
    let (app, _admin_id) = spawn_with_admin().await;
    let csrf = app.csrf().await;

    let (status, _, location) = app
        .post_form(
            "/admin/settings/instance-name",
            &csrf,
            &[("instance_name", "  Acme Status  ")],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/admin/settings?renamed=1"));

    let (_, body) = app.get("/admin/settings?renamed=1").await;
    assert!(
        body.contains("| Acme Status</title>"),
        "tab title should end with the instance name"
    );
    assert!(
        body.contains("<q>Acme Status</q>"),
        "the settings page should confirm the new name in a receipt"
    );
    assert!(
        body.contains(r#"<span class="mast-word-custom">Acme Status</span>"#),
        "masthead should show the instance name without the accented wordmark"
    );
    assert!(
        body.contains(">Statup</a>") && !body.contains(r#"Statu<span class="mast-word-accent">"#),
        "the product should only remain as a footer credit"
    );
    assert!(
        body.contains(r#"value="Acme Status""#),
        "the settings field should show the trimmed saved value"
    );

    let csrf = app.csrf().await;
    app.post_form(
        "/admin/settings/instance-name",
        &csrf,
        &[("instance_name", "")],
    )
    .await;
    let (_, body) = app.get("/").await;
    assert!(body.contains(r#"Statu<span class="mast-word-accent">"#));
}

/// A name past the limit comes back on the settings page with the field
/// still holding it, not on a bare error page.
#[tokio::test]
async fn instance_name_too_long_is_refused_in_the_page() {
    let _brand = BRAND.lock().await;
    let (app, _admin_id) = spawn_with_admin().await;
    let csrf = app.csrf().await;
    let long_name = "a".repeat(41);

    let (status, body, location) = app
        .post_form(
            "/admin/settings/instance-name",
            &csrf,
            &[("instance_name", long_name.as_str())],
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(location, None);
    assert!(body.contains(r#"id="form-error""#));
    assert!(body.contains(&format!(r#"value="{long_name}""#)));
    assert!(body.contains(r#"aria-invalid="true""#));

    let (_, body) = app.get("/").await;
    assert!(
        body.contains(r#"Statu<span class="mast-word-accent">"#),
        "a refused name must not replace the brand"
    );
}

/// Choosing who can see the page says so on the settings page, keeps the
/// chosen side marked, and the receipt for opening links to the page.
#[tokio::test]
async fn public_access_choice_confirms_its_new_state() {
    let (app, _admin_id) = spawn_with_admin().await;
    let csrf = app.csrf().await;

    let (status, _, location) = app
        .post_form(
            "/admin/settings/public-mode",
            &csrf,
            &[("access", "everyone")],
        )
        .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/admin/settings?public=on"));
    let stored: String = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'public_mode'")
        .fetch_one(&app.pool)
        .await
        .expect("the choice is stored");
    assert_eq!(stored, "true");

    let (_, body) = app.get("/admin/settings?public=on").await;
    assert!(body.contains(r#"value="everyone" class="sr-only" checked"#));
    assert!(body.contains(r#"href="/" class="link""#));

    let csrf = app.csrf().await;
    let (_, _, location) = app
        .post_form(
            "/admin/settings/public-mode",
            &csrf,
            &[("access", "members")],
        )
        .await;
    assert_eq!(location.as_deref(), Some("/admin/settings?public=off"));
    let (_, body) = app.get("/admin/settings").await;
    assert!(body.contains(r#"value="members" class="sr-only" checked"#));
    assert!(!body.contains(r#"href="/" class="link""#));
}

#[tokio::test]
async fn icon_picker_exposes_its_choice_as_radios() {
    let (app, _) = spawn_with_admin().await;

    let (status, body) = app.get("/services/new").await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("role=\"radiogroup\""));
    let cells = body.matches("role=\"radio\"").count();
    assert!(cells > 0);
    assert_eq!(
        body.matches("aria-checked=\"false\"").count(),
        cells,
        "a new service has no icon chosen yet"
    );
}

/// A multipart body carrying the CSRF token and one file.
fn icon_upload_body(csrf: &str, filename: &str, content_type: &str, data: &[u8]) -> Vec<u8> {
    let mut body = format!(
        "--statup\r\nContent-Disposition: form-data; name=\"csrf_token\"\r\n\r\n{csrf}\r\n\
         --statup\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\n\
         Content-Type: {content_type}\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(data);
    body.extend_from_slice(b"\r\n--statup--\r\n");
    body
}

async fn upload_icon(app: &TestApp, body: Vec<u8>) -> reqwest::Response {
    app.client
        .post(app.url("/icons/upload"))
        .header("content-type", "multipart/form-data; boundary=statup")
        .body(body)
        .send()
        .await
        .expect("upload failed")
}

#[tokio::test]
async fn uploaded_icons_are_served_sandboxed() {
    let (app, _) = spawn_with_admin().await;
    let csrf = app.csrf_from("/icons").await;
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8"><rect width="8" height="8"/><script>alert(1)</script></svg>"#;

    let resp = upload_icon(
        &app,
        icon_upload_body(&csrf, "logo.svg", "image/svg+xml", svg),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);

    let filename: String = sqlx::query_scalar("SELECT filename FROM icons")
        .fetch_one(&app.pool)
        .await
        .expect("icon stored");
    let resp = app
        .get_response(&format!("/uploads/icons/{filename}"))
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let header = |name| {
        resp.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(ToOwned::to_owned)
    };
    assert_eq!(
        header("content-security-policy").as_deref(),
        Some("default-src 'none'; style-src 'unsafe-inline'; sandbox")
    );
    assert_eq!(
        header("cache-control").as_deref(),
        Some("public, max-age=31536000, immutable")
    );
    let served = resp.text().await.unwrap_or_default();
    assert!(served.starts_with("<svg") && !served.contains("script"));

    let resp = app.get_response("/uploads/icons/missing.svg").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert!(
        resp.headers().get("cache-control").is_none(),
        "a missing file is not cached"
    );
}

#[tokio::test]
async fn an_oversized_icon_is_refused_in_the_page() {
    let (app, _) = spawn_with_admin().await;
    let csrf = app.csrf_from("/icons").await;
    let mut png = vec![0x89, 0x50, 0x4E, 0x47];
    png.resize(300 * 1024, 0);

    let resp = upload_icon(&app, icon_upload_body(&csrf, "big.png", "image/png", &png)).await;
    assert_eq!(resp.status(), StatusCode::OK, "said on the page");
    let body = resp.text().await.unwrap_or_default();
    assert!(body.contains("trop lourd"), "{body}");

    // The server answers before reading the whole body and then closes the
    // connection, so the client may not get to read the page itself.
    let huge = vec![0u8; 3 * 1024 * 1024];
    let resp = upload_icon(
        &app,
        icon_upload_body(&csrf, "huge.png", "image/png", &huge),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("text/html; charset=utf-8"),
        "the error page is rendered"
    );
}

#[tokio::test]
async fn static_files_are_cached_by_version() {
    let app = TestApp::spawn().await;
    let cache_control = |resp: &reqwest::Response| {
        resp.headers()
            .get("cache-control")
            .and_then(|v| v.to_str().ok())
            .map(ToOwned::to_owned)
    };

    let resp = app.get_response("/static/js/htmx.min.js?v=42").await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        cache_control(&resp).as_deref(),
        Some("public, max-age=31536000, immutable")
    );

    let resp = app.get_response("/static/js/htmx.min.js").await;
    assert_eq!(
        cache_control(&resp).as_deref(),
        Some("public, max-age=300, must-revalidate")
    );

    let resp = app.get_response("/static/js/missing.js?v=42").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert_eq!(cache_control(&resp), None);

    let resp = app
        .client
        .get(app.url("/static/fonts/hanken-grotesk-variable.woff2"))
        .header("accept-encoding", "gzip")
        .send()
        .await
        .expect("GET failed");
    assert!(
        resp.headers().get("content-encoding").is_none(),
        "fonts are not compressed again"
    );
}

#[tokio::test]
async fn csrf_token_of_an_authenticated_page_is_never_empty() {
    let (app, _) = spawn_with_admin().await;
    let (_, body) = app.get("/").await;
    assert!(!extract_csrf_token(&body).is_empty());
}

/// The host's logo takes the mark's place in the masthead, and leaves when
/// removed.
#[tokio::test]
async fn a_logo_replaces_the_mark_until_removed() {
    let _brand = BRAND.lock().await;
    let (app, _) = spawn_with_admin().await;
    let csrf = app.csrf_from("/admin/settings").await;
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 40 10"><rect width="40" height="10"/></svg>"#;

    let resp = app
        .client
        .post(app.url("/admin/settings/logo"))
        .header("content-type", "multipart/form-data; boundary=statup")
        .body(icon_upload_body(&csrf, "acme.svg", "image/svg+xml", svg))
        .send()
        .await
        .expect("upload failed");
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);

    let (_, body) = app.get("/admin/settings?logo=set").await;
    let logo = body
        .split("class=\"mast-logo\" src=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("the masthead shows the logo")
        .to_string();
    assert_eq!(app.get_response(&logo).await.status(), StatusCode::OK);

    let csrf = app.csrf_from("/admin/settings").await;
    app.post_form("/admin/settings/logo/remove", &csrf, &[])
        .await;
    let (_, body) = app.get("/").await;
    assert!(!body.contains("mast-logo"), "the mark is back");
    assert_eq!(
        app.get_response(&logo).await.status(),
        StatusCode::NOT_FOUND,
        "the old file is gone"
    );
}
