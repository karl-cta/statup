---
title: Install on Windows Server
description: "Install Statup as a Windows service on Windows Server without Docker, with one script that also upgrades and removes it."
---

One script installs Statup as a Windows service. Docker stays the main way to [install](install.md) Statup.

## Requirements

- Windows Server, 64-bit x86.
- Windows PowerShell 5.1 or PowerShell 7, opened as an administrator.

## Install

```powershell
irm https://github.com/karl-cta/statup/releases/latest/download/install.ps1 | iex
```

To give parameters, run the script as a script block:

```powershell
& ([scriptblock]::Create((irm https://github.com/karl-cta/statup/releases/latest/download/install.ps1))) -Port 8080
```

Or download the file and run it:

```powershell
irm https://github.com/karl-cta/statup/releases/latest/download/install.ps1 -OutFile install.ps1
powershell -ExecutionPolicy Bypass -File install.ps1 -Port 8080
```

> [!IMPORTANT]
> With `-Unattended`, the script opens the port in Windows Firewall without asking. Add `-NoFirewall` to leave the firewall as it is. Nothing is opened when Statup listens on this machine only.

The script asks a few questions, one at a time, then downloads Statup, checks its checksum, installs the service and waits until it answers. It ends with the address of the instance. Open it to create the administrator: see [First launch](install.md#first-launch). An install that stops before Statup starts is undone.

## Questions

Press Enter to take the value in brackets. A parameter given on the command line answers its question, which is then not asked.

| Question | Parameter | Default |
|---|---|---|
| Program folder | `-InstallDir` | `C:\Program Files\Statup` |
| Data folder | `-DataDir` | `C:\ProgramData\Statup` |
| Backup folder | `-BackupDir` | `backups` in the data folder |
| Port | `-Port` | `3000` |
| Listen on every address, or on this machine only | `-Listen` | `all` |
| Account of the service | `-ServiceAccount` | `Virtual` |
| Open the port in Windows Firewall | `-Firewall`, `-NoFirewall` | yes |
| Firewall profiles | `-FirewallProfile` | the profiles of the networks of the machine |

- `-Listen` takes `all` (every address), `local` (this machine only, for a [reverse proxy](reverse-proxy.md) on it) or an IP address.
- A folder is a full path such as `C:\Statup`, without a quote, and not a system folder such as `C:\Windows` or `C:\ProgramData`. The data folder is on this machine. The backup folder may be a share: see [Backups on a share](install-windows.md#backups-on-a-share).
- The firewall profiles are `Domain`, `Private` and `Public`, one or more.
- The questions need a console. Without one, the script stops and asks for `-Unattended`.
- When `C:\ProgramData\Statup\statup.env` exists, from an earlier install that was removed, its settings are kept and only the program folder and the account are asked.

## Other parameters

| Parameter | Effect |
|---|---|
| `-Credential` | The password of the account given in `-ServiceAccount`. |
| `-Version <x.y.z>` | The version to install. By default, the latest. |
| `-Archive <file>` | Installs from an archive already downloaded. |
| `-Mirror <url>` | Downloads from a copy of the release files. Needs `-Version`. |
| `-Unattended` | Takes the default of every question not answered, and confirms `-Uninstall`. |
| `-Uninstall` | Removes Statup and keeps its data and settings. |

See [Without access to GitHub](install-windows.md#without-access-to-github) for `-Archive` and `-Mirror`.

## Install without questions

Give `-Unattended` and the parameters you need:

```powershell
& ([scriptblock]::Create((irm https://github.com/karl-cta/statup/releases/latest/download/install.ps1))) -Unattended -Port 8080 -Listen local
```

## Service account

`-ServiceAccount` takes one of these:

- `Virtual`, the default: the account `NT SERVICE\Statup`, which has no password.
- `LocalSystem`.
- An account with a password, such as `DOMAIN\statup` or `.\statup` for a local account, given with `-Credential`. Without it, the script asks for the password, or stops under `-Unattended`.
- A group managed service account, such as `DOMAIN\statup$`, which has no password.

The password is handed to Windows and never written to a file. The script gives the account the right to log on as a service, and the access it needs to the folders of Statup.

```powershell
$credential = Get-Credential -UserName 'DOMAIN\statup' -Message 'Statup service account'
& ([scriptblock]::Create((irm https://github.com/karl-cta/statup/releases/latest/download/install.ps1))) -ServiceAccount 'DOMAIN\statup' -Credential $credential -Unattended
```

## Windows Firewall

The script opens the port in a rule named `Statup`, for the profiles of the networks the machine is on, or for the ones you give with `-FirewallProfile`. When Statup listens on this machine only, it opens nothing. An upgrade moves the rule to the port of the settings. Uninstalling removes it.

## What the script installs

| Item | Where |
|---|---|
| Program | `C:\Program Files\Statup`: `statup.exe`, `static`, the license files and a `VERSION` file. |
| Data | `C:\ProgramData\Statup`: the database, `uploads`, the backups and `statup.log`. |
| Settings | `C:\ProgramData\Statup\statup.env`, always at this place. |
| Service | `Statup`, started automatically with Windows. |

- The data, uploads and backup folders, and the folder of the settings, are open to the administrators, to the system and to the service account, and to nobody else. `ProgramData` would let every user read them.
- Windows restarts the service after a failure: twice after 5 seconds, then after a minute. The count starts again after a day.
- `statup.log`, next to the database, holds the log of the server. Above 10 MB, it is set aside as `statup.log.1` at the next start, which replaces the previous one.
- The service is managed with `Get-Service Statup`, `Stop-Service Statup` and `Start-Service Statup`.

## Change a setting

`statup.env` holds `HOST`, `PORT`, `DATABASE_URL`, `UPLOAD_DIR` and `BACKUP_DIR`. Add any other [setting](configuration.md) to it, one `NAME=value` per line, then restart the service:

```powershell
notepad C:\ProgramData\Statup\statup.env
Restart-Service Statup
```

> [!WARNING]
> A path is written between single quotes in `statup.env`, as the script writes it. Without them, Statup does not start.

```env
DATABASE_URL='C:\ProgramData\Statup\statup.db'
```

The firewall rule stays on the old port after a change of `PORT`, until the script is run again. After a change of `DATABASE_URL`, `UPLOAD_DIR` or `BACKUP_DIR`, run the script again too: it creates the new folder and lets the service write to it. If Statup does not start, `statup.log` tells why.

## Maintenance commands

Run `statup.exe` in PowerShell as an administrator, with `--env` and the settings file. See [Command line](command-line.md).

```powershell
& 'C:\Program Files\Statup\statup.exe' --env C:\ProgramData\Statup\statup.env backup
& 'C:\Program Files\Statup\statup.exe' --env C:\ProgramData\Statup\statup.env reset-password you@example.com
```

To [restore](command-line.md#restore), stop the service first:

```powershell
Stop-Service Statup
& 'C:\Program Files\Statup\statup.exe' --env C:\ProgramData\Statup\statup.env restore C:\ProgramData\Statup\backups\statup-backup-2026-10-29-163045.db
Start-Service Statup
```

More on backups in [Backups](backups.md).

## Backups on a share

A backup folder such as `\\nas\statup` must let the service account write to it: the script cannot set that on a share. For the virtual account, give the right to the account of the computer, `DOMAIN\SERVER$`.

## HTTPS

For HTTPS and a name of your own, put IIS in front of Statup: see [IIS](reverse-proxy.md#iis). Any other [reverse proxy](reverse-proxy.md) works too.

## Upgrade

Run the install script again. It finds the installed service and updates it: it stops the service, replaces the program, keeps the settings and the data, moves the firewall rule to the port of the settings and starts the service. The parameters of a new install are ignored, and it says so. To choose the version, add `-Version`.

- Statup backs up the database before an upgrade changes it: see [Upgrading](upgrading.md).
- An older version than the installed one is refused unless you confirm, as its database may be too new for it. With `-Unattended`, it is refused.
- The script waits up to 30 seconds for Statup to answer. A long upgrade can end on "does not answer" while Statup is still starting: read `statup.log`.

## Uninstall

```powershell
& ([scriptblock]::Create((irm https://github.com/karl-cta/statup/releases/latest/download/install.ps1))) -Uninstall
```

The script asks first; `-Unattended` skips the question. It stops and removes the service and the firewall rule, and the program files. It keeps the data and backup folders and `statup.env`. Installing again finds the settings and keeps them. The program folder is not in the settings: without `-InstallDir`, it is `C:\Program Files\Statup` again.

## Without access to GitHub

Download `install.ps1`, `statup-<version>-windows-x86_64.zip` and `SHA256SUMS` from the [release page](https://github.com/karl-cta/statup/releases) on another computer, and copy them over.

The script does not check an archive it is given. Compare the hash of the archive with its line in `SHA256SUMS`, then install it:

```powershell
(Get-FileHash -Algorithm SHA256 statup-0.11.0-windows-x86_64.zip).Hash
Select-String statup-0.11.0-windows-x86_64.zip SHA256SUMS
powershell -ExecutionPolicy Bypass -File install.ps1 -Archive statup-0.11.0-windows-x86_64.zip
```

To serve the files from your own server, copy the archive and `SHA256SUMS` under `<url>/v<version>/`. The script downloads them from there and refuses an archive whose checksum differs.

```powershell
powershell -ExecutionPolicy Bypass -File install.ps1 -Mirror https://files.example.com/statup -Version 0.11.0
```
