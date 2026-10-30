//! Integration tests for the reports of monitoring tools: each format Statup
//! reads, the key in a header or in the body, and an outage told as the
//! checks tell theirs.

mod common;

use reqwest::StatusCode;
use serde_json::{Value, json};

use common::TestApp;
use statup::models::{
    ChannelInput, ChannelKind, CheckKind, CreateEventInput, Happening, Kind, Role, ServiceCheck,
    Severity,
};
use statup::repositories::{NotificationRepository, ServiceRepository, UserRepository};
use statup::services::{ApiKeyService, EventService, NewApiKey};

const REPORTS: &str = "/api/v1/reports";

/// The body Alertmanager posts, in the shape of the `webhook_config`
/// section of its documentation
/// (https://github.com/prometheus/alertmanager/blob/main/docs/configuration.md),
/// with the values a rule on two services would give, and the Watchdog
/// alert every Prometheus setup fires, which names no service.
const ALERTMANAGER: &str = r#"{
  "version": "4",
  "groupKey": "{}:{severity=\"critical\"}",
  "truncatedAlerts": 0,
  "status": "firing",
  "receiver": "statup",
  "groupLabels": {"severity": "critical"},
  "commonLabels": {"severity": "critical"},
  "commonAnnotations": {},
  "externalURL": "http://alertmanager.example.com:9093",
  "alerts": [
    {
      "status": "firing",
      "labels": {"alertname": "MailDown", "service": "Mail", "severity": "critical"},
      "annotations": {"summary": "SMTP does not answer"},
      "startsAt": "2026-10-26T09:00:00Z",
      "endsAt": "0001-01-01T00:00:00Z",
      "generatorURL": "http://prometheus.example.com:9090/graph",
      "fingerprint": "a1b2c3d4e5f60718"
    },
    {
      "status": "firing",
      "labels": {"alertname": "VpnDown", "service": "vpn", "severity": "critical"},
      "annotations": {},
      "startsAt": "2026-10-26T09:00:30Z",
      "endsAt": "0001-01-01T00:00:00Z",
      "generatorURL": "http://prometheus.example.com:9090/graph",
      "fingerprint": "b2c3d4e5f6071829"
    },
    {
      "status": "firing",
      "labels": {"alertname": "Watchdog", "severity": "none"},
      "annotations": {},
      "startsAt": "2026-10-26T08:00:00Z",
      "endsAt": "0001-01-01T00:00:00Z",
      "generatorURL": "http://prometheus.example.com:9090/graph",
      "fingerprint": "c3d4e5f60718293a"
    }
  ]
}"#;

/// The example of Grafana's webhook contact point documentation
/// (https://grafana.com/docs/grafana/latest/alerting/configure-notifications/manage-contact-points/integrations/webhook-notifier/),
/// as is, but for a `service` label on each alert, as the rules of a team
/// that reports to Statup carry.
const GRAFANA: &str = r#"{
  "receiver": "My Super Webhook",
  "status": "firing",
  "orgId": 1,
  "alerts": [
    {
      "status": "firing",
      "labels": {"alertname": "High memory usage", "team": "blue", "zone": "us-1", "service": "Mail"},
      "annotations": {
        "description": "The system has high memory usage",
        "runbook_url": "https://myrunbook.com/runbook/1234",
        "summary": "This alert was triggered for zone us-1"
      },
      "startsAt": "2021-10-12T09:51:03.157076+02:00",
      "endsAt": "0001-01-01T00:00:00Z",
      "generatorURL": "https://play.grafana.org/alerting/1afz29v7z/edit",
      "fingerprint": "c6eadffa33fcdf37",
      "silenceURL": "https://play.grafana.org/alerting/silence/new?alertmanager=grafana&matchers=alertname%3DT2%2Cteam%3Dblue%2Czone%3Dus-1",
      "dashboardURL": "",
      "panelURL": "",
      "values": {"B": 44.23943737541908, "C": 1}
    },
    {
      "status": "resolved",
      "labels": {"alertname": "High CPU usage", "team": "blue", "zone": "eu-1", "service": "VPN"},
      "annotations": {
        "description": "The system has high CPU usage",
        "runbook_url": "https://myrunbook.com/runbook/1234",
        "summary": "This alert was triggered for zone eu-1"
      },
      "startsAt": "2021-10-12T09:56:03.157076+02:00",
      "endsAt": "0001-01-01T00:00:00Z",
      "generatorURL": "https://play.grafana.org/alerting/d1rdpdv7k/edit",
      "fingerprint": "bc97ff14869b13e3",
      "silenceURL": "https://play.grafana.org/alerting/silence/new?alertmanager=grafana&matchers=alertname%3DT1%2Cteam%3Dblue%2Czone%3Deu-1",
      "dashboardURL": "",
      "panelURL": "",
      "values": {"B": 44.23943737541908, "C": 1}
    }
  ],
  "groupLabels": {},
  "commonLabels": {"team": "blue"},
  "commonAnnotations": {},
  "externalURL": "https://play.grafana.org/",
  "version": "1",
  "groupKey": "{}:{}",
  "truncatedAlerts": 0,
  "title": "[FIRING:2]  (blue)",
  "state": "alerting",
  "message": "**Firing**"
}"#;

