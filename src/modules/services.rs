//! Services card: the services an administrator did not leave out, with
//! their current status and thirty days of incident history. A service left
//! out comes back as soon as something is wrong with it.

use std::collections::HashMap;

use askama::Template;
use async_trait::async_trait;
use chrono::{DateTime, Duration, NaiveDate, Utc};

use crate::clock;
use crate::db::DbPool;
use crate::error::AppError;
use crate::i18n::I18n;
use crate::models::{Service, ServiceStatus, Severity, split_duration};
use crate::repositories::{
    EventRepository, IncidentSpan, OutageRepository, OutageSpan, ServiceRepository,
};

use super::{ColumnWidth, Module, ModuleOption, ModuleRenderContext, render_template};

const DAYS: i64 = 30;

/// A shorter detected outage does not colour a whole day of the strip.
const MIN_DETECTED_OUTAGE: Duration = Duration::minutes(15);

pub struct ServicesModule;

/// One day of the strip. `level` is `None` before the service existed,
/// otherwise the worst incident severity of the day, 0 for none.
pub struct DayCell {
    pub level: Option<u8>,
    pub date: String,
    pub status: String,
}

impl DayCell {
    pub fn class(&self) -> &'static str {
        match self.level {
            None => "bar bar-none",
            Some(0) => "bar",
            Some(1) => "bar bar-minor",
            Some(_) => "bar bar-crit",
        }
    }
}

pub struct ServiceRow {
    pub service: Service,
    pub days: Vec<DayCell>,
    pub availability_label: String,
}

#[derive(Template)]
#[template(path = "modules/services.html")]
struct ServicesTemplate {
    rows: Vec<ServiceRow>,
    can_publish: bool,
    i18n: I18n,
}

#[async_trait]
impl Module for ServicesModule {
    fn id(&self) -> &'static str {
        "services"
    }

    fn name_key(&self) -> &'static str {
        "modules.services.name"
    }

    fn default_position(&self) -> i64 {
        20
    }

    fn column_width(&self) -> ColumnWidth {
        ColumnWidth::Narrow
    }

    async fn render(&self, ctx: &ModuleRenderContext<'_>) -> Result<String, AppError> {
        let today = clock::today();
        let first_day = today - Duration::days(DAYS - 1);
        let since = clock::day_start(first_day).unwrap_or_default();
        let services = ServiceRepository::list_all(ctx.pool).await?;
        let spans = EventRepository::incident_spans(ctx.pool, since).await?;
        let outages = OutageRepository::since(ctx.pool, since).await?;
        let rows = services
            .into_iter()
            .filter(|service| {
                service.status != ServiceStatus::Operational || !is_left_out(service, ctx.hidden)
            })
            .map(|service| service_row(service, &spans, &outages, today, ctx.i18n))
            .collect();
        let template = ServicesTemplate {
            rows,
            can_publish: ctx.can_publish(),
            i18n: ctx.i18n.clone(),
        };
        render_template(self.id(), &template)
    }

    async fn options(
        &self,
        pool: &DbPool,
        _i18n: &I18n,
        hidden: Option<&[String]>,
    ) -> Result<Vec<ModuleOption>, AppError> {
        Ok(ServiceRepository::list_all(pool)
            .await?
            .into_iter()
            .map(|service| ModuleOption {
                shown: !is_left_out(&service, hidden),
                value: service.id.to_string(),
                label: service.name,
            })
            .collect())
    }

    fn options_note_key(&self) -> Option<&'static str> {
        Some("modules.services.options_note")
    }
}

fn is_left_out(service: &Service, hidden: Option<&[String]>) -> bool {
    let id = service.id.to_string();
    hidden.is_some_and(|values| values.contains(&id))
}

/// One service's last thirty days, for its side panel.
pub async fn service_history(
    pool: &DbPool,
    service: Service,
    i18n: &I18n,
) -> Result<ServiceRow, AppError> {
    let today = clock::today();
    let since = clock::day_start(today - Duration::days(DAYS - 1)).unwrap_or_default();
    let spans = EventRepository::incident_spans(pool, since).await?;
    let outages = OutageRepository::since(pool, since).await?;
    Ok(service_row(service, &spans, &outages, today, i18n))
}

