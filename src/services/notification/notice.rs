//! The message about what happened on the page, written in a destination's
//! language, before any tool's format.

use chrono::{DateTime, Utc};

use crate::clock;
use crate::i18n::I18n;
use crate::models::{
    Event, Happening, Kind, Lifecycle, SEPARATOR, Tone, chip_key, excerpt, html_to_text,
    split_duration, state_tone,
};
use crate::services::sanitize_markdown;

/// Longest text in a message, in characters: every tool takes it whole, and
/// the link leads to the rest.
const MAX_TEXT_CHARS: usize = 600;

/// The square that opens a message, in the hue of what it reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Outage,
    Degraded,
    Maintenance,
    Restored,
    /// An announcement, a cancellation or a test.
    Plain,
}

impl Mark {
    fn of(tone: Tone) -> Self {
        match tone {
            Tone::Crit => Self::Outage,
            Tone::Minor => Self::Degraded,
            Tone::Info => Self::Maintenance,
            Tone::Ok => Self::Restored,
            Tone::Ink | Tone::Neutral => Self::Plain,
        }
    }
}

/// What a message is about, as it stands when the message goes out.
pub enum Subject {
    Event {
        event: Event,
        services: Vec<String>,
        /// The update posted with the change, as stored: sanitized HTML.
        update: Option<String>,
    },
    /// A service the checks found down, or back.
    Service {
        name: String,
        /// When it stopped answering.
        down_since: Option<DateTime<Utc>>,
    },
    Test,
}

/// Who signs the messages and where their links lead.
pub struct Origin {
    pub instance: String,
    /// The address of the page, without a trailing slash.
    pub page: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub url: String,
    pub label: String,
}

/// A message ready for any tool's format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub mark: Mark,
    /// "Incident majeur · Coupure du réseau au siège".
    pub headline: String,
    /// The state and the services: "En correction · Réseau, VPN".
    pub summary: Option<String>,
    /// What the team wrote, as plain text.
    pub text: Option<String>,
    pub link: Option<Link>,
    /// The page the message comes from.
    pub instance: String,
}

impl Notice {
    /// The message for `happening`, which took place `at`. `lifecycle` is
    /// the state the event was in then.
    pub fn write(
        happening: Happening,
        lifecycle: Option<Lifecycle>,
        subject: &Subject,
        origin: &Origin,
        i18n: &I18n,
        at: DateTime<Utc>,
    ) -> Self {
        match subject {
            Subject::Event {
                event,
                services,
                update,
            } => {
                let about = AboutEvent {
                    happening,
                    lifecycle: lifecycle.or(event.lifecycle),
                    event,
                    services,
                    update: update.as_deref(),
                };
                about.notice(origin, i18n)
            }
            Subject::Service { name, down_since } => {
                service_notice(happening, name, *down_since, at, origin, i18n)
            }
            Subject::Test => Self {
                mark: Mark::Plain,
                headline: i18n.t("notice.test").to_string(),
                summary: None,
                text: Some(i18n.tf("notice.test_text", &[("instance", &origin.instance)])),
                link: page_link(origin, i18n),
                instance: origin.instance.clone(),
            },
        }
    }
}

struct AboutEvent<'a> {
    happening: Happening,
    lifecycle: Option<Lifecycle>,
    event: &'a Event,
    services: &'a [String],
    update: Option<&'a str>,
}

