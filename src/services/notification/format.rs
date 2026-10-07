//! A notice in the format of each tool: the JSON a destination takes.

use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use serde_json::{Value, json};

use super::{Mark, Notice, Origin, Subject};
use crate::models::{Category, ChannelKind, Happening, Lifecycle, Severity, excerpt};

/// Longest title of a Discord embed, in characters.
const DISCORD_TITLE_CHARS: usize = 250;

/// Keeps a mention from firing while leaving it readable.
const ZERO_WIDTH_SPACE: char = '\u{200b}';

/// A request for a chat or webhook destination: where to post, and the JSON body.
#[derive(Debug, Clone, PartialEq)]
pub struct Post {
    pub url: String,
    pub body: Value,
}

/// The facts behind a message, for tools that read data rather than text.
/// Version 1 is a public contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Facts {
    pub version: u8,
    pub happening: &'static str,
    pub occurred_at: String,
    pub instance: String,
    pub event: Option<EventFacts>,
    pub service: Option<ServiceFacts>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EventFacts {
    pub id: i64,
    pub kind: &'static str,
    pub severity: Option<&'static str>,
    pub category: Option<&'static str>,
    pub state: Option<&'static str>,
    pub title: String,
    pub services: Vec<String>,
    pub planned_start: Option<String>,
    pub planned_end: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServiceFacts {
    pub name: String,
    pub down_since: Option<String>,
}

fn rfc3339(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

impl Facts {
    pub fn of(
        happening: Happening,
        lifecycle: Option<Lifecycle>,
        subject: &Subject,
        origin: &Origin,
        at: DateTime<Utc>,
    ) -> Self {
        let mut facts = Self {
            version: 1,
            happening: happening.as_str(),
            occurred_at: rfc3339(at),
            instance: origin.instance.clone(),
            event: None,
            service: None,
        };
        match subject {
            Subject::Event {
                event, services, ..
            } => {
                facts.event = Some(EventFacts {
                    id: event.id,
                    kind: event.kind.as_str(),
                    severity: event.severity.map(Severity::as_str),
                    category: event.category.map(Category::as_str),
                    state: lifecycle.or(event.lifecycle).map(Lifecycle::as_str),
                    title: event.title.clone(),
                    services: services.clone(),
                    planned_start: event.planned_start.map(rfc3339),
                    planned_end: event.planned_end.map(rfc3339),
                    url: origin
                        .page
                        .as_ref()
                        .map(|page| format!("{page}/events/{}", event.id)),
                });
            }
            Subject::Service { name, down_since } => {
                facts.service = Some(ServiceFacts {
                    name: name.clone(),
                    down_since: down_since.map(rfc3339),
                });
            }
            Subject::Test => {}
        }
        facts
    }
}

/// `None` for an email destination, sent another way, and for an ntfy address
/// without a topic.
pub fn post(kind: ChannelKind, target: &str, notice: &Notice, facts: &Facts) -> Option<Post> {
    let (url, body) = match kind {
        ChannelKind::Slack => (target.to_string(), slack_body(notice)),
        ChannelKind::GoogleChat => (target.to_string(), json!({ "text": slack_text(notice) })),
        ChannelKind::Mattermost => (target.to_string(), mattermost_body(notice)),
        ChannelKind::Discord => (target.to_string(), discord_body(notice)),
        ChannelKind::Teams => (target.to_string(), teams_body(notice)),
        ChannelKind::Ntfy => return ntfy_post(target, notice),
        ChannelKind::Webhook => (target.to_string(), webhook_body(notice, facts)),
        ChannelKind::Email => return None,
    };
    Some(Post { url, body })
}

fn mark_emoji(mark: Mark) -> Option<&'static str> {
    match mark {
        Mark::Outage => Some("🟥"),
        Mark::Degraded => Some("🟧"),
        Mark::Maintenance => Some("🟦"),
        Mark::Restored => Some("🟩"),
        Mark::Plain => None,
    }
}

fn mark_name(mark: Mark) -> &'static str {
    match mark {
        Mark::Outage => "outage",
        Mark::Degraded => "degraded",
        Mark::Maintenance => "maintenance",
        Mark::Restored => "restored",
        Mark::Plain => "plain",
    }
}

/// `headline` behind the mark's square, with no gap left when there is none.
fn marked(notice: &Notice, headline: &str) -> String {
    match mark_emoji(notice.mark) {
        Some(emoji) => format!("{emoji} {headline}"),
        None => headline.to_string(),
    }
}

fn join_lines<const N: usize>(parts: [Option<String>; N], separator: &str) -> String {
    parts
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(separator)
}