fn service_row(
    service: Service,
    spans: &HashMap<i64, Vec<IncidentSpan>>,
    outages: &HashMap<i64, Vec<OutageSpan>>,
    today: NaiveDate,
    i18n: &I18n,
) -> ServiceRow {
    let strip = day_levels(
        spans.get(&service.id).map_or(&[], Vec::as_slice),
        outages.get(&service.id).map_or(&[], Vec::as_slice),
        clock::local_date(&service.created_at),
        today,
        Utc::now(),
    );
    let days = strip
        .iter()
        .map(|day| DayCell {
            level: day.level,
            date: i18n.format_date_short(&day.date),
            status: day_status(day, i18n),
        })
        .collect();
    ServiceRow {
        service,
        days,
        availability_label: availability_label(&strip, i18n),
    }
}

/// What one day of the strip shows.
struct Day {
    date: NaiveDate,
    /// `None` before the service existed, else 0 to 2.
    level: Option<u8>,
    /// Minutes of detected outage, when no incident that day explains it.
    detected_minutes: Option<i64>,
}

/// The last thirty days, oldest first. `now` bounds the outages under way.
fn day_levels(
    spans: &[IncidentSpan],
    outages: &[OutageSpan],
    created: NaiveDate,
    today: NaiveDate,
    now: DateTime<Utc>,
) -> Vec<Day> {
    (0..DAYS)
        .rev()
        .map(|offset| {
            let date = today - Duration::days(offset);
            if date < created {
                return Day {
                    date,
                    level: None,
                    detected_minutes: None,
                };
            }
            tracked_day(spans, outages, date, today, now)
        })
        .collect()
}

/// An incident decides the day; failing that, a long enough detected outage
/// colours it as a minor incident.
fn tracked_day(
    spans: &[IncidentSpan],
    outages: &[OutageSpan],
    date: NaiveDate,
    today: NaiveDate,
    now: DateTime<Utc>,
) -> Day {
    let level = worst_level_on(spans, date, today);
    let detected = detected_minutes_on(outages, date, now);
    if level == 0 && detected > 0 {
        return Day {
            date,
            level: Some(1),
            detected_minutes: Some(detected),
        };
    }
    Day {
        date,
        level: Some(level),
        detected_minutes: None,
    }
}

fn worst_level_on(spans: &[IncidentSpan], date: NaiveDate, today: NaiveDate) -> u8 {
    spans
        .iter()
        .filter(|span| {
            let start = clock::local_date(&span.start);
            let end = span.end.map_or(today, |end| clock::local_date(&end));
            start <= date && date <= end
        })
        .map(|span| severity_level(span.severity))
        .max()
        .unwrap_or(0)
}

fn severity_level(severity: Option<Severity>) -> u8 {
    match severity {
        Some(Severity::Critical) => 2,
        Some(Severity::Minor) | None => 1,
    }
}

/// Minutes of long enough detected outages that fall within `date`.
fn detected_minutes_on(outages: &[OutageSpan], date: NaiveDate, now: DateTime<Utc>) -> i64 {
    let (Some(day_start), Some(day_end)) = (
        clock::day_start(date),
        clock::day_start(date + Duration::days(1)),
    ) else {
        return 0;
    };
    outages
        .iter()
        .filter(|outage| outage.end.unwrap_or(now) - outage.start >= MIN_DETECTED_OUTAGE)
        .map(|outage| {
            let start = outage.start.max(day_start);
            let end = outage.end.unwrap_or(now).min(day_end);
            (end - start).max(Duration::zero())
        })
        .sum::<Duration>()
        .num_minutes()
}

fn day_status(day: &Day, i18n: &I18n) -> String {
    if let Some(minutes) = day.detected_minutes {
        let parts = split_duration(Duration::minutes(minutes));
        let duration = i18n.format_duration(&parts);
        return i18n.tf("availability.day_detected", &[("duration", &duration)]);
    }
    let key = match day.level {
        None => "availability.day_untracked",
        Some(0) => "availability.day_ok",
        Some(1) => ServiceStatus::Degraded.i18n_key(),
        Some(_) => ServiceStatus::MajorOutage.i18n_key(),
    };
    i18n.t(key).to_string()
}

