---
title: Settings
description: "The page's name, logo, colours, time zone and audience, the version, and the layout of the dashboard."
---

**Settings**, for administrators, lists five pages under **Management**, beside the **General** card on a wide screen and above it otherwise:

- **Team**: the members and their roles. See [Team and roles](team.md).
- **Alerts**: the channels that receive the news. See [Alerts](alerts.md).
- **API**: the keys of the scripts and tools that use the API. See [API](api.md#keys).
- **Journal**: what happened on the instance. See [Journal](journal.md).
- **Version**: the version running.

## General

One form, saved with one **Save** button. Every field is checked before any is written, and leaving the page with changes not saved makes the browser ask first.

| Field | Description |
|---|---|
| **Page name** | Shown instead of Statup in the header and the browser tab. Up to 40 characters; empty shows Statup. |
| **Logo** | Takes the place of the Statup mark in the header; "Powered by Statup" stays at the foot. PNG, JPG, WebP or SVG, up to 256 KB. An image larger than 256 pixels is reduced. **Remove the logo** brings the mark back at once. |
| **Colour** | Colours the main buttons, the ticked boxes, the chosen options, the line under the header and under the current tab, and the dot and last letter of the Statup mark when no logo is chosen, on every page and for every reader. Twelve colours: **Ink** (the default), **Navy**, **Cobalt**, **Teal**, **Pine**, **Rose**, **Violet**, **Burgundy**, **Brick**, **Copper**, **Cocoa** and **Slate**, each drawn for the light and the dark theme. The colours of the states never change. |
| **Ambiance** | The ground and the cards of every page: **Sand** (the default, a white page with cream cards), **Mist** (tinted blue), **Paper** (cream) or **Pebble** (a grey page with white cards), each with its own dark version. Every reader keeps the light or dark switch of the header. |
| **Time zone** | Times on the page, on events and on maintenance are shown and typed in it. |
| **Page access** | **Members only** or **Public**. See [Team and roles](team.md#page-access). |

A colour or an ambiance shows on the settings page as soon as it is picked, and on every page once saved.

What is saved here takes precedence over [`TZ`](configuration.md#tz) and [`PUBLIC_MODE`](configuration.md#public_mode), and is kept across restarts.

## Version

The **Version** row shows the version running and leads to the published versions on GitHub. When a newer one is published, it says "Version 0.7.0 is available." and leads to what changed.

Statup asks GitHub once a day, the first time a minute after it starts. The answer is kept in memory: after a restart, nothing shows until that first question. [`UPDATE_CHECK=false`](configuration.md#update_check) turns it off.

## Arrange the dashboard

**Arrange**, beside **New event** on the dashboard, lets an administrator lay out the page that everyone reads. It is offered on screens 768 pixels wide or more: a phone stacks every block in one column.

| Block | Description |
|---|---|
| **Status banner** | Always shown, full width, above the others. |
| **Services** | A quarter of the width by default. Its options choose the services it lists. A hidden service reappears as soon as it is disrupted. |
| **Recent activity** | Half the width by default. Its options choose what it lists: **Minor incidents**, **Major incidents**, **Maintenance**, **Announcements**. All but maintenance by default. |
| **Maintenance** | A quarter of the width by default. |

- Move a block by dragging it, or with the arrow keys.
- Set its width: **A quarter**, **Half** or **Full width**.
- **Options**, on a block, opens the choice of what it shows. **Hide** takes it off the page; **Add a block** brings it back.

Every change is saved at once, for every reader: there is one layout per instance. **Done**, or Escape, leaves the mode.
