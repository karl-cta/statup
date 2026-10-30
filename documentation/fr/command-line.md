---
title: Ligne de commande
description: "Les commandes que le binaire Statup exécute sur le serveur, à côté de l'application web."
---

Lancé sans argument, le binaire démarre le serveur. Il existe trois commandes de maintenance, et sous Windows une quatrième le fait tourner comme service.

Après `install.sh`, lancez-les avec `sudo statup <commande>` : voir [Installer sur Linux](install-linux.md#maintenance-commands). Après `install.ps1`, lancez `statup.exe --env <fichier> <commande>` dans PowerShell en administrateur : voir [Installer sur Windows Server](install-windows.md#maintenance-commands).

## `reset-password`

Donne un mot de passe provisoire à un compte, et l'affiche.

```bash
docker compose exec statup /app/statup reset-password you@example.com
```

Depuis les sources, à côté de votre `.env` :

```bash
./target/release/statup reset-password you@example.com
```

- La commande lit `.env` et `DATABASE_URL` comme le serveur, et ne travaille que sur une base existante : un `DATABASE_URL` mal saisi est signalé comme tel, et aucune base vide n'est créée au passage.
- Elle met d'abord la base au niveau de la version du binaire, comme le fait le serveur au démarrage.
- Seuls les comptes actifs peuvent être réinitialisés. Le mot de passe provisoire est valable sept jours ; la personne choisit le sien à sa prochaine connexion, et ses sessions ouvertes sont fermées.

Codes de sortie : `0` quand le mot de passe est donné, `1` en cas d'échec, par exemple un compte introuvable ou une base impossible à ouvrir, `2` quand la commande n'est pas comprise.

## `backup`

Écrit une sauvegarde : un fichier SQLite qui contient la base, le logo et les icônes envoyées. Statup peut continuer à tourner.

```bash
docker compose exec statup /app/statup backup
```

Depuis les sources, à côté de votre `.env` :

```bash
./target/release/statup backup
```

- Le fichier est écrit dans [`BACKUP_DIR`](configuration.md#backup_dir), soit `/data/backups` dans le volume Docker, sous un nom qui porte la date, comme `statup-backup-2026-10-29-163045.db` (UTC). Le dossier est créé s'il manque.
- `--output <fichier>` écrit dans ce fichier à la place. Son dossier doit exister.
- Un fichier qui existe n'est jamais écrasé.
- La commande sauvegarde la base telle qu'elle est : elle ne la met pas au niveau de la version du binaire, et ne travaille que sur une base existante, donc un `DATABASE_URL` mal saisi est signalé comme tel.

Codes de sortie : `0` quand la sauvegarde est écrite, `1` en cas d'échec, `2` quand la commande n'est pas comprise.

## `restore`

Remplace les données de Statup (comptes, événements, réglages, logo et icônes) par celles d'une sauvegarde.

Arrêtez d'abord Statup. Avec Docker Compose :

```bash
docker compose stop statup
docker compose run --rm statup restore /data/backups/statup-backup-2026-10-29-163045.db --yes
docker compose start statup
```

Depuis les sources, serveur arrêté :

```bash
./target/release/statup restore statup-backup-2026-10-29-163045.db
```

- La commande affiche le nom de l'instance, la date et la version de Statup de la sauvegarde, puis demande `Restore it? [y/N]`. `--yes` supprime la question ; sans terminal, il est obligatoire.
- Les données remplacées sont d'abord copiées dans `statup-before-restore-<date>.db`, dans [`BACKUP_DIR`](configuration.md#backup_dir). Une base qui n'existe pas encore est créée, sans copie.
- Une sauvegarde faite par un Statup plus ancien est mise au niveau de la version du binaire. Une sauvegarde faite par un Statup plus récent est refusée : mettez d'abord Statup à jour.
- Le fichier de sauvegarde n'est pas modifié, et rien ne change quand le fichier n'est pas une sauvegarde de Statup.

Codes de sortie : `0` quand les données sont restaurées, `1` en cas d'échec ou de réponse négative à la question, `2` quand la commande n'est pas comprise ou que `--yes` manque sans terminal.

## `service`

Windows seulement. Fait tourner Statup comme service Windows : c'est le gestionnaire de services de Windows qui le lance, jamais une personne. C'est la ligne de commande avec laquelle un service est enregistré, par exemple :

```text
"C:\Program Files\Statup\statup.exe" --env C:\ProgramData\Statup\statup.env service
```

- Il travaille depuis le dossier de `statup.exe`, qui contient le dossier `static`. Donnez à `--env` un chemin complet.
- Son journal va dans `statup.log`, dans le dossier de la base. Au-delà de 10 Mo, le fichier est renommé `statup.log.1`, à la place du précédent, et un nouveau fichier commence.
- Arrêter le service laisse les requêtes en cours se terminer, comme Ctrl+C pour le serveur.
- Un échec est écrit dans `statup.log`, et Windows note que le service s'est arrêté sur une erreur.

Sur un autre système, la commande n'est pas comprise : code de sortie `2`.

## `--env`

`statup --env <fichier>`, placé avant la commande, lit les réglages dans ce fichier à la place de `.env`. Un fichier qui n'existe pas est une erreur.

```bash
./target/release/statup --env /etc/statup/statup.env backup
```

## `--help`

`statup --help`, ou `-h`, affiche la liste des commandes.
