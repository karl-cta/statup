//! Upgrades from every published database shape. Each fixture is the
//! database a release left, made by its published image with a fixed data
//! set; the server of today starts on it as after `docker compose pull`. A
//! release that changes the database adds its fixture.

mod common;

use common::{Options, TestApp};
use reqwest::StatusCode;

/// The accounts every fixture holds, with this test password.
const PASSWORD: &str = "Fixture-Upgrade-2026!";
const ADMIN: &str = "admin@upgrade.test";
const EDITOR: &str = "editor@upgrade.test";
const READER: &str = "reader@upgrade.test";

const OPEN_INCIDENT: &str = "Fixture incident: mail delays";
const RESOLVED_INCIDENT: &str = "Fixture incident: VPN drop";
const MAINTENANCE: &str = "Fixture maintenance: VPN upgrade";
const ANNOUNCEMENT: &str = "Fixture announcement: new intranet";

/// The pages an administrator reaches from the masthead and the settings.
const ADMIN_PAGES: [&str; 13] = [
    "/",
    "/events",
    "/services",
    "/notices",
    "/profile",
    "/admin/settings",
    "/admin/users",
    "/admin/notifications",
    "/admin/notifications/subscribers",
    "/admin/api-keys",
    "/admin/journal",
    "/admin/backups",
    "/admin/embed",
];

/// What a visitor without an account reads after the upgrade.
enum Visitors {
    /// A members-only instance stays shut.
    Shut,
    /// A public page shows every service: the fixture predates visibility.
    SeeAll,
    /// A public page hides Intranet (members) and Payroll (a group).
    SeePublicOnly,
}

async fn upgrades(dump: &'static str, visitors: Visitors) {
    let app = TestApp::spawn_with(Options {
        seed: Some(dump),
        ..Options::default()
    })
    .await;
    assert_every_migration_applied(&app).await;
    assert_backed_up_before(&app);
    assert_visitor_view(&app, visitors).await;

    app.login(ADMIN, PASSWORD).await;
    for page in ADMIN_PAGES {
        assert_ok(&app, page).await;
    }
    for page in pages_of_kept_records(&app).await {
        assert_ok(&app, &page).await;
    }
    assert_events_kept(&app).await;
    logout(&app).await;

    for member in [EDITOR, READER] {
        app.login(member, PASSWORD).await;
        let (_, home) = app.get("/").await;
        assert!(
            home.contains("Fixture Mail"),
            "{member} does not see the services"
        );
        logout(&app).await;
    }
}

async fn assert_every_migration_applied(app: &TestApp) {
    let (latest, failed): (i64, i64) = sqlx::query_as(
        "SELECT MAX(version), COUNT(*) FILTER (WHERE NOT success) FROM _sqlx_migrations",
    )
    .fetch_one(&app.pool)
    .await
    .expect("failed to read the migrations");
    assert_eq!(latest, statup::db::latest_migration());
    assert_eq!(failed, 0);
}

async fn assert_visitor_view(app: &TestApp, visitors: Visitors) {
    let (_, home) = app.get("/").await;
    let (_, feed) = app.get("/feed").await;
    if matches!(visitors, Visitors::Shut) {
        assert!(!home.contains(OPEN_INCIDENT), "a members-only page opened");
        assert!(!feed.contains(OPEN_INCIDENT), "a members-only feed opened");
        return;
    }
    assert!(home.contains("Fixture Mail") && home.contains(OPEN_INCIDENT));
    assert!(feed.contains(OPEN_INCIDENT));
    if matches!(visitors, Visitors::SeePublicOnly) {
        assert!(
            !home.contains("Fixture Intranet"),
            "a members service shows"
        );
        assert!(!home.contains("Fixture Payroll"), "a group service shows");
    } else {
        assert!(home.contains("Fixture Intranet"));
    }
}

fn assert_backed_up_before(app: &TestApp) {
    let backups: Vec<String> = std::fs::read_dir(app.backup_dir())
        .expect("no backup folder: nothing was backed up before the upgrade")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("statup-before-") && !name.contains("restore"))
        .collect();
    assert_eq!(backups.len(), 1, "{backups:?}");
}

