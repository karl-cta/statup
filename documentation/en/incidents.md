---
title: Incidents
description: "Declare what is broken, keep people posted with updates, resolve it, and write the report."
---

An incident says what is broken, which services it touches and how badly, and what the team is doing about it. Editors and administrators publish one from **New event**, on the dashboard or on the Events page.

## Declare an incident

In the form, with **Type** set to **Incident**:

| Field | Description |
|---|---|
| **Impact** | Required: **Minor** or **Major**. |
| **Services affected** | Optional. Services are listed under their [folders](services.md#folders), each folder with a box that ticks its services; past eight services, a search narrows the list, and the box then ticks only the services it shows. A line under the list says what will change, such as "Email: outage." |
| **Title** | Required, up to 200 characters. Until you type one, it writes itself, such as "Outage on Email". |
| **Description** | Optional: what is affected, what is known. |

Under **More options**:

- **Progress**: **Investigating** by default, or **Fixing** or **Monitoring**.
- **Real start**, if the incident began earlier than now. It cannot be in the future.
- **Next update**: see [below](#next-update).
- **Save as a template**: see [Templates](#templates).

**Publish the incident** shows it at once, and sends it to the [alert channels](alerts.md) that receive incidents.

The description, the updates and the report take simple formatting: `**bold**`, `*italic*`, lists, `[links](https://example.com)`, code and tables. No images.

### From a detected outage

When [monitoring](monitoring.md) finds a service down, **Declare an incident**, on the banner or the Services page, opens the form with the impact **Major**, that service, and the start of the outage as the real start.

## What it does to the services

While the incident is **Investigating** or **Fixing**, the services it names show **Outage** for a **Major** impact, **Degraded** for a **Minor** one. From **Monitoring** on, they go back to their own state. See [Services](services.md#states).

## Progress

| Progress | Means | Can move to |
|---|---|---|
| **Investigating** | The team is looking into it. | Fixing, Monitoring, Resolved, Cancelled |
| **Fixing** | The fix is under way. | Monitoring, Resolved, Cancelled |
| **Monitoring** | The service is restored and watched. The timeline says "Service restored". | Fixing, Resolved |
| **Resolved** | Over. The incident shows how long it lasted. | |
| **Cancelled** | Closed without counting in the services' history. | |

**Back to “Investigating”**, or to whichever state came before, undoes the last move, one step, on the incident's page.

## Post an update

**Post an update**, on the incident's page or in its side panel, takes a message, a new progress, or both, and the next update. **Publish** adds it to the timeline and sends it to the alert channels.

Each update shows its time. The name of its author is shown to signed-in members only.

## Next update

Announcing when the next update will come tells readers when to look again.

- A delay: `30`, `30 min`, `1 h`, `1h30`. Written in hours, a delay goes up to `4h59`: from `5h` on, it reads as a time of day. Written in minutes, up to `999`.
- A time of day: `14:30`, `14h30`, `9h`. It means the next time that hour comes, today or tomorrow, in the instance's time zone.

The banner then says "update due at 14:30", and "update due 5 min ago" once it is late. The incident's page says "last update 9 min ago, next due at 14:30".

Every update sets the next one again: left empty, it clears it. A delay is offered again in the following update; a time of day is not. Resolving or cancelling the incident drops it.

### Reminder

When an update is more than 5 minutes late, the member who announced it gets a notification, "Update overdue", and an email when an SMTP server is set. Once for each time announced.

## Resolve

Choose **Resolved** in **Post an update**, with a message or without. The services go back to their own state, and the incident shows "lasted 2h". A message posted within two minutes of the resolution shows with it, as one moment of the timeline.

## Write the report

Once an incident is **Resolved**, **Write the report**: what happened, why, and what changes so it does not happen again. **Publish the report** shows it at the head of the incident's page and in its side panel, marks the incident "Resolved · report" in the lists, adds it to the feed, and sends it once to the alert channels.

**Edit**, in the **Report** card, corrects it and sends nothing again; **Delete** removes it. A cancelled incident has no report. If an administrator reopens the incident, its report is hidden until it is resolved again.

## Edit or delete

**Edit** changes the title, the description, the impact and the services. The type, the real start and the next update stay as they are. An edit sends no alert.

**Delete the event**, at the foot of the edit page, removes it everywhere, its updates and report included, and sends no alert. One update can be deleted on its own: by its author while the incident is open, or by an administrator.

Once an incident is **Resolved** or **Cancelled**, only an administrator can still edit it, post on it, change its progress or delete it. Editors can still write its report.

## Templates

A template keeps the title, the description, the type, the impact and the category of an event, without its services or times.

- Tick **Save as a template** when publishing a new event.
- **Start from a template**, at the top of the form, lists the 30 most used. Typing a title of two letters or more also suggests saved templates.
- Applying one fills the title, the description, the type, the impact and the category. Services and times stay to be chosen.
- A template is deleted from the **Start from a template** list.

## Who can do what

| | Reader | Editor | Administrator |
|---|---|---|---|
| Read the incidents | Yes | Yes | Yes |
| Declare, update, edit or delete an open incident | | Yes | Yes |
| Write, correct or remove the report | | Yes | Yes |
| Change a resolved or cancelled incident | | | Yes |
