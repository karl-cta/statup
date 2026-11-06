---
title: The status page
description: "What colleagues and visitors read: the banner, the services, the recent activity, the events and the feed."
---

What everyone reads, from a desk or a phone. With **Page access** set to **Public**, visitors read it without an account; with **Members only**, they are asked to sign in. The names of the team are shown to signed-in members only.

Each reader sees only the services their [visibility](services.md#visibility) allows, and the events that name them.

## The dashboard

The page refreshes itself every minute while it is open, and when the reader comes back to its tab a minute or more after the last refresh. If the connection drops, it says "Connection lost, trying again."

Administrators choose its blocks and their layout: see [Settings](settings.md#arrange-the-dashboard). Signed-in members find a **Documentation** link at the foot of every page, to these pages.

### The status banner

Always at the top, it gives the answer first:

- "All services are working normally";
- or "2 services disrupted", with a line for each, the most serious first: its state, the event that explains it and since when, the next update announced, and the start of the latest update;
- or "Maintenance in progress", when work is under way and nothing is disrupted;
- or "No visible service", with "Sign in to see every service.", when a visitor may see none of them.

An outage found by [monitoring](monitoring.md) and not yet declared reads "Detected automatically", with how long it has lasted. When nothing is disrupted, the banner also announces a maintenance planned within seven days.

### Services

Each service with its state and its availability day by day, under the title of its [folder](services.md#folders); the services in no folder come first. A service opens in a side panel: its state, its last 30 days, its three latest events. See [Services](services.md).

### Recent activity

The ten latest events, by day. By default it shows incidents and announcements, not maintenance. **Get notified** and **View all** lead further.

### Maintenance

The maintenance under way, then the ones planned, then the five last completed or cancelled. See [Maintenance](maintenance.md).

## Events

The **Events** page lists every event, the newest first, 20 at a time.

- **Search** looks for words in the titles and the descriptions, not in the updates. Each word may be the start of a longer one; case and accents do not matter.
- **Filters**: **Type**, **Progress** (**Open or upcoming**, or **Closed**), **Service**, and **From** and **To**, by the day the event was created.

An event opens in a side panel without leaving the list; on a phone, it opens its page. Its page holds its **Details** and its **Timeline**, the newest first; an announcement shows its text, then its **Updates**.

Signed-in members see on **Events** how many incidents and maintenance were published since their last visit.

## Get notified

**Get notified**, at the foot of every page and on the **Recent activity** block, gives the ways to follow without opening the page:

- **By email**: a visitor types their address, ticks what they want to receive (**Incidents**, **Maintenance**, **Announcements**) and confirms through the link sent to them, which works for one day. A signed-in member subscribes at the address of their account, with no link to confirm, and changes their boxes or stops in the same place.
- **With a feed reader**: the address of the Atom feed, to copy.
- **In Teams or Slack**: through the channels an administrator sets in [Alerts](alerts.md).
- **Or this page**: kept open, it updates itself every minute.

Each email tells one piece of news: an incident opened, an update posted, its end and its review, a maintenance announced, moved, started or finished, an announcement. It names only the services its reader may see: a visitor never reads the name of a service kept for members. Every email ends with **Unsubscribe**, and mail clients can offer their own unsubscribe button.

**By email** shows only when the instance can send email ([`SMTP_HOST`](configuration.md#smtp_host)) and knows its own address ([`PUBLIC_URL`](configuration.md#public_url), or the **Page URL** of [Alerts](alerts.md#page-url)). For visitors, the page must also be open to everyone, and an administrator must open subscriptions to them in [Alerts](alerts.md): they are closed on a new instance.

## The Atom feed

`/feed` holds the fifty latest events: their kind, impact and progress, their services, their description, their report and their updates, without any member's name. With **Members only**, a feed reader without a session is sent to sign in, so it can no longer read the feed. Its links use [`PUBLIC_URL`](configuration.md#public_url) when it is set.

## Language and theme

Statup speaks French, English, German and Spanish. It uses, in order: the language chosen with the language switch, kept on the account of a signed-in member; then the browser's language; then [`DEFAULT_LOCALE`](configuration.md#default_locale).

The light or dark theme follows the system until the reader chooses one; each browser keeps its own choice.
