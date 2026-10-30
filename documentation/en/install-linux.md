---
title: Install on Linux
description: "Install Statup as a systemd service on a Linux server without Docker, with one command that also upgrades and removes it."
---

One script installs Statup as a systemd service on any Linux distribution with systemd, x86-64 or ARM (aarch64). Docker stays the main way to [install](install.md) Statup.

## Requirements

- systemd, and a 64-bit machine: `x86_64` or `aarch64`.
- `root`, through `sudo` for example.
- `curl` or `wget`.

## Install

```bash
curl -fsSL https://github.com/karl-cta/statup/releases/latest/download/install.sh | sudo sh
```

> [!IMPORTANT]
> With `--yes` and an active firewall, ufw or firewalld, the script opens the port without asking. Add `--no-firewall` to leave the firewall as it is. Nothing is opened when Statup listens on this machine only.

The script asks a few questions, one at a time, then downloads Statup, checks its checksum, installs it and waits until it answers. It ends with the address of the instance. Open it to create the administrator: see [First launch](install.md#first-launch).

## Questions

Press Enter to take the value in brackets. An option given on the command line answers its question, which is then not asked.

| Question | Option | Default |
|---|---|---|
| Program folder | `--prefix <dir>` | `/opt/statup` |
| Data folder | `--data-dir <dir>` | `/var/lib/statup` |
| Backup folder | `--backup-dir <dir>` | `backups` in the data folder |
| Port | `--port <port>` | `3000` |
| Listen on every address, or on this machine only | `--listen <value>` | `all` |
| Account of the service | `--user <name>` | `statup` |
| Open the port in the firewall | `--firewall`, `--no-firewall` | yes |

- `--listen` takes `all` (every address), `local` (this machine only, for a [reverse proxy](reverse-proxy.md) on it) or an IP address.
- The firewall question is asked only when ufw or firewalld is active and Statup does not listen on this machine only.
- A folder is a full path of letters, digits, dots, dashes and underscores, and not a system folder such as `/opt` or `/var/lib`.
- The questions need a terminal. Without one, the script stops and asks for `--yes`.
- When `/etc/statup/statup.env` exists, from an earlier install that was removed, its settings are kept and only the program folder and the account are asked.

## Other options

| Option | Effect |
|---|---|
| `--version <x.y.z>` | The version to install. By default, the latest. |
| `--archive <file>` | Installs from an archive already downloaded. |
| `--mirror <url>` | Downloads from a copy of the release files. Needs `--version`. |
| `--yes` | Takes the default of every question not answered, and confirms `--uninstall`. |
| `--uninstall` | Removes Statup and keeps its data, settings and account. |
| `--help` | Shows the list of options. |

See [Without access to GitHub](install-linux.md#without-access-to-github) for `--archive` and `--mirror`.

## Install without questions

For Ansible or a script, give `--yes` and the options you need:

```bash
curl -fsSL https://github.com/karl-cta/statup/releases/latest/download/install.sh | sudo sh -s -- --yes --port 8080 --listen local
```

## What the script installs

| Item | Where |
|---|---|
| Program | `/opt/statup`: `statup`, `static/`, the license files and a `VERSION` file, owned by `root`. |
| Data | `/var/lib/statup`: the database, `uploads/` and the backups, owned by the service account, mode `750`. |
| Account | `statup`, a system account without login, created when it does not exist. |
| Settings | `/etc/statup/statup.env`, owned by `root` and readable by the service account only. |
| Service | `/etc/systemd/system/statup.service`, enabled and started. |
| Command | `/usr/local/bin/statup`, to run the [commands](command-line.md) of Statup. |

The service starts after the network, restarts after a failure and is limited to what it needs: of the powers of `root`, only the one to listen on a port below 1024, no new privileges, a private `/tmp`, the file system read-only except the data and backup folders, and the home folders hidden, or read-only when one of the two folders is under `/home`.

Logs go to the journal: `journalctl -u statup`.

## Change a setting

`/etc/statup/statup.env` holds `HOST`, `PORT`, `DATABASE_URL`, `UPLOAD_DIR` and `BACKUP_DIR`. Add any other [setting](configuration.md) to it, one `NAME=value` per line, then restart:

```bash
sudo nano /etc/statup/statup.env
sudo systemctl restart statup
```

- After a change of `DATABASE_URL`, `UPLOAD_DIR` or `BACKUP_DIR`, run the [install command](install-linux.md#upgrade) again: it creates the new folder and lets the service write to it, which a restart alone does not.
- The firewall rule opened at install stays on the old port after a change of `PORT`.
- Keep `DATABASE_URL` a plain path: an [upgrade](install-linux.md#upgrade) finds the data folder from it, and a path with parameters, such as `?mode=rwc`, would mislead it.
- The unit file is written again at each upgrade: settings belong in `statup.env`.

## Maintenance commands

`statup` runs the [commands](command-line.md) as the account of the service, with its settings. Use `sudo`:

```bash
sudo statup backup
sudo statup reset-password you@example.com
```

[`backup`](command-line.md#backup) writes to the backup folder. With `--output`, the file must be in the data folder or the backup folder: the service account cannot write elsewhere, and the command fails.

To [restore](command-line.md#restore), stop Statup. The command reads the file as the service account, so a file in your own folder is out of its reach: copy it to the backup folder first.

```bash
sudo systemctl stop statup
sudo cp statup-backup-2026-10-29-163045.db /var/lib/statup/backups/
sudo chown statup: /var/lib/statup/backups/statup-backup-2026-10-29-163045.db
sudo statup restore /var/lib/statup/backups/statup-backup-2026-10-29-163045.db
sudo systemctl start statup
```

Replace `statup` in `chown` with your account if you chose another. More on backups in [Backups](backups.md).

## Upgrade

Run the install command again. The script finds the installed Statup and updates it: it keeps the settings, replaces the program and restarts the service. The options of a new install are ignored, and it says so. To choose the version, add `--version`.

```bash
curl -fsSL https://github.com/karl-cta/statup/releases/latest/download/install.sh | sudo sh
```

- Statup backs up the database before an upgrade changes it: see [Upgrading](upgrading.md).
- An older version than the installed one is refused unless you confirm, as its database may be too new for it. With `--yes`, it is refused.
- The script waits up to 30 seconds for Statup to answer. A long upgrade, such as the backup of a large database, can end on "does not answer" while Statup is still starting: read `journalctl -u statup`.

## Uninstall

```bash
curl -fsSL https://github.com/karl-cta/statup/releases/latest/download/install.sh | sudo sh -s -- --uninstall
```

The script asks first; `--yes` skips the question. It stops and removes the service, the `statup` command and the program files. It keeps the data and backup folders, `/etc/statup/statup.env`, the account and any firewall rule. Installing again finds the settings and keeps them.

## Without access to GitHub

Download `install.sh`, the archive of your machine and `SHA256SUMS` from the [release page](https://github.com/karl-cta/statup/releases) on another computer, and copy them over. The archive is named `statup-<version>-linux-x86_64.tar.gz` or `statup-<version>-linux-aarch64.tar.gz`, as `uname -m` tells.

The script does not check an archive it is given. Check it yourself, then install it:

```bash
sha256sum -c SHA256SUMS --ignore-missing
sudo sh install.sh --archive statup-0.11.0-linux-x86_64.tar.gz
```

To serve the files from your own server, copy the archive and `SHA256SUMS` under `<url>/v<version>/`, and `install.sh` wherever you keep scripts. The script downloads them from there and refuses an archive whose checksum differs.

```bash
sudo sh install.sh --mirror https://files.example.com/statup --version 0.11.0
```
