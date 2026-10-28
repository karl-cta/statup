---
title: Services
description: "The tools people rely on, who sees them, their folders, their state, how it changes on its own, and their availability history."
---

A service is a tool people rely on every day: mail, the VPN, the intranet, the phones, a website. Each one shows a state on the page, and the events that name it.

Editors and administrators manage services from the **Services** page. Readers and visitors see them on the dashboard, where a service opens in a side panel with its state, its last 30 days and its latest events.

## Create a service

**New**, then **Service**, on the Services page, opens the form:

| Field | Description |
|---|---|
| **Name** | Required, up to 100 characters. Two services may share a name. |
| **Description** | Optional, one sentence on what the service is for, up to 500 characters. |
| **Monitoring** | **None**, **Web** or **Port**: see [Monitoring](monitoring.md). |
| **Icon** | Optional: a built-in icon, or one you upload. |
| **Visibility** | Who sees the service: see [below](#visibility). **Public** by default. |

**Create the service** adds it at the end of the list, in the state **Operational**. Administrators can also add services during the [first launch](install.md#first-launch).

To change a service later, open it by its name or by its pencil on the Services page, then **Save**. Its state is not in the form: it is set from the list.

To create at once the services of an Uptime Kuma status page, under the names of its monitors: **New**, then **Import from Uptime Kuma**. See [Uptime Kuma](monitoring-tools.md#uptime-kuma).

## Icons

- 24 built-in icons, from Server to Printing.
- **Upload an icon**, in the head of the **Your icons** card on the icon library page: PNG, JPG, WebP or SVG, up to 256 KB. Images larger than 128 pixels are reduced; an SVG is cleaned of anything that could run or load something.
- Uploaded icons form one library, shared by services, events and templates, reached by **Manage your icons** in a service's icon choice. An icon still in use cannot be deleted.

## Visibility

Each service says who sees it, in its form and in the panel of a [folder](#folders):

| Value | Who sees the service |
|---|---|
| **Public** | Everyone who can see the page, visitors without an account included. The default. |
| **Members** | Signed-in members only. |
| **Restricted** | The groups and members listed under **Access**, plus editors and administrators. |

When the page is for members only, the choice is **All members** or **Restricted**.

With **Restricted**, **Add** opens a search over the [groups](team.md#groups), with their number of members, and over the readers, by name for editors and with their address for administrators; each one shows as a tag, with **Remove**. Past five tags, the list ends with "and 2 more". A line says who will see the service, such as "Visible to 3 members, plus editors and administrators." A restricted service needs at least one group or member.

Editors and administrators always see every service. For the others:

- Incidents, maintenance and announcements follow their services: an event is seen by anyone who sees one of the services it names, and shows only those. An announcement that names no service is for everyone.
- A service someone may not see leaves no trace for them: not on the dashboard or the banner, not in the events, the search or the filters, and a direct link answers "not found". When a visitor sees no service at all, the banner says "No visible service".
- The [Atom feed](status-page.md#the-atom-feed) always shows what a visitor without an account sees.
- The text of an event (its title, description, updates and report) is shown as written to everyone who sees the event: name a restricted service there only when they may know about it.

## Folders

A folder groups services, such as Network or Lyon office, on one level: a folder holds no folder, and a service sits in one folder at most. On the dashboard, each folder is a title over its services, after the services in no folder; a reader sees a folder's title only when they see one of its services.

On the Services page, each folder is a card with its name, its number of services and **Manage folder**; the services in no folder come first, in a card without a title.

### Create a folder

**New**, then **Folder**, at the top of the Services page, then a **Folder name** of up to 100 characters, unique, and **Create the folder**. The folder's page opens.

### A folder's page

- "Restricted, Accounting", at the right of its name, with **Change**, opens its **Folder name** and its visibility, the same setting as a service's, then **Save**.
- **Add services** lists the services not in this folder, with a search past eight; tick them, then **Add**. A service in another folder moves here.
- **Remove**, on a service's row, takes it out of the folder.

### Inherited visibility

A service in a folder inherits the folder's visibility by default: its form shows **Inherit from folder “Accounting”**, ticked, with the folder's setting. Changing the folder's visibility changes all the services that inherit it. Unticking the box keeps the setting as a starting point for the service's own.

Folders never open a service by mistake:

- A service added to a folder inherits from it when it was **Public**, or already had the folder's visibility. Any other keeps its own, and the folder's page lists it under **Own visibility**. The message after adding says which, such as "3 services added. 2 inherit the folder's visibility, 1 keeps its own."
- A service taken out of a folder, or left by a deleted folder, keeps the visibility it had.

### Delete a folder

**Delete the folder**, at the foot of its page, with **Delete** to confirm. Its services stay, in no folder, with the visibility they had.

## States

| State | Set by hand | Set on its own |
|---|---|---|
| **Operational** | Yes | |
| **Degraded** | Yes | By an open incident of impact **Minor**, or of no impact |
| **Outage** | Yes | By an open incident of impact **Major**, or by [monitoring](monitoring.md) |
| **Under maintenance** | Yes | By a maintenance under way, unless it is marked **No downtime** |

### Set a state by hand

On the Services page, each service has a state menu. A choice is published at once, with **Undo** beside it. **Outage** first asks "Publish status “Outage”?", then **Publish**.

### When a state changes on its own

An incident sets the state of the services it names while it is **Investigating** or **Fixing**. A maintenance does so while it is **Under way**. Announcements never do. See [Incidents](incidents.md) and [Maintenance](maintenance.md).

When several things apply, the service shows the most serious: **Outage**, then **Degraded**, then **Under maintenance**, then **Operational**. A maintenance under way does not hide a detected outage, and a state set by hand to **Outage** stays during a minor incident.

When the incident moves to **Monitoring**, is resolved or cancelled, or the maintenance ends, the service goes back to the most serious of what is left, which is usually the state set by hand.

While an event or a detected outage holds a service, its menu gives way to the state shown, with a link to that event, or **Detected automatically** and **Declare an incident**. The menu comes back when it ends.

## Order

**Arrange** puts an up and a down arrow on each service, which moves it within its card, and on each folder, which moves the folder among the others. The order is saved a moment after the last move, and applies wherever services are listed, the dashboard included. **Done** or Escape leaves the mode.

In the same mode, **Show disrupted services first** lists every service that is not **Operational** before the others, each group in the chosen order. It is off by default.

## Availability history

Each service has one bar per day, in the instance's time zone, ending today. The dashboard and the Services page show 90 days where there is room and the last 30 on a narrow screen; the side panel always shows 30.

| Bar | When |
|---|---|
| Green | No incident that day |
| Amber | A **Minor** incident or one of no impact, or a detected outage of fifteen minutes or more |
| Red | A **Major** incident |
| Grey | Before the service existed |

- The most serious incident of the day wins. An incident counts from its start until it moves to **Monitoring**, or until it ends; a cancelled incident never counts.
- Maintenance never colours a day.
- On a computer, a day coloured by a detected outage shows that outage when clicked: see [Monitoring](monitoring.md#in-the-history).
- A detected outage colours a day only when no incident covers it, and only when it lasts fifteen minutes or more in total.

There is no percentage: the caption counts the days with an incident.

## Delete a service

A service that no event ever named, and no [repeating maintenance](maintenance.md#repeat) still running names, can be deleted from its form, under **Delete the service**. It leaves without a trace, its detected outages included.

A service that events name is kept, so their history stays whole. To stop showing it on the dashboard, an administrator leaves it out in **Arrange** on the dashboard: see [Settings](settings.md#arrange-the-dashboard). A service left out comes back as soon as it is not **Operational**.

## Who can do what

| | Reader | Editor | Administrator |
|---|---|---|---|
| See the states and the history, open a service's side panel | Yes | Yes | Yes |
| Create, edit, set a state, change the order, delete, manage icons | | Yes | Yes |
| Choose who sees a service, manage folders | | Yes | Yes |
| Read why a detected outage happened | Yes | Yes | Yes |
| Erase a detected outage that was not one | | Yes | Yes |
| Leave a service out of the dashboard | | | Yes |
