//! The facts under a journal line, in the reader's language.
//!
//! A line keeps its detail in English, as the server output and the export
//! show it. The page reads back the forms the app writes and says them in
//! the reader's words; a detail it does not know, such as an error of the
//! app, is shown as it was written.

use chrono::{DateTime, Utc};

use crate::clock;
use crate::i18n::I18n;
use crate::models::{
    ChannelKind, JournalEntry, JournalKind, Kind, Lifecycle, Role, ServiceStatus, Visibility,
};
use crate::routes::notifications::reason_of;

/// The separator the app writes between the parts of a detail.
const PART: &str = " · ";

/// A line as the page shows it: the newest of the identical lines that
/// follow one another, its facts, and how many times it came.
pub struct JournalRow {
    pub entry: JournalEntry,
    pub facts: Vec<String>,
}

/// Folds the identical lines that follow one another on the same day into
/// one row that says how many there were and since when. Failed checks of
/// one service fold too: only their count in a row differs.
pub fn rows(entries: Vec<JournalEntry>, i18n: &I18n) -> Vec<JournalRow> {
    let mut rows: Vec<(JournalEntry, usize, DateTime<Utc>)> = Vec::new();
    for entry in entries {
        if let Some((newest, count, since)) = rows.last_mut()
            && repeats(newest, &entry)
        {
            *count += 1;
            *since = entry.created_at;
            continue;
        }
        let since = entry.created_at;
        rows.push((entry, 1, since));
    }
    rows.into_iter()
        .map(|(entry, count, since)| {
            let mut facts = facts(&entry, i18n);
            if count > 1 {
                let time = i18n.format_time(&since);
                facts.push(i18n.tf(
                    "journal.repeated",
                    &[("n", &count.to_string()), ("time", &time)],
                ));
            }
            JournalRow { entry, facts }
        })
        .collect()
}

fn repeats(newer: &JournalEntry, older: &JournalEntry) -> bool {
    newer.kind == older.kind
        && newer.subject == older.subject
        && newer.actor_name == older.actor_name
        && newer.ip == older.ip
        && clock::local_date(&newer.created_at) == clock::local_date(&older.created_at)
        && same_detail(newer, older)
}

/// Two failed checks in a row differ only by their count.
fn same_detail(newer: &JournalEntry, older: &JournalEntry) -> bool {
    let core = |entry: &JournalEntry| {
        let detail = entry.detail.as_deref().unwrap_or_default();
        if entry.kind == JournalKind::CheckFailed && detail.ends_with(" failed in a row") {
            return detail
                .rsplit_once(PART)
                .map_or(detail, |(core, _)| core)
                .to_string();
        }
        detail.to_string()
    };
    core(newer) == core(older)
}

/// What the page shows under the sentence of a line.
pub fn facts(entry: &JournalEntry, i18n: &I18n) -> Vec<String> {
    let mut facts = entry
        .detail
        .as_deref()
        .map(|detail| detail_facts(entry.kind, detail, i18n))
        .unwrap_or_default();
    facts.extend(entry.ip.clone());
    facts.retain(|fact| !fact.is_empty());
    facts
}

fn detail_facts(kind: JournalKind, detail: &str, i18n: &I18n) -> Vec<String> {
    use JournalKind as K;
    let said = |key: &str| vec![i18n.t(key).to_string()];
    match kind {
        K::SignInFailed => match detail {
            "wrong password" => said("journal.fact_wrong_password"),
            "temporary password expired" => said("journal.fact_temporary_expired"),
            // The sentence already says the account is unknown.
            _ => Vec::new(),
        },
        // The sentence says what was blocked and why.
        K::SignInBlocked
        | K::PasswordCheckBlocked
        | K::PasswordResetBlocked
        | K::DashboardArranged => Vec::new(),
        K::PasswordResetNotSent | K::SubscriberMailsAbandoned | K::SubscriberMailsPaused => {
            vec![reason_of(Some(detail), i18n)]
        }
        K::MemberAdded | K::MemberRoleChanged => detail
            .parse::<Role>()
            .ok()
            .map_or_else(|| vec![detail.to_string()], |role| said(role.i18n_key())),
        K::ServiceStatusSet => status_facts(detail, i18n),
        K::ServiceVisibilityChanged | K::FolderVisibilityChanged => visibility_facts(detail, i18n),
        K::GroupRenamed | K::FolderRenamed => {
            vec![i18n.tf("journal.fact_formerly", &[("name", detail)])]
        }
        K::ProblemsFirstChanged | K::VisitorSubscriptionsChanged => match detail {
            "on" => said("journal.fact_on"),
            _ => said("journal.fact_off"),
        },
        K::SettingsSaved => vec![settings_fields(detail, i18n)],
        K::JournalSettingsSaved => journal_settings(detail, i18n),
        K::EventUpdatePosted => lifecycle(detail).map_or_else(
            || vec![detail.to_string()],
            |state| said(state.label_key(Kind::Incident)),
        ),
        K::ChannelTested | K::AlertSent | K::AlertRetrying | K::AlertAbandoned => {
            delivery_facts(detail, i18n)
        }
        K::CheckFailed | K::OutageDetected | K::OutageEnded => check_facts(detail, i18n),
        K::MonitoringRoundSkipped => round_skipped(detail, i18n),
        K::NetworkLost => services_off_the_network(detail, i18n),
        K::OutageErased => erased_outage(detail, i18n),
        K::ReportIgnored => match detail {
            "unknown service" => said("journal.fact_unknown_service"),
            "checked by Statup" => said("journal.fact_checked_by_statup"),
            "under maintenance" => said("journal.fact_under_maintenance"),
            other => vec![other.to_string()],
        },
        _ => vec![detail.to_string()],
    }
}