/// The edit page of every service and event, and the page of every group
/// and folder: old records drawn by today's forms.
async fn pages_of_kept_records(app: &TestApp) -> Vec<String> {
    let ids = |sql: &'static str| async move {
        sqlx::query_scalar::<_, i64>(sql)
            .fetch_all(&app.pool)
            .await
            .expect("failed to list records")
    };
    let mut pages = Vec::new();
    pages.extend(
        ids("SELECT id FROM services")
            .await
            .into_iter()
            .map(|id| format!("/services/{id}/edit")),
    );
    pages.extend(
        ids("SELECT id FROM events")
            .await
            .into_iter()
            .map(|id| format!("/events/{id}/edit")),
    );
    pages.extend(
        ids("SELECT id FROM member_groups")
            .await
            .into_iter()
            .map(|id| format!("/admin/groups/{id}")),
    );
    pages.extend(
        ids("SELECT id FROM service_folders")
            .await
            .into_iter()
            .map(|id| format!("/services/folders/{id}")),
    );
    pages
}

async fn assert_events_kept(app: &TestApp) {
    let (_, home) = app.get("/").await;
    assert!(home.contains("Fixture Mail"));
    assert!(home.contains(OPEN_INCIDENT));

    let open = event_page(app, OPEN_INCIDENT).await;
    assert!(open.contains("Investigating the mail queue."));
    assert!(open.contains("Fix deployed, watching."));
    let resolved = event_page(app, RESOLVED_INCIDENT).await;
    assert!(resolved.contains("Resolved."));
    let review: Option<String> = sqlx::query_scalar("SELECT body FROM event_reviews")
        .fetch_optional(&app.pool)
        .await
        .expect("failed to read the review");
    if let Some(review) = review {
        assert!(resolved.contains(&review), "the review is not drawn");
    }
    event_page(app, MAINTENANCE).await;
    event_page(app, ANNOUNCEMENT).await;
}

/// The page of the event with this title, which must name it.
async fn event_page(app: &TestApp, title: &str) -> String {
    let id: i64 = sqlx::query_scalar("SELECT id FROM events WHERE title = ?")
        .bind(title)
        .fetch_one(&app.pool)
        .await
        .unwrap_or_else(|e| panic!("{title}: {e}"));
    let (status, body) = app.get(&format!("/events/{id}")).await;
    assert_eq!(status, StatusCode::OK, "{title}");
    assert!(body.contains(title), "{title} is not on its page");
    body
}

async fn assert_ok(app: &TestApp, page: &str) {
    let (status, _) = app.get(page).await;
    assert_eq!(status, StatusCode::OK, "{page}");
}

async fn logout(app: &TestApp) {
    let csrf = app.csrf_from("/profile").await;
    let (status, _, _) = app.post_form("/logout", &csrf, &[]).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn from_0_1_0() {
    upgrades(include_str!("fixtures/upgrade/0.1.0.sql"), Visitors::SeeAll).await;
}

#[tokio::test]
async fn from_0_2_0() {
    upgrades(include_str!("fixtures/upgrade/0.2.0.sql"), Visitors::SeeAll).await;
}

#[tokio::test]
async fn from_0_3_0() {
    upgrades(include_str!("fixtures/upgrade/0.3.0.sql"), Visitors::SeeAll).await;
}

#[tokio::test]
async fn from_0_4_0() {
    upgrades(include_str!("fixtures/upgrade/0.4.0.sql"), Visitors::SeeAll).await;
}

#[tokio::test]
async fn from_0_5_0() {
    upgrades(include_str!("fixtures/upgrade/0.5.0.sql"), Visitors::SeeAll).await;
}

#[tokio::test]
async fn from_0_6_0() {
    upgrades(include_str!("fixtures/upgrade/0.6.0.sql"), Visitors::Shut).await;
}

#[tokio::test]
async fn from_0_7_0() {
    upgrades(
        include_str!("fixtures/upgrade/0.7.0.sql"),
        Visitors::SeePublicOnly,
    )
    .await;
}

#[tokio::test]
async fn from_0_8_0() {
    upgrades(
        include_str!("fixtures/upgrade/0.8.0.sql"),
        Visitors::SeePublicOnly,
    )
    .await;
}

#[tokio::test]
async fn from_0_9_0() {
    upgrades(
        include_str!("fixtures/upgrade/0.9.0.sql"),
        Visitors::SeePublicOnly,
    )
    .await;
}
