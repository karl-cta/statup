---
title: Montée de version
description: "Passer à une version plus récente de Statup, ou rester sur une version jusqu'à ce que vous décidiez d'en changer."
---

## Mettre à jour

[Sauvegardez](backups.md) d'abord, puis :

```bash
docker compose pull
docker compose up -d
```

La base de données est migrée au démarrage de la nouvelle version : les migrations sont compilées dans le binaire. Une base migrée par une version plus récente est refusée par une version plus ancienne : revenir en arrière implique donc de restaurer la sauvegarde.

Les administrateurs voient la version en service dans les Paramètres, et la plus récente dès qu'elle est publiée, sauf si [`UPDATE_CHECK`](configuration.md#update_check) vaut `false`.

## Rester sur une version

`latest` suit chaque nouvelle version. Pour ne mettre à jour que quand vous le décidez, remplacez `latest` par un numéro de version sous `image:` dans `docker-compose.yml` :

```yaml
image: ghcr.io/karl-cta/statup:0.10.0
```

Chaque version et ce qu'elle a changé figurent dans les [notes de version](https://github.com/karl-cta/statup/blob/main/.github/CHANGELOG.md). Statup n'a pas encore atteint sa 1.0 : attendez-vous à des changements d'une version à l'autre, et sauvegardez avant chaque mise à jour.
