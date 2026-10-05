<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset=".github/assets/wordmark-dark.svg">
  <img src=".github/assets/wordmark.svg" width="220" height="59" alt="Statup">
</picture>

### Every team in the loop, before anyone asks.

Statup is a self-hosted internal status page. IT, or whoever runs a service, posts outages, maintenance and news once; colleagues read them in plain words.

[![License: AGPL-3.0-or-later](https://img.shields.io/badge/license-AGPL--3.0--or--later-blue.svg)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-orange.svg)](https://www.rust-lang.org/)
[![Status: pre-v1](https://img.shields.io/badge/status-pre--v1-yellow.svg)](#status)

[Live demo](https://demo.statup.dev) · [Website](https://statup.dev) · [Features](#features) · [How it works](#how-it-works) · [Quick start](#quick-start) · [Roadmap](#roadmap) · [Self-hosting guide](.github/SELF-HOSTING.md)

</div>

<br>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset=".github/assets/status-page-dark.png">
  <img src=".github/assets/status-page.png" alt="The status page: a banner saying one service is disrupted, with the incident and its latest update, then the services with thirty days of availability, the recent activity and the maintenance schedule" width="1280">
</picture>

**Monitoring tools are made for the people who fix things. Statup is made for everyone else.**

> *"Is the internet down?"* *"Is it just me, or is Outlook broken?"*<br>
> Every outage starts with the same questions, by phone, by chat and at the IT office door.

With Statup, the answer is already there: what is broken, what is being fixed, what is planned. And what changed, too: the new version of the payroll software, the printer that moved to the second floor, the VPN client everyone must install. Accounting, HR and everyone else read it from a desk or a phone.

> [!TIP]
> **See it live** at [demo.statup.dev](https://demo.statup.dev): the page is open to everyone. Sign in with `demo@statup.dev` and `StatupDemo#1` to publish incidents, maintenance and news as an editor would. Visitors share this account, and the demo starts over every hour.

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
      <strong>What's new</strong><br>
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
  <tr>
    <td colspan="2" valign="top">
      <strong>Statup notices first</strong><br>
      Every minute, it checks that a website answers, or that a server, a router or a firewall accepts a connection. After three failed checks in a row, the service shows an outage on its own, marked "detected automatically", and comes back as soon as it answers. Your team declares the incident in one click, already filled in, and a Test button says why an address does not answer before you save it.
    </td>
  </tr>
</table>

**Light, private, safe by default.** One Rust binary and one SQLite file: no Redis, no Postgres, a Docker image under 10 MB to download. Fonts and scripts come from your instance, so no visitor's browser calls a third party; the server itself only reaches the addresses you ask it to check, and GitHub once a day to see whether a newer version is out, which `UPDATE_CHECK=false` turns off. Passwords are hashed with Argon2id, every form carries a CSRF token, and a strict Content Security Policy guards every page.

## How it works

- **Services** are the tools people rely on: mail, the VPN, the ERP, the phones. Each shows one state: operational, degraded, outage or maintenance. Statup can also check on its own that a service answers.
- **Events** are what gets published. An **incident** when something breaks, a **maintenance** when work is planned, an **announcement** for news. An incident or a maintenance sets the state of the services it names until it ends.
- **People** read the page, with or without an account. **Editors** publish events and set service states; **administrators** also run the settings, the team and the layout of the page.

## Quick start

You need Docker with Docker Compose 2.24 or newer.

**1. Start Statup**

```bash
mkdir statup && cd statup
curl -O https://raw.githubusercontent.com/karl-cta/statup/main/docker-compose.yml
docker compose up -d
```

Docker downloads the ready image, for x86-64 and ARM servers alike, and starts it in seconds.

**2. Open it in your browser**

At http://localhost:3000, or your server's address on port 3000, an empty instance walks you through four steps: your administrator account, the page's name, logo and audience, the services to follow, then the address to share.

**3. Invite your team**

Add them from the **Team** page: they open the same address. For HTTPS and a name of your own, such as `status.example.com`, put Statup behind a reverse proxy.

> [!TIP]
> The [self-hosting guide](.github/SELF-HOSTING.md) has the rest: every setting, a reverse proxy with nginx or Caddy, backups, upgrades and a forgotten password.

## Roadmap

Planned, without dates:

- **Incidents from your monitoring**: Zabbix, Grafana or any other tool opens and closes an incident by itself.
- **Alerts where people are**: messages in Microsoft Teams and Slack, and email subscriptions.
- **Groups and visibility**: gather colleagues into groups and choose which services and events each group sees.
- **Recurring maintenance**: announce once a slot that comes back every week or month.
- **Themes** and an accent colour.

Ideas and requests are welcome in the [issues](https://github.com/karl-cta/statup/issues).

## Status

Statup is **pre-v1**: usable and self-hostable, with the interface being finished before the first stable release. Expect changes between versions and back up before upgrading. Every change is in the [changelog](.github/CHANGELOG.md).

Statup is not related to Statping, a Go project first published under the name Statup.

## Contributing

Built with Rust, Axum, SQLite, Askama templates and htmx. The [contributing guide](.github/CONTRIBUTING.md) explains how to build Statup, where things are in the code, and the checks a change must pass.

## License

Statup is licensed under the [GNU Affero General Public License v3.0 or later](LICENSE). If you run a modified version for others over a network, offer them its source code.

It ships htmx, the Hanken Grotesk font, icons from Heroicons and the base styles of Tailwind CSS, under their own licenses: see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
