//! A notice as an email: a subject, a plain text body with a short HTML one,
//! and the headers that keep the messages of one event in one conversation.

use askama::Template;
use lettre::Message;
use lettre::message::{Mailbox, MultiPart};

use super::format::marked;
use super::{Facts, Notice};
use crate::models::Happening;

/// The recipients of an email destination, as stored: addresses separated
/// by commas.
pub fn recipients(target: &str) -> Option<Vec<Mailbox>> {
    target
        .split(',')
        .map(str::trim)
        .filter(|address| !address.is_empty())
        .map(|address| address.parse().ok())
        .collect()
}

/// The email of a notice. The recipients are in blind copy, so none of them
/// sees the others; the sender stands in the `To` field.
pub fn email(
    from: &Mailbox,
    recipients: &[Mailbox],
    notice: &Notice,
    facts: &Facts,
    locale: &str,
) -> Result<Message, lettre::error::Error> {
    let mut builder = Message::builder()
        .from(from.clone())
        .to(from.clone())
        .subject(marked(notice, &notice.headline));
    for recipient in recipients {
        builder = builder.bcc(recipient.clone());
    }
    if let Some(event) = &facts.event {
        let thread = thread_id(event.id, from);
        builder = if starts_a_thread(facts.happening) {
            builder.message_id(Some(thread))
        } else {
            builder.in_reply_to(thread.clone()).references(thread)
        };
    }
    builder.multipart(MultiPart::alternative_plain_html(
        plain_text(notice),
        html(notice, locale),
    ))
}

/// The first message of an event names the conversation the next ones join.
fn starts_a_thread(happening: &str) -> bool {
    [Happening::Opened, Happening::Published]
        .iter()
        .any(|first| first.as_str() == happening)
}

fn thread_id(event_id: i64, from: &Mailbox) -> String {
    format!("<statup.event.{event_id}@{}>", from.email.domain())
}

fn plain_text(notice: &Notice) -> String {
    let mut lines = vec![marked(notice, &notice.headline)];
    lines.extend(notice.summary.clone());
    if let Some(text) = &notice.text {
        lines.extend([String::new(), text.clone()]);
    }
    if let Some(link) = &notice.link {
        lines.extend([String::new(), link.label.clone(), link.url.clone()]);
    }
    lines.extend([String::new(), notice.instance.clone()]);
    lines.join("\n")
}

#[derive(Template)]
#[template(path = "emails/notice.html")]
struct NoticeEmail<'a> {
    lang: &'a str,
    headline: String,
    notice: &'a Notice,
}

/// A short HTML version, for the clients that show it. Writing it cannot
/// fail with these fields; the plain text alone is sent if it ever does.
fn html(notice: &Notice, locale: &str) -> String {
    NoticeEmail {
        lang: locale,
        headline: marked(notice, &notice.headline),
        notice,
    }
    .render()
    .unwrap_or_else(|_| plain_text(notice))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::notification::{Link, Mark};

    fn notice() -> Notice {
        Notice {
            mark: Mark::Outage,
            headline: "Incident majeur\u{a0}· Coupure <réseau>".to_string(),
            summary: Some("En analyse\u{a0}· Réseau".to_string()),
            text: Some("Le prestataire intervient.".to_string()),
            link: Some(Link {
                url: "https://status.example/events/7".to_string(),
                label: "Voir le détail".to_string(),
            }),
            instance: "ACME".to_string(),
        }
    }

    fn facts(happening: &'static str, event_id: Option<i64>) -> Facts {
        Facts {
            version: 1,
            happening,
            occurred_at: "2026-10-07T09:00:00Z".to_string(),
            instance: "ACME".to_string(),
            event: event_id.map(|id| super::super::EventFacts {
                id,
                kind: "incident",
                severity: Some("critical"),
                category: None,
                state: Some("investigating"),
                title: "Coupure".to_string(),
                services: Vec::new(),
                planned_start: None,
                planned_end: None,
                url: None,
            }),
            service: None,
        }
    }

    fn sender() -> Mailbox {
        "Statup <status@example.com>".parse().unwrap()
    }

    fn written(happening: &'static str, event_id: Option<i64>) -> (Message, String) {
        let to = recipients("it@example.com, Board@Example.com").unwrap();
        let message = email(&sender(), &to, &notice(), &facts(happening, event_id), "fr").unwrap();
        let text = String::from_utf8(message.formatted()).unwrap();
        (message, text)
    }

    #[test]
    fn the_recipients_are_read_from_the_stored_list() {
        let parsed = recipients(" it@example.com ,board@example.com, ").unwrap();
        assert_eq!(parsed.len(), 2);
        assert!(recipients("it@example.com, not an address").is_none());
    }

    #[test]
    fn the_recipients_are_hidden_from_each_other() {
        let (message, text) = written("opened", Some(7));

        let envelope: Vec<String> = message
            .envelope()
            .to()
            .iter()
            .map(ToString::to_string)
            .collect();
        assert!(envelope.contains(&"it@example.com".to_string()));
        assert!(envelope.contains(&"Board@Example.com".to_string()));
        assert!(!text.contains("Bcc"));
        assert!(text.contains("To: Statup <status@example.com>"));
    }

    #[test]
    fn the_first_message_of_an_event_starts_its_conversation() {
        let (_, opened) = written("opened", Some(7));
        let (_, update) = written("updated", Some(7));
        let (_, test) = written("test", None);

        assert!(opened.contains("Message-ID: <statup.event.7@example.com>"));
        assert!(update.contains("In-Reply-To: <statup.event.7@example.com>"));
        assert!(update.contains("References: <statup.event.7@example.com>"));
        assert!(!test.contains("statup.event"));
    }

    #[test]
    fn the_body_reads_as_plain_text_and_as_escaped_html() {
        let plain = plain_text(&notice());
        assert_eq!(
            plain,
            "🟥 Incident majeur\u{a0}· Coupure <réseau>\nEn analyse\u{a0}· Réseau\n\n\
             Le prestataire intervient.\n\nVoir le détail\nhttps://status.example/events/7\n\nACME"
        );
        let page = html(&notice(), "fr");
        assert!(page.contains("lang=\"fr\""));
        assert!(
            page.contains("Coupure &#60;réseau&#62;") || page.contains("Coupure &lt;réseau&gt;")
        );
        assert!(page.contains("href=\"https://status.example/events/7\""));
    }
}