/// `major_outage` or `major_outage, undo`.
fn status_facts(detail: &str, i18n: &I18n) -> Vec<String> {
    let (status, undo) = match detail.strip_suffix(", undo") {
        Some(status) => (status, true),
        None => (detail, false),
    };
    let mut facts = vec![status.parse::<ServiceStatus>().map_or_else(
        |()| status.to_string(),
        |status| i18n.t(status.i18n_key()).to_string(),
    )];
    if undo {
        facts.push(i18n.t("journal.fact_undo").to_string());
    }
    facts
}

/// Who sees the service: `everyone`, `members`, `folder` (it follows its
/// folder), or `groups:2;members:1`. Lines written before members could be
/// granted say `groups:3` alone.
fn visibility_facts(detail: &str, i18n: &I18n) -> Vec<String> {
    if detail == "folder" {
        return vec![i18n.t("journal.fact_inherited").to_string()];
    }
    if let Ok(mode @ (Visibility::Everyone | Visibility::Members)) = detail.parse::<Visibility>() {
        return vec![i18n.t(mode.i18n_key()).to_string()];
    }
    let Some(counts) = detail.strip_prefix("groups:") else {
        return vec![detail.to_string()];
    };
    let (groups, members) = counts.split_once(";members:").unwrap_or((counts, "0"));
    match (leading_number(groups), leading_number(members)) {
        (Some(groups), Some(0)) => vec![i18n.plural("journal.fact_groups", groups)],
        (Some(0), Some(members)) => vec![i18n.plural("journal.fact_members", members)],
        (Some(groups), Some(members)) => vec![
            i18n.plural("journal.fact_groups", groups),
            i18n.plural("journal.fact_members", members),
        ],
        _ => vec![detail.to_string()],
    }
}

