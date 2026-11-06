//! Folders of services: created from the Services page, then managed from
//! their own page (rename, visibility, add and remove services, delete), all
//! of it for publishers and administrators.

mod common;

use reqwest::StatusCode;

use common::TestApp;
use statup::i18n::{I18n, LANGUAGES};
use statup::models::{Grants, Role, Visibility};
use statup::repositories::{FolderRepository, GroupRepository, ServiceRepository, UserRepository};

const PUBLISHER_EMAIL: &str = "editor@test.com";
const PUBLISHER_PASSWORD: &str = "editor_password_12";
const ADMIN_EMAIL: &str = "admin@test.com";
const ADMIN_PASSWORD: &str = "admin_password_123";
const READER_EMAIL: &str = "reader@test.com";
const READER_PASSWORD: &str = "reader_password_12";

/// A page text, in the language a client that asks for none gets.
fn text(key: &str) -> String {
    I18n::new(LANGUAGES[0].code).t(key).to_string()
}

fn plural(key: &str, n: usize) -> String {
    I18n::new(LANGUAGES[0].code).plural(key, n)
}

/// An instance with a signed-in publisher.
async fn spawn_with_publisher() -> TestApp {
    let app = TestApp::spawn().await;
    app.create_user(
        PUBLISHER_EMAIL,
        PUBLISHER_PASSWORD,
        "Editor",
        Role::Publisher,
    )
    .await;
    app.login(PUBLISHER_EMAIL, PUBLISHER_PASSWORD).await;
    app
}

async fn create_folder(app: &TestApp, name: &str) -> i64 {
    FolderRepository::create(&app.pool, name)
        .await
        .expect("failed to create folder")
}

async fn create_service(app: &TestApp, name: &str) -> i64 {
    ServiceRepository::create(&app.pool, name, &name.to_lowercase(), None, None, None)
        .await
        .expect("failed to create service")
        .id
}

async fn create_group(app: &TestApp, name: &str) -> i64 {
    GroupRepository::create(&app.pool, name)
        .await
        .expect("failed to create group")
}

/// A folder opened to these groups only.
async fn create_guarded_folder(app: &TestApp, name: &str, groups: &[i64]) -> i64 {
    let id = create_folder(app, name).await;
    FolderRepository::save_settings(
        &app.pool,
        id,
        name,
        Visibility::Groups,
        Grants {
            groups,
            ..Default::default()
        },
    )
    .await
    .expect("failed to set the folder visibility");
    id
}

/// A service with its own restriction to these groups.
async fn create_restricted_service(app: &TestApp, name: &str, groups: &[i64]) -> i64 {
    let id = create_service(app, name).await;
    ServiceRepository::set_visibility(
        &app.pool,
        id,
        Visibility::Groups,
        Grants {
            groups,
            ..Default::default()
        },
    )
    .await
    .expect("failed to restrict the service");
    id
}

async fn folder_names(app: &TestApp) -> Vec<String> {
    FolderRepository::list(&app.pool)
        .await
        .expect("failed to list folders")
        .into_iter()
        .map(|folder| folder.name)
        .collect()
}

/// What a service holds about its folder: folder, follows, visibility, groups.
async fn placement(app: &TestApp, id: i64) -> (Option<i64>, bool, Visibility, Vec<i64>) {
    let service = ServiceRepository::find_by_id(&app.pool, id)
        .await
        .expect("failed to read the service")
        .expect("the service exists");
    let groups = ServiceRepository::group_ids_of(&app.pool, id)
        .await
        .expect("failed to read the service groups");
    (
        service.folder_id,
        service.follows_folder,
        service.visibility,
        groups,
    )
}

/// A folder line of the journal: kind, subject, detail.
type Line = (String, Option<String>, Option<String>);

/// The folder lines of the journal, oldest first.
async fn lines(app: &TestApp) -> Vec<Line> {
    sqlx::query_as(
        "SELECT kind, subject, detail FROM journal_entries \
         WHERE substr(kind, 1, 7) = 'folder_' ORDER BY id",
    )
    .fetch_all(&app.pool)
    .await
    .expect("failed to read the journal")
}

fn line(kind: &str, subject: &str, detail: Option<&str>) -> Line {
    (
        kind.to_string(),
        Some(subject.to_string()),
        detail.map(str::to_string),
    )
}

