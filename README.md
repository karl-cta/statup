<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="static/logo-dark.svg">
  <img src="static/logo.svg" width="60" height="80" alt="Statup logo">
</picture>

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
- **Atom feed** at `/feed`, so feed readers, Slack or Teams follow incidents without anyone opening the page
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
git clone https://github.com/karl-cta/statup.git && cd statup
docker compose up -d
# → http://localhost:3000, create the administrator account on the first visit
```

No setting is required. To change one, copy `.env.example` to `.env` and edit it.

<details>
<summary><strong>Build from source</strong></summary>

Requires Rust 1.88 or newer and the [Tailwind CSS v4 standalone CLI](https://github.com/tailwindlabs/tailwindcss/releases), saved at the repository root as `tailwindcss`.

```bash
./scripts/build-css.sh && cargo build --release
# Run it from the repository root, which holds the static/ directory it serves
./target/release/statup
```

</details>

<details>
<summary><strong>Configuration</strong></summary>

Everything lives in `.env`, and nothing is required: each setting falls back to its default. With Docker Compose, the database and the icons stay in the `statup_data` volume whatever `DATABASE_URL` and `UPLOAD_DIR` say.

| Variable | Required | Default | Description |
|---|---|---|---|
| `DATABASE_URL` | No | `./statup.db` | Path to SQLite database |
| `UPLOAD_DIR` | No | `data/uploads` | Where uploaded icons are stored |
| `HOST` | No | `0.0.0.0` | Listen address |
| `PORT` | No | `3000` | Listen port |
| `LOG_LEVEL` | No | `info` | trace, debug, info, warn, error |
| `PUBLIC_MODE` | No | `false` | Allow guest access to read-only pages |
| `TRUST_PROXY_HEADERS` | No | `false` | Read the client IP from `Forwarded` / `X-Forwarded-For` when rate limiting. Enable it behind a reverse proxy, otherwise every visitor shares the proxy address and the limit becomes site wide. Never enable it without a proxy in front: the headers are then attacker controlled |
| `PUBLIC_URL` | No | request host | Address visitors use to reach the instance, e.g. `https://status.example.com`. Feed entries link back with it, and an `https://` address marks the session cookie Secure |
| `ADMIN_EMAIL` | No | | Creates an administrator on first run. Without it, the first account created from the sign-in page is the administrator |
| `ADMIN_PASSWORD` | No | | Password of that administrator, first run only |

See [`.env.example`](.env.example) for the full reference.

</details>

### Forgotten password

Give the account a temporary password from the server:

```bash
docker compose exec statup /app/statup reset-password you@example.com
# From source: ./target/release/statup reset-password you@example.com
```

Hand it over. The person signs in with it and is asked to choose their own, and any session still open on that account is signed out.

### Health check

`GET /health` → `200 OK`

### Stack

Built with [Rust](https://www.rust-lang.org/) · [Axum](https://github.com/tokio-rs/axum) · [SQLite](https://www.sqlite.org/) (sqlx) · [HTMX](https://htmx.org/) · [Tailwind CSS](https://tailwindcss.com/) · [Askama](https://github.com/djc/askama)

### License

[AGPL-3.0](LICENSE)
