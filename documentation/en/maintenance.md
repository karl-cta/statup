---
title: Maintenance
description: "Announce planned work ahead, let it start and end on its own, or start it right away."
---

A maintenance tells people that work is planned or under way, on which services, and until when. Editors and administrators publish one from **New event**, with **Type** set to **Maintenance**.

## Announce a maintenance

| Field | Description |
|---|---|
| **Start** | **Now** by default. Pick a day and a **Start time** to announce it ahead: the list offers every half hour, and any time can be typed. |
| **Duration** | In hours and minutes. The **Estimated end** follows, such as "Tomorrow at 02:00". Without a duration: "No set end". |
| **Repeat** | Offered once a day is picked, never with **Now**, and only when the maintenance is created. See [Repeat](#repeat). |
| **Services affected** | The services the work touches. |
| **Title** | Required. Until you type one, it writes itself, such as "Maintenance of Email". |
| **Description** | Optional. |

Times are in the instance's time zone, which the form shows. Under **More options**, **No downtime** keeps the services usable during the work: their state never changes. It can only be set when the maintenance is created.

**Announce the maintenance** publishes it and sends it to the [alert channels](alerts.md) that receive maintenance.

## Ahead, or right away

- **With a start**, the maintenance is announced: **Scheduled**. Its services do not change until it starts.
- **Without one**, it begins at once: **Under way**.

## It starts and ends on its own

Every minute, Statup starts each scheduled maintenance whose start has come, and completes each maintenance under way whose end has come. Both are recorded at the times announced, and each sends its alert.

A maintenance without a duration never ends on its own: complete it by hand. A start or an end made on its own cannot be undone.

## By hand

In **Post an update**, choose:

- **Under way** to start it now;
- **Completed** to end it now, earlier than planned;
- **Cancelled** to call it off, before or during the work.

## Move it

While it is scheduled, **Edit** changes its start or its duration, and the alert channels receive the new window ("Moved to …"). Once it has started, its start is locked; its duration can still change.

## Add to a calendar

A maintenance still to come, **Scheduled**, can be copied into a calendar. The **Details** card shows **Add to my calendar** under the state, and the side panel has the same link. Anyone who can read the page sees it, visitors included on a public page.

The link opens a menu of three choices:

- **Outlook** and **Apple** download the same calendar file (`.ics`). Outlook opens it as an appointment to save (the new Outlook for Windows asks to import it); Apple Calendar adds it after asking which calendar.
- **Google Calendar** opens Google Calendar in a new tab, with the event filled in.

The entry is titled "Maintenance: …" in the reader's language. It carries the planned start and end, the **Description**, the services affected and the address of the page. Without a planned end, the entry has no duration.

The entry is a copy: if the maintenance is moved or cancelled afterwards, the calendar keeps the old entry. Email subscribers receive the change as usual. The link is not offered on an incident or an announcement, nor on a maintenance that has started, been completed or been cancelled.

## Repeat

**Repeat** brings the same work back every week or every month. Its two choices are named after the day picked: "Every Sunday", or "The first Sunday of the month" for the rank of that weekday in its month. The rank comes from the day of the month: the 1st to the 7th is the first, the 8th to the 14th the second, the 15th to the 21st the third, the 22nd to the 28th the fourth, the 29th to the 31st the last. For "the last", pick a date from the 29th to the 31st.

A duration is required: without one, the form answers "A repeating maintenance needs a duration."

A repeating maintenance is announced once. The [alert channels](alerts.md) and the email subscribers receive one message, when it is created, with the rhythm after the window, such as "Every Sunday". After that, each session starts and ends on its own, in silence: no message at its start or its end. [Monitoring](monitoring.md#when-no-outage-is-declared) stays quiet during each session, as for any maintenance. **No downtime** and the services carry over to every session.

Statup keeps one session scheduled ahead. Once it starts, or is cancelled or deleted, the next one appears; its timeline opens on **Added automatically**, with no author. Sessions due while the server was off are not caught up: the next one is the first still to come.

Times follow the instance's clock: 02:00 stays 02:00 across daylight saving changes. On the night a time does not exist, the session starts at the first minute after it.

On a session's page, the **Details** card shows **Repeats** with the rhythm. Editors and administrators find **Stop repeating** under it: it asks to confirm, cancels the session scheduled ahead (its cancellation is sent like any other) and adds no more. A session under way runs to its end. On a repeating maintenance, **Add to my calendar** adds the session of that page only, not the repetition.

**Edit** changes one session only. Ticking **Apply to the following ones too** gives its title, description, services, time of day and duration to the sessions after it, the one already scheduled included. The day it now falls on becomes the rhythm's day, or its rank in the month.

Cancelling one session from **Post an update** skips that time only: the next session appears. Moving one session by hand sends "Moved to …" as usual.

Stopping, and **Apply to the following ones too**, are recorded in the [Journal](journal.md).

There are only these two rhythms, and no end date: stop it by hand. A repeat cannot be set through the [API](api.md), which reads each session as an ordinary maintenance.

## What the services show

While it is under way, the services it names show **Under maintenance**, unless it is marked **No downtime**, or unless something more serious holds them. A scheduled maintenance changes nothing yet.

[Monitoring](monitoring.md#when-no-outage-is-declared) declares no outage for a service under maintenance.

## On the page

- When nothing is disrupted, the banner says "Maintenance in progress" during the work, and "Maintenance planned for …" when a one-off maintenance starts within seven days. A repeating maintenance is never announced ahead there: the banner shows a session only while it is under way.
- The **Maintenance** block lists those under way, then the scheduled ones by start, then the five last completed or cancelled. A repeating maintenance appears by its scheduled session and its latest finished one only, not by each past session.
- The maintenance's page says "in 2d" before it starts and "started 20 min ago" during it, with the planned start and end on its timeline.

## After the work

Once a maintenance is completed, **Publish what is new** opens an announcement already filled in: "What is new after …", in the category **What's new**, on the same services, following that maintenance. See [Announcements](announcements.md).

## Edit or delete

The rules are those of [incidents](incidents.md#edit-or-delete): once a maintenance is **Completed** or **Cancelled**, only an administrator can still change or delete it.