/// POST a form with a token taken from the Services page.
async fn post(
    app: &TestApp,
    path: &str,
    fields: &[(&str, &str)],
) -> (StatusCode, String, Option<String>) {
    let csrf = app.csrf_from("/services").await;
    app.post_form(path, &csrf, fields).await
}

/// Follow a redirect and return the page it leads to.
async fn page_after(app: &TestApp, location: Option<String>) -> String {
    let path = location.expect("a redirect");
    let (status, body) = app.get(&path).await;
    assert_eq!(status, StatusCode::OK, "{path}");
    body
}

/// Every POST route of a folder, with fields that would be accepted. The
/// delete comes last, after the service was added and removed.
fn folder_posts(folder: i64, service: i64) -> Vec<(String, Vec<(&'static str, String)>)> {
    let base = format!("/services/folders/{folder}");
    vec![
        (
            "/services/folders/new".to_string(),
            vec![("name", "Another".to_string())],
        ),
        (
            format!("{base}/settings"),
            vec![
                ("name", "Renamed".to_string()),
                ("visibility", "members".to_string()),
            ],
        ),
        (
            format!("{base}/services"),
            vec![("service_ids", service.to_string())],
        ),
        (format!("{base}/services/{service}/remove"), vec![]),
        (format!("{base}/delete"), vec![]),
    ]
}

async fn send(
    app: &TestApp,
    csrf: &str,
    path: &str,
    fields: &[(&'static str, String)],
) -> StatusCode {
    let fields: Vec<(&str, &str)> = fields.iter().map(|(k, v)| (*k, v.as_str())).collect();
    app.post_form(path, csrf, &fields).await.0
}

#[tokio::test]
async fn a_reader_is_refused_on_every_folder_route() {
    let app = TestApp::spawn().await;
    app.create_user(READER_EMAIL, READER_PASSWORD, "Reader", Role::Reader)
        .await;
    app.login(READER_EMAIL, READER_PASSWORD).await;
    let folder = create_folder(&app, "Network").await;
    let service = create_service(&app, "Wiki").await;
    let csrf = app.csrf_from("/").await;

    let (status, _) = app.get(&format!("/services/folders/{folder}")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    for (path, fields) in folder_posts(folder, service) {
        assert_eq!(
            send(&app, &csrf, &path, &fields).await,
            StatusCode::FORBIDDEN,
            "{path}"
        );
    }

    assert_eq!(folder_names(&app).await, ["Network"]);
    assert!(lines(&app).await.is_empty());
}

#[tokio::test]
async fn a_visitor_is_refused_on_every_folder_route() {
    let app = TestApp::spawn().await;
    app.create_user(READER_EMAIL, READER_PASSWORD, "Reader", Role::Reader)
        .await;
    let folder = create_folder(&app, "Network").await;
    let service = create_service(&app, "Wiki").await;
    let csrf = app.csrf_from("/login").await;

    let (status, _) = app.get(&format!("/services/folders/{folder}")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    for (path, fields) in folder_posts(folder, service) {
        assert_eq!(
            send(&app, &csrf, &path, &fields).await,
            StatusCode::UNAUTHORIZED,
            "{path}"
        );
    }

    assert_eq!(folder_names(&app).await, ["Network"]);
    assert!(lines(&app).await.is_empty());
}

#[tokio::test]
async fn a_publisher_can_use_every_folder_route() {
    let app = spawn_with_publisher().await;
    let folder = create_folder(&app, "Network").await;
    let service = create_service(&app, "Wiki").await;
    let csrf = app.csrf_from("/services").await;

    let (status, _) = app.get(&format!("/services/folders/{folder}")).await;
    assert_eq!(status, StatusCode::OK);
    for (path, fields) in folder_posts(folder, service) {
        assert_eq!(
            send(&app, &csrf, &path, &fields).await,
            StatusCode::SEE_OTHER,
            "{path}"
        );
    }
}

#[tokio::test]
async fn creating_a_folder_lands_on_its_page() {
    let app = spawn_with_publisher().await;

    let (status, _, location) = post(&app, "/services/folders/new", &[("name", "Réseau")]).await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    let folders = FolderRepository::list(&app.pool).await.unwrap();
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].name, "Réseau");
    assert_eq!(
        location.as_deref(),
        Some(format!("/services/folders/{}?created=1", folders[0].id).as_str())
    );
    assert_eq!(lines(&app).await, [line("folder_created", "Réseau", None)]);
    let body = page_after(&app, location).await;
    assert!(body.contains(&text("folders.notice_created")));
}

#[tokio::test]
async fn a_folder_name_taken_whatever_its_case_or_accents_is_refused() {
    let app = spawn_with_publisher().await;
    create_folder(&app, "Réseau").await;

    let (status, body, _) = post(&app, "/services/folders/new", &[("name", "RÉSEAU")]).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(&text("validation.folder_name_taken")));
    assert!(body.contains(r#"value="RÉSEAU""#), "the typed name stays");
    assert_eq!(folder_names(&app).await, ["Réseau"]);
    assert!(lines(&app).await.is_empty());
}

#[tokio::test]
async fn an_empty_folder_name_is_refused() {
    let app = spawn_with_publisher().await;

    let (status, body, _) = post(&app, "/services/folders/new", &[("name", "   ")]).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(&text("validation.folder_name_required")));
    assert!(folder_names(&app).await.is_empty());
}

#[tokio::test]
async fn renaming_a_folder_keeps_the_new_name_and_the_old_one_in_the_journal() {
    let app = spawn_with_publisher().await;
    let folder = create_folder(&app, "Network").await;

    let (status, _, location) = post(
        &app,
        &format!("/services/folders/{folder}/settings"),
        &[("name", "Lyon office"), ("visibility", "everyone")],
    )
    .await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        location.as_deref(),
        Some(format!("/services/folders/{folder}?saved=1").as_str())
    );
    assert_eq!(folder_names(&app).await, ["Lyon office"]);
    assert_eq!(
        lines(&app).await,
        [line("folder_renamed", "Lyon office", Some("Network"))]
    );
    let body = page_after(&app, location).await;
    assert!(body.contains(&text("folders.notice_saved")));
}

#[tokio::test]
async fn renaming_and_restricting_a_folder_at_once_leaves_both_lines() {
    let app = spawn_with_publisher().await;
    let folder = create_folder(&app, "Network").await;

    let (status, _, _) = post(
        &app,
        &format!("/services/folders/{folder}/settings"),
        &[("name", "Lyon office"), ("visibility", "members")],
    )
    .await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(folder_names(&app).await, ["Lyon office"]);
    assert_eq!(
        lines(&app).await,
        [
            line("folder_renamed", "Lyon office", Some("Network")),
            line("folder_visibility_changed", "Lyon office", Some("members")),
        ]
    );
}

#[tokio::test]
async fn a_refused_name_saves_nothing_not_even_the_visibility() {
    let app = spawn_with_publisher().await;
    create_folder(&app, "Network").await;
    let apps = create_folder(&app, "Apps").await;

    let (status, body, _) = post(
        &app,
        &format!("/services/folders/{apps}/settings"),
        &[("name", "network"), ("visibility", "members")],
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(&text("validation.folder_name_taken")));
    assert!(
        body.contains(r#"value="network""#),
        "the typed name is kept"
    );
    let saved = FolderRepository::find(&app.pool, apps)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (saved.name.as_str(), saved.visibility),
        ("Apps", Visibility::Everyone)
    );
    assert!(lines(&app).await.is_empty());
}

#[tokio::test]
async fn a_folder_restricted_to_nobody_can_still_be_renamed() {
    let app = TestApp::spawn().await;
    app.create_user(ADMIN_EMAIL, ADMIN_PASSWORD, "Admin", Role::Admin)
        .await;
    app.login(ADMIN_EMAIL, ADMIN_PASSWORD).await;
    let group = create_group(&app, "Compta").await;
    let folder = create_guarded_folder(&app, "Network", &[group]).await;
    let removal = format!("/admin/groups/{group}/folders/{folder}/remove");
    let (status, _, _) = post(&app, &removal, &[]).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let after_removal = lines(&app).await;

    let (status, _, location) = post(
        &app,
        &format!("/services/folders/{folder}/settings"),
        &[("name", "Lyon office"), ("visibility", "groups")],
    )
    .await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        location.as_deref(),
        Some(format!("/services/folders/{folder}?saved=1").as_str())
    );
    let saved = FolderRepository::find(&app.pool, folder)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (saved.name.as_str(), saved.visibility),
        ("Lyon office", Visibility::Groups)
    );
    assert!(
        FolderRepository::group_ids_of(&app.pool, folder)
            .await
            .unwrap()
            .is_empty()
    );
    let mut expected = after_removal;
    expected.push(line("folder_renamed", "Lyon office", Some("Network")));
    assert_eq!(lines(&app).await, expected, "only the rename is added");
}

