---
title: Derrière un reverse proxy
description: "Servir Statup en HTTPS sous un nom à vous, avec nginx, Caddy, IIS ou tout proxy qui pose les en-têtes habituels."
---

Statup parle en HTTP simple sur le port 3000. Pour le HTTPS et un nom comme `status.example.com`, placez un reverse proxy devant : il porte le certificat et transmet les requêtes.

## Publier Statup sur la boucle locale uniquement

Pour que personne ne joigne Statup en contournant le proxy, publiez-le sur l'adresse de boucle locale de l'hôte. Dans `docker-compose.yml`, sous `ports:` :

```yaml
ports:
  - "127.0.0.1:3000:3000"
```

Puis, dans `.env` :

```env
PUBLIC_URL=https://status.example.com
TRUST_PROXY_HEADERS=true
```

Relancez `docker compose up -d`.

Installé sans Docker, choisissez `local` à la question de l'écoute ([Linux](install-linux.md), [Windows Server](install-windows.md)), et ajoutez les deux réglages à `/etc/statup/statup.env` ou `C:\ProgramData\Statup\statup.env`.

## Ce que changent ces deux réglages

- [`PUBLIC_URL`](configuration.md#public_url) est l'adresse de chaque lien écrit par Statup, dans le flux et dans les alertes. Son `https://` marque le cookie de session `Secure` et active HSTS.
- [`TRUST_PROXY_HEADERS`](configuration.md#trust_proxy_headers) fait lire à Statup l'adresse du client dans la dernière entrée de `X-Forwarded-For`, celle que votre proxy a ajoutée, quoi qu'un client ait envoyé avant elle, et le schéma dans `X-Forwarded-Proto`. C'est cette adresse qu'utilisent les limites de requêtes et le [Journal](journal.md).

nginx, Caddy, Traefik, Apache et HAProxy (avec `option forwardfor`) ajoutent tous l'adresse à `X-Forwarded-For`. Si votre proxy l'écrit dans un autre en-tête, indiquez cet en-tête dans [`CLIENT_IP_HEADER`](configuration.md#client_ip_header).

> [!WARNING]
> Ne réglez jamais `TRUST_PROXY_HEADERS=true` quand Statup est joignable directement : n'importe qui pourrait alors choisir sa propre adresse et contourner les limites de requêtes.

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

`client_max_body_size` laisse passer une icône de 1 Mo et une sauvegarde jusqu'à 256 Mo pour [Restaurer une sauvegarde](backups.md#restore-a-backup) : nginx refuse de lui-même une requête de plus de 1 Mo.

## Caddy

Caddy pose les deux en-têtes de lui-même :

```caddy
status.example.com {
    reverse_proxy 127.0.0.1:3000
}
```

## IIS

Sous Windows Server, IIS fait de même avec deux modules de Microsoft : [URL Rewrite](https://www.iis.net/downloads/microsoft/url-rewrite) et [Application Request Routing](https://www.iis.net/downloads/microsoft/application-request-routing), installés dans cet ordre. Statup écoute sur cette machine seulement (`-Listen local` à l'[installation](install-windows.md)), avec les deux réglages ci-dessus dans `C:\ProgramData\Statup\statup.env`, puis `Restart-Service Statup`.

Dans PowerShell en administrateur, activez le proxy d'Application Request Routing, et autorisez la règle ci-dessous à poser `X-Forwarded-Proto` :

```powershell
$appcmd = "$env:windir\system32\inetsrv\appcmd.exe"
& $appcmd set config -section:system.webServer/proxy /enabled:True /commit:apphost
& $appcmd set config -section:system.webServer/rewrite/allowedServerVariables "/+[name='HTTP_X_FORWARDED_PROTO']" /commit:apphost
```

Donnez au site une liaison HTTPS avec votre certificat, puis placez ce `web.config` dans son dossier, `C:\inetpub\wwwroot` pour le site par défaut :

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

- Application Request Routing ajoute l'adresse du client à `X-Forwarded-For`, que lit `TRUST_PROXY_HEADERS`.
- `maxAllowedContentLength` laisse passer une sauvegarde jusqu'à 256 Mo pour [Restaurer une sauvegarde](backups.md#restore-a-backup) : IIS refuse de lui-même une requête de plus de 30 Mo.
- `existingResponse="PassThrough"` garde les pages d'erreur de Statup, pas celles d'IIS.