/// What Uptime Kuma's webhook posts when a monitor goes down: `heartbeat`
/// with the fields of its `toJSON`, `monitor`, and `msg`
/// (https://github.com/louislam/uptime-kuma/blob/master/server/notification-providers/webhook.js,
/// server/model/heartbeat.js).
const KUMA_DOWN: &str = r#"{
  "heartbeat": {
    "monitorID": 3, "status": 0, "time": "2026-10-26 09:00:00.123",
    "msg": "connect ECONNREFUSED 10.0.0.9:443", "ping": null, "important": true,
    "duration": 60, "retries": 0, "response": null
  },
  "monitor": {"id": 3, "name": "Mail", "url": "https://mail.example.com", "type": "http"},
  "msg": "[Mail] [Down] connect ECONNREFUSED 10.0.0.9:443"
}"#;

/// What Kuma's Test button posts: `Notification.send` with no monitor and
/// no heartbeat, the message being the notification's name and "Testing".
const KUMA_TEST: &str = r#"{"heartbeat": null, "monitor": null, "msg": "Statup Testing"}"#;

struct Answer {
    status: StatusCode,
    text: String,
    body: Value,
}

impl Answer {
    fn code(&self) -> &str {
        self.body["error"]["code"].as_str().unwrap_or_default()
    }

    fn applied(&self) -> Vec<(String, String)> {
        self.list("applied", "state")
    }

    fn ignored(&self) -> Vec<(String, String)> {
        self.list("ignored", "reason")
    }

    fn list(&self, name: &str, field: &str) -> Vec<(String, String)> {
        self.body[name]
            .as_array()
            .unwrap_or_else(|| panic!("no {name} in {}", self.text))
            .iter()
            .map(|item| {
                let service = item["service"].as_str().unwrap_or("-").to_string();
                (
                    service,
                    item[field].as_str().unwrap_or_default().to_string(),
                )
            })
            .collect()
    }
}

fn pair(service: &str, word: &str) -> (String, String) {
    (service.to_string(), word.to_string())
}

/// An instance with an admin, a key named Zabbix, a channel that takes
/// detected outages, and the services Mail and VPN.
async fn spawn() -> (TestApp, NewApiKey) {
    let app = TestApp::spawn().await;
    let admin = app
        .create_user("admin@test.com", "admin_password_12", "Admin", Role::Admin)
        .await;
    let creator = UserRepository::find_by_id(&app.pool, admin)
        .await
        .unwrap()
        .expect("the admin");
    let key = ApiKeyService::create(&app.pool, "Zabbix", &creator)
        .await
        .unwrap();
    let channel = ChannelInput {
        name: "IT team".to_string(),
        kind: ChannelKind::Slack,
        target: "https://hooks.slack.com/services/team".to_string(),
        locale: "en".to_string(),
        on_incidents: true,
        on_maintenances: true,
        on_publications: true,
        on_detected: true,
    };
    NotificationRepository::create_channel(&app.pool, &channel)
        .await
        .unwrap();
    for (name, slug) in [("Mail", "mail"), ("VPN", "vpn")] {
        ServiceRepository::create(&app.pool, name, slug, None, None, None)
            .await
            .unwrap();
    }
    (app, key)
}

async fn post(
    app: &TestApp,
    content_type: &str,
    authorization: Option<&str>,
    body: impl Into<reqwest::Body>,
) -> Answer {
    try_post(app, content_type, authorization, body)
        .await
        .expect("report failed")
}

async fn try_post(
    app: &TestApp,
    content_type: &str,
    authorization: Option<&str>,
    body: impl Into<reqwest::Body>,
) -> reqwest::Result<Answer> {
    let mut request = app
        .client
        .post(app.url(REPORTS))
        .header("Content-Type", content_type)
        .body(body);
    if let Some(value) = authorization {
        request = request.header("Authorization", value);
    }
    let resp = request.send().await?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    let body = serde_json::from_str(&text).unwrap_or(Value::Null);
    Ok(Answer { status, text, body })
}