fn availability_label(days: &[Day], i18n: &I18n) -> String {
    let untracked = days.iter().filter(|day| day.level.is_none()).count();
    let clear = days.iter().filter(|day| day.level == Some(0)).count();
    i18n.format_availability(clear, days.len() - untracked - clear, untracked)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(
        severity: Option<Severity>,
        start_days_ago: i64,
        end_days_ago: Option<i64>,
    ) -> IncidentSpan {
        let now = Utc::now();
        IncidentSpan {
            severity,
            start: now - Duration::days(start_days_ago),
            end: end_days_ago.map(|d| now - Duration::days(d)),
        }
    }

    fn at(date: NaiveDate, minutes: i64) -> DateTime<Utc> {
        clock::day_start(date).unwrap_or_default() + Duration::minutes(minutes)
    }

    fn outage(start: DateTime<Utc>, end: Option<DateTime<Utc>>) -> OutageSpan {
        OutageSpan { start, end }
    }

    /// The strip with the service created long ago, at noon of `today`.
    fn strip(spans: &[IncidentSpan], outages: &[OutageSpan]) -> Vec<Day> {
        let today = clock::today();
        day_levels(
            spans,
            outages,
            today - Duration::days(40),
            today,
            at(today, 12 * 60),
        )
    }

    #[test]
    fn days_before_creation_are_untracked() {
        let today = clock::today();
        let days = day_levels(&[], &[], today - Duration::days(1), today, Utc::now());
        assert_eq!(days.len(), 30);
        assert_eq!(days.iter().filter(|d| d.level.is_none()).count(), 28);
        assert_eq!(days.last().map(|d| d.date), Some(today));
    }

    #[test]
    fn an_open_incident_colours_every_day_until_today() {
        let spans = [span(Some(Severity::Critical), 2, None)];
        let days = strip(&spans, &[]);
        let coloured: Vec<u8> = days.iter().rev().take(3).filter_map(|d| d.level).collect();
        assert_eq!(coloured, vec![2, 2, 2]);
        assert_eq!(days[0].level, Some(0));
    }

    #[test]
    fn the_worst_incident_of_the_day_wins() {
        let today = clock::today();
        let spans = [span(Some(Severity::Minor), 0, None), span(None, 0, Some(0))];
        assert_eq!(worst_level_on(&spans, today, today), 1);
        let spans = [
            span(Some(Severity::Critical), 0, None),
            span(Some(Severity::Minor), 0, None),
        ];
        assert_eq!(worst_level_on(&spans, today, today), 2);
    }

    #[test]
    fn a_detected_outage_of_42_minutes_colours_the_day_and_says_so() {
        let today = clock::today();
        let outages = [outage(at(today, 60), Some(at(today, 102)))];
        let days = strip(&[], &outages);
        let day = &days[29];
        assert_eq!(day.level, Some(1));
        assert_eq!(day.detected_minutes, Some(42));
        assert_eq!(day_status(day, &I18n::new("en")), "Outage detected, 42 min");
        assert_eq!(
            day_status(day, &I18n::new("fr")),
            "Panne détectée, 42\u{a0}min"
        );
    }

    #[test]
    fn a_detected_outage_under_fifteen_minutes_leaves_the_day_clear() {
        let today = clock::today();
        let outages = [outage(at(today, 60), Some(at(today, 70)))];
        let days = strip(&[], &outages);
        assert_eq!(days[29].level, Some(0));
        assert_eq!(days[29].detected_minutes, None);
    }

    #[test]
    fn an_incident_of_the_day_wins_over_a_detected_outage() {
        let today = clock::today();
        let outages = [outage(at(today, 60), Some(at(today, 102)))];
        let minor = [span(Some(Severity::Minor), 0, None)];
        let day = &strip(&minor, &outages)[29];
        assert_eq!((day.level, day.detected_minutes), (Some(1), None));
        let critical = [span(Some(Severity::Critical), 0, None)];
        let day = &strip(&critical, &outages)[29];
        assert_eq!((day.level, day.detected_minutes), (Some(2), None));
    }

    #[test]
    fn an_outage_under_way_for_twenty_minutes_colours_today() {
        let today = clock::today();
        let outages = [outage(at(today, 12 * 60 - 20), None)];
        let days = strip(&[], &outages);
        assert_eq!(days[29].level, Some(1));
        assert_eq!(days[29].detected_minutes, Some(20));
    }

    #[test]
    fn an_outage_across_midnight_is_shared_between_both_days() {
        let today = clock::today();
        let outages = [outage(at(today, -30), Some(at(today, 30)))];
        let days = strip(&[], &outages);
        for day in &days[28..] {
            assert_eq!(day.level, Some(1));
            assert_eq!(day.detected_minutes, Some(30));
        }
    }
}
