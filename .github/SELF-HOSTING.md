# Self-hosting Statup

Everything past the [quick start](../README.md#quick-start): who can do what, the settings, a reverse proxy in front, backups, upgrades and recovery.

## Reaching Statup from your network

`docker-compose.yml` publishes Statup on port 3000 of the host, so your colleagues reach it at the server's address. To use another port, change the part before `:3000` under `ports:` and run `docker compose up -d` again.

The first account created becomes the administrator: create yours right after the first start, or preset it with `ADMIN_EMAIL` and `ADMIN_PASSWORD`.

The time zone of the instance is taken from the browser that creates the first account, and can be changed later in Settings.

## Access and roles

| Role | Can |
|---|---|
| Reader | Read the dashboard, the events and the feed, edit their own profile |
| Editor | Everything a reader can, plus publish incidents, maintenances and announcements, set service states, manage services, templates and icons |
| Administrator | Everything, plus the settings, the team, the dashboard blocks, and changing or deleting closed events |

Who can see the page is chosen at the first launch, then in **Settings, Who can see the page**:

- **Everyone**: visitors read the page and the feed without an account.
- **Members only**: visitors are asked to sign in, and feed readers can no longer read the feed.

Accounts are created by an administrator on the **Team** page, with a temporary password shown once and valid for seven days; the member chooses their own at first sign-in. Self-registration only exists on an empty instance, for its first administrator.

A chosen password needs 12 characters with upper and lower case letters, a digit and a symbol, or 20 characters of any kind.

## Configuration

Every setting is optional. Write the ones you change in a `.env` file next to `docker-compose.yml`; [`.env.example`](../.env.example) lists them with an explanation each. With Docker Compose, `DATABASE_URL`, `UPLOAD_DIR`, `HOST` and `PORT` belong to the image, so the data stays in its volume.

| Variable | Default | Description |
|---|---|---|
| `TZ` | `UTC` | Time zone used until one is chosen, e.g. `Europe/Paris`. The zone set in Settings, or taken from the first account's browser, takes precedence. Dates are shown in it, with the UTC offset where it matters, and maintenance times are typed in it |
| `PUBLIC_URL` | request host | Address visitors use, e.g. `https://status.example.com`. Feed links use it; an `https://` address marks the session cookie `Secure` and sends HSTS |
| `TRUST_PROXY_HEADERS` | `false` | Read the client address from `CLIENT_IP_HEADER` and the scheme from `X-Forwarded-Proto`. Only behind a reverse proxy that sets them |
| `CLIENT_IP_HEADER` | `X-Forwarded-For` | The header your proxy writes the client address in, such as `X-Real-IP`, `Forwarded` or `CF-Connecting-IP`. Only its last entry counts, and no other header is read |
| `PUBLIC_MODE` | `false` | Starting public access. Once an administrator chooses in Settings, that choice is kept across restarts |
| `UPDATE_CHECK` | `true` | Ask GitHub once a day whether a newer version is published, and tell administrators in Settings. The request carries no information about the instance. `false` for an instance that must not reach the internet |
| `MONITORING` | `true` | Run the automatic checks of the services that have one. `false` turns every check off; the addresses stay saved |
| `DEFAULT_LOCALE` | `fr` | `fr` or `en`, for visitors whose browser asks for neither |
| `ADMIN_EMAIL`, `ADMIN_PASSWORD` | unset | Create an administrator at start when no account exists. Both are needed, and the password follows the rule above. Remove them afterwards |
| `DATABASE_URL` | `./statup.db` | SQLite database file |
| `UPLOAD_DIR` | `data/uploads` | Where uploaded icons and the logo are kept |
| `HOST` | `0.0.0.0` | Listen address, an IP address |
| `PORT` | `3000` | Listen port |
| `SESSION_EXPIRY` | `3600` | Seconds a sign-in form stays valid. Once signed in, a session lasts 30 days without a visit with "Stay signed in", 24 hours otherwise |
| `DB_MAX_CONNECTIONS` | `10` | Database pool size |
| `LOG_LEVEL` | `info` | `trace`, `debug`, `info`, `warn`, `error` or `off` |
| `RUST_LOG` | unset | Finer log filter, e.g. `statup=debug,tower_http=info`. Replaces `LOG_LEVEL` when set |

## Automatic checks

A service gets a check from its form, under **Automatic check**: **Web** for an address that should answer (`https://intranet.example.com`), **Port** for a host and its port (`192.168.1.1:443`), such as a server, or a router or firewall by its administration port. **Test** runs the check at once and says why it fails: port closed, no answer, unknown name, certificate refused or a server error.

- Statup checks every minute, from the server it runs on, so internal addresses work. A web address that answers with any status below 500 counts as answering, even a refusal such as 401 or 404. Tick **Internal certificate** for a tool whose certificate no public authority signed.
- After three failed checks in a row, the service shows an outage, marked "detected automatically", and comes back as soon as it answers. Statup never writes an incident itself: the team declares one in a click, from the banner or the Services page, with the service and the start already filled in.
- Nothing is declared during a maintenance that takes the service down, nor when every checked service fails at once, which points at Statup's own connection rather than at the services. If that lasts five minutes, the checks count again.
- A detected outage of fifteen minutes or more counts in the service's thirty days. Choosing **None** or changing the address of a service in a false outage clears it, from its state and from the thirty days.
- The checks only reach the addresses you set, never read a page's content, and keep no credentials: an address with a user name or a password is refused.

## Running behind a reverse proxy

Terminate TLS at the proxy, publish Statup on loopback only (`"127.0.0.1:3000:3000"` under `ports:`), so nobody reaches it around the proxy, then set:

```bash
PUBLIC_URL=https://status.example.com
TRUST_PROXY_HEADERS=true
```

Statup then takes the client address from the last entry of `X-Forwarded-For`: the one your proxy appended, whatever a client sent before it. nginx, Caddy, Traefik, Apache and HAProxy (with `option forwardfor`) all append it. If your proxy writes the address in another header, name that header in `CLIENT_IP_HEADER`.

With nginx:

```nginx
location / {
    proxy_pass http://127.0.0.1:3000;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
}
```

With Caddy, which sets both headers on its own:

```caddy
status.example.com {
    reverse_proxy 127.0.0.1:3000
}
```

Never enable `TRUST_PROXY_HEADERS` when Statup can be reached directly: anyone could then pick their own address and walk around the rate limits.

## Backups

Everything lives in the SQLite database (with its `-wal` and `-shm` files) and the uploads directory. With Docker, both are in the `statup_data` volume, which Compose names after the project folder: `statup_statup_data` for a clone called `statup`.

```bash
docker compose stop statup
docker run --rm -v statup_statup_data:/data -v "$PWD":/backup alpine \
  tar czf /backup/statup-backup.tar.gz -C /data .
docker compose start statup
```

From source, stop the server and copy `statup.db*` and `data/uploads/`.

## Upgrading

```bash
docker compose pull
docker compose up -d
```

`latest` follows each new version. To upgrade only when you choose, replace `latest` with a version number, such as `0.1.0`, under `image:` in `docker-compose.yml`; the versions are listed in the [changelog](CHANGELOG.md).

Database migrations are compiled into the binary and run at start. Back up first: a database migrated by a newer version is refused by an older one.

## Forgotten password

Give the account a temporary password from the server:

```bash
docker compose exec statup /app/statup reset-password you@example.com
# From source, next to your .env: ./target/release/statup reset-password you@example.com
```

Hand it over within seven days. The person signs in with it and is asked to choose their own, and any session still open on that account is signed out. Only active accounts can be reset, and the command never creates a database: a mistyped `DATABASE_URL` is reported as such.

## Health check

`GET /health` answers `{"status":"ok"}` with 200, or `{"status":"degraded"}` with 503 when the database does not respond. It is not rate limited and opens no session.
