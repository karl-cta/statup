---
title: NixOS
description: "Lancer Statup comme service NixOS depuis le flake de son dépôt."
---

Le dépôt contient un flake avec un paquet et un module NixOS, proposé par la communauté et vérifié par le projet à chaque changement : les contrôles construisent Statup avec Nix, démarrent une machine NixOS avec le service et lancent les commandes de cette page. Docker reste la voie principale pour [installer](install.md) Statup.

## Ajouter le flake

Dans le `flake.nix` de votre système, ajoutez Statup en entrée et son module à votre machine :

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

Statup est construit avec le Rust et le Tailwind CSS de votre nixpkgs : il lui faut Rust 1.94 ou plus récent.

## Activer le service

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

Lancez ensuite `sudo nixos-rebuild switch`. Statup répond sur le port 3000 et guide le [premier lancement](install.md#first-launch).

### `enable`

- Par défaut : `false`
- Valeurs : `true`, `false`

Lance Statup comme service système `statup`, sous l'utilisateur `statup`.

### `settings`

- Par défaut : `{ }`, avec `HOST` à `"0.0.0.0"` et `PORT` à `3000`
- Valeurs : les [réglages](configuration.md) de Statup, en chaînes, nombres ou booléens

Transmis à Statup en variables d'environnement. Le module règle lui-même `DATABASE_URL` et `UPLOAD_DIR` : voir [Données](nixos.md#data).

### `environmentFile`

- Par défaut : `null`
- Valeurs : le chemin d'un fichier de lignes `NOM=valeur`

Lu par systemd au démarrage du service, hors du store Nix : la place de [`SMTP_PASSWORD`](configuration.md#smtp_password), de [`ADMIN_PASSWORD`](configuration.md#admin_password) et des autres secrets. Un réglage de ce fichier l'emporte sur le même dans `settings`.

### `openFirewall`

- Par défaut : `false`
- Valeurs : `true`, `false`

Ouvre `PORT` dans le pare-feu. Laissez-le fermé derrière un [reverse proxy](reverse-proxy.md) sur la même machine.

### `package`

- Par défaut : le paquet du flake
- Valeurs : un paquet

Le Statup que lance le service.

## Données

La base de données, les fichiers envoyés et les sauvegardes vivent dans `/var/lib/statup`, qui appartient à l'utilisateur `statup`. Les [sauvegardes automatiques](backups.md#automatic-backups) et celles d'avant une mise à jour vont dans `/var/lib/statup/backups`, et **Paramètres › Sauvegardes** les télécharge. Un [`BACKUP_DIR`](configuration.md#backup_dir) hors de `/var/lib/statup` ne peut pas être écrit par le service.

## Mettre à jour

Statup sauvegarde la base lui-même avant qu'une mise à jour la modifie, dans `/var/lib/statup/backups`. Mettez l'entrée à jour et reconstruisez :

```bash
nix flake update statup
sudo nixos-rebuild switch
```

La base de données est migrée au démarrage de la nouvelle version. Pour rester sur une version, pointez l'entrée sur son étiquette, comme `github:karl-cta/statup/v0.7.1`.

## Réinitialiser un mot de passe

```bash
sudo -u statup env DATABASE_URL=/var/lib/statup/statup.db statup reset-password you@example.com
```

La page [Ligne de commande](command-line.md#reset-password) dit ce qu'elle fait.

## Logs

`journalctl -u statup` les lit. [`LOG_LEVEL`](configuration.md#log_level) règle leur niveau de détail.
