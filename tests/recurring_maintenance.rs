//! A maintenance announced to repeat, through the event form.

mod common;

use chrono::{Duration, Utc};
use reqwest::StatusCode;

use common::TestApp;
use statup::i18n::{I18n, LANGUAGES};
use statup::models::{Rank, Rhythm, Role};
use statup::repositories::{EventRepository, SeriesRepository, ServiceRepository};

impl TestApp {
    async fn as_publisher(&self) -> i64 {
        self.create_user(
            "publisher@example.com",
            "publisher_pass_12",
            "Publisher",
            Role::Publisher,
        )
        .await;
        self.login("publisher@example.com", "publisher_pass_12")
            .await;
        ServiceRepository::create(&self.pool, "Backups", "backups", None, None, None)
            .await
            .unwrap()
            .id
    }

    async fn announce(&self, fields: &[(&str, String)]) -> (StatusCode, String, Option<String>) {
        let csrf = self.csrf_from("/events/new").await;
        let fields: Vec<(&str, &str)> = fields.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.post_form("/events/new", &csrf, &fields).await
    }

    async fn series_count(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM maintenance_series")
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }
}

/// A Sunday at 02:00 on the instance clock, weeks ahead, as the form sends it.
fn sunday_ahead() -> chrono::NaiveDateTime {
    let mut day = statup::clock::local(&(Utc::now() + Duration::weeks(3))).date_naive();
    while chrono::Datelike::weekday(&day) != chrono::Weekday::Sun {
        day = day.succ_opt().unwrap();
    }
    day.and_hms_opt(2, 0, 0).unwrap()
}

fn input(time: chrono::NaiveDateTime) -> String {
    time.format("%Y-%m-%dT%H:%M").to_string()
}

fn maintenance(service: i64, start: chrono::NaiveDateTime) -> Vec<(&'static str, String)> {
    vec![
        ("title", "Sunday backups".to_string()),
        ("description", String::new()),
        ("kind", "maintenance".to_string()),
        ("planned_start", input(start)),
        ("service_ids", service.to_string()),
        ("repeats", "on".to_string()),
    ]
}

fn event_id(location: &str) -> i64 {
    location
        .split('?')
        .next()
        .and_then(|path| path.rsplit('/').next())
        .and_then(|id| id.parse().ok())
        .unwrap()
}

#[tokio::test]
async fn a_maintenance_announced_to_repeat_starts_its_series() {
    let app = TestApp::spawn().await;
    let service = app.as_publisher().await;
    let start = sunday_ahead();
    let mut fields = maintenance(service, start);
    fields.push(("planned_end", input(start + Duration::hours(2))));
    fields.push(("repeat", "monthly".to_string()));

    let (status, _, location) = app.announce(&fields).await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    let event = EventRepository::find_by_id(&app.pool, event_id(&location.unwrap()))
        .await
        .unwrap()
        .unwrap();
    let series = SeriesRepository::find(&app.pool, event.series_id.unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(series.recurrence.rhythm(), Rhythm::Monthly);
    assert_eq!(series.recurrence.weekday(), chrono::Weekday::Sun);
    let rank = Rank::of_day(chrono::Datelike::day(&start));
    assert_eq!(series.recurrence.rank(), Some(rank));
    assert_eq!(series.duration_minutes, 120);
    assert!(series.next_start > event.planned_start.unwrap());
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT service_id FROM maintenance_series_services WHERE series_id = ?",
        )
        .bind(series.id)
        .fetch_all(&app.pool)
        .await
        .unwrap(),
        vec![service]
    );
}

#[tokio::test]
async fn a_repeating_maintenance_without_a_length_is_refused() {
    let app = TestApp::spawn().await;
    let service = app.as_publisher().await;

    let (status, body, _) = app.announce(&maintenance(service, sunday_ahead())).await;

    assert_eq!(status, StatusCode::OK);
    let message = I18n::new(LANGUAGES[0].code).t("validation.repeat_needs_duration");
    let escaped = message.replace('\'', "&#39;");
    assert!(body.contains(message) || body.contains(&escaped));
    assert!(body.contains("data-repeat-box checked"));
    assert_eq!(app.series_count().await, 0);
    let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(events, 0);
}