impl AboutEvent<'_> {
    fn notice(&self, origin: &Origin, i18n: &I18n) -> Notice {
        let event = self.event;
        let services = (!self.services.is_empty()).then(|| self.services.join(", "));
        let summary = [self.state(i18n), services]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let kind = i18n.t(chip_key(event.kind, event.severity, event.category));
        Notice {
            mark: Mark::of(state_tone(event.kind, event.severity, self.lifecycle)),
            headline: format!("{kind}{SEPARATOR}{}", event.title),
            summary: (!summary.is_empty()).then(|| summary.join(SEPARATOR)),
            text: self.text(),
            link: origin.page.as_ref().map(|page| Link {
                url: format!("{page}/events/{}", event.id),
                label: i18n.t("notice.see_details").to_string(),
            }),
            instance: origin.instance.clone(),
        }
    }

    /// Where the event stands: its state, with the window of a maintenance
    /// or how long the work lasted once it is over.
    fn state(&self, i18n: &I18n) -> Option<String> {
        let event = self.event;
        let label = self
            .lifecycle
            .map(|lifecycle| i18n.t(lifecycle.label_key(event.kind)).to_string());
        let window = event
            .planned_start
            .map(|start| window(start, event.planned_end, i18n));
        match self.happening {
            Happening::Opened if event.kind == Kind::Maintenance => window
                .map(|window| i18n.tf("notice.scheduled", &[("window", &window)]))
                .or(label),
            Happening::Rescheduled => {
                window.map(|window| i18n.tf("notice.rescheduled", &[("window", &window)]))
            }
            Happening::Started => match (label, event.planned_end) {
                (Some(label), Some(end)) => Some(format!("{label}{SEPARATOR}{}", ends(end, i18n))),
                (label, _) => label,
            },
            Happening::Closed => match (label, event.duration()) {
                (Some(label), Some(parts)) => Some(i18n.tf(
                    "notice.after",
                    &[
                        ("state", &label),
                        ("duration", &i18n.format_duration(&parts)),
                    ],
                )),
                (label, _) => label,
            },
            Happening::Updated if label.is_none() => Some(i18n.t("notice.update").to_string()),
            _ => label,
        }
    }

    /// The message posted with the change, or what the event says when it
    /// begins.
    fn text(&self) -> Option<String> {
        if let Some(html) = self.update {
            return plain_text(html);
        }
        matches!(
            self.happening,
            Happening::Opened | Happening::Published | Happening::Started
        )
        .then(|| plain_text(&sanitize_markdown(&self.event.description)))
        .flatten()
    }
}

fn service_notice(
    happening: Happening,
    name: &str,
    down_since: Option<DateTime<Utc>>,
    at: DateTime<Utc>,
    origin: &Origin,
    i18n: &I18n,
) -> Notice {
    let (mark, key, summary) = if happening == Happening::ServiceUp {
        let summary = down_since.filter(|since| at > *since).map(|since| {
            let duration = i18n.format_duration(&split_duration(at - since));
            i18n.tf("notice.down_for", &[("duration", &duration)])
        });
        (Mark::Restored, "notice.service_up", summary)
    } else {
        let summary = down_since.map_or_else(
            || i18n.t("notice.detected").to_string(),
            |since| {
                i18n.tf(
                    "notice.detected_since",
                    &[
                        ("time", &i18n.format_time(&since)),
                        ("zone", &clock::offset_label(&since)),
                    ],
                )
            },
        );
        (Mark::Outage, "notice.service_down", Some(summary))
    };
    Notice {
        mark,
        headline: i18n.tf(key, &[("service", name)]),
        summary,
        text: None,
        link: page_link(origin, i18n),
        instance: origin.instance.clone(),
    }
}

fn page_link(origin: &Origin, i18n: &I18n) -> Option<Link> {
    origin.page.as_ref().map(|page| Link {
        url: format!("{page}/"),
        label: i18n.t("notice.open_page").to_string(),
    })
}