fn escape_slack(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Slack's markup, which Google Chat reads too.
fn slack_text(notice: &Notice) -> String {
    let headline = marked(notice, &format!("*{}*", escape_slack(&notice.headline)));
    let link = notice
        .link
        .as_ref()
        .map(|link| format!("<{}|{}>", link.url, escape_slack(&link.label)));
    join_lines(
        [
            Some(headline),
            notice.summary.as_deref().map(escape_slack),
            notice.text.as_deref().map(escape_slack),
            link,
        ],
        "\n",
    )
}

fn slack_body(notice: &Notice) -> Value {
    json!({
        "text": slack_text(notice),
        "unfurl_links": false,
        "unfurl_media": false,
    })
}

fn defuse_mentions(text: &str) -> String {
    text.replace('@', &format!("@{ZERO_WIDTH_SPACE}"))
        .replace("<!", &format!("<{ZERO_WIDTH_SPACE}!"))
}

fn mattermost_body(notice: &Notice) -> Value {
    let headline = marked(
        notice,
        &format!("**{}**", defuse_mentions(&notice.headline)),
    );
    let link = notice
        .link
        .as_ref()
        .map(|link| format!("[{}]({})", defuse_mentions(&link.label), link.url));
    let text = join_lines(
        [
            Some(headline),
            notice.summary.as_deref().map(defuse_mentions),
            notice.text.as_deref().map(defuse_mentions),
            link,
        ],
        "\n",
    );
    json!({ "text": text })
}

fn discord_color(mark: Mark) -> Option<u32> {
    match mark {
        Mark::Outage => Some(0x00D0_3833),
        Mark::Degraded => Some(0x00D2_B373),
        Mark::Maintenance => Some(0x0050_7FA7),
        Mark::Restored => Some(0x008B_AE66),
        Mark::Plain => None,
    }
}

fn discord_body(notice: &Notice) -> Value {
    let mut embed = json!({
        "title": excerpt(&marked(notice, &notice.headline), DISCORD_TITLE_CHARS),
        "footer": { "text": notice.instance },
    });
    let description = join_lines([notice.summary.clone(), notice.text.clone()], "\n\n");
    if !description.is_empty() {
        embed["description"] = json!(description);
    }
    if let Some(link) = &notice.link {
        embed["url"] = json!(link.url);
    }
    if let Some(color) = discord_color(notice.mark) {
        embed["color"] = json!(color);
    }
    json!({ "embeds": [embed], "allowed_mentions": { "parse": [] } })
}

fn teams_blocks(notice: &Notice) -> Vec<Value> {
    let mut blocks = vec![json!({
        "type": "TextBlock",
        "text": marked(notice, &notice.headline),
        "weight": "Bolder",
        "size": "Medium",
        "wrap": true,
    })];
    if let Some(summary) = &notice.summary {
        blocks.push(json!({
            "type": "TextBlock",
            "text": summary,
            "isSubtle": true,
            "spacing": "None",
            "wrap": true,
        }));
    }
    if let Some(text) = &notice.text {
        blocks.push(json!({ "type": "TextBlock", "text": text, "wrap": true }));
    }
    blocks.push(json!({
        "type": "TextBlock",
        "text": notice.instance,
        "isSubtle": true,
        "size": "Small",
        "wrap": true,
    }));
    blocks
}

/// A card for a Workflows webhook.
fn teams_body(notice: &Notice) -> Value {
    let mut card = json!({
        "$schema": "http://adaptivecards.io/schemas/adaptive-card.json",
        "type": "AdaptiveCard",
        "version": "1.4",
        "msteams": { "width": "Full" },
        "body": teams_blocks(notice),
    });
    if let Some(link) = &notice.link {
        card["actions"] = json!([
            { "type": "Action.OpenUrl", "title": link.label, "url": link.url }
        ]);
    }
    json!({
        "type": "message",
        "attachments": [{
            "contentType": "application/vnd.microsoft.card.adaptive",
            "contentUrl": null,
            "content": card,
        }],
    })
}

/// Posts to the server root with the topic in the body, so that the address a
/// person pastes is the topic's, as ntfy shows it.
fn ntfy_post(target: &str, notice: &Notice) -> Option<Post> {
    let mut url = reqwest::Url::parse(target).ok()?;
    let path = url.path().trim_end_matches('/').to_string();
    let (prefix, topic) = path.rsplit_once('/')?;
    if topic.is_empty() {
        return None;
    }
    url.set_path(&format!("{prefix}/"));
    let message = join_lines([notice.summary.clone(), notice.text.clone()], "\n");
    let mut body = json!({
        "topic": topic,
        "title": marked(notice, &notice.headline),
        "message": if message.is_empty() { notice.headline.clone() } else { message },
    });
    if let Some(link) = &notice.link {
        body["click"] = json!(link.url);
    }
    if notice.mark == Mark::Outage {
        body["priority"] = json!(4);
    }
    Some(Post {
        url: url.to_string(),
        body,
    })
}

#[derive(Serialize)]
struct WebhookBody<'a> {
    #[serde(flatten)]
    facts: &'a Facts,
    message: WebhookMessage<'a>,
}

