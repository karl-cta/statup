<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="static/logo-dark.svg">
  <img src="static/logo.svg" width="60" height="80" alt="Statup logo">
</picture>

# Statup

**The status page your whole company reads.**<br>
Outages, maintenance and what changed, published by IT, in plain words for everyone.

[![License: AGPL-3.0-or-later](https://img.shields.io/badge/license-AGPL--3.0--or--later-blue.svg)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-orange.svg)](https://www.rust-lang.org/)
[![Status: pre-v1](https://img.shields.io/badge/status-pre--v1-yellow.svg)](#status)

[Features](#features) · [How it works](#how-it-works) · [Quick start](#quick-start) · [Roadmap](#roadmap) · [Self-hosting guide](.github/SELF-HOSTING.md)

</div>

<br>

> *"Is the internet down?"* *"Is it just me, or is Outlook broken?"*<br>
> Every outage starts with the same questions, by phone, by chat and at the IT office door.

Statup answers them before they are asked. The IT team says what is broken, what is being fixed and what is planned, and also what changed: the new version of the payroll software, the printer that moved to the second floor, the VPN client everyone must install. Accounting, HR and everyone else read it in plain words, from a desk or a phone.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset=".github/assets/status-page-dark.png">
  <img src=".github/assets/status-page.png" alt="The status page: a banner saying one service is disrupted, with the incident and its latest update, then the services with thirty days of availability, the recent activity and the maintenance schedule" width="1280">
</picture>

## Features

<table>
  <tr>
    <td width="50%" valign="top">
      <strong>A page that answers first</strong><br>
      A banner says whether something is wrong, what is affected and since when, above every service and its last 30 days. It refreshes itself every minute.
    </td>
    <td width="50%" valign="top">
      <strong>Incidents, start to finish</strong><br>
      From investigation to resolution, with dated updates and reusable templates. The services concerned change state on their own, and come back when it is over.
    </td>
  </tr>
  <tr>
    <td valign="top">
      <strong>Maintenance that runs itself</strong><br>
      Announced ahead, it starts and ends at the planned times, and says so when the services stay usable.
    </td>
    <td valign="top">
      <strong>News from IT</strong><br>
      Announcements for what changed: a software update, a new tool, an office move. Read as a short article, linked to the maintenance it follows.
    </td>
  </tr>
  <tr>
    <td valign="top">
      <strong>Everyone in the loop</strong><br>
      An events list with search and filters, a side panel to read without leaving it, and an Atom feed for feed readers and chat tools.
    </td>
    <td valign="top">
      <strong>Your page, your way</strong><br>
      Open to everyone or to members only, with your name and logo, and blocks you arrange on the page itself. French and English, light and dark, accessible.
    </td>
  </tr>
</table>

**Light, private, safe by default.** One Rust binary and one SQLite file: no Redis, no Postgres, a Docker image under 10 MB. Fonts and scripts come from your instance, so no visitor's browser calls a third party. Passwords are hashed with Argon2id, every form carries a CSRF token, and a strict Content Security Policy guards every page.

## How it works

- **Services** are the tools people rely on: mail, the VPN, the ERP, the phones. Each shows one state: operational, degraded, outage or maintenance.
- **Events** are what the IT team publishes. An **incident** when something breaks, a **maintenance** when work is planned, an **announcement** for news. An incident or a maintenance sets the state of the services it names until it ends.
- **People** read the page, with or without an account. **Editors** publish events and set service states; **administrators** also run the settings, the team and the layout of the page.

## Quick start

You need Docker with Docker Compose 2.24 or newer.

**1. Get Statup and start it**

```bash
git clone https://github.com/karl-cta/statup.git
cd statup
docker compose up -d
```

The first start builds Statup, which takes a few minutes.

**2. Open http://localhost:3000**

An empty instance walks you through four steps: your administrator account, the page's name, logo and audience, the services to follow, then the address to share.

**3. Open it to your colleagues**

At first Statup answers this machine only, so nobody else can claim the administrator account. When yours exists, change `"127.0.0.1:3000:3000"` to `"3000:3000"` under `ports:` in `docker-compose.yml` and run `docker compose up -d` again, or put Statup behind a reverse proxy with HTTPS. Then add your colleagues from the **Team** page.

> [!TIP]
> The [self-hosting guide](.github/SELF-HOSTING.md) has the rest: every setting, a reverse proxy with nginx or Caddy, backups, upgrades and a forgotten password.

## Roadmap

Planned, without dates:

- **Automatic monitoring**: Statup checks that a service answers and sets its state on its own.
- **Incidents from your monitoring**: Zabbix, Grafana or any other tool opens and closes an incident by itself.
- **Alerts where people are**: messages in Microsoft Teams and Slack, and email subscriptions.
- **Groups and visibility**: gather colleagues into groups and choose which services and events each group sees.
- **Recurring maintenance**: announce once a slot that comes back every week or month.
- **Themes** and an accent colour.

Ideas and requests are welcome in the [issues](https://github.com/karl-cta/statup/issues).

## Status

Statup is **pre-v1**: usable and self-hostable, with the interface being finished before the first stable release. Expect changes between versions and back up before upgrading. Every change is in the [changelog](.github/CHANGELOG.md).

## Contributing

Built with Rust, Axum, SQLite, Askama templates and htmx. The [contributing guide](.github/CONTRIBUTING.md) explains how to build Statup, where things are in the code, and the checks a change must pass.

## License

Statup is licensed under the [GNU Affero General Public License v3.0 or later](LICENSE). If you run a modified version for others over a network, offer them its source code.

It ships htmx, the Hanken Grotesk font, icons from Heroicons and the base styles of Tailwind CSS, under their own licenses: see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