/// When a maintenance runs, in the instance zone: one day, two days, or a
/// start alone.
fn window(start: DateTime<Utc>, end: Option<DateTime<Utc>>, i18n: &I18n) -> String {
    let day = |at: &DateTime<Utc>| i18n.format_dateline(&clock::local_date(at));
    let start_time = i18n.format_time(&start);
    let zone = clock::offset_label(&start);
    match end {
        Some(end) if clock::local_date(&end) == clock::local_date(&start) => i18n.tf(
            "notice.window_day",
            &[
                ("day", &day(&start)),
                ("start", &start_time),
                ("end", &i18n.format_time(&end)),
                ("zone", &zone),
            ],
        ),
        Some(end) => i18n.tf(
            "notice.window_days",
            &[
                ("day", &day(&start)),
                ("start", &start_time),
                ("end_day", &day(&end)),
                ("end", &i18n.format_time(&end)),
                ("zone", &zone),
            ],
        ),
        None => i18n.tf(
            "notice.window_from",
            &[
                ("day", &day(&start)),
                ("start", &start_time),
                ("zone", &zone),
            ],
        ),
    }
}

/// When a maintenance under way is due to end.
fn ends(end: DateTime<Utc>, i18n: &I18n) -> String {
    i18n.tf(
        "notice.ends",
        &[
            ("day", &i18n.format_dateline(&clock::local_date(&end))),
            ("time", &i18n.format_time(&end)),
            ("zone", &clock::offset_label(&end)),
        ],
    )
}