#[derive(Serialize)]
struct WebhookMessage<'a> {
    mark: &'static str,
    headline: &'a str,
    summary: Option<&'a str>,
    text: Option<&'a str>,
    url: Option<&'a str>,
}

fn webhook_body(notice: &Notice, facts: &Facts) -> Value {
    let body = WebhookBody {
        facts,
        message: WebhookMessage {
            mark: mark_name(notice.mark),
            headline: &notice.headline,
            summary: notice.summary.as_deref(),
            text: notice.text.as_deref(),
            url: notice.link.as_ref().map(|link| link.url.as_str()),
        },
    };
    // Plain strings and numbers always serialize, so the fallback never shows.
    serde_json::to_value(body).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;
    use crate::models::{Event, Kind};
    use crate::services::notification::Link;

    fn fixture() -> Notice {
        Notice {
            mark: Mark::Outage,
            headline: "Incident majeur\u{a0}· Coupure <réseau> & co".to_string(),
            summary: Some("En analyse\u{a0}· Réseau, VPN".to_string()),
            text: Some("Ping @channel et <!here> pour @all".to_string()),
            link: Some(Link {
                url: "https://status.example/events/7".to_string(),
                label: "Voir le détail".to_string(),
            }),
            instance: "ACME".to_string(),
        }
    }

    fn bare() -> Notice {
        Notice {
            mark: Mark::Plain,
            headline: "Nouveauté".to_string(),
            summary: None,
            text: None,
            link: None,
            instance: "ACME".to_string(),
        }
    }

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 7, 9, 0, 0).unwrap()
    }

    fn origin() -> Origin {
        Origin {
            instance: "ACME".to_string(),
            page: Some("https://status.example".to_string()),
        }
    }

    fn event() -> Event {
        Event {
            id: 7,
            kind: Kind::Incident,
            severity: Some(Severity::Critical),
            planned: false,
            lifecycle: Some(Lifecycle::Investigating),
            category: None,
            title: "Coupure du réseau".to_string(),
            description: String::new(),
            planned_start: Some(at()),
            planned_end: None,
            started_at: Some(at()),
            ended_at: None,
            restored_at: None,
            author_id: 1,
            previous_lifecycle: None,
            follows_event_id: None,
            keeps_services_up: false,
            created_at: at(),
            updated_at: at(),
        }
    }

    fn event_subject() -> Subject {
        Subject::Event {
            event: event(),
            services: vec!["Réseau".to_string(), "VPN".to_string()],
            update: None,
        }
    }

    fn facts() -> Facts {
        Facts::of(Happening::Opened, None, &event_subject(), &origin(), at())
    }

    fn post_for(kind: ChannelKind, target: &str, notice: &Notice) -> Option<Post> {
        post(kind, target, notice, &facts())
    }

    const SLACK_TEXT: &str = "🟥 *Incident majeur\u{a0}· Coupure &lt;réseau&gt; &amp; co*\nEn analyse\u{a0}· Réseau, VPN\nPing @channel et &lt;!here&gt; pour @all\n<https://status.example/events/7|Voir le détail>";

    #[test]
    fn slack_escapes_the_text_and_turns_link_previews_off() {
        let posted = post_for(ChannelKind::Slack, "https://hooks.example/s", &fixture());

        assert_eq!(
            posted,
            Some(Post {
                url: "https://hooks.example/s".to_string(),
                body: json!({
                    "text": SLACK_TEXT,
                    "unfurl_links": false,
                    "unfurl_media": false,
                }),
            })
        );
    }

    #[test]
    fn google_chat_takes_the_slack_text_alone() {
        let posted = post_for(
            ChannelKind::GoogleChat,
            "https://chat.example/g",
            &fixture(),
        );

        assert_eq!(
            posted,
            Some(Post {
                url: "https://chat.example/g".to_string(),
                body: json!({ "text": SLACK_TEXT }),
            })
        );
    }

    #[test]
    fn mattermost_keeps_markdown_and_defuses_mentions() {
        let posted = post_for(ChannelKind::Mattermost, "https://mm.example/h", &fixture());

        assert_eq!(
            posted,
            Some(Post {
                url: "https://mm.example/h".to_string(),
                body: json!({
                    "text": "🟥 **Incident majeur\u{a0}· Coupure <réseau> & co**\nEn analyse\u{a0}· Réseau, VPN\nPing @\u{200b}channel et <\u{200b}!here> pour @\u{200b}all\n[Voir le détail](https://status.example/events/7)"
                }),
            })
        );
    }

    #[test]
    fn discord_sends_an_embed_that_mentions_nobody() {
        let posted = post_for(
            ChannelKind::Discord,
            "https://discord.example/w",
            &fixture(),
        );

        assert_eq!(
            posted,
            Some(Post {
                url: "https://discord.example/w".to_string(),
                body: json!({
                    "embeds": [{
                        "title": "🟥 Incident majeur\u{a0}· Coupure <réseau> & co",
                        "url": "https://status.example/events/7",
                        "description": "En analyse\u{a0}· Réseau, VPN\n\nPing @channel et <!here> pour @all",
                        "color": 13_645_875,
                        "footer": { "text": "ACME" },
                    }],
                    "allowed_mentions": { "parse": [] },
                }),
            })
        );
    }

    #[test]
    fn teams_sends_an_adaptive_card_with_a_button() {
        let posted = post_for(ChannelKind::Teams, "https://teams.example/w", &fixture());

        assert_eq!(
            posted,
            Some(Post {
                url: "https://teams.example/w".to_string(),
                body: json!({
                    "type": "message",
                    "attachments": [{
                        "contentType": "application/vnd.microsoft.card.adaptive",
                        "contentUrl": null,
                        "content": {
                            "$schema": "http://adaptivecards.io/schemas/adaptive-card.json",
                            "type": "AdaptiveCard",
                            "version": "1.4",
                            "msteams": { "width": "Full" },
                            "body": [
                                {
                                    "type": "TextBlock",
                                    "text": "🟥 Incident majeur\u{a0}· Coupure <réseau> & co",
                                    "weight": "Bolder",
                                    "size": "Medium",
                                    "wrap": true,
                                },
                                {
                                    "type": "TextBlock",
                                    "text": "En analyse\u{a0}· Réseau, VPN",
                                    "isSubtle": true,
                                    "spacing": "None",
                                    "wrap": true,
                                },
                                {
                                    "type": "TextBlock",
                                    "text": "Ping @channel et <!here> pour @all",
                                    "wrap": true,
                                },
                                {
                                    "type": "TextBlock",
                                    "text": "ACME",
                                    "isSubtle": true,
                                    "size": "Small",
                                    "wrap": true,
                                },
                            ],
                            "actions": [{
                                "type": "Action.OpenUrl",
                                "title": "Voir le détail",
                                "url": "https://status.example/events/7",
                            }],
                        },
                    }],
                }),
            })
        );
    }

    #[test]
    fn ntfy_posts_to_the_server_with_the_topic_in_the_body() {
        let posted = post_for(ChannelKind::Ntfy, "https://ntfy.sh/statup-acme", &fixture());

        assert_eq!(
            posted,
            Some(Post {
                url: "https://ntfy.sh/".to_string(),
                body: json!({
                    "topic": "statup-acme",
                    "title": "🟥 Incident majeur\u{a0}· Coupure <réseau> & co",
                    "message": "En analyse\u{a0}· Réseau, VPN\nPing @channel et <!here> pour @all",
                    "click": "https://status.example/events/7",
                    "priority": 4,
                }),
            })
        );
    }

    #[test]
    fn ntfy_splits_the_address_into_server_path_and_topic() {
        let trailing = post_for(ChannelKind::Ntfy, "https://ntfy.sh/alerts/", &bare());
        let prefixed = post_for(
            ChannelKind::Ntfy,
            "https://push.example.com/ntfy/alerts?auth=abc",
            &bare(),
        );

        let trailing = trailing.unwrap();
        assert_eq!(trailing.url, "https://ntfy.sh/");
        assert_eq!(trailing.body["topic"], "alerts");
        let prefixed = prefixed.unwrap();
        assert_eq!(prefixed.url, "https://push.example.com/ntfy/?auth=abc");
        assert_eq!(prefixed.body["topic"], "alerts");
    }

    #[test]
    fn ntfy_without_a_topic_or_an_address_posts_nothing() {
        assert_eq!(
            post_for(ChannelKind::Ntfy, "https://ntfy.sh/", &bare()),
            None
        );
        assert_eq!(
            post_for(ChannelKind::Ntfy, "https://ntfy.sh", &bare()),
            None
        );
        assert_eq!(post_for(ChannelKind::Ntfy, "not an address", &bare()), None);
    }

    #[test]
    fn the_webhook_carries_the_facts_and_the_message() {
        let posted = post_for(
            ChannelKind::Webhook,
            "https://hooks.example/any",
            &fixture(),
        );

        assert_eq!(
            posted,
            Some(Post {
                url: "https://hooks.example/any".to_string(),
                body: json!({
                    "version": 1,
                    "happening": "opened",
                    "occurred_at": "2026-10-07T09:00:00Z",
                    "instance": "ACME",
                    "event": {
                        "id": 7,
                        "kind": "incident",
                        "severity": "critical",
                        "category": null,
                        "state": "investigating",
                        "title": "Coupure du réseau",
                        "services": ["Réseau", "VPN"],
                        "planned_start": "2026-10-07T09:00:00Z",
                        "planned_end": null,
                        "url": "https://status.example/events/7",
                    },
                    "service": null,
                    "message": {
                        "mark": "outage",
                        "headline": "Incident majeur\u{a0}· Coupure <réseau> & co",
                        "summary": "En analyse\u{a0}· Réseau, VPN",
                        "text": "Ping @channel et <!here> pour @all",
                        "url": "https://status.example/events/7",
                    },
                }),
            })
        );
    }

    #[test]
    fn email_is_sent_another_way() {
        assert_eq!(
            post_for(ChannelKind::Email, "a@example.com", &fixture()),
            None
        );
    }

    #[test]
    fn a_plain_notice_has_no_square_and_no_optional_parts() {
        let slack = post_for(ChannelKind::Slack, "https://hooks.example/s", &bare()).unwrap();
        let discord = post_for(ChannelKind::Discord, "https://d.example/w", &bare()).unwrap();
        let teams = post_for(ChannelKind::Teams, "https://t.example/w", &bare()).unwrap();
        let ntfy = post_for(ChannelKind::Ntfy, "https://ntfy.sh/alerts", &bare()).unwrap();

        assert_eq!(slack.body["text"], "*Nouveauté*");
        assert_eq!(
            discord.body["embeds"][0],
            json!({ "title": "Nouveauté", "footer": { "text": "ACME" } })
        );
        let card = &teams.body["attachments"][0]["content"];
        assert_eq!(card["actions"], Value::Null);
        assert_eq!(card["body"].as_array().map(Vec::len), Some(2));
        assert_eq!(
            ntfy.body,
            json!({ "topic": "alerts", "title": "Nouveauté", "message": "Nouveauté" })
        );
    }

    #[test]
    fn facts_of_an_event_take_the_state_given_before_the_events_own() {
        let given = Facts::of(
            Happening::Updated,
            Some(Lifecycle::Resolved),
            &event_subject(),
            &origin(),
            at(),
        );

        assert_eq!(facts().event.unwrap().state, Some("investigating"));
        let event = given.event.unwrap();
        assert_eq!(given.happening, "updated");
        assert_eq!(event.state, Some("resolved"));
        assert_eq!(event.services, ["Réseau", "VPN"]);
        assert_eq!(event.planned_start.as_deref(), Some("2026-10-07T09:00:00Z"));
        assert_eq!(
            event.url.as_deref(),
            Some("https://status.example/events/7")
        );
        assert_eq!(given.service, None);
    }

    #[test]
    fn facts_of_an_event_have_no_url_without_the_page_address() {
        let origin = Origin {
            instance: "ACME".to_string(),
            page: None,
        };

        let facts = Facts::of(Happening::Opened, None, &event_subject(), &origin, at());

        assert_eq!(facts.event.unwrap().url, None);
    }

    #[test]
    fn facts_of_a_service_tell_since_when_it_is_down() {
        let subject = Subject::Service {
            name: "Messagerie".to_string(),
            down_since: Some(at()),
        };

        let facts = Facts::of(Happening::ServiceDown, None, &subject, &origin(), at());

        assert_eq!(facts.event, None);
        assert_eq!(
            facts.service,
            Some(ServiceFacts {
                name: "Messagerie".to_string(),
                down_since: Some("2026-10-07T09:00:00Z".to_string()),
            })
        );
    }

    #[test]
    fn facts_of_a_test_name_neither_event_nor_service() {
        let facts = Facts::of(Happening::Test, None, &Subject::Test, &origin(), at());

        assert_eq!(facts.happening, "test");
        assert_eq!(facts.version, 1);
        assert_eq!(facts.event, None);
        assert_eq!(facts.service, None);
    }
}
