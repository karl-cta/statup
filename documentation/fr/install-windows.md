---
title: Installer sur Windows Server
description: "Installer Statup comme service Windows sur Windows Server sans Docker, avec un seul script qui le met aussi à jour et le désinstalle."
---

Un script installe Statup comme service Windows. Docker reste la voie principale pour [installer](install.md) Statup.

## Prérequis

- Windows Server, x86 64 bits.
- Windows PowerShell 5.1 ou PowerShell 7, ouvert en administrateur.

## Installer

```powershell
irm https://github.com/karl-cta/statup/releases/latest/download/install.ps1 | iex
```

Pour donner des paramètres, lancez le script comme un bloc de script :

```powershell
& ([scriptblock]::Create((irm https://github.com/karl-cta/statup/releases/latest/download/install.ps1))) -Port 8080
```

Ou téléchargez le fichier et lancez-le :

```powershell
irm https://github.com/karl-cta/statup/releases/latest/download/install.ps1 -OutFile install.ps1
powershell -ExecutionPolicy Bypass -File install.ps1 -Port 8080
```

> [!IMPORTANT]
> Avec `-Unattended`, le script ouvre le port dans le Pare-feu Windows sans demander. Ajoutez `-NoFirewall` pour laisser le pare-feu tel qu'il est. Rien n'est ouvert quand Statup n'écoute que sur cette machine.