#[tokio::test]
async fn a_member_who_is_no_longer_a_reader_does_not_make_a_rename_a_visibility_change() {
    let app = spawn_with_publisher().await;
    let group = create_group(&app, "Compta").await;
    let ann = app
        .create_user(READER_EMAIL, READER_PASSWORD, "Ann", Role::Reader)
        .await;
    let folder = create_folder(&app, "Network").await;
    FolderRepository::save_settings(
        &app.pool,
        folder,
        "Network",
        Visibility::Groups,
        Grants {
            groups: &[group],
            members: &[ann],
        },
    )
    .await
    .unwrap();
    UserRepository::update_role(&app.pool, ann, Role::Publisher)
        .await
        .unwrap();

    let group_id = group.to_string();
    let (status, _, _) = post(
        &app,
        &format!("/services/folders/{folder}/settings"),
        &[
            ("name", "Lyon office"),
            ("visibility", "groups"),
            ("group_ids", group_id.as_str()),
        ],
    )
    .await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        lines(&app).await,
        [line("folder_renamed", "Lyon office", Some("Network"))]
    );
}

#[tokio::test]
async fn a_refused_visibility_with_a_new_name_saves_nothing() {
    let app = spawn_with_publisher().await;
    let folder = create_folder(&app, "Network").await;

    let (status, body, _) = post(
        &app,
        &format!("/services/folders/{folder}/settings"),
        &[("name", "Lyon office"), ("visibility", "groups")],
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(&text("validation.visibility_required")));
    assert!(
        body.contains(r#"value="Lyon office""#),
        "the typed name is kept"
    );
    assert!(
        body.contains(r#"folder-menu" data-more-actions open"#),
        "the panel stays open"
    );
    let saved = FolderRepository::find(&app.pool, folder)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (saved.name.as_str(), saved.visibility),
        ("Network", Visibility::Everyone)
    );
    assert!(lines(&app).await.is_empty());
}

#[tokio::test]
async fn renaming_a_folder_to_its_own_name_leaves_no_line() {
    let app = spawn_with_publisher().await;
    let folder = create_folder(&app, "Network").await;

    let (status, _, _) = post(
        &app,
        &format!("/services/folders/{folder}/settings"),
        &[("name", "Network"), ("visibility", "everyone")],
    )
    .await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(folder_names(&app).await, ["Network"]);
    assert!(lines(&app).await.is_empty());
}

#[tokio::test]
async fn renaming_a_folder_to_another_ones_name_is_refused() {
    let app = spawn_with_publisher().await;
    create_folder(&app, "Network").await;
    let apps = create_folder(&app, "Apps").await;

    let (status, body, _) = post(
        &app,
        &format!("/services/folders/{apps}/settings"),
        &[("name", "NETWORK"), ("visibility", "everyone")],
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(&text("validation.folder_name_taken")));
    assert_eq!(folder_names(&app).await, ["Network", "Apps"]);
    assert!(lines(&app).await.is_empty());
}

#[tokio::test]
async fn adding_services_lets_open_ones_inherit_and_restricted_ones_keep_theirs() {
    let app = spawn_with_publisher().await;
    let (g1, g2) = (
        create_group(&app, "G1").await,
        create_group(&app, "G2").await,
    );
    let folder = create_guarded_folder(&app, "Guarded", &[g1]).await;
    let open = create_service(&app, "Open").await;
    let own = create_restricted_service(&app, "Own", &[g2]).await;

    let (status, _, location) = post(
        &app,
        &format!("/services/folders/{folder}/services"),
        &[
            ("service_ids", &open.to_string()),
            ("service_ids", &own.to_string()),
        ],
    )
    .await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        location.as_deref(),
        Some(format!("/services/folders/{folder}?added=2&kept=1").as_str())
    );
    assert_eq!(
        placement(&app, open).await,
        (Some(folder), true, Visibility::Groups, vec![g1])
    );
    assert_eq!(
        placement(&app, own).await,
        (Some(folder), false, Visibility::Groups, vec![g2])
    );
    let mut added = lines(&app).await;
    added.sort();
    assert_eq!(
        added,
        [
            line("folder_service_added", "Open", Some("Guarded")),
            line("folder_service_added", "Own", Some("Guarded")),
        ]
    );
}

