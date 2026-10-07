//! What a destination may be: its name and the address it posts to.

use crate::models::ChannelKind;

/// Long enough for a channel and its team, short enough for one line of the
/// list.
const MAX_NAME_CHARS: usize = 60;
const MAX_TARGET_CHARS: usize = 2000;

/// Why a name is refused, as a message key.
pub fn destination_name_refusal(name: &str) -> Option<&'static str> {
    let name = name.trim();
    if name.is_empty() {
        return Some("notifications.name_required");
    }
    (name.chars().count() > MAX_NAME_CHARS).then_some("notifications.name_too_long")
}

/// The one gate for what a destination may reach, as a message key when
/// refused. A hosted instance will add its own limits here, such as private
/// addresses, and the sending stays as it is.
pub fn destination_allowed(kind: ChannelKind, target: &str) -> Result<(), &'static str> {
    if kind == ChannelKind::Email {
        return Err("notifications.email_unavailable");
    }
    if target.is_empty() {
        return Err("notifications.target_required");
    }
    let Ok(url) = reqwest::Url::parse(target) else {
        return Err("notifications.target_invalid");
    };
    if target.chars().count() > MAX_TARGET_CHARS
        || !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
    {
        return Err("notifications.target_invalid");
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("notifications.target_credentials");
    }
    if kind.needs_https() && url.scheme() != "https" {
        return Err("notifications.target_https");
    }
    if kind == ChannelKind::Ntfy && !has_topic(&url) {
        return Err("notifications.target_topic");
    }
    Ok(())
}

/// An ntfy address ends with the topic: `https://ntfy.sh/statup-acme`.
fn has_topic(url: &reqwest::Url) -> bool {
    url.path_segments()
        .is_some_and(|mut segments| segments.any(|segment| !segment.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_needed_and_kept_short() {
        assert_eq!(
            destination_name_refusal("  "),
            Some("notifications.name_required")
        );
        assert_eq!(
            destination_name_refusal(&"é".repeat(61)),
            Some("notifications.name_too_long")
        );
        assert_eq!(destination_name_refusal("Canal #informatique"), None);
    }

    #[test]
    fn a_hosted_tool_takes_https_only() {
        let teams = "https://prod-00.westeurope.logic.azure.com/workflows/abc";
        assert_eq!(destination_allowed(ChannelKind::Teams, teams), Ok(()));
        assert_eq!(
            destination_allowed(ChannelKind::Slack, "http://hooks.slack.com/services/x"),
            Err("notifications.target_https")
        );
        assert_eq!(
            destination_allowed(ChannelKind::Mattermost, "http://chat.local/hooks/x"),
            Ok(())
        );
        assert_eq!(
            destination_allowed(ChannelKind::Webhook, "http://n8n.local:5678/webhook/x"),
            Ok(())
        );
    }

    #[test]
    fn an_address_must_be_a_web_address_without_credentials() {
        let refused = |target: &str| destination_allowed(ChannelKind::Webhook, target);
        assert_eq!(refused(""), Err("notifications.target_required"));
        assert_eq!(
            refused("hooks.slack.com"),
            Err("notifications.target_invalid")
        );
        assert_eq!(
            refused("ftp://files.local/x"),
            Err("notifications.target_invalid")
        );
        assert_eq!(
            refused(&format!("https://example.com/{}", "a".repeat(2000))),
            Err("notifications.target_invalid")
        );
        assert_eq!(
            refused("https://user:secret@example.com/hook"),
            Err("notifications.target_credentials")
        );
    }

    #[test]
    fn an_ntfy_address_names_its_topic() {
        assert_eq!(
            destination_allowed(ChannelKind::Ntfy, "https://ntfy.sh/"),
            Err("notifications.target_topic")
        );
        assert_eq!(
            destination_allowed(ChannelKind::Ntfy, "https://ntfy.sh/statup-acme"),
            Ok(())
        );
    }

    #[test]
    fn email_waits_for_a_mail_server() {
        assert_eq!(
            destination_allowed(ChannelKind::Email, "it@example.com"),
            Err("notifications.email_unavailable")
        );
    }
}
