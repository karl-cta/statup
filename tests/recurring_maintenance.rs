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
        SeriesRepository::service_ids(&app.pool, series.id)
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