#[tokio::test]
async fn the_page_after_adding_says_what_was_inherited_and_what_was_kept() {
    let app = spawn_with_publisher().await;
    let (g1, g2) = (
        create_group(&app, "G1").await,
        create_group(&app, "G2").await,
    );
    let folder = create_guarded_folder(&app, "Guarded", &[g1]).await;
    let open = create_service(&app, "Open").await;
    let own = create_restricted_service(&app, "Own", &[g2]).await;
    let (_, _, location) = post(
        &app,
        &format!("/services/folders/{folder}/services"),
        &[
            ("service_ids", &open.to_string()),
            ("service_ids", &own.to_string()),
        ],
    )
    .await;

    let body = page_after(&app, location).await;

    let i18n = I18n::new(LANGUAGES[0].code);
    let effect = i18n.tf(
        "folders.added_mixed",
        &[
            ("inherit", &plural("folders.added_inherit", 1)),
            ("keep", &plural("folders.added_keep", 1)),
        ],
    );
    let expected = format!("{} {effect}", plural("folders.added", 2));
    assert!(body.contains(&expected), "{expected}");
}

#[tokio::test]
async fn adding_a_service_takes_it_out_of_its_other_folder() {
    let app = spawn_with_publisher().await;
    let first = create_folder(&app, "First").await;
    let second = create_folder(&app, "Second").await;
    let wiki = create_service(&app, "Wiki").await;
    FolderRepository::add_services(&app.pool, first, &[wiki])
        .await
        .unwrap();

    let (status, _, _) = post(
        &app,
        &format!("/services/folders/{second}/services"),
        &[("service_ids", &wiki.to_string())],
    )
    .await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    let (folder_id, ..) = placement(&app, wiki).await;
    assert_eq!(folder_id, Some(second));
}

