---
title: Install
description: "Run Statup with Docker Compose on an x86-64 or ARM server, then create the first administrator."
---

## Requirements

- Docker with Docker Compose 2.24 or newer.
- A 64-bit server, x86-64 or ARM: the image is published for both.

Docker is the main way to install Statup. Without it, Statup runs as a service on [Linux](install-linux.md) and on [Windows Server](install-windows.md), and from its flake on [NixOS](nixos.md).

## Start Statup

```bash
mkdir statup && cd statup
curl -O https://raw.githubusercontent.com/karl-cta/statup/main/docker-compose.yml
docker compose up -d
```

Docker downloads the image `ghcr.io/karl-cta/statup:latest` and starts it. Statup answers on port 3000 of the server.

## What docker-compose.yml sets

| Setting | Value |
|---|---|
| Image | `ghcr.io/karl-cta/statup:latest`, which follows each new version. See [Upgrading](upgrading.md) to stay on one. |
| Port | `3000:3000`: the port of the server first, then the port of the container. |
| Volume | `statup_data`, mounted at `/data`: the database and the uploaded files. See [Backups](backups.md). |
| Settings | A `.env` file next to `docker-compose.yml`, optional. See [Configuration](configuration.md). |
| Restart | `unless-stopped`: Statup starts again with the server. |
| Privileges | None: `no-new-privileges`, every capability dropped. The container runs as the user `statup`. |
| Logs | Rotated at 10 MB, three files kept. `docker compose logs statup` reads them. |

The image checks its own [health](health-check.md) every 30 seconds.

## First launch

Open `http://localhost:3000`, or your server's address on port 3000. An empty instance leads through four steps, with a preview of the page beside each one.

1. **Account**: **Name**, **Email address** and **Password**. This first account is the administrator. The instance takes its time zone from your browser.
2. **Page**: **Page name** (optional, up to 40 characters), **Logo** (optional), and **Page access**: **Members only** or **Public**.
3. **Services**: "Your services". Tick some of the **Suggestions**, or type your own, and open **Monitor this service** to add a check. Optional, up to 50 at once.
4. **Ready**: the address of the page, with **Copy**, to share with the people who use your services. **Open the dashboard** ends the setup.

To bring back an instance from a backup instead, the first screen also offers **Restore a backup**: see [Backups](backups.md#on-a-new-server).

Everything can be changed later in [Settings](settings.md), and services added from the [Services](services.md) page.

> [!WARNING]
> Until the first account exists, anyone who opens the address can create it and become the administrator. Create yours right after the first start, or preset it as below.

### Preset the administrator

Write [`ADMIN_EMAIL`](configuration.md#admin_email) and [`ADMIN_PASSWORD`](configuration.md#admin_password) in `.env` before the first start. Statup creates the administrator when it starts, named after the part of the address before the `@`. A value that breaks the [password rule](team.md#password-rule) stops it at start.

Then sign in at `/login`. The setup steps are not shown: the dashboard holds a **First steps** card instead, to add services and share the address. The time zone stays [`TZ`](configuration.md#tz), or UTC, until it is set in Settings. Remove both values from `.env` once the account exists.

## Reach Statup from your network

Your colleagues open the server's address on port 3000. To use another port, change the part before `:3000` under `ports:` in `docker-compose.yml`, then run `docker compose up -d` again.

For HTTPS and a name of your own, such as `status.example.com`, put Statup [behind a reverse proxy](reverse-proxy.md).

## Next

- Add your colleagues: [Team and roles](team.md).
- Tell people where they already read: [Alerts](alerts.md).
- Let Statup notice an outage first: [Monitoring](monitoring.md).
