//! Integration tests for the Backups page: who may open it, its schedule,
//! and the backups it lists and downloads.

mod common;

use reqwest::StatusCode;

use common::{Options, TestApp};
use statup::models::Role;
use statup::services::backup::{BackupSchedule, Frequency, read_info};

const ADMIN_EMAIL: &str = "admin@test.com";
const ADMIN_PASSWORD: &str = "admin_password_12";

async fn spawn_with_admin() -> TestApp {
    let app = TestApp::spawn_with(Options {
        file_database: true,
        ..Options::default()
    })
    .await;
    app.create_user(ADMIN_EMAIL, ADMIN_PASSWORD, "Admin", Role::Admin)
        .await;
    app.login(ADMIN_EMAIL, ADMIN_PASSWORD).await;
    app
}

async fn save_schedule(
    app: &TestApp,
    fields: &[(&str, &str)],
) -> (StatusCode, String, Option<String>) {
    let csrf = app.csrf_from("/admin/backups").await;
    app.post_form("/admin/backups/schedule", &csrf, fields)
        .await
}

#[tokio::test]
async fn only_administrators_open_the_page() {
    let app = TestApp::spawn().await;
    app.create_user("editor@test.com", ADMIN_PASSWORD, "Editor", Role::Publisher)
        .await;
    app.login("editor@test.com", ADMIN_PASSWORD).await;

    let (status, _) = app.get("/admin/backups").await;

    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn the_page_says_when_backups_run_and_that_none_exists_yet() {
    let app = spawn_with_admin().await;

    let (status, body) = app.get("/admin/backups").await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Chaque jour à 03:00"));
    assert!(body.contains("7 gardées"));
    assert!(
        body.contains("Aucune sauvegarde pour l&#39;instant.")
            || body.contains("Aucune sauvegarde pour l'instant.")
    );
    assert!(body.contains(&app.backup_dir().display().to_string()));
}

#[tokio::test]
async fn the_schedule_is_saved_as_typed() {
    let app = spawn_with_admin().await;

    let (status, _, location) = save_schedule(
        &app,
        &[
            ("frequency", "weekly"),
            ("weekday", "5"),
            ("time", "22h15"),
            ("keep", "30"),
        ],
    )
    .await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/admin/backups?saved=1"));
    let schedule = BackupSchedule::load(&app.pool).await.unwrap();
    assert_eq!(schedule.frequency, Frequency::Weekly);
    assert_eq!(schedule.time.format("%H:%M").to_string(), "22:15");
    assert_eq!(schedule.keep, 30);
    assert!(!schedule.before_upgrade);
    let (_, body) = app.get("/admin/backups?saved=1").await;
    assert!(body.contains("Chaque vendredi à 22:15"));
}

#[tokio::test]
async fn a_wrong_time_is_refused_with_what_was_typed() {
    let app = spawn_with_admin().await;

    let (status, body, _) = save_schedule(
        &app,
        &[
            ("frequency", "daily"),
            ("time", "25:00"),
            ("keep", "7"),
            ("before_upgrade", "on"),
        ],
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Saisissez une heure, comme 03:00."));
    assert!(body.contains("value=\"25:00\""));
    assert_eq!(
        BackupSchedule::load(&app.pool).await.unwrap(),
        BackupSchedule::default()
    );
}

#[tokio::test]
async fn a_backup_is_downloaded_on_the_spot_and_not_kept() {
    let app = spawn_with_admin().await;
    let csrf = app.csrf_from("/admin/backups").await;

    let response = app
        .client
        .post(app.url("/admin/backups/download"))
        .form(&[("csrf_token", csrf.as_str())])
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers().clone();
    let disposition = headers["content-disposition"].to_str().unwrap();
    assert!(disposition.starts_with("attachment; filename=\"statup-backup-"));
    assert_eq!(headers["cache-control"], "no-store");
    let body = response.bytes().await.unwrap();
    assert!(body.starts_with(b"SQLite format 3\0"));
    let saved = std::env::temp_dir().join(format!("statup-downloaded-{}.db", uuid::Uuid::new_v4()));
    std::fs::write(&saved, &body).unwrap();
    assert!(read_info(&saved).await.is_ok());
    std::fs::remove_file(&saved).unwrap();
    assert!(!app.backup_dir().exists());
}

#[tokio::test]
async fn a_listed_backup_is_downloaded_by_its_name_only() {
    let app = spawn_with_admin().await;
    std::fs::create_dir_all(app.backup_dir()).unwrap();
    std::fs::write(
        app.backup_dir().join("statup-auto-2026-10-29-030000.db"),
        b"backup",
    )
    .unwrap();
    std::fs::write(app.backup_dir().join("notes.db"), b"private").unwrap();

    let (_, page) = app.get("/admin/backups").await;
    let listed = app
        .get_response("/admin/backups/files/statup-auto-2026-10-29-030000.db")
        .await;
    let other = app.get_response("/admin/backups/files/notes.db").await;
    let outside = app
        .get_response("/admin/backups/files/..%2Fstatup.db")
        .await;

    assert!(page.contains("Automatique"));
    assert!(!page.contains("notes.db"));
    assert_eq!(listed.status(), StatusCode::OK);
    assert_eq!(listed.bytes().await.unwrap().as_ref(), b"backup");
    assert_eq!(other.status(), StatusCode::NOT_FOUND);
    assert_eq!(outside.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_settings_tile_says_when_the_newest_backup_was_taken() {
    let app = spawn_with_admin().await;
    let (_, before) = app.get("/admin/settings").await;
    std::fs::create_dir_all(app.backup_dir()).unwrap();
    std::fs::write(
        app.backup_dir().join("statup-auto-2026-10-29-030000.db"),
        b"x",
    )
    .unwrap();

    let (_, after) = app.get("/admin/settings").await;

    assert!(before.contains("Aucune sauvegarde"));
    assert!(after.contains("Dernière\u{a0}: "));
}
