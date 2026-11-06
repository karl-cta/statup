---
title: Team and roles
description: "Readers, editors and administrators, how their accounts are made, the groups they belong to, and who can see the page."
---

## Roles

| Role | Can |
|---|---|
| **Reader** | Read the dashboard, the events and the feed, and edit their own profile. |
| **Editor** | Everything a reader can, plus publish incidents, maintenance and announcements, set service states, and manage services, folders, templates and icons. |
| **Administrator** | Everything, plus the settings, the team and its groups, the alerts, the Journal, the layout of the dashboard, and changing or deleting closed events. |

## Page access

Chosen at the [first launch](install.md#first-launch), then in **Settings**, under **Page access**:

- **Members only**: signing in is required, and feed readers can no longer read the feed.
- **Public**: visitors read the page and the feed without an account.

## Add a member

Accounts are created by an administrator: anyone else can only sign up on an empty instance, to become its first administrator.

**Settings**, **Team**, then **New member** at the head of the **Members** card: **Name**, **Email address** and **Role**, **Reader** by default. **Create the account** shows a temporary password once, with **Copy**.

- No email is sent: hand the password over yourself. It is shown only this once and kept nowhere.
- It works for seven days. At the first sign-in, the member is asked to choose their own, on **Choose your password**.
- An address already used by another account, disabled ones included, is refused.

## Password rule

Every chosen password needs 12 characters with upper and lower case letters, a digit and a symbol, or at least 20 characters of any kind. Passwords are hashed with Argon2id.

## Change a role

**Manage** on a member's row, then the role under **Role** and **Save**. Each member's role is written under their name. A disabled member keeps their role, which can be changed once the account is enabled again. Nobody can change their own role, and the last administrator cannot become something else: there is always at least one active administrator.

## Reset a password

**Manage** on their row, then **Reset** under **Reset password**, gives an active member a new temporary password, valid seven days. The old one stops working and their open sessions are signed out. For your own password, use **My profile**. When nobody can sign in any more, the host can do it from the [command line](command-line.md).

## Disable a member

**Manage** on their row, then **Disable**, stops a member from signing in; their open sessions end at their next click. **Enable**, in the same place, lets them back in. Their address stays reserved meanwhile. Nobody can disable themselves, nor the last active administrator.

Members cannot be deleted: disable them instead.

## Groups

A group gathers readers who see the same restricted services, such as Accounting or the Lyon office. Groups only open services: see [Visibility](services.md#visibility). Editors and administrators see every service, so a group holds readers only.

Administrators manage groups from **Settings**, **Team**, in the **Groups** card, which lists each group with its number of members.

### Create a group

**New group**, then a **Group name** of up to 100 characters, unique, and **Create the group**. The group's page opens. **Rename**, at the right of its name, changes it.

### Members of a group

On the group's page, **Add members** lists the readers not yet in it, with a search past eight; tick them, then **Add**. **Remove**, on a member's row, takes them out at once. A reader may belong to several groups.

### Visible services

The **Visible services** card lists what the group opens: the restricted [folders](services.md#folders), with their number of services, then the restricted services that have a visibility of their own.

**Add services** offers only restricted folders and services: adding opens them to the group and hides them from nobody. A service that inherits from its folder is opened through its folder. **Remove** takes the access away; a service that no group or member can see any more is then seen by editors and administrators only, and the page says how many.

### Delete a group

Under **Delete the group**, at the foot of its page, **Delete** asks to be confirmed. When some services can be seen through this group only, the card says how many: they are then seen by editors and administrators only, until their visibility says otherwise. The members keep their accounts.

## My profile

Every member reaches **My profile** from their menu:

- **Account details**: their name and email address. A new email address asks for the current password, since the password reset links go to it.
- **Password**: the current one, then the new one twice. The other sessions of the account are signed out.

The language is chosen from the language switch, and is kept on the account. The light or dark theme is kept by each browser.

## Sessions

Ticking **Stay signed in for 30 days** at sign-in keeps the session 30 days without a visit; otherwise it ends after 24 hours without a visit.