#[tokio::test]
async fn a_maintenance_begun_now_does_not_repeat() {
    let app = TestApp::spawn().await;
    let service = app.as_publisher().await;
    let mut fields = maintenance(service, sunday_ahead());
    fields.retain(|(name, _)| *name != "planned_start");

    let (status, _, location) = app.announce(&fields).await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    let event = EventRepository::find_by_id(&app.pool, event_id(&location.unwrap()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(event.series_id, None);
    assert_eq!(app.series_count().await, 0);
}

impl TestApp {
    /// A weekly maintenance announced through the form; returns its first
    /// session and when it starts.
    async fn weekly_series(&self, service: i64) -> (i64, chrono::NaiveDateTime) {
        let start = sunday_ahead();
        let mut fields = maintenance(service, start);
        fields.push(("planned_end", input(start + Duration::hours(2))));
        let (_, _, location) = self.announce(&fields).await;
        (event_id(&location.unwrap()), start)
    }

    async fn series_of(&self, session: i64) -> statup::models::MaintenanceSeries {
        let event = EventRepository::find_by_id(&self.pool, session)
            .await
            .unwrap()
            .unwrap();
        SeriesRepository::find(&self.pool, event.series_id.unwrap())
            .await
            .unwrap()
            .unwrap()
    }
}

#[tokio::test]
async fn a_session_names_its_rhythm_and_a_publisher_stops_it_there() {
    let app = TestApp::spawn().await;
    let service = app.as_publisher().await;
    let (session, _) = app.weekly_series(service).await;
    let rhythm = I18n::new(LANGUAGES[0].code).tf(
        "events.repeat_weekly",
        &[(
            "weekday",
            I18n::new(LANGUAGES[0].code).weekday_name(chrono::Weekday::Sun),
        )],
    );

    let (_, page) = app.get(&format!("/events/{session}")).await;
    assert!(page.contains(&rhythm));
    assert!(page.contains(&format!("/events/{session}/stop-repeat")));

    let csrf = app.csrf_from(&format!("/events/{session}")).await;
    let (status, _, _) = app
        .post_form(&format!("/events/{session}/stop-repeat"), &csrf, &[])
        .await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(app.series_of(session).await.stopped_at.is_some());
    let (_, page) = app.get(&format!("/events/{session}")).await;
    assert!(!page.contains(&rhythm));
    let journal: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM journal_entries WHERE kind = 'repeat_stopped'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(journal, 1);
}

#[tokio::test]
async fn an_edit_of_a_session_carries_to_the_next_ones_when_asked() {
    let app = TestApp::spawn().await;
    let service = app.as_publisher().await;
    let (session, start) = app.weekly_series(service).await;
    let edit = format!("/events/{session}/edit");
    let (_, form) = app.get(&edit).await;
    assert!(form.contains("name=\"also_following\""));

    let csrf = app.csrf_from(&edit).await;
    let fields = [
        ("title", "Longer backups".to_string()),
        ("description", String::new()),
        ("planned_start", input(start)),
        ("planned_end", input(start + Duration::hours(3))),
        ("service_ids", service.to_string()),
        ("also_following", "on".to_string()),
    ];
    let fields: Vec<(&str, &str)> = fields.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (status, _, _) = app.post_form(&edit, &csrf, &fields).await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    let series = app.series_of(session).await;
    assert_eq!(series.title, "Longer backups");
    assert_eq!(series.duration_minutes, 180);
}

#[tokio::test]
async fn a_session_the_schedule_adds_opens_as_added_on_its_page() {
    let app = TestApp::spawn().await;
    let service = app.as_publisher().await;
    let (first, _) = app.weekly_series(service).await;
    let start = EventRepository::find_by_id(&app.pool, first)
        .await
        .unwrap()
        .unwrap()
        .planned_start
        .unwrap();
    statup::services::EventService::apply_schedule(&app.pool, start + Duration::minutes(1))
        .await
        .unwrap();
    let added: i64 = sqlx::query_scalar("SELECT id FROM events WHERE added_by_schedule = 1")
        .fetch_one(&app.pool)
        .await
        .unwrap();

    let (_, page) = app.get(&format!("/events/{added}")).await;

    let i18n = I18n::new(LANGUAGES[0].code);
    assert!(page.contains(i18n.t("timeline.added")));
    assert!(!page.contains(i18n.t("timeline.announced")));
}
