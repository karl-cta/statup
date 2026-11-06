---
title: Alerts
description: "Post each incident, maintenance and announcement to Teams, Slack, Google Chat, Discord, Mattermost, ntfy, email or any other tool."
---

Statup posts each incident, maintenance and announcement where people already read. Administrators set the channels in **Settings**, **Alerts**.

In the app, **Notifications** is the bell in the header; **Alerts** are the messages sent to other tools.

## Add a channel

**New channel**, at the head of the **Channels** card, opens the form:

| Field | Description |
|---|---|
| **Type** | Microsoft Teams, Slack, Google Chat, Discord, Mattermost, ntfy (phone), Email or Webhook. |
| **Name in Statup** | Required, up to 60 characters: how the channel reads in Statup's lists, such as "My phone". Its example follows the type. |
| **Webhook URL** | Or **Topic URL** for ntfy, **Addresses** for email. Where to find it is below. |
| **Message language** | Français, English, Deutsch or Español. The instance's [default language](configuration.md#default_locale) at first. |
| **Receives** | **Incidents**, **Maintenance**, **Announcements**, all ticked at first, and **Outages detected automatically**, not ticked. |

**Send a test** posts a test message with the values of the form, without saving them, and says "Test message sent." or why it failed. A channel with nothing ticked under **Receives** is **Paused**.

To change a channel, open it by its name in the list. **Delete this channel** stops every message to it, waiting ones included.

## Where to find the URL

| Type | What to paste |
|---|---|
| Microsoft Teams | In the channel: ⋯, Workflows, then the template that posts a webhook request to the channel. The former incoming webhooks (Office 365 connectors) stopped in May 2026. |
| Slack | On api.slack.com/apps, create an app, turn on Incoming Webhooks and add it to the channel. Rocket.Chat takes the same format: choose Slack. |
| Google Chat | In the space: Apps & integrations, Webhooks, Add. |
| Discord | In the channel settings: Integrations, Webhooks, New Webhook, Copy Webhook URL. |
| Mattermost | Integrations, Incoming Webhooks, Add. |
| ntfy | The field starts with `https://ntfy.sh/`: type the topic after it, or only the topic. On your own ntfy server, the full topic URL. Then subscribe to the topic in the ntfy app. A topic on ntfy.sh is readable by anyone who knows its name: choose one nobody guesses, or use your own ntfy server, with `?auth=` in the URL for a protected topic. |
| Email | Addresses separated by commas, 50 at most. |
| Webhook | A URL that takes a JSON `POST`: Zapier, Make, n8n, Power Automate, Home Assistant. See [Webhook body](webhook.md). |

## Email

Email channels need an SMTP server: set [`SMTP_HOST`](configuration.md#smtp_host) and the others in `.env`. Without it, the type shows "Email (SMTP server not configured)".

- Each address receives a copy of its own, so an address the server refuses keeps the message from no one else.
- Every message of an event has the same subject and joins one conversation in the mail client.
- The same server sends the [forgotten password](forgotten-password.md) links, and reminds a member when an update they announced on an incident is more than 5 minutes late, once.

## Delivery

- Messages leave within seconds, in order for each channel.
- A channel that does not answer is tried again after 30 seconds, 2, 10 and 30 minutes. After that, or at once when the service refuses the URL (404, 401), its waiting messages are given up.
- The list of channels says, for each one, when it last sent, when it tries again and why, or when it failed and why.

Outages found by [monitoring](monitoring.md), or reported by a [monitoring tool](monitoring-tools.md), at the same minute are grouped in one message per channel, and their return in one more.

## Email subscribers

Visitors and members subscribe themselves from **Get notified**, described in [the status page](status-page.md#get-notified). The **Email subscribers** card of the Alerts page counts them and leads to their list.

- **See the subscribers** lists each person: who they are, what they receive, since when, and how their last email went, with the reason when it failed. **Remove** ends a subscription after a question under the button; the person receives nothing more.
- **Visitors can subscribe** is ticked by an administrator and saved with **Save**. It is off on a new instance. Unticking it stops the emails to the visitors already subscribed, who come back when it is ticked again; members go on receiving theirs.
- Nothing is sent without an SMTP server ([`SMTP_HOST`](configuration.md#smtp_host)): the card then says so and offers no switch.
- The [Journal](journal.md) keeps the address of a subscriber who confirmed, left or was removed for as long as it keeps its lines.

## Page URL

Every message links to the page. At the foot of the Alerts page, **Page URL** shows that address:

- With [`PUBLIC_URL`](configuration.md#public_url) set, it is that address, and cannot be changed here.
- Otherwise it can be typed here. The first channel added fills it with the address the Alerts page was opened at; an address emptied on purpose stays empty.

## Keep the addresses secret

Anyone who knows a webhook address can post in its channel. Statup shows the addresses only to administrators and never writes them to the logs or the Journal. Your [backups](backups.md) contain them.
