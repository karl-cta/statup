---
title: Journal
description: "What happened on the instance, day by day, kept for administrators and exported as a CSV file."
---

The Journal lists what happened on the instance, day by day. Administrators open it from **Settings**, **Journal**, then **Open the journal**. The server output (`docker compose logs statup`) shows the same lines in English.

## Capture level

**Change**, at the top, sets the capture level and the retention.

**Essential**, the default, keeps the major changes and the errors:

- services added, changed or deleted, the settings, the alert channels, email subscribers removed and visitor subscriptions opened or closed;
- members added, their role or their account changed, an event deleted;
- groups and folders created, renamed or deleted, the members of a group, the services of a folder, and who sees a service or a folder;
- an outage detected, ended or erased, a report from a monitoring tool ignored, the checks paused when Statup is off the network or a round ignored, and messages to an alert channel or emails to subscribers given up or on hold;
- the API keys, the widget link, the backup schedule, a backup downloaded or restored, an automatic backup failed, and a repeating maintenance stopped or changed with the following ones;
- sign-ins, checks of the current password and reset requests blocked after too many attempts, a reset email not sent, and the app's errors.

**Detailed** keeps every action from the moment it is turned on: sign-ins and refused attempts, password resets, events published and updated, each failed check, each message sent.

**Retention** is 90 days by default, from 1 to 365. Nobody can edit or delete an entry; only the retention removes them.

## Find an entry

**Search** looks through the entries. **Type** narrows them to **Members**, **Sign-ins**, **Monitoring**, **Alerts** or **System**, **Member** to one person, and **From** and **To** to a period.

## Export

**Export** downloads the entries shown, filters included, as a CSV file in UTF-8 named `journal-YYYY-MM-DD.csv`, of up to 10,000 entries. A spreadsheet opens it.

## What is never written

Sign-in entries keep the client address, read like the rest of the app: see [`TRUST_PROXY_HEADERS`](configuration.md#trust_proxy_headers). No password, reset link, webhook address, or anything after the host of a checked address is ever written.