/// Sanitized HTML as plain text, cut to a length every tool takes.
fn plain_text(html: &str) -> Option<String> {
    let text = html_to_text(html);
    (!text.is_empty()).then(|| excerpt(&text, MAX_TEXT_CHARS))
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};

    use super::*;
    use crate::models::{Category, Severity};

    fn at(minute: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 7, 9, 0, 0).unwrap() + Duration::minutes(minute)
    }

    fn event(kind: Kind, severity: Option<Severity>, lifecycle: Option<Lifecycle>) -> Event {
        Event {
            id: 7,
            kind,
            severity,
            planned: false,
            lifecycle,
            category: None,
            title: "Coupure du réseau au siège".to_string(),
            description: String::new(),
            planned_start: None,
            planned_end: None,
            started_at: Some(at(0)),
            ended_at: None,
            restored_at: None,
            author_id: 1,
            previous_lifecycle: None,
            follows_event_id: None,
            keeps_services_up: false,
            created_at: at(0),
            updated_at: at(0),
        }
    }

    fn origin() -> Origin {
        Origin {
            instance: "ACME".to_string(),
            page: Some("https://status.example".to_string()),
        }
    }

    fn about(event: Event, update: Option<&str>) -> Subject {
        Subject::Event {
            event,
            services: vec!["Réseau".to_string(), "VPN".to_string()],
            update: update.map(ToString::to_string),
        }
    }

    fn write(happening: Happening, subject: &Subject, locale: &str) -> Notice {
        Notice::write(
            happening,
            None,
            subject,
            &origin(),
            &I18n::new(locale),
            at(0),
        )
    }

    /// Facts joined as the messages join them.
    fn line(parts: &[&str]) -> String {
        parts.join(SEPARATOR)
    }

    #[test]
    fn an_incident_opens_with_its_severity_its_state_and_what_it_says() {
        let mut incident = event(
            Kind::Incident,
            Some(Severity::Critical),
            Some(Lifecycle::Investigating),
        );
        incident.description = "Le **prestataire** intervient.".to_string();

        let notice = write(Happening::Opened, &about(incident, None), "fr");

        assert_eq!(notice.mark, Mark::Outage);
        assert_eq!(
            notice.headline,
            line(&["Incident majeur", "Coupure du réseau au siège"])
        );
        assert_eq!(
            notice.summary.as_deref(),
            Some(line(&["En analyse", "Réseau, VPN"]).as_str())
        );
        assert_eq!(notice.text.as_deref(), Some("Le prestataire intervient."));
        let link = notice.link.unwrap();
        assert_eq!(link.url, "https://status.example/events/7");
        assert_eq!(link.label, "Voir le détail");
        assert_eq!(notice.instance, "ACME");
    }

    #[test]
    fn an_update_tells_the_message_posted_in_the_state_it_was_posted() {
        let incident = event(
            Kind::Incident,
            Some(Severity::Minor),
            Some(Lifecycle::Resolved),
        );
        let subject = about(incident, Some("<p>Le câble est <em>remplacé</em>.</p>"));

        let notice = Notice::write(
            Happening::Updated,
            Some(Lifecycle::InProgress),
            &subject,
            &origin(),
            &I18n::new("fr"),
            at(0),
        );

        assert_eq!(notice.mark, Mark::Degraded);
        assert_eq!(
            notice.summary.as_deref(),
            Some(line(&["En correction", "Réseau, VPN"]).as_str())
        );
        assert_eq!(notice.text.as_deref(), Some("Le câble est remplacé."));
    }

    #[test]
    fn a_resolved_incident_says_how_long_it_lasted() {
        let mut incident = event(
            Kind::Incident,
            Some(Severity::Critical),
            Some(Lifecycle::Resolved),
        );
        incident.ended_at = Some(at(72));

        let notice = write(Happening::Closed, &about(incident, None), "fr");

        let lasted = format!(
            "Résolu après {}",
            I18n::new("fr").format_duration(&(0, 1, 12))
        );
        assert_eq!(notice.mark, Mark::Restored);
        assert_eq!(
            notice.summary.as_deref(),
            Some(line(&[&lasted, "Réseau, VPN"]).as_str())
        );
        assert_eq!(notice.text, None);
    }

    #[test]
    fn an_announced_maintenance_gives_its_window_in_the_instance_zone() {
        let mut maintenance = event(Kind::Maintenance, None, Some(Lifecycle::Scheduled));
        maintenance.planned = true;
        maintenance.planned_start = Some(at(60 * 13));
        maintenance.planned_end = Some(at(60 * 14));
        let i18n = I18n::new("fr");
        let (start, end) = (at(60 * 13), at(60 * 14));

        let notice = write(Happening::Opened, &about(maintenance, None), "fr");

        let expected = format!(
            "Programmée le {} de {} à {} ({})",
            i18n.format_dateline(&clock::local_date(&start)),
            i18n.format_time(&start),
            i18n.format_time(&end),
            clock::offset_label(&start),
        );
        assert_eq!(notice.mark, Mark::Maintenance);
        assert_eq!(
            notice.summary.as_deref(),
            Some(line(&[&expected, "Réseau, VPN"]).as_str())
        );
    }

    #[test]
    fn a_window_over_two_days_names_both() {
        let mut maintenance = event(Kind::Maintenance, None, Some(Lifecycle::Scheduled));
        maintenance.planned_start = Some(at(60 * 13));
        maintenance.planned_end = Some(at(60 * 40));
        let i18n = I18n::new("en");
        let (start, end) = (at(60 * 13), at(60 * 40));

        let notice = write(Happening::Rescheduled, &about(maintenance, None), "en");

        let expected = format!(
            "Moved to {}, {} to {}, {} ({})",
            i18n.format_dateline(&clock::local_date(&start)),
            i18n.format_time(&start),
            i18n.format_dateline(&clock::local_date(&end)),
            i18n.format_time(&end),
            clock::offset_label(&start),
        );
        assert_eq!(
            notice.summary.as_deref(),
            Some(line(&[&expected, "Réseau, VPN"]).as_str())
        );
    }

    #[test]
    fn a_maintenance_under_way_says_when_it_ends() {
        let mut maintenance = event(Kind::Maintenance, None, Some(Lifecycle::InProgress));
        maintenance.planned_end = Some(at(90));
        let i18n = I18n::new("fr");

        let notice = write(Happening::Started, &about(maintenance, None), "fr");

        let ends = format!(
            "fin prévue le {} à {} ({})",
            i18n.format_dateline(&clock::local_date(&at(90))),
            i18n.format_time(&at(90)),
            clock::offset_label(&at(90)),
        );
        assert_eq!(
            notice.summary.as_deref(),
            Some(line(&["En cours", &ends, "Réseau, VPN"]).as_str())
        );
    }

    #[test]
    fn an_announcement_reads_as_its_category_without_a_state() {
        let mut announcement = event(Kind::Publication, None, None);
        announcement.category = Some(Category::Changelog);
        announcement.title = "Nouvel intranet".to_string();
        announcement.description = "Il ouvre lundi.".to_string();
        let subject = Subject::Event {
            event: announcement,
            services: Vec::new(),
            update: None,
        };

        let published = write(Happening::Published, &subject, "fr");
        let updated = write(Happening::Updated, &subject, "fr");

        assert_eq!(published.mark, Mark::Plain);
        assert_eq!(published.headline, line(&["Nouveauté", "Nouvel intranet"]));
        assert_eq!(published.summary, None);
        assert_eq!(published.text.as_deref(), Some("Il ouvre lundi."));
        assert_eq!(updated.summary.as_deref(), Some("Mise à jour"));
        assert_eq!(updated.text, None);
    }

    #[test]
    fn a_long_message_is_cut_and_the_link_leads_to_the_rest() {
        let incident = event(
            Kind::Incident,
            Some(Severity::Minor),
            Some(Lifecycle::Investigating),
        );
        let long = format!("<p>{}</p>", "a".repeat(700));

        let notice = write(Happening::Updated, &about(incident, Some(&long)), "fr");

        let text = notice.text.unwrap();
        assert_eq!(text.chars().count(), MAX_TEXT_CHARS + 1);
        assert!(text.ends_with('…'));
    }

    #[test]
    fn without_the_page_address_a_message_has_no_link() {
        let origin = Origin {
            instance: "ACME".to_string(),
            page: None,
        };

        let notice = Notice::write(
            Happening::Test,
            None,
            &Subject::Test,
            &origin,
            &I18n::new("fr"),
            at(0),
        );

        assert_eq!(notice.link, None);
        assert_eq!(notice.headline, "Message d'essai");
        assert_eq!(
            notice.text.as_deref(),
            Some("Les nouvelles de ACME arriveront ici.")
        );
    }

    #[test]
    fn a_service_found_down_then_back() {
        let since = at(0);
        let i18n = I18n::new("fr");
        let down = Subject::Service {
            name: "Messagerie".to_string(),
            down_since: Some(since),
        };

        let lost = Notice::write(Happening::ServiceDown, None, &down, &origin(), &i18n, at(3));
        let back = Notice::write(Happening::ServiceUp, None, &down, &origin(), &i18n, at(18));

        assert_eq!(lost.mark, Mark::Outage);
        assert_eq!(lost.headline, "Messagerie ne répond plus");
        let detected = format!(
            "Détecté automatiquement, sans réponse depuis {} ({})",
            i18n.format_time(&since),
            clock::offset_label(&since),
        );
        assert_eq!(lost.summary.as_deref(), Some(detected.as_str()));
        assert_eq!(lost.link.unwrap().url, "https://status.example/");
        assert_eq!(back.mark, Mark::Restored);
        assert_eq!(back.headline, "Messagerie répond de nouveau");
        let lasted = format!("Panne de {}", i18n.format_duration(&(0, 0, 18)));
        assert_eq!(back.summary.as_deref(), Some(lasted.as_str()));
    }

    #[test]
    fn the_message_follows_the_destination_language() {
        let incident = event(
            Kind::Incident,
            Some(Severity::Critical),
            Some(Lifecycle::Investigating),
        );

        let notice = write(Happening::Opened, &about(incident, None), "en");

        assert_eq!(
            notice.headline,
            line(&["Major incident", "Coupure du réseau au siège"])
        );
        assert_eq!(
            notice.summary.as_deref(),
            Some(line(&["Investigating", "Réseau, VPN"]).as_str())
        );
        assert_eq!(notice.link.unwrap().label, "See the details");
    }
}
