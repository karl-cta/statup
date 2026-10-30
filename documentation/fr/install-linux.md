---
title: Installer sur Linux
description: "Installer Statup comme service systemd sur un serveur Linux sans Docker, avec une seule commande qui le met aussi à jour et le désinstalle."
---

Un script installe Statup comme service systemd sur toute distribution Linux avec systemd, x86-64 ou ARM (aarch64). Docker reste la voie principale pour [installer](install.md) Statup.

## Prérequis

- systemd, et une machine 64 bits : `x86_64` ou `aarch64`.
- `root`, par `sudo` par exemple.
- `curl` ou `wget`.

## Installer

```bash
curl -fsSL https://github.com/karl-cta/statup/releases/latest/download/install.sh | sudo sh
```

> [!IMPORTANT]
> Avec `--yes` et un pare-feu actif, ufw ou firewalld, le script ouvre le port sans demander. Ajoutez `--no-firewall` pour laisser le pare-feu tel qu'il est. Rien n'est ouvert quand Statup n'écoute que sur cette machine.

Le script pose quelques questions, une à une, puis télécharge Statup, vérifie sa somme de contrôle, l'installe et attend qu'il réponde. Il se termine sur l'adresse de l'instance. Ouvrez-la pour créer l'administrateur : voir [Premier lancement](install.md#first-launch).

## Questions

Appuyez sur Entrée pour prendre la valeur entre crochets. Une option donnée sur la ligne de commande répond à sa question, qui n'est alors plus posée.

| Question | Option | Par défaut |
|---|---|---|
| Dossier du programme | `--prefix <dossier>` | `/opt/statup` |
| Dossier des données | `--data-dir <dossier>` | `/var/lib/statup` |
| Dossier des sauvegardes | `--backup-dir <dossier>` | `backups` dans le dossier des données |
| Port | `--port <port>` | `3000` |
| Écouter sur toutes les adresses, ou sur cette machine seulement | `--listen <valeur>` | `all` |
| Compte du service | `--user <nom>` | `statup` |
| Ouvrir le port dans le pare-feu | `--firewall`, `--no-firewall` | oui |

- `--listen` prend `all` (toutes les adresses), `local` (cette machine seulement, pour un [reverse proxy](reverse-proxy.md) installé dessus) ou une adresse IP.
- La question du pare-feu n'est posée que si ufw ou firewalld est actif et que Statup n'écoute pas sur cette machine seulement.
- Un dossier est un chemin complet de lettres, de chiffres, de points, de tirets et de tirets bas, qui n'est pas un dossier système comme `/opt` ou `/var/lib`.
- Les questions demandent un terminal. Sans terminal, le script s'arrête et demande `--yes`.
- Quand `/etc/statup/statup.env` existe, d'une installation précédente qui a été retirée, ses réglages sont gardés et seuls le dossier du programme et le compte sont demandés.

## Autres options

| Option | Effet |
|---|---|
| `--version <x.y.z>` | La version à installer. Par défaut, la dernière. |
| `--archive <fichier>` | Installe depuis une archive déjà téléchargée. |
| `--mirror <url>` | Télécharge depuis une copie des fichiers de la version. Demande `--version`. |
| `--yes` | Prend la valeur par défaut de chaque question sans réponse, et confirme `--uninstall`. |
| `--uninstall` | Retire Statup et garde ses données, ses réglages et son compte. |
| `--help` | Affiche la liste des options. |

