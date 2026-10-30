---
title: Command line
description: "The commands the Statup binary runs on the server, next to the web app."
---

The binary starts the server when it is run without arguments. Three maintenance commands exist, and on Windows a fourth one runs it as a service.

## `reset-password`

Gives an account a temporary password, and prints it.

```bash
docker compose exec statup /app/statup reset-password you@example.com
```

From source, next to your `.env`:

```bash
./target/release/statup reset-password you@example.com
```

- It reads `.env` and `DATABASE_URL` like the server, and works on an existing database only: a mistyped `DATABASE_URL` is reported as such, and no empty database is left behind.
- It brings the database to the binary's version first, as the server does at start.
- Only active accounts can be reset. The temporary password is valid for seven days; the person chooses their own at the next sign-in, and their open sessions are signed out.

Exit codes: `0` when the password is given, `1` when it fails, such as an account not found or a database that cannot be opened, `2` when the command is not understood.

## `backup`

Writes a backup: one SQLite file that holds the database, the logo and the uploaded icons. Statup can keep running.

```bash
docker compose exec statup /app/statup backup
```

From source, next to your `.env`:

```bash
./target/release/statup backup
```

- The file goes to [`BACKUP_DIR`](configuration.md#backup_dir), which is `/data/backups` in the Docker volume, under a name that holds the date, such as `statup-backup-2026-10-29-163045.db` (UTC). The folder is created when it is missing.
- `--output <file>` writes to that file instead. Its folder must exist.
- A file that exists is never overwritten.
- It backs up the database as it is: it does not bring it to the binary's version, and it works on an existing database only, so a mistyped `DATABASE_URL` is reported as such.

Exit codes: `0` when the backup is written, `1` when it fails, `2` when the command is not understood.

## `restore`

Replaces the data of Statup (accounts, events, settings, logo and icons) with a backup.

Stop Statup first. With Docker Compose:

```bash
docker compose stop statup
docker compose run --rm statup restore /data/backups/statup-backup-2026-10-29-163045.db --yes
docker compose start statup
```

From source, with the server stopped:

```bash
./target/release/statup restore statup-backup-2026-10-29-163045.db
```

- It prints the name of the instance, the date and the Statup version of the backup, then asks `Restore it? [y/N]`. `--yes` skips the question; without a terminal, it is required.
- The data it replaces are first copied to `statup-before-restore-<date>.db` in [`BACKUP_DIR`](configuration.md#backup_dir). A database that does not exist yet is created, with no copy.
- A backup made by an older Statup is brought to the binary's version. A backup made by a newer one is refused: upgrade Statup first.
- The backup file is not modified, and nothing changes when the file is not a Statup backup.

Exit codes: `0` when the data is restored, `1` when it fails or the question is answered no, `2` when the command is not understood or `--yes` is missing without a terminal.

## `service`

Windows only. Runs Statup as a Windows service: the Windows service manager starts it, never a person. It is the command line a service is registered with, for example:

```text
"C:\Program Files\Statup\statup.exe" --env C:\ProgramData\Statup\statup.env service
```

- It works from the folder of `statup.exe`, which holds the `static` folder. Give `--env` a full path.
- Its log goes to `statup.log`, in the folder of the database. Above 10 MB, the file is renamed `statup.log.1` at the next start, which replaces the previous one.
- Stopping the service lets open requests finish, as Ctrl+C does for the server.
- A failure is written to `statup.log`, and Windows records that the service stopped on an error.

On another system, the command is not understood: exit code `2`.

## `--env`

`statup --env <file>`, placed before the command, reads the settings from that file instead of `.env`. A file that does not exist is an error.

```bash
./target/release/statup --env /etc/statup/statup.env backup
```

## `--help`

`statup --help`, or `-h`, prints the list of commands.
