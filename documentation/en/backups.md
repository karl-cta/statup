---
title: Backups
description: "How Statup backs itself up, how to download a backup, and how to bring an instance back on the same server or on another."
---

A backup is one SQLite file. Statup writes it while it runs, so nothing has to be stopped. Backups are made on a schedule, before an upgrade, on demand from Settings, and on the [command line](command-line.md#backup).

## What a backup contains

The accounts, the events, the services, the settings, the [Journal](journal.md), the logo, the uploaded icons, and the addresses of the alert channels. Anyone who knows a webhook address can post in its channel: keep the backups where you keep other secrets.

## Automatic backups

Out of the box, Statup backs up every day at 03:00, in the instance time zone, keeps the latest seven, and backs up once more before each upgrade that changes the database. **Settings › Backups** shows the schedule beside its title; **Change** opens the settings:

| Field | Description |
|---|---|
| **Schedule** | **Every day**, **Every week** or **Never**. **Never** turns the automatic backups off. |
| **Day** | The day of the week, for **Every week**. |
| **Time** | The time of day, such as `03:00` or `22h15`, in the [time zone](settings.md#general) of the instance. |
| **Keep** | How many automatic backups stay on the server, from 1 to 90. The oldest are removed as new ones are written. |
| **Back up before each upgrade** | Backs up the database when a new version has changes to apply to it, before it applies them. On by default, and independent of the schedule. |

- A backup that was due while Statup was stopped is taken when Statup starts.
- The backups made before an upgrade are kept three at a time. A backup made by hand and the one made before a restore are never removed by Statup.
- When an automatic backup fails, the **Backups** page says so, the **Backups** entry of [Settings](settings.md) shows the failure, and the [Journal](journal.md) records it. A backup before an upgrade that fails is logged by the server and does not stop the upgrade.

The files are written to [`BACKUP_DIR`](configuration.md#backup_dir), with a name that holds the kind of backup and its date in UTC:

| Name | Made |
|---|---|
| `statup-auto-<date>.db` | On the schedule. |
| `statup-before-<version>-<date>.db` | Before an upgrade to that version. |
| `statup-backup-<date>.db` | By hand, on the command line. |
| `statup-before-restore-<date>.db` | Before a restore, from the data it replaces. |

## Download a backup

The **On the server** list of the **Backups** page shows the backups in the folder, with their date, their kind and their size, and the folder at the foot. **Download** sends one to your computer.

**Download a backup** makes a new one on the spot, as a file named `statup-backup-<date>.db`. It is sent to your computer and not kept on the server. Each download is recorded in the [Journal](journal.md).

## Command line

`statup backup` writes a backup and `statup restore` restores one, on the server and without the web app. See [`backup`](command-line.md#backup) and [`restore`](command-line.md#restore).

## Restore a backup

A backup made by an older Statup is brought up to date when it is restored. One made by a newer Statup is refused: upgrade Statup first. When the file is not a Statup backup, nothing changes.

### On a new server

An instance with no account yet offers **Restore a backup** on its first screen. Pick one of the backups found in [`BACKUP_DIR`](configuration.md#backup_dir), or **A file from this computer**, up to 256 MB. Then sign in with an account of the backup.

A backup copied into `BACKUP_DIR` under the name Statup gave it appears in the list. Once an account exists, this screen is gone: a running instance restores on the command line.

### On a running instance

Stop Statup, then run [`statup restore`](command-line.md#restore) with the backup. The data it replaces are first copied to `statup-before-restore-<date>.db`. Start Statup again, and sign in with an account of the backup.

## Keep a copy elsewhere

The backups sit on the disk that holds Statup: a lost disk loses them too. Copy them to another machine, or point [`BACKUP_DIR`](configuration.md#backup_dir) to a folder on a mounted share.

With Docker, the folder is `/data/backups` in the `statup_data` volume. To copy it out, with `statup` the service of your `docker-compose.yml`:

```bash
docker compose cp statup:/data/backups ./backups
```

A copy of the whole volume while Statup is stopped also works.
