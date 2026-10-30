---
title: Installation
description: "Lancer Statup avec Docker Compose sur un serveur x86-64 ou ARM, puis créer le premier administrateur."
---

## Prérequis

- Docker avec Docker Compose 2.24 ou plus récent.
- Un serveur 64 bits, x86-64 ou ARM : l'image est publiée pour les deux.

Docker est la voie principale pour installer Statup. Sans lui, Statup tourne comme service sur [Linux](install-linux.md) et sur [Windows Server](install-windows.md), et depuis son flake sur [NixOS](nixos.md).

## Lancer Statup

```bash
mkdir statup && cd statup
curl -O https://raw.githubusercontent.com/karl-cta/statup/main/docker-compose.yml
docker compose up -d
```

Docker télécharge l'image `ghcr.io/karl-cta/statup:latest` et la lance. Statup répond sur le port 3000 du serveur.

## Ce que règle docker-compose.yml

| Réglage | Valeur |
|---|---|
| Image | `ghcr.io/karl-cta/statup:latest`, qui suit chaque nouvelle version. Pour rester sur une version, voir [Montée de version](upgrading.md). |
| Port | `3000:3000` : le port du serveur d'abord, puis celui du conteneur. |
| Volume | `statup_data`, monté sur `/data` : la base de données et les fichiers envoyés. Voir [Sauvegardes](backups.md). |
| Réglages | Un fichier `.env` à côté de `docker-compose.yml`, facultatif. Voir [Configuration](configuration.md). |
| Redémarrage | `unless-stopped` : Statup redémarre avec le serveur. |
| Privilèges | Aucun : `no-new-privileges`, toutes les capabilities retirées. Le conteneur tourne sous l'utilisateur `statup`. |
| Logs | Rotation à 10 Mo, trois fichiers conservés. `docker compose logs statup` les affiche. |

L'image vérifie son propre [état de santé](health-check.md) toutes les 30 secondes.

## Premier lancement

Ouvrez `http://localhost:3000`, ou l'adresse de votre serveur sur le port 3000. Une instance vide vous guide en quatre étapes, avec un aperçu de la page à côté de chacune.

1. **Compte** : **Nom**, **Adresse e-mail** et **Mot de passe**. Ce premier compte est celui de l'administrateur. L'instance reprend le fuseau horaire de votre navigateur.
2. **Page** : **Nom de la page** (facultatif, 40 caractères au plus), **Logo** (facultatif), et **Accès à la page** : **Membres seulement** ou **Publique**.
3. **Services** : « Vos services ». Cochez des services parmi les **Suggestions**, ou saisissez les vôtres, et ouvrez **Surveiller ce service** pour ajouter une vérification. Facultatif, 50 au plus en une fois.
4. **Prête** : l'adresse de la page, avec **Copier**, à partager avec ceux qui utilisent vos services. **Ouvrir le tableau de bord** termine la mise en route.

Pour remettre en route une instance depuis une sauvegarde, le premier écran propose aussi **Restaurer une sauvegarde** : voir [Sauvegardes](backups.md#on-a-new-server).

Tout se change ensuite dans les [Paramètres](settings.md), et d'autres services s'ajoutent depuis la page [Services](services.md).

> [!WARNING]
> Tant que le premier compte n'existe pas, n'importe qui ouvrant l'adresse peut le créer et devenir administrateur. Créez le vôtre dès le premier démarrage, ou définissez-le à l'avance comme ci-dessous.

### Définir l'administrateur à l'avance

Inscrivez [`ADMIN_EMAIL`](configuration.md#admin_email) et [`ADMIN_PASSWORD`](configuration.md#admin_password) dans `.env` avant le premier démarrage. Statup crée l'administrateur en démarrant, avec pour nom la partie de l'adresse qui précède le `@`. Une valeur qui ne respecte pas la [règle des mots de passe](team.md#password-rule) l'empêche de démarrer.

Connectez-vous ensuite sur `/login`. Les étapes de mise en route ne s'affichent pas : le tableau de bord présente à la place une carte **Premiers pas**, pour ajouter des services et partager l'adresse. Le fuseau horaire reste celui de [`TZ`](configuration.md#tz), ou UTC, tant qu'il n'est pas réglé dans les Paramètres. Retirez les deux valeurs de `.env` une fois le compte créé.

## Accéder à Statup depuis votre réseau

Vos collègues ouvrent l'adresse du serveur sur le port 3000. Pour utiliser un autre port, changez la partie avant `:3000` sous `ports:` dans `docker-compose.yml`, puis relancez `docker compose up -d`.

Pour le HTTPS et un nom à vous, comme `status.example.com`, placez Statup [derrière un reverse proxy](reverse-proxy.md).

## Pour continuer

- Ajouter vos collègues : [Équipe et rôles](team.md).
- Prévenir les gens là où ils lisent déjà : [Alertes](alerts.md).
- Laisser Statup repérer une panne avant tout le monde : [Surveillance](monitoring.md).