#[tokio::test]
async fn removing_a_service_keeps_its_visibility() {
    let app = spawn_with_publisher().await;
    let g1 = create_group(&app, "G1").await;
    let folder = create_guarded_folder(&app, "Guarded", &[g1]).await;
    let wiki = create_service(&app, "Wiki").await;
    FolderRepository::add_services(&app.pool, folder, &[wiki])
        .await
        .unwrap();

    let (status, _, location) = post(
        &app,
        &format!("/services/folders/{folder}/services/{wiki}/remove"),
        &[],
    )
    .await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        placement(&app, wiki).await,
        (None, false, Visibility::Groups, vec![g1])
    );
    assert_eq!(
        lines(&app).await,
        [line("folder_service_removed", "Wiki", Some("Guarded"))]
    );
    let body = page_after(&app, location).await;
    assert!(body.contains(&text("folders.notice_removed")));
}

#[tokio::test]
async fn removing_a_service_that_is_not_in_the_folder_is_not_found() {
    let app = spawn_with_publisher().await;
    let folder = create_folder(&app, "Network").await;
    let wiki = create_service(&app, "Wiki").await;
    FolderRepository::add_services(&app.pool, folder, &[wiki])
        .await
        .unwrap();
    let path = format!("/services/folders/{folder}/services/{wiki}/remove");
    post(&app, &path, &[]).await;

    let (status, _, _) = post(&app, &path, &[]).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(lines(&app).await.len(), 1, "only the first removal is told");
}

#[tokio::test]
async fn a_folder_visibility_reaches_the_services_that_inherit_it_only() {
    let app = spawn_with_publisher().await;
    let (g1, g2) = (
        create_group(&app, "G1").await,
        create_group(&app, "G2").await,
    );
    let folder = create_folder(&app, "Network").await;
    let heir = create_service(&app, "Heir").await;
    let own = create_restricted_service(&app, "Own", &[g2]).await;
    FolderRepository::add_services(&app.pool, folder, &[heir, own])
        .await
        .unwrap();

    let (status, _, location) = post(
        &app,
        &format!("/services/folders/{folder}/settings"),
        &[
            ("name", "Network"),
            ("visibility", "groups"),
            ("group_ids", &g1.to_string()),
        ],
    )
    .await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        location.as_deref(),
        Some(format!("/services/folders/{folder}?saved=1").as_str())
    );
    assert_eq!(
        placement(&app, heir).await,
        (Some(folder), true, Visibility::Groups, vec![g1])
    );
    assert_eq!(
        placement(&app, own).await,
        (Some(folder), false, Visibility::Groups, vec![g2])
    );
    assert_eq!(
        lines(&app).await,
        [line(
            "folder_visibility_changed",
            "Network",
            Some("groups:1;members:0")
        )]
    );
}

