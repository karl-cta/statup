---
title: Montée de version
description: "Passer à une version plus récente de Statup, ou rester sur une version jusqu'à ce que vous décidiez d'en changer."
---

## Mettre à jour

Statup sauvegarde la base avant qu'une mise à jour la modifie, sauf si **Sauvegarder avant chaque mise à jour** est désactivé dans **Paramètres › Sauvegardes**. Le fichier, `statup-before-<version>-<date>.db`, est écrit dans [`BACKUP_DIR`](configuration.md#backup_dir), et les trois derniers sont gardés. Si cette sauvegarde ne peut pas être écrite, la mise à jour continue et le serveur consigne l'échec. Pour une copie qui ne soit pas sur le même disque, voir [Garder une copie ailleurs](backups.md#keep-a-copy-elsewhere).

Puis :

```bash
docker compose pull
docker compose up -d
```

La base de données est migrée au démarrage de la nouvelle version : les migrations sont compilées dans le binaire. Une base migrée par une version plus récente est refusée par une version plus ancienne : revenir en arrière implique donc de [restaurer](backups.md#restore-a-backup) la sauvegarde faite avant la mise à jour.

Les administrateurs voient la version en service dans les Paramètres, et la plus récente dès qu'elle est publiée, sauf si [`UPDATE_CHECK`](configuration.md#update_check) vaut `false`.

## Rester sur une version

`latest` suit chaque nouvelle version. Pour ne mettre à jour que quand vous le décidez, remplacez `latest` par un numéro de version sous `image:` dans `docker-compose.yml` :

```yaml
image: ghcr.io/karl-cta/statup:0.11.0
```

Chaque version et ce qu'elle a changé figurent dans les [notes de version](https://github.com/karl-cta/statup/blob/main/.github/CHANGELOG.md). Statup n'a pas encore atteint sa 1.0 : attendez-vous à des changements d'une version à l'autre, et gardez une [copie des sauvegardes](backups.md#keep-a-copy-elsewhere) hors du serveur.

## Mettre à jour sous Linux

Relancez la commande d'installation : le script met Statup à jour et garde ses réglages. Voir [Installer sur Linux](install-linux.md#upgrade).

## Mettre à jour sous Windows Server

Relancez le script d'installation : il met le service à jour et garde ses réglages. Voir [Installer sur Windows Server](install-windows.md#upgrade).
