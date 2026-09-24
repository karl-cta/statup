# Changelog

All notable changes to Statup are listed here. The project is pre-v1: until
1.0, a minor version may change the interface or the database, and the
README asks you to back up before upgrading.

## Unreleased

First public version. Services with their state and thirty days of history;
incidents, maintenances and announcements with dated updates and templates;
an Atom feed; reader, editor and administrator roles; a public or
members-only page; French and English; light and dark themes; a single
binary with an embedded SQLite database, a Docker image and a Compose file.

Since the first public commit:

- A first launch in four steps beside a live preview of the page.
- One dashboard layout for everyone, arranged on the page itself: blocks
  are dragged, sized in quarters, hidden, and the services and activity
  blocks choose what they show.
- Incidents start at the step you choose; maintenance can leave its
  services up; announcements read as an article; kind labels carry the
  impact ("Major incident") or the category.
- Administrators give a member a new temporary password from the Team page.
- Settings laid out as one row per setting; the services page names the
  event that holds a service in its state.
- Page transitions and a short highlight on what just arrived or changed,
  both off with reduced motion.
- Fixed: publishing needed two clicks in Safari.

Database: two migrations, applied on start (a maintenance without downtime
flag, and a single dashboard layout that keeps the members' arrangement).