#[tokio::test]
async fn an_unknown_visibility_is_refused_and_nothing_changes() {
    let app = spawn_with_publisher().await;
    let folder = create_folder(&app, "Network").await;
    let heir = create_service(&app, "Heir").await;
    FolderRepository::add_services(&app.pool, folder, &[heir])
        .await
        .unwrap();

    let (status, _, _) = post(
        &app,
        &format!("/services/folders/{folder}/settings"),
        &[("name", "Network"), ("visibility", "anyone")],
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        placement(&app, heir).await,
        (Some(folder), true, Visibility::Everyone, vec![])
    );
    let saved = FolderRepository::find(&app.pool, folder)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.visibility, Visibility::Everyone);
    assert!(lines(&app).await.is_empty());
}

#[tokio::test]
async fn saving_the_same_visibility_again_leaves_no_second_line() {
    let app = spawn_with_publisher().await;
    let folder = create_folder(&app, "Network").await;
    let path = format!("/services/folders/{folder}/settings");
    let fields = [("name", "Network"), ("visibility", "members")];
    post(&app, &path, &fields).await;

    let (status, _, _) = post(&app, &path, &fields).await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        lines(&app).await,
        [line(
            "folder_visibility_changed",
            "Network",
            Some("members")
        )]
    );
}

/// The notices after a deletion never repeat a name from the address, so a
/// forged link cannot announce that a given folder or service is gone.
#[tokio::test]
async fn a_deletion_notice_names_nothing_from_the_address() {
    let app = spawn_with_publisher().await;
    let (status, body) = app
        .get("/services?folder_deleted=Payroll%20archive&deleted=Payroll%20mail")
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains("Payroll"));
    assert!(body.contains(&text("services.folder_deleted_notice")));
    assert!(body.contains(&text("services.deleted_notice")));
}

#[tokio::test]
async fn deleting_a_folder_keeps_its_services_as_they_were_seen() {
    let app = spawn_with_publisher().await;
    let g1 = create_group(&app, "G1").await;
    let folder = create_guarded_folder(&app, "Réseau & Co", &[g1]).await;
    let heir = create_service(&app, "Heir").await;
    FolderRepository::add_services(&app.pool, folder, &[heir])
        .await
        .unwrap();

    let (status, _, location) =
        post(&app, &format!("/services/folders/{folder}/delete"), &[]).await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(location.as_deref(), Some("/services?folder_deleted=1"));
    let page = page_after(&app, location).await;
    assert!(page.contains(&text("services.folder_deleted_notice")));
    assert!(folder_names(&app).await.is_empty());
    assert_eq!(
        placement(&app, heir).await,
        (None, false, Visibility::Groups, vec![g1])
    );
    assert_eq!(
        lines(&app).await,
        [line("folder_deleted", "Réseau & Co", None)]
    );
}

