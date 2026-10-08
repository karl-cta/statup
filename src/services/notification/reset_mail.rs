//! The email that carries a password reset link to the member who asked.

use askama::Template;
use lettre::Message;
use lettre::message::{Mailbox, MultiPart};

use super::mail::own_message_id;
use crate::i18n::I18n;

/// What the reset email says, in the member's language.
pub struct ResetEmail<'a> {
    pub to: &'a str,
    /// The full address of the reset page, token included.
    pub url: &'a str,
    pub instance: &'a str,
    pub i18n: &'a I18n,
}

/// The words of the email, shared by its plain text and HTML versions.
struct Words {
    subject: String,
    text: String,
    ignore: String,
    action: String,
}

impl Words {
    fn of(mail: &ResetEmail<'_>) -> Self {
        let i18n = mail.i18n;
        Self {
            subject: i18n.tf("reset_mail.subject", &[("instance", mail.instance)]),
            text: i18n.tf("reset_mail.text", &[("email", mail.to)]),
            ignore: i18n.t("reset_mail.ignore").to_string(),
            action: i18n.t("reset_mail.action").to_string(),
        }
    }
}

#[derive(Template)]
#[template(path = "emails/password_reset.html")]
struct ResetHtml<'a> {
    lang: &'a str,
    words: &'a Words,
    url: &'a str,
    instance: &'a str,
}

/// The email, or `None` when the recipient's address cannot be written in
/// a message.
pub fn reset_email(from: &Mailbox, mail: &ResetEmail<'_>) -> Option<Message> {
    let to: Mailbox = mail.to.parse().ok()?;
    let words = Words::of(mail);
    Message::builder()
        .from(from.clone())
        .to(to)
        .subject(words.subject.clone())
        .message_id(Some(own_message_id(from)))
        .multipart(MultiPart::alternative_plain_html(
            plain_text(&words, mail),
            html(&words, mail),
        ))
        .ok()
}

fn plain_text(words: &Words, mail: &ResetEmail<'_>) -> String {
    [
        words.text.as_str(),
        "",
        words.action.as_str(),
        mail.url,
        "",
        words.ignore.as_str(),
        "",
        mail.instance,
    ]
    .join("\n")
}

/// The plain text alone is sent if the HTML version ever fails to render.
fn html(words: &Words, mail: &ResetEmail<'_>) -> String {
    ResetHtml {
        lang: mail.i18n.locale(),
        words,
        url: mail.url,
        instance: mail.instance,
    }
    .render()
    .unwrap_or_else(|_| plain_text(words, mail))
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://status.example/password/reset?token=7.abc";

    fn written(to: &str, locale: &str) -> Option<String> {
        let i18n = I18n::new(locale);
        let mail = ResetEmail {
            to,
            url: URL,
            instance: "ACME <IT>",
            i18n: &i18n,
        };
        let from: Mailbox = "Statup <status@example.com>".parse().unwrap();
        reset_email(&from, &mail).map(|message| String::from_utf8(message.formatted()).unwrap())
    }

    #[test]
    fn the_email_carries_the_link_in_both_versions() {
        let i18n = I18n::new("en");
        let mail = ResetEmail {
            to: "ana@example.com",
            url: URL,
            instance: "ACME <IT>",
            i18n: &i18n,
        };
        let words = Words::of(&mail);

        assert!(plain_text(&words, &mail).contains(URL));
        let page = html(&words, &mail);
        assert!(page.contains(&format!("href=\"{}\"", URL.replace('&', "&amp;"))));
        assert!(page.contains("lang=\"en\""));
        assert!(!page.contains("ACME <IT>"), "the instance name is escaped");
    }

    #[test]
    fn the_email_goes_to_the_member_from_the_sender() {
        let text = written("ana@example.com", "fr").unwrap();

        assert!(text.contains("To: ana@example.com"));
        assert!(text.contains("From: Statup <status@example.com>"));
        assert!(text.contains("Message-ID: <statup."));
    }

    #[test]
    fn a_member_reads_it_in_their_language() {
        let en = I18n::new("en");
        let fr = I18n::new("fr");
        let subject = |i18n: &I18n| i18n.tf("reset_mail.subject", &[("instance", "ACME")]);

        assert_ne!(subject(&en), subject(&fr));
        assert!(subject(&en).contains("ACME"));
    }

    #[test]
    fn an_address_that_cannot_be_written_sends_nothing() {
        assert!(written("not an address", "en").is_none());
    }
}