async fn report(app: &TestApp, key: &NewApiKey, body: &str) -> Answer {
    let bearer = format!("Bearer {}", key.key);
    post(app, "application/json", Some(&bearer), body.to_string()).await
}

async fn service_id(app: &TestApp, slug: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM services WHERE slug = ?")
        .bind(slug)
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

/// Shown status, detected status and whether an outage is open.
async fn state_of(app: &TestApp, slug: &str) -> (String, Option<String>, bool) {
    let id = service_id(app, slug).await;
    let (status, detected): (String, Option<String>) =
        sqlx::query_as("SELECT status, detected_status FROM services WHERE id = ?")
            .bind(id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    let open: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM service_outages WHERE service_id = ? AND ended_at IS NULL)",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    (status, detected, open)
}

async fn queued(app: &TestApp) -> Vec<Happening> {
    sqlx::query_scalar("SELECT happening FROM notification_deliveries ORDER BY id")
        .fetch_all(&app.pool)
        .await
        .unwrap()
}

/// Kind, actor, subject and detail of each Journal line.
async fn journal(app: &TestApp) -> Vec<(String, Option<String>, Option<String>, Option<String>)> {
    sqlx::query_as("SELECT kind, actor_name, subject, detail FROM journal_entries ORDER BY id")
        .fetch_all(&app.pool)
        .await
        .unwrap()
}

fn line(
    kind: &str,
    subject: &str,
    detail: Option<&str>,
) -> (String, Option<String>, Option<String>, Option<String>) {
    (
        kind.to_string(),
        Some("Zabbix".to_string()),
        Some(subject.to_string()),
        detail.map(str::to_string),
    )
}

#[tokio::test]
async fn a_service_reported_down_then_up_is_told_as_the_checks_tell() {
    let (app, key) = spawn().await;

    let down = r#"{"service": " mail ", "state": "down", "message": "SMTP refuses"}"#;
    let answer = report(&app, &key, down).await;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.text);
    assert_eq!(answer.applied(), [pair("Mail", "down")]);
    assert_eq!(
        state_of(&app, "mail").await,
        (
            "major_outage".to_string(),
            Some("major_outage".to_string()),
            true
        )
    );
    assert_eq!(queued(&app).await, [Happening::ServiceDown]);
    let notices: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM member_notices")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(notices, 1, "the admin's bell offers the incident");
    let source: String = sqlx::query_scalar("SELECT source FROM service_outages")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(source, "report", "the panel says a tool found it");

    let answer = report(&app, &key, r#"{"service": "Mail", "state": "up"}"#).await;
    assert_eq!(answer.applied(), [pair("Mail", "up")]);
    assert_eq!(
        state_of(&app, "mail").await,
        ("operational".to_string(), None, false)
    );
    assert_eq!(
        queued(&app).await,
        [Happening::ServiceDown, Happening::ServiceUp]
    );

    let again = report(&app, &key, r#"{"service": "Mail", "state": "up"}"#).await;
    assert_eq!(again.status, StatusCode::OK);
    assert_eq!(again.ignored(), [pair("Mail", "unchanged")]);
    assert_eq!(
        journal(&app).await,
        [
            line("outage_detected", "Mail", Some("SMTP refuses")),
            line("outage_ended", "Mail", None),
        ]
    );
}

#[tokio::test]
async fn a_tool_without_headers_sends_the_key_in_a_form() {
    let (app, key) = spawn().await;

    let form = format!("key={}&service=VPN&state=down", key.key);
    let answer = post(&app, "application/x-www-form-urlencoded", None, form).await;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.text);
    assert_eq!(answer.applied(), [pair("VPN", "down")]);

    let json = format!(
        r#"{{"key": "{}", "service": "VPN", "state": "up"}}"#,
        key.key
    );
    let answer = post(&app, "text/plain", None, json).await;
    assert_eq!(answer.applied(), [pair("VPN", "up")]);
}

#[tokio::test]
async fn the_key_is_never_said_back_and_a_bad_one_is_refused() {
    let (app, key) = spawn().await;

    let bad_state = format!(
        r#"{{"key": "{}", "service": "Mail", "state": "broken"}}"#,
        key.key
    );
    let answer = post(&app, "application/json", None, bad_state).await;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST);
    assert_eq!(answer.code(), "invalid_state");
    assert!(!answer.text.contains(&key.key));

    let malformed = format!(r#"{{"key": "{}", "service": "#, key.key);
    let answer = post(&app, "application/json", None, malformed).await;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST);
    assert_eq!(answer.code(), "invalid_data");
    assert!(!answer.text.contains(&key.key));

    let unknown_key = format!(
        r#"{{"key": "stp_{}", "service": "Mail", "state": "down"}}"#,
        "a".repeat(40)
    );
    for body in [
        unknown_key,
        r#"{"service": "Mail", "state": "down"}"#.to_string(),
    ] {
        let answer = post(&app, "application/json", None, body).await;
        assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
        assert_eq!(answer.code(), "unauthorized");
    }

    let resp = app
        .client
        .post(app.url(&format!("{REPORTS}?key={}", key.key)))
        .header("Content-Type", "application/json")
        .body(r#"{"service": "Mail", "state": "down"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(state_of(&app, "mail").await.1, None);
    assert!(journal(&app).await.is_empty());
}

#[tokio::test]
async fn a_report_statup_cannot_act_on_is_refused_and_journaled() {
    let (app, key) = spawn().await;
    let check = ServiceCheck {
        kind: CheckKind::Tcp,
        target: "10.0.0.9:443".to_string(),
        internal_cert: false,
    };
    let vpn = service_id(&app, "vpn").await;
    ServiceRepository::set_check(&app.pool, vpn, Some(&check))
        .await
        .unwrap();
    let mail = service_id(&app, "mail").await;
    let author = UserRepository::find_by_email(&app.pool, "admin@test.com")
        .await
        .unwrap()
        .expect("the admin");
    let works = CreateEventInput {
        kind: Kind::Maintenance,
        severity: None,
        planned: false,
        category: None,
        title: "Mail upgrade".to_string(),
        description: String::new(),
        planned_start: None,
        planned_end: None,
        started_at: None,
        opening_step: None,
        keeps_services_up: false,
        service_ids: vec![mail],
        follows_event_id: None,
        author_id: author.id,
        api_key_id: None,
        next_update: None,
    };
    EventService::create(&app.pool, works).await.unwrap();

    let refusals = [
        (
            "Payroll",
            StatusCode::NOT_FOUND,
            "unknown_service",
            "unknown service",
        ),
        (
            "VPN",
            StatusCode::CONFLICT,
            "monitored_by_statup",
            "checked by Statup",
        ),
        (
            "Mail",
            StatusCode::CONFLICT,
            "under_maintenance",
            "under maintenance",
        ),
    ];
    for (service, status, code, _) in refusals {
        let body = format!(r#"{{"service": "{service}", "state": "down"}}"#);
        let answer = report(&app, &key, &body).await;
        assert_eq!((answer.status, answer.code()), (status, code), "{service}");
    }
    let ignored: Vec<_> = journal(&app)
        .await
        .into_iter()
        .filter(|entry| entry.0 == "report_ignored")
        .collect();
    let expected: Vec<_> = refusals
        .iter()
        .map(|(service, _, _, words)| line("report_ignored", service, Some(words)))
        .collect();
    assert_eq!(ignored, expected);

    app.login("admin@test.com", "admin_password_12").await;
    let (status, page) = app.get("/admin/journal").await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("Signalement ignoré"), "{page}");
    assert!(page.contains("Payroll"));
    assert!(page.contains("aucun service de ce nom"));
    assert!(page.contains("Statup vérifie ce service lui-même"));

    let answer = report(&app, &key, r#"{"state": "down"}"#).await;
    assert_eq!(answer.code(), "service_required");
    let answer = report(&app, &key, r#"{"name": "Mail", "down": true}"#).await;
    assert_eq!(
        (answer.status, answer.code()),
        (StatusCode::BAD_REQUEST, "unknown_format")
    );
}

#[tokio::test]
async fn an_alertmanager_group_is_told_as_one_wave() {
    let (app, key) = spawn().await;

    let answer = report(&app, &key, ALERTMANAGER).await;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.text);
    assert_eq!(
        answer.applied(),
        [pair("Mail", "down"), pair("VPN", "down")]
    );
    assert_eq!(answer.ignored(), [pair("-", "no_service_label")]);
    let waves: Vec<(String, String)> = sqlx::query_as(
        "SELECT created_at, next_attempt_at FROM notification_deliveries ORDER BY id",
    )
    .fetch_all(&app.pool)
    .await
    .unwrap();
    assert_eq!(waves.len(), 2);
    assert_eq!(waves[0].1, waves[1].1, "both leave together");
    assert!(
        waves[0].1 > waves[0].0,
        "a little ahead, to gather the wave"
    );
    let detail = journal(&app).await[0].3.clone();
    assert_eq!(detail.as_deref(), Some("SMTP does not answer"));

    let resolved = ALERTMANAGER.replace(r#""status": "firing""#, r#""status": "resolved""#);
    let answer = report(&app, &key, &resolved).await;
    assert_eq!(answer.applied(), [pair("Mail", "up"), pair("VPN", "up")]);
}

#[tokio::test]
async fn grafana_is_read_by_its_shape_whatever_its_version() {
    let (app, key) = spawn().await;
    report(&app, &key, r#"{"service": "VPN", "state": "down"}"#).await;

    let answer = report(&app, &key, GRAFANA).await;
    assert_eq!(answer.status, StatusCode::OK, "{}", answer.text);
    assert_eq!(answer.applied(), [pair("Mail", "down"), pair("VPN", "up")]);
}

#[tokio::test]
async fn uptime_kuma_is_read_and_its_test_button_changes_nothing() {
    let (app, key) = spawn().await;

    let answer = report(&app, &key, KUMA_TEST).await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.body, json!({"applied": [], "ignored": []}));

    let answer = report(&app, &key, KUMA_DOWN).await;
    assert_eq!(answer.applied(), [pair("Mail", "down")]);
    assert_eq!(
        journal(&app).await[0].3.as_deref(),
        Some("connect ECONNREFUSED 10.0.0.9:443")
    );

    let pending = KUMA_DOWN.replace(r#""status": 0"#, r#""status": 2"#);
    let answer = report(&app, &key, &pending).await;
    assert_eq!(answer.ignored(), [pair("Mail", "not_down_or_up")]);

    let up = KUMA_DOWN.replace(r#""status": 0"#, r#""status": 1"#);
    let answer = report(&app, &key, &up).await;
    assert_eq!(answer.applied(), [pair("Mail", "up")]);
}

#[tokio::test]
async fn an_incident_under_way_already_tells_people() {
    let (app, key) = spawn().await;
    let mail = service_id(&app, "mail").await;
    let author = UserRepository::find_by_email(&app.pool, "admin@test.com")
        .await
        .unwrap()
        .expect("the admin");
    let incident = CreateEventInput {
        kind: Kind::Incident,
        severity: Some(Severity::Critical),
        planned: false,
        category: None,
        title: "Mail down".to_string(),
        description: String::new(),
        planned_start: None,
        planned_end: None,
        started_at: None,
        opening_step: None,
        keeps_services_up: false,
        service_ids: vec![mail],
        follows_event_id: None,
        author_id: author.id,
        api_key_id: None,
        next_update: None,
    };
    EventService::create(&app.pool, incident).await.unwrap();

    let answer = report(&app, &key, r#"{"service": "Mail", "state": "down"}"#).await;
    assert_eq!(answer.applied(), [pair("Mail", "down")]);
    assert_eq!(queued(&app).await, [Happening::Opened]);
}

#[tokio::test]
async fn a_service_named_twice_in_one_report_changes_once() {
    let (app, key) = spawn().await;
    let twice = json!({"alerts": [
        {"status": "firing", "labels": {"service": "Mail", "alertname": "SmtpDown"}},
        {"status": "firing", "labels": {"service": "mail", "alertname": "ImapDown"}},
    ]});

    let answer = report(&app, &key, &twice.to_string()).await;
    assert_eq!(answer.applied(), [pair("Mail", "down")]);
    assert_eq!(answer.ignored(), [pair("mail", "unchanged")]);
    assert_eq!(queued(&app).await, [Happening::ServiceDown]);
}

#[tokio::test]
async fn a_report_past_the_body_limit_is_refused_in_json() {
    let (app, key) = spawn().await;
    let padding = "a".repeat(65 * 1024);
    let body = format!(r#"{{"service": "Mail", "state": "down", "message": "{padding}"}}"#);
    let bearer = format!("Bearer {}", key.key);

    // The limit answers before the body is read: Windows may then reset the
    // connection under the upload, and the answer is lost.
    match try_post(&app, "application/json", Some(&bearer), body).await {
        Err(e) if cfg!(windows) => assert!(e.is_request(), "{e}"),
        sent => {
            let answer = sent.expect("report failed");
            assert_eq!(answer.status, StatusCode::PAYLOAD_TOO_LARGE);
            assert_eq!(answer.code(), "payload_too_large");
        }
    }
    assert_eq!(state_of(&app, "mail").await.1, None);
}