#[tokio::test]
async fn the_page_of_an_unknown_folder_is_not_found() {
    let app = spawn_with_publisher().await;

    let (status, _) = app.get("/services/folders/999").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_services_page_shows_one_card_per_folder_after_the_services_in_none() {
    let app = spawn_with_publisher().await;
    create_service(&app, "Mail").await;
    let vpn = create_service(&app, "VPN").await;
    let network = create_folder(&app, "Network").await;
    let empty = create_folder(&app, "Empty").await;
    FolderRepository::add_services(&app.pool, network, &[vpn])
        .await
        .unwrap();

    let (status, body) = app.get("/services").await;

    assert_eq!(status, StatusCode::OK);
    let at = |needle: &str| {
        body.find(needle)
            .unwrap_or_else(|| panic!("{needle} is on the page"))
    };
    let order = [
        at(r#"data-service-name="Mail""#),
        at(&format!(r#"id="folder-{network}""#)),
        at(r#"data-service-name="VPN""#),
        at(&format!(r#"id="folder-{empty}""#)),
    ];
    assert!(order.is_sorted(), "{order:?}");
    assert!(body.contains(&format!(r#"href="/services/folders/{network}""#)));
    let empty_card = &body[at(&format!(r#"id="folder-{empty}""#))..];
    assert!(
        empty_card.contains(&text("folders.no_service")),
        "an empty folder stays within reach"
    );
}

#[tokio::test]
async fn a_publisher_sets_the_order_of_folders() {
    let app = spawn_with_publisher().await;
    let ids = [
        create_folder(&app, "Network").await.to_string(),
        create_folder(&app, "Mail").await.to_string(),
        create_folder(&app, "Payroll").await.to_string(),
    ];

    let order = [
        ("order", &*ids[2]),
        ("order", &*ids[0]),
        ("order", &*ids[1]),
    ];
    let (status, _, _) = post(&app, "/services/folders/order", &order).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(folder_names(&app).await, ["Payroll", "Network", "Mail"]);

    let stale = [("order", &*ids[1]), ("order", &*ids[0])];
    let (status, _, _) = post(&app, "/services/folders/order", &stale).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "a list missing a folder");
    assert_eq!(folder_names(&app).await, ["Payroll", "Network", "Mail"]);
}

#[tokio::test]
async fn a_reader_cannot_set_the_order_of_folders() {
    let app = TestApp::spawn().await;
    app.create_user(READER_EMAIL, READER_PASSWORD, "Reader", Role::Reader)
        .await;
    app.login(READER_EMAIL, READER_PASSWORD).await;
    let first = create_folder(&app, "Network").await.to_string();
    let second = create_folder(&app, "Mail").await.to_string();
    let csrf = app.csrf_from("/").await;

    let order = [("order", &*second), ("order", &*first)];
    let (status, _, _) = app
        .post_form("/services/folders/order", &csrf, &order)
        .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(folder_names(&app).await, ["Network", "Mail"]);
}

#[tokio::test]
async fn a_refused_form_that_inherits_shows_the_folders_setting() {
    let app = spawn_with_publisher().await;
    let group = create_group(&app, "Compta").await;
    let folder = create_guarded_folder(&app, "Paie", &[group]).await;
    let service = create_service(&app, "Notes de frais").await;
    FolderRepository::add_services(&app.pool, folder, &[service])
        .await
        .expect("failed to add the service");

    let edit = format!("/services/{service}/edit");
    let fields = [("name", ""), ("check_kind", "none"), ("inherit", "on")];
    let (status, body, _) = post(&app, &edit, &fields).await;

    assert_eq!(status, StatusCode::OK, "the form comes back");
    assert!(body.contains(&text("validation.service_name_required")));
    // The sentence under the block, not the texts its script keeps in
    // attributes.
    let effect = |key: &str| format!("data-access-effect>{}</p>", text(key));
    assert!(
        body.contains(&effect("services.effect_staff_only")),
        "the folder's restriction, not the public default"
    );
    assert!(!body.contains(&effect("services.effect_public")));
}

#[tokio::test]
async fn an_editor_picks_readers_by_name_and_never_sees_their_address() {
    let app = spawn_with_publisher().await;
    let reader = app
        .create_user(READER_EMAIL, READER_PASSWORD, "Marie Dupont", Role::Reader)
        .await;
    let service = create_service(&app, "Wiki").await;
    ServiceRepository::set_visibility(
        &app.pool,
        service,
        Visibility::Groups,
        Grants {
            groups: &[],
            members: &[reader],
        },
    )
    .await
    .expect("failed to open the service to the reader");
    let folder = create_folder(&app, "Paie").await;

    for page in [
        format!("/services/{service}/edit"),
        format!("/services/folders/{folder}"),
    ] {
        let (status, body) = app.get(&page).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        assert!(body.contains("Marie Dupont"), "{page}: the reader by name");
        assert!(
            !body.contains(READER_EMAIL),
            "{page}: no address for an editor"
        );
    }
}

#[tokio::test]
async fn an_administrator_still_sees_the_readers_addresses() {
    let app = TestApp::spawn().await;
    app.create_user("admin@test.com", "admin_password_12", "Admin", Role::Admin)
        .await;
    app.login("admin@test.com", "admin_password_12").await;
    app.create_user(READER_EMAIL, READER_PASSWORD, "Marie Dupont", Role::Reader)
        .await;
    let service = create_service(&app, "Wiki").await;

    let (status, body) = app.get(&format!("/services/{service}/edit")).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(READER_EMAIL));
}
