---
title: Forgotten password
description: "How a member gets back in, by email or through an administrator, and how the host resets any account."
---

## By email

With an SMTP server set ([`SMTP_HOST`](configuration.md#smtp_host) and the others) and the page URL known ([`PUBLIC_URL`](configuration.md#public_url), or **Page URL** at the foot of the [Alerts](alerts.md) page), members reset their own password: **Forgot your password?** on the sign-in page emails them a link.

- The link works once, for one hour.
- Choosing a new password signs them in and signs out every other session of the account.
- The page answers the same whether the address has an account or not, and an account receives at most three links an hour.

## Through an administrator

Without an SMTP server, or while the page URL is unknown, the same page asks the member to see an administrator, who gives a temporary password from the [Team](team.md) page. Its **Reset from the server** link leads to the part below.

## From the server

As the host, you can give a temporary password to any active account, even when no administrator can sign in:

```bash
docker compose exec statup /app/statup reset-password you@example.com
```

Hand it over within seven days. The person signs in with it and is asked to choose their own, and any session still open on that account is signed out. See [Command line](command-line.md).