Le script pose quelques questions, une à une, puis télécharge Statup, vérifie sa somme de contrôle, installe le service et attend qu'il réponde. Il se termine sur l'adresse de l'instance. Ouvrez-la pour créer l'administrateur : voir [Premier lancement](install.md#first-launch). Une installation qui s'arrête avant que Statup démarre est défaite.

## Questions

Appuyez sur Entrée pour prendre la valeur entre crochets. Un paramètre donné sur la ligne de commande répond à sa question, qui n'est alors plus posée.

| Question | Paramètre | Par défaut |
|---|---|---|
| Dossier du programme | `-InstallDir` | `C:\Program Files\Statup` |
| Dossier des données | `-DataDir` | `C:\ProgramData\Statup` |
| Dossier des sauvegardes | `-BackupDir` | `backups` dans le dossier des données |
| Port | `-Port` | `3000` |
| Écouter sur toutes les adresses, ou sur cette machine seulement | `-Listen` | `all` |
| Compte du service | `-ServiceAccount` | `Virtual` |
| Ouvrir le port dans le Pare-feu Windows | `-Firewall`, `-NoFirewall` | oui |
| Profils du pare-feu | `-FirewallProfile` | les profils des réseaux de la machine |

- `-Listen` prend `all` (toutes les adresses), `local` (cette machine seulement, pour un [reverse proxy](reverse-proxy.md) installé dessus) ou une adresse IP.
- Un dossier est un chemin complet comme `C:\Statup`, sans apostrophe ni guillemet, qui n'est pas un dossier système comme `C:\Windows` ou `C:\ProgramData`. Le dossier des données est sur cette machine. Le dossier des sauvegardes peut être un partage : voir [Sauvegardes sur un partage](install-windows.md#backups-on-a-share). Un dossier existant qui contient d'autres fichiers est refusé.
- Les profils du pare-feu sont `Domain`, `Private` et `Public`, un ou plusieurs.
- Les questions demandent une console. Sans console, le script s'arrête et demande `-Unattended`.
- Quand `C:\ProgramData\Statup\statup.env` existe, d'une installation précédente qui a été retirée, ses réglages sont gardés et seuls le dossier du programme et le compte sont demandés.

## Autres paramètres

| Paramètre | Effet |
|---|---|
| `-Credential` | Le mot de passe du compte donné dans `-ServiceAccount`. |
| `-Version <x.y.z>` | La version à installer. Par défaut, la dernière. |
| `-Archive <fichier>` | Installe depuis une archive déjà téléchargée. |
| `-Mirror <url>` | Télécharge depuis une copie des fichiers de la version. Demande `-Version`. |
| `-Unattended` | Prend la valeur par défaut de chaque question sans réponse, et confirme `-Uninstall`. |
| `-Uninstall` | Retire Statup et garde ses données et ses réglages. |

Voir [Sans accès à GitHub](install-windows.md#without-access-to-github) pour `-Archive` et `-Mirror`.

## Installer sans questions

Donnez `-Unattended` et les paramètres dont vous avez besoin :

```powershell
& ([scriptblock]::Create((irm https://github.com/karl-cta/statup/releases/latest/download/install.ps1))) -Unattended -Port 8080 -Listen local
```

## Compte du service

`-ServiceAccount` prend l'une de ces valeurs :

- `Virtual`, par défaut : le compte `NT SERVICE\Statup`, qui n'a pas de mot de passe.
- `LocalSystem`.
- Un compte avec mot de passe, comme `DOMAIN\statup`, ou `.\statup` pour un compte local, donné avec `-Credential`. Sans lui, le script demande le mot de passe, ou s'arrête sous `-Unattended`.
- Un compte de service géré de groupe, comme `DOMAIN\statup$`, qui n'a pas de mot de passe.

Le mot de passe est confié à Windows et n'est jamais écrit dans un fichier. Le script donne au compte le droit d'ouvrir une session en tant que service, et l'accès dont il a besoin aux dossiers de Statup.

```powershell
$credential = Get-Credential -UserName 'DOMAIN\statup' -Message 'Statup service account'
& ([scriptblock]::Create((irm https://github.com/karl-cta/statup/releases/latest/download/install.ps1))) -ServiceAccount 'DOMAIN\statup' -Credential $credential -Unattended
```

## Pare-feu Windows

Le script ouvre le port dans une règle nommée `Statup`, pour les profils des réseaux de la machine, ou pour ceux que vous donnez avec `-FirewallProfile`. Quand Statup n'écoute que sur cette machine, il n'ouvre rien. Une mise à jour déplace la règle sur le port des réglages. La désinstallation la retire.

## Ce que le script installe

| Élément | Où |
|---|---|
| Programme | `C:\Program Files\Statup` : `statup.exe`, `static`, les fichiers de licence et un fichier `VERSION`. |
| Données | `C:\ProgramData\Statup` : la base de données, `uploads`, les sauvegardes et `statup.log`. |
| Réglages | `C:\ProgramData\Statup\statup.env`, toujours à cet endroit. |
| Service | `Statup`, démarré automatiquement avec Windows. |

- Les dossiers du programme, des données, des fichiers envoyés et des sauvegardes, et le dossier des réglages, sont ouverts aux administrateurs, au système et au compte du service, et à personne d'autre : `ProgramData` laisserait tout utilisateur les lire, et un dossier du programme hors de `Program Files` le laisserait y ajouter des fichiers. Le service lit `statup.env` sans pouvoir le modifier.
- Windows redémarre le service après un échec : deux fois après 5 secondes, puis après une minute. Le compte repart de zéro après un jour.
- `statup.log`, à côté de la base, contient le journal du serveur. Au-delà de 10 Mo, il est mis de côté sous le nom `statup.log.1`, à la place du précédent, et un nouveau fichier commence.
- Le service se gère avec `Get-Service Statup`, `Stop-Service Statup` et `Start-Service Statup`.

## Changer un réglage

`statup.env` contient `HOST`, `PORT`, `DATABASE_URL`, `UPLOAD_DIR` et `BACKUP_DIR`. Ajoutez-y tout autre [réglage](configuration.md), un `NOM=valeur` par ligne, puis redémarrez le service :

```powershell
notepad C:\ProgramData\Statup\statup.env
Restart-Service Statup
```

> [!WARNING]
> Un chemin s'écrit entre apostrophes dans `statup.env`, comme le script l'écrit. Sans elles, Statup ne démarre pas.

```env
DATABASE_URL='C:\ProgramData\Statup\statup.db'
```

La règle du pare-feu reste sur l'ancien port après un changement de `PORT`, jusqu'à ce que le script soit relancé. Après un changement de `DATABASE_URL`, `UPLOAD_DIR` ou `BACKUP_DIR`, relancez aussi le script : il crée le nouveau dossier et laisse le service y écrire. Si Statup ne démarre pas, `statup.log` dit pourquoi.

## Commandes de maintenance

Lancez `statup.exe` dans PowerShell en administrateur, avec `--env` et le fichier de réglages. Voir [Ligne de commande](command-line.md).

```powershell
& 'C:\Program Files\Statup\statup.exe' --env C:\ProgramData\Statup\statup.env backup
& 'C:\Program Files\Statup\statup.exe' --env C:\ProgramData\Statup\statup.env reset-password you@example.com
```

Pour [restaurer](command-line.md#restore), arrêtez d'abord le service :

```powershell
Stop-Service Statup
& 'C:\Program Files\Statup\statup.exe' --env C:\ProgramData\Statup\statup.env restore C:\ProgramData\Statup\backups\statup-backup-2026-10-29-163045.db
Start-Service Statup
```

Plus de détails dans [Sauvegardes](backups.md).

## Sauvegardes sur un partage

Un dossier de sauvegardes comme `\\nas\statup` doit laisser écrire le compte du service : le script ne peut pas le régler sur un partage. Pour le compte virtuel, donnez le droit au compte de l'ordinateur, `DOMAIN\SERVER$`.

## HTTPS

Pour le HTTPS et un nom à vous, placez IIS devant Statup : voir [IIS](reverse-proxy.md#iis). Tout autre [reverse proxy](reverse-proxy.md) convient aussi.

## Mettre à jour

Relancez le script d'installation. Il repère le service installé et le met à jour : il arrête le service, remplace le programme, garde les réglages et les données, déplace la règle du pare-feu sur le port des réglages et démarre le service. Les paramètres d'une nouvelle installation sont ignorés, et il le dit. Pour choisir la version, ajoutez `-Version`.

- Statup sauvegarde la base avant qu'une mise à jour la modifie : voir [Montée de version](upgrading.md).
- Une version plus ancienne que celle installée est refusée sauf confirmation, car sa base peut être trop récente pour elle. Avec `-Unattended`, elle est refusée.
- Le script attend jusqu'à 30 secondes que Statup réponde. Une longue mise à jour peut se terminer sur « ne répond pas » alors que Statup démarre encore : lisez `statup.log`.

## Désinstaller

```powershell
& ([scriptblock]::Create((irm https://github.com/karl-cta/statup/releases/latest/download/install.ps1))) -Uninstall
```

Le script demande d'abord confirmation ; `-Unattended` supprime la question. Il arrête et retire le service, la règle du pare-feu et les fichiers du programme. Il garde les dossiers des données et des sauvegardes et `statup.env`. Une nouvelle installation retrouve les réglages et les garde. Le dossier du programme n'est pas dans les réglages : sans `-InstallDir`, c'est de nouveau `C:\Program Files\Statup`.

## Sans accès à GitHub

Téléchargez `install.ps1`, `statup-<version>-windows-x86_64.zip` et `SHA256SUMS` depuis la [page des versions](https://github.com/karl-cta/statup/releases) sur un autre ordinateur, puis copiez-les.

Le script ne vérifie pas une archive qu'on lui donne. Comparez l'empreinte de l'archive à sa ligne dans `SHA256SUMS`, puis installez-la :

```powershell
(Get-FileHash -Algorithm SHA256 statup-0.11.0-windows-x86_64.zip).Hash
Select-String statup-0.11.0-windows-x86_64.zip SHA256SUMS
powershell -ExecutionPolicy Bypass -File install.ps1 -Archive statup-0.11.0-windows-x86_64.zip
```

Pour servir les fichiers depuis votre propre serveur, copiez l'archive et `SHA256SUMS` sous `<url>/v<version>/`. Le script les télécharge de là et refuse une archive dont la somme de contrôle diffère.

```powershell
powershell -ExecutionPolicy Bypass -File install.ps1 -Mirror https://files.example.com/statup -Version 0.11.0
```
