---
title: Upgrading
description: "Move to a newer version of Statup, or stay on one until you choose."
---

## Upgrade

[Back up](backups.md) first, then:

```bash
docker compose pull
docker compose up -d
```

The database is migrated when the new version starts: the migrations are compiled into the binary. A database migrated by a newer version is refused by an older one, so going back means restoring the backup.

Administrators see the running version in Settings, and the newer one once it is published, unless [`UPDATE_CHECK`](configuration.md#update_check) is `false`.

## Stay on a version

`latest` follows each new version. To upgrade only when you choose, replace `latest` with a version number under `image:` in `docker-compose.yml`:

```yaml
image: ghcr.io/karl-cta/statup:0.10.0
```

Every version and what it changed are in the [changelog](https://github.com/karl-cta/statup/blob/main/.github/CHANGELOG.md). Statup is before its 1.0: expect changes between versions, and back up before each upgrade.
