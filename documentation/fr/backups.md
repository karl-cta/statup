---
title: Sauvegardes
description: "Comment Statup se sauvegarde, comment télécharger une sauvegarde, et comment remettre une instance en route sur le même serveur ou sur un autre."
---

Une sauvegarde est un seul fichier SQLite. Statup l'écrit en tournant, sans rien arrêter. Les sauvegardes sont faites selon un horaire, avant une mise à jour, à la demande depuis les Paramètres, et en [ligne de commande](command-line.md#backup).

## Ce que contient une sauvegarde

Les comptes, les événements, les services, les paramètres, le [Journal](journal.md), le logo, les icônes envoyées, et les adresses des canaux d'alerte. Quiconque connaît l'adresse d'un webhook peut publier dans son canal : rangez les sauvegardes là où vous gardez vos autres secrets.

## Sauvegardes automatiques

Par défaut, Statup se sauvegarde chaque jour à 03:00, dans le fuseau horaire de l'instance, garde les sept dernières, et se sauvegarde une fois de plus avant chaque mise à jour qui modifie la base. **Paramètres › Sauvegardes** indique le rythme à côté de son titre ; **Modifier** ouvre les réglages :

| Champ | Description |
|---|---|
| **Rythme** | **Chaque jour**, **Chaque semaine** ou **Jamais**. **Jamais** arrête les sauvegardes automatiques. |
| **Jour** | Le jour de la semaine, pour **Chaque semaine**. |
| **Heure** | L'heure de la journée, comme `03:00` ou `22h15`, dans le [fuseau horaire](settings.md#general) de l'instance. |
| **Garder** | Combien de sauvegardes automatiques restent sur le serveur, de 1 à 90. Les plus anciennes sont supprimées à mesure que de nouvelles sont écrites. |
| **Sauvegarder avant chaque mise à jour** | Sauvegarde la base quand une nouvelle version a des changements à lui appliquer, avant de les appliquer. Activé par défaut, et indépendant du rythme. |

- Une sauvegarde qui était due pendant que Statup était arrêté est faite au démarrage.
- Les sauvegardes d'avant une mise à jour sont gardées trois à la fois. Une sauvegarde faite à la main et celle d'avant une restauration ne sont jamais supprimées par Statup.
- Quand une sauvegarde automatique échoue, la page **Sauvegardes** le dit, l'entrée **Sauvegardes** des [Paramètres](settings.md) montre l'échec, et le [Journal](journal.md) l'enregistre. L'échec d'une sauvegarde d'avant une mise à jour est consigné par le serveur et n'empêche pas la mise à jour.

Les fichiers sont écrits dans [`BACKUP_DIR`](configuration.md#backup_dir), sous un nom qui porte le type de sauvegarde et sa date en UTC :

| Nom | Faite |
|---|---|
| `statup-auto-<date>.db` | Selon l'horaire. |
| `statup-before-<version>-<date>.db` | Avant une mise à jour vers cette version. |
| `statup-backup-<date>.db` | À la main, en ligne de commande. |
| `statup-before-restore-<date>.db` | Avant une restauration, avec les données qu'elle remplace. |

## Télécharger une sauvegarde

La liste **Sur le serveur** de la page **Sauvegardes** montre les sauvegardes du dossier, avec leur date, leur type et leur taille, et le dossier en bas. **Télécharger** en envoie une sur votre ordinateur.

**Télécharger une sauvegarde** en fait une nouvelle sur le moment, dans un fichier nommé `statup-backup-<date>.db`. Elle est envoyée sur votre ordinateur et n'est pas conservée sur le serveur. Chaque téléchargement est enregistré dans le [Journal](journal.md).

## Ligne de commande

`statup backup` écrit une sauvegarde et `statup restore` en restaure une, sur le serveur et sans l'application web. Voir [`backup`](command-line.md#backup) et [`restore`](command-line.md#restore).

## Restaurer une sauvegarde

Une sauvegarde faite par un Statup plus ancien est mise à niveau quand elle est restaurée. Celle d'un Statup plus récent est refusée : mettez d'abord Statup à jour. Quand le fichier n'est pas une sauvegarde de Statup, rien ne change.

### Sur un nouveau serveur

Une instance sans compte propose **Restaurer une sauvegarde** sur son premier écran. Choisissez l'une des sauvegardes trouvées dans [`BACKUP_DIR`](configuration.md#backup_dir), ou **Un fichier de cet ordinateur**, jusqu'à 256 Mo. Connectez-vous ensuite avec un compte de la sauvegarde.

Une sauvegarde copiée dans `BACKUP_DIR` sous le nom que Statup lui a donné apparaît dans la liste. Dès qu'un compte existe, cet écran disparaît : une instance en service se restaure en ligne de commande.

### Sur une instance en service

Arrêtez Statup, puis lancez [`statup restore`](command-line.md#restore) avec la sauvegarde. Les données remplacées sont d'abord copiées dans `statup-before-restore-<date>.db`. Relancez Statup, puis connectez-vous avec un compte de la sauvegarde.

## Garder une copie ailleurs

Les sauvegardes sont sur le disque qui porte Statup : un disque perdu les emporte aussi. Copiez-les sur une autre machine, ou faites pointer [`BACKUP_DIR`](configuration.md#backup_dir) vers un dossier d'un partage monté.

Avec Docker, le dossier est `/data/backups` dans le volume `statup_data`. Pour le copier hors du conteneur, `statup` étant le service de votre `docker-compose.yml` :

```bash
docker compose cp statup:/data/backups ./backups
```

Une copie du volume entier, Statup étant arrêté, convient aussi.
