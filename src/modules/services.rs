//! Services card: the services an administrator did not leave out, with
//! their current status and thirty days of incident history. A service left
//! out comes back as soon as something is wrong with it.

use std::collections::HashMap;

use askama::Template;
use async_trait::async_trait;
use chrono::{Duration, NaiveDate};

use crate::clock;
use crate::db::DbPool;
use crate::error::AppError;
use crate::i18n::I18n;
use crate::models::{Service, ServiceStatus, Severity};
use crate::repositories::{EventRepository, IncidentSpan, ServiceRepository};

use super::{ColumnWidth, Module, ModuleOption, ModuleRenderContext, render_template};

const DAYS: i64 = 30;

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
        let rows = services
            .into_iter()
            .filter(|service| {
                service.status != ServiceStatus::Operational || !is_left_out(service, ctx.hidden)
            })
            .map(|service| service_row(service, &spans, today, ctx.i18n))
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
    Ok(service_row(service, &spans, today, i18n))
}

fn service_row(
    service: Service,
    spans: &HashMap<i64, Vec<IncidentSpan>>,
    today: NaiveDate,
    i18n: &I18n,
) -> ServiceRow {
    let levels = day_levels(
        spans.get(&service.id).map_or(&[], Vec::as_slice),
        clock::local_date(&service.created_at),
        today,
    );
    let days = levels
        .iter()
        .map(|(date, level)| DayCell {
            level: *level,
            date: i18n.format_date_short(date),
            status: day_status(*level, i18n),
        })
        .collect();
    ServiceRow {
        service,
        days,
        availability_label: availability_label(&levels, i18n),
    }
}

/// Worst incident level of each of the last thirty days, oldest first.
fn day_levels(
    spans: &[IncidentSpan],
    created: NaiveDate,
    today: NaiveDate,
) -> Vec<(NaiveDate, Option<u8>)> {
    (0..DAYS)
        .rev()
        .map(|offset| {
            let date = today - Duration::days(offset);
            let level = (date >= created).then(|| worst_level_on(spans, date, today));
            (date, level)
        })
        .collect()
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

fn day_status(level: Option<u8>, i18n: &I18n) -> String {
    let key = match level {
        None => "availability.day_untracked",
        Some(0) => "availability.day_ok",
        Some(1) => ServiceStatus::Degraded.i18n_key(),
        Some(_) => ServiceStatus::MajorOutage.i18n_key(),
    };
    i18n.t(key).to_string()
}

fn availability_label(levels: &[(NaiveDate, Option<u8>)], i18n: &I18n) -> String {
    let untracked = levels.iter().filter(|(_, l)| l.is_none()).count();
    let clear = levels.iter().filter(|(_, l)| *l == Some(0)).count();
    i18n.format_availability(clear, levels.len() - untracked - clear, untracked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

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

    #[test]
    fn days_before_creation_are_untracked() {
        let today = clock::today();
        let levels = day_levels(&[], today - Duration::days(1), today);
        assert_eq!(levels.len(), 30);
        assert_eq!(levels.iter().filter(|(_, l)| l.is_none()).count(), 28);
        assert_eq!(levels.last().map(|(d, _)| *d), Some(today));
    }

    #[test]
    fn an_open_incident_colours_every_day_until_today() {
        let today = clock::today();
        let spans = [span(Some(Severity::Critical), 2, None)];
        let levels = day_levels(&spans, today - Duration::days(40), today);
        let coloured: Vec<u8> = levels
            .iter()
            .rev()
            .take(3)
            .filter_map(|(_, l)| *l)
            .collect();
        assert_eq!(coloured, vec![2, 2, 2]);
        assert_eq!(levels[0].1, Some(0));
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
}
