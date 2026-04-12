<div align="center">

<img src="static/logo.svg" width="80" height="80" alt="Statup logo">

# Statup

A lightweight, self-hosted status page for IT teams. Single binary, zero dependencies.

[![License: AGPL-3.0](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-orange.svg)](https://www.rust-lang.org/)
[![Status: pre-v1](https://img.shields.io/badge/status-pre--v1-yellow.svg)](#status)

</div>

Stop answering "is it down?" at the helpdesk. Statup gives your entire organization one place to check service health, follow incidents and know about planned maintenance. Your IT team communicates proactively. Your colleagues stop guessing.

### Status

Statup is **pre-v1, under active development**. The core is usable and self-hostable, but the product is going through a refactor pass before its first stable release. Expect schema changes, UI reworks, and feature churn. Production use at your own risk.

### Features

- **Live dashboard** with color-coded service status (operational, degraded, outage, maintenance), updated in real-time
- **Incident lifecycle** from investigation to resolution, with timeline updates and Markdown descriptions
- **Scheduled and urgent maintenances** so users know before it happens, not after
- **Changelogs and announcements** to communicate releases and important changes
- **Full-text search** across all events, filterable by type, service and date range
- **Unread notifications** so nobody misses a critical event
- **Three roles** (Reader, Publisher, Admin) with optional public mode for guest access
- **Dark mode** and full i18n (FR and EN), WCAG AA accessible

### Why Statup

- **One binary, one file.** No Redis, no Postgres, no external service to maintain. Embedded SQLite, deploy in minutes.
- **Secure out of the box.** Argon2 password hashing, CSRF protection, CSP headers, rate-limiting, parameterized SQL. Nothing to configure.
- **Lightweight.** Fast startup, small memory footprint, minimal dependencies.
- **No JavaScript framework.** HTMX handles real-time updates server-side. Lightweight for you and your users.
- **Your infrastructure, your data.** Self-hosted, fully under your control.

### Quick start

```bash
git clone <repo-url> && cd statup
cp .env.example .env
# Edit .env → SESSION_SECRET, ADMIN_EMAIL, ADMIN_PASSWORD
docker compose up -d
# → http://localhost:3000
```

<details>
<summary><strong>Build from source</strong></summary>

Requires Rust 1.84+ and [Tailwind CLI](https://github.com/tailwindlabs/tailwindcss/releases).

```bash
./scripts/build-css.sh --minify && cargo build --release
# Binary is at target/release/statup
```

</details>

<details>
<summary><strong>Configuration</strong></summary>

Everything lives in `.env`. Only `SESSION_SECRET` is required.

| Variable | Required | Default | Description |
|---|---|---|---|
| `SESSION_SECRET` | Yes | | Session encryption key (min 32 chars) |
| `DATABASE_URL` | No | `./statup.db` | Path to SQLite database |
| `HOST` | No | `0.0.0.0` | Listen address |
| `PORT` | No | `3000` | Listen port |
| `LOG_LEVEL` | No | `info` | trace, debug, info, warn, error |
| `PUBLIC_MODE` | No | `false` | Allow guest access to read-only pages |
| `ADMIN_EMAIL` | No | | Initial admin email (first run only) |
| `ADMIN_PASSWORD` | No | | Initial admin password (first run only) |

See [`.env.example`](.env.example) for the full reference.

</details>

### Health check

`GET /health` → `200 OK`

### Stack

Built with [Rust](https://www.rust-lang.org/) · [Axum](https://github.com/tokio-rs/axum) · [SQLite](https://www.sqlite.org/) (sqlx) · [HTMX](https://htmx.org/) · [Tailwind CSS](https://tailwindcss.com/) · [Askama](https://github.com/djc/askama)

### License

[AGPL-3.0](LICENSE)
