//! The bell of the masthead and the page of every notice, for publishers and
//! administrators.

use askama::Template;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::Utc;

use crate::db::DbPool;
use crate::error::AppError;
use crate::i18n::{I18n, Locale};
use crate::middleware::{CsrfToken, RequirePublisher};
use crate::models::{MemberNotice, NoticeKind, SEPARATOR, User};
use crate::repositories::MemberNoticeRepository;
use crate::routes::{Frame, render};
use crate::services::KEEP_HISTORY;
use crate::state::AppState;

/// How many notices the bell's menu lists.
const BELL_ROWS: i64 = 6;
/// How many the page lists: about a month of them, after which they go.
const PAGE_ROWS: i64 = 100;

/// The bell as the masthead draws it.
pub struct Bell {
    pub unread: i64,
    /// What a screen reader says of the bell.
    pub label: String,
    /// What it says once the notices are seen.
    pub seen_label: String,
    pub rows: Vec<NoticeRow>,
}

/// A notice as a line of the list.
pub struct NoticeRow {
    pub title: String,
    pub text: String,
    pub href: String,
    pub tone: &'static str,
    pub unread: bool,
    pub ago: String,
}

/// The bell of a member who publishes; readers have none.
pub async fn bell(pool: &DbPool, user: &User, i18n: &I18n) -> Result<Option<Bell>, AppError> {
    if !user.role.can_publish() {
        return Ok(None);
    }
    let (unread, notices) = tokio::try_join!(
        MemberNoticeRepository::unread_count(pool, user.id),
        MemberNoticeRepository::list_for(pool, user.id, BELL_ROWS),
    )?;
    let title = i18n.t("notices.title");
    let label = if unread > 0 {
        let count = i18n.plural("notices.unread", usize::try_from(unread).unwrap_or(0));
        format!("{title}, {count}")
    } else {
        title.to_string()
    };
    Ok(Some(Bell {
        unread,
        label,
        seen_label: title.to_string(),
        rows: notices.iter().map(|notice| row(notice, i18n)).collect(),
    }))
}

fn row(notice: &MemberNotice, i18n: &I18n) -> NoticeRow {
    NoticeRow {
        title: title(notice, i18n),
        text: text(notice, i18n),
        href: href(notice),
        tone: notice.kind.tone(),
        unread: notice.is_unread(),
        ago: i18n.format_ago(&notice.created_at),
    }
}

fn title(notice: &MemberNotice, i18n: &I18n) -> String {
    let name = |name: &Option<String>| name.clone().unwrap_or_default();
    match notice.kind {
        NoticeKind::UpdateOverdue => i18n.t("notices.update_overdue").to_string(),
        NoticeKind::OutageDetected if notice.others > 0 => {
            let others = i18n.plural(
                "notices.others",
                usize::try_from(notice.others).unwrap_or(0),
            );
            i18n.tf(
                "notices.outage_detected_many",
                &[
                    ("service", &name(&notice.service_name)),
                    ("others", &others),
                ],
            )
        }
        NoticeKind::OutageDetected => i18n.tf(
            "notices.outage_detected",
            &[("service", &name(&notice.service_name))],
        ),
        NoticeKind::DeliveryFailed => i18n.tf(
            "notices.delivery_failed",
            &[("channel", &name(&notice.channel_name))],
        ),
    }
}

/// "Messagerie très lente · prévue à 14:30" for a late update.
fn text(notice: &MemberNotice, i18n: &I18n) -> String {
    match notice.kind {
        NoticeKind::UpdateOverdue => {
            let incident = notice.event_title.clone().unwrap_or_default();
            match notice.due_at {
                Some(due) => {
                    let due = i18n.tf("notices.due", &[("when", &i18n.format_due(&due))]);
                    format!("{incident}{SEPARATOR}{due}")
                }
                None => incident,
            }
        }
        NoticeKind::OutageDetected => i18n.t("notices.outage_detected_text").to_string(),
        NoticeKind::DeliveryFailed => i18n.t("notices.delivery_failed_text").to_string(),
    }
}

/// Where a notice leads: the late incident, the incident to declare (the
/// dashboard, which lists them, for several services), the destinations.
fn href(notice: &MemberNotice) -> String {
    match notice.kind {
        NoticeKind::OutageDetected if notice.others > 0 => "/".to_string(),
        NoticeKind::UpdateOverdue => format!("/events/{}", notice.event_id.unwrap_or_default()),
        NoticeKind::OutageDetected => format!(
            "/events/new?service={}",
            notice.service_id.unwrap_or_default()
        ),
        NoticeKind::DeliveryFailed => "/admin/notifications".to_string(),
    }
}

#[derive(Template)]
#[template(path = "notices/index.html")]
struct NoticesTemplate {
    frame: Frame,
    rows: Vec<NoticeRow>,
    /// "30 derniers jours": how far back the list goes.
    period: String,
    i18n: I18n,
}

pub async fn index(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
    csrf_token: CsrfToken,
    Locale(i18n): Locale,
) -> Result<Response, AppError> {
    let notices = MemberNoticeRepository::list_for(&state.pool, user.id, PAGE_ROWS).await?;
    let rows = notices.iter().map(|notice| row(notice, &i18n)).collect();
    let frame = Frame::load(&state.pool, Some(&user), csrf_token.0, &i18n).await?;
    let days = KEEP_HISTORY.num_days().to_string();
    let period = i18n.tf("availability.legend", &[("n", &days)]);
    render(&NoticesTemplate {
        frame,
        rows,
        period,
        i18n,
    })
}

/// The bell was opened: what it lists is seen.
pub async fn seen(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
) -> Result<Response, AppError> {
    MemberNoticeRepository::mark_all_read(&state.pool, user.id, Utc::now()).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// "Clear all" in the bell: the member's list is emptied.
pub async fn clear(
    RequirePublisher(user): RequirePublisher,
    State(state): State<AppState>,
) -> Result<Response, AppError> {
    MemberNoticeRepository::clear_for(&state.pool, user.id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