Voir [Sans accès à GitHub](install-linux.md#without-access-to-github) pour `--archive` et `--mirror`.

## Installer sans questions

Pour Ansible ou un script, donnez `--yes` et les options dont vous avez besoin :

```bash
curl -fsSL https://github.com/karl-cta/statup/releases/latest/download/install.sh | sudo sh -s -- --yes --port 8080 --listen local
```

## Ce que le script installe

| Élément | Où |
|---|---|
| Programme | `/opt/statup` : `statup`, `static/`, les fichiers de licence et un fichier `VERSION`, qui appartiennent à `root`. |
| Données | `/var/lib/statup` : la base de données, `uploads/` et les sauvegardes, qui appartiennent au compte du service, en mode `750`. |
| Compte | `statup`, un compte système sans connexion, créé s'il n'existe pas. |
| Réglages | `/etc/statup/statup.env`, qui appartient à `root` et que seul le compte du service peut lire. |
| Service | `/etc/systemd/system/statup.service`, activé et démarré. |
| Commande | `/usr/local/bin/statup`, pour lancer les [commandes](command-line.md) de Statup. |

Le service démarre après le réseau, redémarre après un échec et se limite à ce dont il a besoin : aucun nouveau privilège, un `/tmp` privé, le système de fichiers en lecture seule sauf les dossiers des données et des sauvegardes, et les dossiers personnels masqués, ou en lecture seule quand l'un des deux dossiers est sous `/home`.

Les logs vont dans le journal : `journalctl -u statup`.

## Changer un réglage

`/etc/statup/statup.env` contient `HOST`, `PORT`, `DATABASE_URL`, `UPLOAD_DIR` et `BACKUP_DIR`. Ajoutez-y tout autre [réglage](configuration.md), un `NOM=valeur` par ligne, puis redémarrez :

```bash
sudo nano /etc/statup/statup.env
sudo systemctl restart statup
```

- La règle de pare-feu ouverte à l'installation reste sur l'ancien port après un changement de `PORT`.
- Gardez `DATABASE_URL` à l'état de simple chemin : une [mise à jour](install-linux.md#upgrade) en déduit le dossier des données, et un chemin avec des paramètres, comme `?mode=rwc`, la tromperait.
- Le fichier d'unité est réécrit à chaque mise à jour : les réglages ont leur place dans `statup.env`.

## Commandes de maintenance

`statup` lance les [commandes](command-line.md) sous le compte du service, avec ses réglages. Utilisez `sudo` :

```bash
sudo statup backup
sudo statup reset-password you@example.com
```

[`backup`](command-line.md#backup) écrit dans le dossier des sauvegardes. Avec `--output`, le fichier doit être dans le dossier des données ou celui des sauvegardes : le compte du service ne peut pas écrire ailleurs, et la commande échoue.

Pour [restaurer](command-line.md#restore), arrêtez Statup. La commande lit le fichier sous le compte du service, donc un fichier de votre propre dossier lui est inaccessible : copiez-le d'abord dans le dossier des sauvegardes.

```bash
sudo systemctl stop statup
sudo cp statup-backup-2026-10-29-163045.db /var/lib/statup/backups/
sudo chown statup: /var/lib/statup/backups/statup-backup-2026-10-29-163045.db
sudo statup restore /var/lib/statup/backups/statup-backup-2026-10-29-163045.db
sudo systemctl start statup
```

Remplacez `statup` dans `chown` par votre compte si vous en avez choisi un autre. Plus de détails dans [Sauvegardes](backups.md).

## Mettre à jour

Relancez la commande d'installation. Le script repère le Statup installé et le met à jour : il garde les réglages, remplace le programme et redémarre le service. Les options d'une nouvelle installation sont ignorées, et il le dit. Pour choisir la version, ajoutez `--version`.

```bash
curl -fsSL https://github.com/karl-cta/statup/releases/latest/download/install.sh | sudo sh
```

- Statup sauvegarde la base avant qu'une mise à jour la modifie : voir [Montée de version](upgrading.md).
- Une version plus ancienne que celle installée est refusée sauf confirmation, car sa base peut être trop récente pour elle. Avec `--yes`, elle est refusée.
- Le script attend jusqu'à 30 secondes que Statup réponde. Une longue mise à jour, comme la sauvegarde d'une grosse base, peut se terminer sur « ne répond pas » alors que Statup démarre encore : lisez `journalctl -u statup`.

## Désinstaller

```bash
curl -fsSL https://github.com/karl-cta/statup/releases/latest/download/install.sh | sudo sh -s -- --uninstall
```

Le script demande d'abord confirmation ; `--yes` supprime la question. Il arrête et retire le service, la commande `statup` et les fichiers du programme. Il garde les dossiers des données et des sauvegardes, `/etc/statup/statup.env`, le compte et toute règle de pare-feu. Une nouvelle installation retrouve les réglages et les garde.

## Sans accès à GitHub

Téléchargez `install.sh`, l'archive de votre machine et `SHA256SUMS` depuis la [page des versions](https://github.com/karl-cta/statup/releases) sur un autre ordinateur, puis copiez-les. L'archive s'appelle `statup-<version>-linux-x86_64.tar.gz` ou `statup-<version>-linux-aarch64.tar.gz`, selon ce que dit `uname -m`.

Le script ne vérifie pas une archive qu'on lui donne. Vérifiez-la vous-même, puis installez-la :

```bash
sha256sum -c SHA256SUMS --ignore-missing
sudo sh install.sh --archive statup-0.11.0-linux-x86_64.tar.gz
```

Pour servir les fichiers depuis votre propre serveur, copiez l'archive et `SHA256SUMS` sous `<url>/v<version>/`, et `install.sh` là où vous rangez vos scripts. Le script les télécharge de là et refuse une archive dont la somme de contrôle diffère.

```bash
sudo sh install.sh --mirror https://files.example.com/statup --version 0.11.0
```
