---
title: Behind a reverse proxy
description: "Serve Statup over HTTPS under a name of your own, with nginx, Caddy, IIS or any proxy that sets the usual headers."
---

Statup speaks plain HTTP on port 3000. For HTTPS and a name such as `status.example.com`, put a reverse proxy in front: it holds the certificate and passes the requests on.

## Publish Statup on loopback only

So that nobody reaches Statup around the proxy, publish it on the loopback address of the host. In `docker-compose.yml`, under `ports:`:

```yaml
ports:
  - "127.0.0.1:3000:3000"
```

Then, in `.env`:

```env
PUBLIC_URL=https://status.example.com
TRUST_PROXY_HEADERS=true
```

Run `docker compose up -d` again.

Installed without Docker, choose `local` at the listening question ([Linux](install-linux.md), [Windows Server](install-windows.md)), and add the two settings to `/etc/statup/statup.env` or `C:\ProgramData\Statup\statup.env`.

## What the two settings change

- [`PUBLIC_URL`](configuration.md#public_url) is the address in every link Statup writes, in the feed and in the alerts. Its `https://` marks the session cookie `Secure` and sends HSTS.
- [`TRUST_PROXY_HEADERS`](configuration.md#trust_proxy_headers) makes Statup read the client address from the last entry of `X-Forwarded-For`, the one your proxy appended whatever a client sent before it, and the scheme from `X-Forwarded-Proto`. The address is what the rate limits and the [Journal](journal.md) use.

nginx, Caddy, Traefik, Apache and HAProxy (with `option forwardfor`) all append the address to `X-Forwarded-For`. If your proxy writes it in another header, name that header in [`CLIENT_IP_HEADER`](configuration.md#client_ip_header).

> [!WARNING]
> Never set `TRUST_PROXY_HEADERS=true` when Statup can be reached directly: anyone could then choose their own address and walk around the rate limits.

## nginx

```nginx
location / {
    proxy_pass http://127.0.0.1:3000;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
    client_max_body_size 256m;
}
```

`client_max_body_size` lets an icon of 1 MB and a backup of up to 256 MB through to [Restore a backup](backups.md#restore-a-backup): nginx refuses a request above 1 MB on its own.

## Caddy

Caddy sets both headers on its own:

```caddy
status.example.com {
    reverse_proxy 127.0.0.1:3000
}
```

## IIS

On Windows Server, IIS does the same with two modules from Microsoft: [URL Rewrite](https://www.iis.net/downloads/microsoft/url-rewrite) and [Application Request Routing](https://www.iis.net/downloads/microsoft/application-request-routing), installed in that order. Statup listens on this machine only (`-Listen local` at the [install](install-windows.md)), with the two settings above in `C:\ProgramData\Statup\statup.env`, then `Restart-Service Statup`.

In PowerShell as an administrator, turn the proxy of Application Request Routing on, and let the rule below set `X-Forwarded-Proto`:

```powershell
$appcmd = "$env:windir\system32\inetsrv\appcmd.exe"
& $appcmd set config -section:system.webServer/proxy /enabled:True /commit:apphost
& $appcmd set config -section:system.webServer/rewrite/allowedServerVariables "/+[name='HTTP_X_FORWARDED_PROTO']" /commit:apphost
```

Give the site an HTTPS binding with your certificate, then put this `web.config` in its folder, `C:\inetpub\wwwroot` for the default site:

```xml
<?xml version="1.0" encoding="utf-8"?>
<configuration>
  <system.webServer>
    <rewrite>
      <rules>
        <rule name="Statup" stopProcessing="true">
          <match url="(.*)" />
          <serverVariables>
            <set name="HTTP_X_FORWARDED_PROTO" value="https" />
          </serverVariables>
          <action type="Rewrite" url="http://127.0.0.1:3000/{R:1}" />
        </rule>
      </rules>
    </rewrite>
    <security>
      <requestFiltering>
        <requestLimits maxAllowedContentLength="268435456" />
      </requestFiltering>
    </security>
    <httpErrors existingResponse="PassThrough" />
  </system.webServer>
</configuration>
```

- Application Request Routing appends the client address to `X-Forwarded-For`, which `TRUST_PROXY_HEADERS` reads.
- `maxAllowedContentLength` lets a backup of up to 256 MB through to [Restore a backup](backups.md#restore-a-backup): IIS refuses a request above 30 MB on its own.
- `existingResponse="PassThrough"` keeps the error pages of Statup, not those of IIS.
