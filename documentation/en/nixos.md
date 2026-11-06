---
title: NixOS
description: "Run Statup as a NixOS service from the flake in its repository."
---

The repository holds a flake with a package and a NixOS module, contributed by the community and checked by the project on each change: the checks build Statup with Nix, boot a NixOS machine with the service and run the commands of this page. Docker stays the main way to [install](install.md) Statup.

## Add the flake

In the `flake.nix` of your system, add Statup as an input and its module to your machine:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    statup = {
      url = "github:karl-cta/statup";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, statup, ... }: {
    nixosConfigurations.server = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        statup.nixosModules.default
        ./configuration.nix
      ];
    };
  };
}
```

Statup is built with the Rust and the Tailwind CSS of your nixpkgs: it needs Rust 1.94 or newer.

## Enable the service

```nix
services.statup = {
  enable = true;
  openFirewall = true;
  settings = {
    PUBLIC_URL = "https://status.example.com";
    TZ = "Europe/Dublin";
  };
  environmentFile = "/run/secrets/statup.env";
};
```

Then run `sudo nixos-rebuild switch`. Statup answers on port 3000 and leads through the [first launch](install.md#first-launch).

### `enable`

- Default: `false`
- Values: `true`, `false`

Runs Statup as the `statup` system service, under the `statup` user.

### `settings`

- Default: `{ }`, with `HOST` at `"0.0.0.0"` and `PORT` at `3000`
- Values: the [settings](configuration.md) of Statup, as strings, numbers or booleans

Passed to Statup as environment variables. The module sets `DATABASE_URL` and `UPLOAD_DIR` itself: see [Data](nixos.md#data).

### `environmentFile`

- Default: `null`
- Values: the path of a file of `NAME=value` lines

Read by systemd when the service starts, out of the Nix store: the place for [`SMTP_PASSWORD`](configuration.md#smtp_password), [`ADMIN_PASSWORD`](configuration.md#admin_password) and other secrets. A setting in this file wins over the same one in `settings`.

### `openFirewall`

- Default: `false`
- Values: `true`, `false`

Opens `PORT` in the firewall. Leave it closed behind a [reverse proxy](reverse-proxy.md) on the same machine.

### `package`

- Default: the package of the flake
- Values: a package

The Statup that the service runs.

## Data

The database and the uploads live in `/var/lib/statup`, owned by the `statup` user. To [back up](backups.md), stop the service with `sudo systemctl stop statup`, copy `statup.db`, `statup.db-wal`, `statup.db-shm` and `uploads/`, then start it again.

## Upgrade

[Back up](backups.md) first, then update the input and rebuild:

```bash
nix flake update statup
sudo nixos-rebuild switch
```

The database is migrated when the new version starts. To stay on one version, point the input at its tag, such as `github:karl-cta/statup/v0.7.1`.

## Reset a password

```bash
sudo -u statup env DATABASE_URL=/var/lib/statup/statup.db statup reset-password you@example.com
```

The [command line](command-line.md#reset-password) page tells what it does.

## Logs

`journalctl -u statup` reads them. [`LOG_LEVEL`](configuration.md#log_level) sets how much they say.
