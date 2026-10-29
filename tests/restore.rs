//! Integration tests for restoring a backup at first launch: only on an
//! empty instance, from a file sent or from the backup folder, the token
//! checked before the file is read.

mod common;

use std::path::{Path, PathBuf};

use reqwest::StatusCode;

use common::{Options, TestApp, extract_csrf_token};
use statup::models::Role;
use statup::repositories::SettingsRepository;
use statup::services::backup::create_backup;

const ADMIN_EMAIL: &str = "admin@test.com";
const ADMIN_PASSWORD: &str = "admin_password_12";

async fn spawn() -> TestApp {
    TestApp::spawn_with(Options {
        file_database: true,
        ..Options::default()
    })
    .await
}

fn uploads_of(app: &TestApp) -> PathBuf {
    app.backup_dir().parent().unwrap().to_path_buf()
}

/// A backup of an instance named "Acme", with its administrator and an
/// icon large enough to pass the usual 2 MB limit of a form.
async fn backup_of_acme(destination: &Path) {
    let acme = spawn().await;
    acme.create_user(ADMIN_EMAIL, ADMIN_PASSWORD, "Ada", Role::Admin)
        .await;
    SettingsRepository::set(&acme.pool, "instance_name", "Acme")
        .await
        .unwrap();
    let uploads = uploads_of(&acme);
    std::fs::write(uploads.join("icons/big.png"), vec![7u8; 3 * 1024 * 1024]).unwrap();
    create_backup(&acme.pool, &uploads.to_string_lossy(), destination)
        .await
        .unwrap();
}

fn scratch_file(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("statup-restore-{}-{name}", uuid::Uuid::new_v4()))
}

/// Sends the restore form as a browser does: the token first, then the
/// listed choice, then the file.
async fn send(
    app: &TestApp,
    token: Option<&str>,
    source: &str,
    file: Option<&[u8]>,
) -> (StatusCode, String, Option<String>) {
    let mut body = Vec::new();
    let mut text = |name: &str, value: &str| {
        body.extend(
            format!(
                "--statup\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .into_bytes(),
        );
    };
    if let Some(token) = token {
        text("csrf_token", token);
    }
    text("source", source);
    body.extend(
        b"--statup\r\nContent-Disposition: form-data; name=\"file\"; filename=\"acme.db\"\r\n\
          Content-Type: application/octet-stream\r\n\r\n"
            .iter(),
    );
    body.extend_from_slice(file.unwrap_or_default());
    body.extend_from_slice(b"\r\n--statup--\r\n");
    let response = app
        .client
        .post(app.url("/restore"))
        .header("content-type", "multipart/form-data; boundary=statup")
        .body(body)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let location = response
        .headers()
        .get("location")
        .map(|value| value.to_str().unwrap().to_string());
    (status, response.text().await.unwrap_or_default(), location)
}

#[tokio::test]
async fn the_first_screen_offers_to_restore_and_a_full_instance_does_not() {
    let empty = spawn().await;
    let full = spawn().await;
    full.create_user(ADMIN_EMAIL, ADMIN_PASSWORD, "Ada", Role::Admin)
        .await;

    let (_, register) = empty.get("/register").await;
    let (status, page) = empty.get("/restore").await;
    let (_, location) = full.redirect_of("/restore").await;

    assert!(register.contains("href=\"/restore\""));
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("Restaurer une sauvegarde"));
    assert_eq!(location.as_deref(), Some("/login"));
}

#[tokio::test]
async fn a_backup_sent_from_the_computer_brings_the_instance_back() {
    let backup = scratch_file("acme.db");
    backup_of_acme(&backup).await;
    let app = spawn().await;
    let csrf = app.csrf_from("/restore").await;

    let (status, _, location) = send(
        &app,
        Some(&csrf),
        "",
        Some(&std::fs::read(&backup).unwrap()),
    )
    .await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/login?restored=1"));
    let (_, login) = app.get("/login?restored=1").await;
    assert!(login.contains("Sauvegarde restaurée."));
    assert!(login.contains("Acme"));
    app.login(ADMIN_EMAIL, ADMIN_PASSWORD).await;
    let (status, _) = app.get("/admin/settings").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        std::fs::read(uploads_of(&app).join("icons/big.png"))
            .unwrap()
            .len(),
        3 * 1024 * 1024
    );
    std::fs::remove_file(&backup).unwrap();
}

#[tokio::test]
async fn a_backup_of_the_backup_folder_is_picked_by_its_name() {
    let app = spawn().await;
    std::fs::create_dir_all(app.backup_dir()).unwrap();
    let name = "statup-auto-2026-10-29-030000.db";
    backup_of_acme(&app.backup_dir().join(name)).await;
    let (_, page) = app.get("/restore").await;
    let csrf = extract_csrf_token(&page);

    let (status, _, location) = send(&app, Some(&csrf), name, None).await;

    assert!(page.contains(&format!("value=\"{name}\"")));
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/login?restored=1"));
}

#[tokio::test]
async fn a_name_outside_the_list_is_not_a_backup_to_restore() {
    let app = spawn().await;
    let csrf = app.csrf_from("/restore").await;

    let (status, page, _) = send(&app, Some(&csrf), "../statup.db", None).await;

    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("Choisissez un fichier de sauvegarde."));
}

#[tokio::test]
async fn a_form_without_its_token_is_refused_before_the_file_is_read() {
    let app = spawn().await;
    let _ = app.csrf_from("/restore").await;

    let (status, _, _) = send(&app, None, "", Some(b"anything")).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_file_that_is_not_a_backup_is_refused_in_the_page() {
    let app = spawn().await;
    let csrf = app.csrf_from("/restore").await;

    let (status, page, _) = send(&app, Some(&csrf), "", Some(b"holiday photos")).await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        page.contains("Ce fichier n&#39;est pas une sauvegarde de Statup.")
            || page.contains("Ce fichier n'est pas une sauvegarde de Statup.")
    );
}

#[tokio::test]
async fn an_instance_that_got_an_account_meanwhile_is_not_replaced() {
    let backup = scratch_file("acme.db");
    backup_of_acme(&backup).await;
    let app = spawn().await;
    let csrf = app.csrf_from("/restore").await;
    app.create_user("first@test.com", ADMIN_PASSWORD, "First", Role::Admin)
        .await;

    let (status, page, _) = send(
        &app,
        Some(&csrf),
        "",
        Some(&std::fs::read(&backup).unwrap()),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("Cette page a déjà un compte"));
    let name = SettingsRepository::get(&app.pool, "instance_name")
        .await
        .unwrap();
    assert_ne!(name.as_deref(), Some("Acme"));
    std::fs::remove_file(&backup).unwrap();
}