/// `name, logo, time_zone, access, accent, ambiance`, as the settings page
/// names them.
fn settings_fields(detail: &str, i18n: &I18n) -> String {
    detail
        .split(", ")
        .map(|field| match field {
            "name" => i18n.t("admin.instance_name").to_string(),
            "logo" => i18n.t("admin.logo").to_string(),
            "time_zone" => i18n.t("admin.time_zone").to_string(),
            "access" => i18n.t("admin.public_mode").to_string(),
            "accent" => i18n.t("admin.accent").to_string(),
            "ambiance" => i18n.t("admin.ambiance").to_string(),
            other => other.to_string(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// `capture=detailed, retention=90 days`.
fn journal_settings(detail: &str, i18n: &I18n) -> Vec<String> {
    let mut facts = Vec::new();
    for part in detail.split(", ") {
        if let Some(capture) = part.strip_prefix("capture=") {
            let key = if capture == "detailed" {
                "journal.summary_detailed"
            } else {
                "journal.summary_essential"
            };
            facts.push(i18n.t(key).to_string());
        } else if let Some(days) = part.strip_prefix("retention=").and_then(leading_number) {
            facts.push(i18n.plural("journal.days", days));
        }
    }
    facts
}

fn lifecycle(code: &str) -> Option<Lifecycle> {
    Some(match code {
        "investigating" => Lifecycle::Investigating,
        "in_progress" => Lifecycle::InProgress,
        "monitoring" => Lifecycle::Monitoring,
        "resolved" => Lifecycle::Resolved,
        "cancelled" => Lifecycle::Cancelled,
        "scheduled" => Lifecycle::Scheduled,
        "completed" => Lifecycle::Completed,
        _ => return None,
    })
}

/// `slack · sent` or `slack · status_404`: the tool, then how it went.
fn delivery_facts(detail: &str, i18n: &I18n) -> Vec<String> {
    let Some((tool, outcome)) = detail.split_once(PART) else {
        return vec![detail.to_string()];
    };
    let tool = tool.parse::<ChannelKind>().map_or_else(
        |()| tool.to_string(),
        |kind| i18n.t(kind.label_key()).to_string(),
    );
    let outcome = if outcome == "sent" {
        i18n.t("journal.fact_sent").to_string()
    } else {
        reason_of(Some(outcome), i18n)
    };
    vec![tool, outcome]
}

/// `https://host/… · no answer in time · operation timed out · 3 failed in a
/// row`: the address, the reason in the reader's words and the count. The
/// raw error that follows the reason stays in the export.
fn check_facts(detail: &str, i18n: &I18n) -> Vec<String> {
    let mut parts = detail.split(PART);
    let mut facts: Vec<String> = parts.next().map(String::from).into_iter().collect();
    for part in parts {
        if let Some(reason) = finding(part, i18n) {
            facts.push(reason);
        } else if let Some(count) = part
            .strip_suffix(" failed in a row")
            .and_then(leading_number)
        {
            facts.push(i18n.plural("journal.fact_failures", count));
        }
    }
    facts
}

/// The words [`crate::services::Finding::words`] writes, said again.
pub(crate) fn finding(words: &str, i18n: &I18n) -> Option<String> {
    let key = match words {
        "connection refused" => "journal.fact_refused",
        "no answer in time" => "journal.fact_timed_out",
        "name not found" => "journal.fact_name_not_found",
        "name server not answering" => "journal.fact_name_server",
        "no network route" => "journal.fact_no_route",
        "certificate refused" => "journal.fact_certificate",
        "too many redirects" => "journal.fact_redirects",
        "unreachable" => "journal.fact_unreachable",
        other => {
            let status = other.strip_prefix("server error ")?;
            return Some(i18n.tf("journal.fact_server_error", &[("status", status)]));
        }
    };
    Some(i18n.t(key).to_string())
}

/// `12 services, Statup's own connection is suspected`.
fn round_skipped(detail: &str, i18n: &I18n) -> Vec<String> {
    leading_number(detail).map_or_else(
        || vec![detail.to_string()],
        |count| vec![i18n.plural("journal.fact_services_at_once", count)],
    )
}

/// `13 services`.
fn services_off_the_network(detail: &str, i18n: &I18n) -> Vec<String> {
    leading_number(detail).map_or_else(
        || vec![detail.to_string()],
        |count| vec![i18n.plural("journal.fact_services_off_network", count)],
    )
}

/// `2026-10-26T23:56:00Z · 2026-10-27T08:42:00Z`, then the service it was
/// erased from when erased with another's.
fn erased_outage(detail: &str, i18n: &I18n) -> Vec<String> {
    // The origin comes last and may hold the separator in its name.
    let mut parts = detail.splitn(3, " · ");
    let span = parts.next().zip(parts.next()).and_then(|(start, end)| {
        let start = DateTime::parse_from_rfc3339(start).ok()?.to_utc();
        let end = DateTime::parse_from_rfc3339(end).ok()?.to_utc();
        Some(i18n.tf(
            "journal.fact_outage_span",
            &[
                ("start", &i18n.format_datetime(&start)),
                ("end", &i18n.format_datetime(&end)),
            ],
        ))
    });
    let Some(span) = span else {
        return vec![detail.to_string()];
    };
    let mut facts = vec![span];
    facts.extend(
        parts
            .next()
            .map(|name| i18n.tf("journal.fact_erased_with", &[("name", name)])),
    );
    facts
}

fn leading_number(text: &str) -> Option<usize> {
    let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    fn entry(kind: JournalKind, detail: &str) -> JournalEntry {
        JournalEntry {
            id: 1,
            kind,
            actor_name: None,
            subject: None,
            detail: Some(detail.to_string()),
            ip: None,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn identical_lines_in_a_row_fold_into_one_and_say_since_when() {
        let refused = |minute: i64| {
            let mut line = entry(JournalKind::SignInFailed, "wrong password");
            line.subject = Some("Lise".into());
            line.created_at = Utc::now() - chrono::Duration::minutes(minute);
            line
        };
        let check = |count: u8| {
            entry(
                JournalKind::CheckFailed,
                &format!("https://intranet/… · no answer in time · {count} failed in a row"),
            )
        };
        let lines = vec![
            refused(0),
            refused(1),
            refused(2),
            check(3),
            check(2),
            check(1),
        ];

        let rows = rows(lines, &I18n::new("fr"));

        assert_eq!(rows.len(), 2);
        assert!(rows[0].facts.last().unwrap().starts_with("3 fois depuis "));
        assert_eq!(rows[1].facts[2], "3 échecs de suite");
        assert!(rows[1].facts[3].starts_with("3 fois depuis "));
    }

    fn said(kind: JournalKind, detail: &str) -> Vec<String> {
        facts(&entry(kind, detail), &I18n::new("fr"))
    }

    #[test]
    fn an_erased_outage_says_its_span_and_where_it_was_erased_from() {
        let facts = said(
            JournalKind::OutageErased,
            "2026-10-26T23:56:00Z · 2026-10-27T08:42:00Z · Mail · Paris",
        );
        assert_eq!(facts.len(), 2);
        assert!(facts[0].starts_with("du 26 oct."), "{}", facts[0]);
        assert_eq!(facts[1], "effacée avec «\u{a0}Mail · Paris\u{a0}»");
        assert_eq!(
            said(JournalKind::OutageErased, "not a span"),
            ["not a span"]
        );
    }

    #[test]
    fn a_failed_check_says_its_reason_and_count_without_the_raw_error() {
        assert_eq!(
            said(
                JournalKind::OutageDetected,
                "https://intranet.acme.local/… · no answer in time · operation timed out · 3 failed in a row"
            ),
            [
                "https://intranet.acme.local/…",
                "délai dépassé",
                "3 échecs de suite"
            ]
        );
        assert_eq!(
            said(
                JournalKind::CheckFailed,
                "nas.local:5000 · server error 503 · 1 failed in a row"
            ),
            [
                "nas.local:5000",
                "erreur du serveur (503)",
                "1 échec de suite"
            ]
        );
    }

    #[test]
    fn roles_settings_and_deliveries_are_said_in_the_reader_s_words() {
        assert_eq!(said(JournalKind::MemberAdded, "publisher"), ["Éditeur"]);
        assert_eq!(
            said(JournalKind::SettingsSaved, "name, access"),
            ["Nom de la page, Accès à la page"]
        );
        assert_eq!(
            said(
                JournalKind::JournalSettingsSaved,
                "capture=detailed, retention=90 days"
            ),
            ["Capture détaillée", "90 jours"]
        );
        assert_eq!(
            said(JournalKind::AlertAbandoned, "teams · status_404"),
            ["Microsoft Teams", "adresse introuvable (404)"]
        );
        assert_eq!(
            said(JournalKind::AlertSent, "slack · sent"),
            ["Slack", "envoyé"]
        );
        assert_eq!(
            said(JournalKind::ServiceStatusSet, "major_outage, undo"),
            ["En panne", "annulation"]
        );
    }

    #[test]
    fn who_sees_a_service_is_said_in_the_reader_s_words() {
        let said = |detail| said(JournalKind::ServiceVisibilityChanged, detail);
        assert_eq!(said("everyone"), ["Publique"]);
        assert_eq!(said("members"), ["Membres"]);
        assert_eq!(said("folder"), ["héritée du dossier"]);
        assert_eq!(said("groups:1"), ["1 groupe"]);
        assert_eq!(said("groups:12"), ["12 groupes"]);
        assert_eq!(said("groups:2;members:0"), ["2 groupes"]);
        assert_eq!(said("groups:0;members:3"), ["3 membres"]);
        assert_eq!(said("groups:1;members:1"), ["1 groupe", "1 membre"]);
        assert_eq!(said("somebody"), ["somebody"]);
        assert_eq!(
            super::facts(
                &entry(JournalKind::ServiceVisibilityChanged, "groups:2"),
                &I18n::new("en")
            ),
            ["2 groups"]
        );
    }

    #[test]
    fn a_renamed_group_says_its_former_name() {
        assert_eq!(
            said(JournalKind::GroupRenamed, "Support"),
            ["anciennement «\u{a0}Support\u{a0}»"]
        );
    }

    #[test]
    fn a_sign_in_line_keeps_only_what_the_sentence_does_not_say() {
        let mut refused = entry(JournalKind::SignInFailed, "wrong password");
        refused.ip = Some("203.0.113.7".into());
        assert_eq!(
            facts(&refused, &I18n::new("fr")),
            ["mot de passe incorrect", "203.0.113.7"]
        );
        assert_eq!(
            said(JournalKind::SignInBlocked, "too many attempts"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn an_unknown_detail_is_shown_as_written() {
        assert_eq!(
            said(JournalKind::AppError, "POST /admin/settings · code=7"),
            ["POST /admin/settings · code=7"]
        );
        assert_eq!(said(JournalKind::MemberAdded, "owner"), ["owner"]);
    }
}
