---
title: Upgrading
description: "Move to a newer version of Statup, or stay on one until you choose."
---

## Upgrade

Statup backs up the database before an upgrade changes it, unless **Back up before each upgrade** is turned off in **Settings › Backups**. The file, `statup-before-<version>-<date>.db`, goes to [`BACKUP_DIR`](configuration.md#backup_dir), and the three latest are kept. If that backup cannot be written, the upgrade goes on and the server logs the failure. For a copy that does not sit on the same disk, see [Keep a copy elsewhere](backups.md#keep-a-copy-elsewhere).

Then:

```bash
docker compose pull
docker compose up -d
```

The database is migrated when the new version starts: the migrations are compiled into the binary. A database migrated by a newer version is refused by an older one, so going back means [restoring](backups.md#restore-a-backup) the backup made before the upgrade.

Administrators see the running version in Settings, and the newer one once it is published, unless [`UPDATE_CHECK`](configuration.md#update_check) is `false`.

## Stay on a version

`latest` follows each new version. To upgrade only when you choose, replace `latest` with a version number under `image:` in `docker-compose.yml`:

```yaml
image: ghcr.io/karl-cta/statup:0.11.0
```

Every version and what it changed are in the [changelog](https://github.com/karl-cta/statup/blob/main/.github/CHANGELOG.md). Statup is before its 1.0: expect changes between versions, and keep a [copy of the backups](backups.md#keep-a-copy-elsewhere) away from the server.

## Upgrade on Linux

Run the install command again: the script updates Statup and keeps its settings. See [Install on Linux](install-linux.md#upgrade).

## Upgrade on Windows Server

Run the install script again: it updates the service and keeps its settings. See [Install on Windows Server](install-windows.md#upgrade).
