# Changelog

All notable changes to Statup are listed here. The project is pre-v1: until 1.0, a minor version may change the interface or the database, and the README asks you to back up before upgrading.

## Unreleased

- Fixed: in Safari with the dark theme, the page no longer darkens for a moment on each new page, and the toolbar keeps its colour.
- Pages now change at once instead of fading into each other; the first launch keeps its step transitions.
- In the dark theme, the masthead takes the colour of the page, as it does in the light theme.
- Fixed: in Safari, opening the account menu no longer scrolls the page back to its top.
- In the footer, the mark and the separator line up with the text beside them.
- Fixed: with several services disrupted, the banner gives the latest update of each incident on its own row, on one line, instead of a single update at the bottom that named no incident.

## 0.2.0, 2026-10-05

- Statup monitors a service on its own: in its form, under Monitoring, choose Web for an address that should answer, or Port for a server, a router or a firewall by its host and port, and test it before saving; the test says why an address does not answer.
- At the first launch, a service typed in can be monitored right away, from a fold under its name, and gets a built-in icon of its kind; the suggestions no longer include Google Workspace, Sage, Confluence and SharePoint.
- After three failed checks in a row, the service shows an outage marked "detected automatically", and comes back on its own as soon as it answers; nothing is declared during a maintenance that takes it down, or when every service fails at once.
- An editor declares the incident of a detected outage in one click, from the banner or the Services page, with the service, the impact and the start already filled in.
- A detected outage of fifteen minutes or more counts in the thirty days of the service; choosing None or changing the address of a service in a false outage clears it, from its state and from its thirty days.
- The Services page shows the last latency of each monitored service, or that its last check got no answer, in place of a label.
- `MONITORING=false` turns every check off.
- The database is upgraded on start to keep the checks of the services and the outages they detect.
- The web framework, the templates, the database layer and the sign-in sessions run on their current major versions; nothing changes on screen and nobody has to sign in again.

## 0.1.3, 2026-10-04

- On a phone, the masthead tucks away while you read down the page and comes back as soon as you scroll up.
- On a phone, the menu opens over the page without moving it, its pages are rows you can tap across the whole width, and signing out is a button of its own.
- An editor who tries to change a closed event is now refused with an access denied answer (403) that says only an administrator can change it, instead of a bad request answer.
- The libraries Statup is built on are updated to their latest compatible releases, with their bug and security fixes.

## 0.1.2, 2026-10-03

- On a page open to everyone, the sign-in page leads back to the dashboard, so a visitor who opened it out of curiosity is not stuck there.
- On a phone, tapping a service or an event opens its page instead of a panel over the whole screen.
- On a phone, the masthead scrolls away with the page instead of staying pinned over it.
- On a phone, the open menu fills the screen, its clock at the foot, instead of leaving the page showing under it.
- On a phone, the icon choice of a service fits the screen: the grid takes as many columns as the width holds.
- Fixed: filtering the events list by service showed an error page.

## 0.1.1, 2026-10-02

- The Compose file runs the published image instead of building Statup: installing takes seconds, and upgrading is `docker compose pull`.
- The Compose file opens Statup to the network again, so an install on a server is reached from every workstation right away.
- The first launch speaks to whoever runs a service, for colleagues or customers: its texts no longer assume an IT team, and the choice of who can read the page explains that an open page is how you tell your customers.
- The masthead tabs show where you are more clearly: the current tab's line sits under its word and slides to the next tab as you change page, and a hovered tab draws a lighter line instead of looking current.

## 0.1.0, 2026-09-29

First public version. Services with their state and thirty days of history; incidents, maintenances and announcements with dated updates and templates; an Atom feed; reader, editor and administrator roles; a public or members-only page; French and English; light and dark themes; a single binary with an embedded SQLite database, a Docker image and a Compose file.

Since the first public commit:

- A first launch in four steps beside a live preview of the page.
- One dashboard layout for everyone, arranged on the page itself: blocks are dragged, sized in quarters, hidden, and the services and activity blocks choose what they show.
- Incidents start at the step you choose; maintenance can leave its services up; announcements read as an article; kind labels carry the impact ("Major incident") or the category.
- Administrators give a member a new temporary password from the Team page.
- Settings laid out as one row per setting; the services page names the event that holds a service in its state.
- Page transitions and a short highlight on what just arrived or changed, both off with reduced motion.
- Fixed: publishing needed two clicks in Safari; switching language right after a refused form could land on an error page; three menus kept the light theme's shadow in the dark theme; the Docker image and the release archive left out the first launch's transitions.
- Passwords chosen from now on need 12 characters with upper and lower case letters, a digit and a symbol, or 20 characters of any kind.
- The Compose file publishes Statup on this machine only until you open it, so nobody else can create the first administrator.
- Administrators see the version in Settings, and the newer one once it is published: the server asks GitHub once a day (`UPDATE_CHECK=false` turns it off).
- Security: behind a proxy, the client address is read from the one header named in `CLIENT_IP_HEADER` (the last entry of `X-Forwarded-For` by default); an IPv6 client is limited as its whole /64; a sign-in form stays valid one hour instead of seven days; wrong current passwords are limited on the profile page; a temporary password is kept nowhere in clear and expires after seven days; past 64 password checks waiting, a sign-in is turned away instead of queued.
- A Docker image for x86-64 and ARM servers, published with each version to the GitHub Container Registry.

Database: four migrations, applied on start (a maintenance without downtime flag, a single dashboard layout that keeps the members' arrangement, the expiry of temporary passwords, and the layout table without its unused user column).
