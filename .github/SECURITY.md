# Security

Statup is pre-v1 and has not been audited by a third party. Reports are welcome.

## Reporting a vulnerability

Do not open a public issue for a security problem. Use GitHub's private
reporting on this repository ("Report a vulnerability" under the Security
tab), or write to the address on the maintainer's GitHub profile. Include the
version or commit, the steps to reproduce, and what an attacker gains.

You will get an acknowledgement within a few days. A fix ships as a new
release with a note in the changelog; the report is credited if you want it
to be.

## In scope

The application as shipped: authentication and sessions, access control
between roles and between members and visitors, forms and uploads, the feed,
the Docker image and the default configuration.

## Out of scope

Reports that need a compromised host, a compromised administrator account, or
a reverse proxy configured against the [self-hosting guide](SELF-HOSTING.md).
Denial of service by volume: the built-in rate limits are a courtesy, not a
defence, and belong in front of the application.

## What the application does by design

- Sign-in uses Argon2id. A wrong password costs the same time whether the
  account exists or not.
- A chosen password needs 12 characters mixing upper and lower case letters,
  digits and symbols, or 20 characters. A temporary password is shown once,
  kept nowhere in clear, and expires after seven days.
- Every form carries a CSRF token bound to the session; htmx sends it as a
  header.
- The Content Security Policy allows the instance's own files only. No inline
  script or style exists.
- Uploaded icons are decoded and re-encoded, or filtered against an SVG
  allowlist, then served from random names with a sandboxing policy.
- Markdown in updates is rendered to a safe subset of HTML.
- `/health` and the uploaded icons answer without an account even on a
  members-only instance: the first reveals nothing but that the database
  answers, the second is reachable only by a name that appears on a page.
