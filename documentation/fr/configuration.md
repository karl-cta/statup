---
title: Configuration
description: "Chaque réglage que Statup lit au démarrage, sa valeur par défaut et les valeurs qu'il accepte."
---

Tous les réglages sont facultatifs. Inscrivez ceux que vous modifiez dans un fichier `.env` à côté de `docker-compose.yml`, puis relancez `docker compose up -d` pour que le conteneur redémarre avec eux.

```env
# .env, à côté de docker-compose.yml
PUBLIC_URL=https://status.example.com
TRUST_PROXY_HEADERS=true
TZ=Europe/Paris
```

> [!NOTE]
> Avec Docker Compose, `DATABASE_URL`, `UPLOAD_DIR`, `HOST` et `PORT` appartiennent à l'image et `docker-compose.yml` les fixe, pour que les données restent dans leur volume. Une valeur inscrite pour eux dans `.env` n'est pas prise en compte.

Une valeur que Statup ne sait pas lire, comme un port à `0` ou un mot à la place d'un nombre, l'arrête au démarrage, avec le nom du réglage dans sa sortie (`docker compose logs statup`).

## Adresse et proxy

### `PUBLIC_URL`

- Par défaut : non défini, l'adresse de chaque requête est utilisée
- Valeurs : une adresse, comme `https://status.example.com`

L'adresse qu'utilisent les visiteurs. Les liens du flux et des alertes s'en servent. Une adresse en `https://` marque le cookie de session `Secure` et active HSTS.

### `TRUST_PROXY_HEADERS`

- Par défaut : `false`
- Valeurs : `true`, `false`

Statup lit l'adresse du client dans `CLIENT_IP_HEADER` et le schéma dans `X-Forwarded-Proto`. À activer uniquement derrière un reverse proxy qui les renseigne : voir [Derrière un reverse proxy](reverse-proxy.md).

### `CLIENT_IP_HEADER`

- Par défaut : `X-Forwarded-For`
- Valeurs : un nom d'en-tête, comme `X-Real-IP`, `Forwarded` ou `CF-Connecting-IP`

L'en-tête dans lequel votre proxy écrit l'adresse du client. Seule sa dernière entrée compte, et aucun autre en-tête n'est lu. Utilisé seulement quand `TRUST_PROXY_HEADERS` vaut `true`.

## Accès

### `PUBLIC_MODE`

- Par défaut : `false`
- Valeurs : `true`, `false`

L'accès à la page tant qu'un administrateur n'a pas choisi : `true` pour tout le monde, `false` pour les membres seulement. Une fois le choix fait, au premier lancement ou dans les Paramètres, c'est lui qui prime, et il est conservé d'un redémarrage à l'autre.

### `ADMIN_EMAIL`

- Par défaut : non défini
- Valeurs : une adresse e-mail

Avec `ADMIN_PASSWORD`, crée un administrateur au démarrage quand aucun compte n'existe encore. Les deux sont nécessaires. Retirez-les une fois le compte créé.

### `ADMIN_PASSWORD`

- Par défaut : non défini
- Valeurs : un mot de passe de 12 caractères avec majuscule, minuscule, chiffre et caractère spécial, ou de 20 caractères quels qu'ils soient

Le mot de passe de l'administrateur créé par `ADMIN_EMAIL`.

### `SESSION_EXPIRY`

- Par défaut : `3600`
- Valeurs : un nombre de secondes, supérieur à 0

La durée de validité d'un formulaire de connexion. Une fois connecté, une session dure 30 jours sans visite si **Rester connecté 30 jours** est coché, 24 heures sinon.

## Heure et langue

### `TZ`

- Par défaut : `UTC`
- Valeurs : un fuseau horaire, comme `Europe/Paris`

Le fuseau horaire utilisé tant qu'aucun n'est choisi. Celui réglé dans les Paramètres, ou repris du navigateur qui crée le premier compte, prime sur lui. Les dates s'affichent dans ce fuseau, avec le décalage UTC quand il compte, et les heures des maintenances s'y saisissent.

### `DEFAULT_LOCALE`

- Par défaut : `fr`
- Valeurs : `fr`, `en`, `de`, `es`

La langue des visiteurs dont le navigateur ne demande aucune de ces quatre langues. `de` et `es` depuis la 0.3.0.

## Surveillance et nouvelles versions

### `MONITORING`

- Par défaut : `true`
- Valeurs : `true`, `false`
- Depuis : 0.2.0

Statup lance chaque minute les vérifications des services qui en ont une. `false` les arrête ; les adresses restent enregistrées, et **Tester**, dans le formulaire d'un service, vérifie toujours une adresse à la demande. Voir [Surveillance](monitoring.md).

### `UPDATE_CHECK`

- Par défaut : `true`
- Valeurs : `true`, `false`

Statup demande une fois par jour à GitHub si une version plus récente est publiée, et le signale aux administrateurs dans les Paramètres. La requête ne transmet aucune information sur l'instance. `false` pour une instance qui ne doit pas sortir sur internet.

## E-mail

Le serveur SMTP des alertes par e-mail, des liens de [mot de passe oublié](forgotten-password.md) et des e-mails aux [abonnés](status-page.md#get-notified). Les canaux E-mail et l'abonnement ne sont proposés que si `SMTP_HOST` est défini.

> [!NOTE]
> Un incident envoie un e-mail à chaque abonné. Une boîte de messagerie, comme Microsoft 365 ou Gmail, limite le nombre d'e-mails envoyés par minute et par jour : au-delà de quelques centaines d'abonnés, passez par un service d'envoi transactionnel. Signez le domaine de `SMTP_FROM` avec DKIM et publiez ses enregistrements SPF et DMARC, pour que les e-mails arrivent en boîte de réception ; les messageries peuvent alors afficher leur propre bouton de désabonnement, quand la page est servie en `https`.

### `SMTP_HOST`

- Par défaut : non défini
- Valeurs : un nom d'hôte, comme `smtp.example.com`
- Depuis : 0.3.0

Le serveur SMTP. Le définir rend `SMTP_FROM` obligatoire.

### `SMTP_SECURITY`

- Par défaut : `starttls`
- Valeurs : `starttls`, `tls`, `none`
- Depuis : 0.3.0

La façon dont la connexion au serveur SMTP est sécurisée. `none` pour un relais sur votre propre réseau.

### `SMTP_PORT`

- Par défaut : `587` avec `starttls`, `465` avec `tls`, `25` avec `none`
- Valeurs : un numéro de port
- Depuis : 0.3.0

Le port du serveur SMTP, quand ce n'est pas le port habituel pour sa sécurisation.

### `SMTP_USERNAME`

- Par défaut : non défini
- Valeurs : un nom d'utilisateur
- Depuis : 0.3.0

Avec `SMTP_PASSWORD`, quand le serveur SMTP les demande. Définissez les deux ou aucun.

### `SMTP_PASSWORD`

- Par défaut : non défini
- Valeurs : un mot de passe
- Depuis : 0.3.0

Le mot de passe qui va avec `SMTP_USERNAME`.

### `SMTP_FROM`

- Par défaut : aucun, obligatoire avec `SMTP_HOST`
- Valeurs : un expéditeur, comme `Statup <status@example.com>`
- Depuis : 0.3.0

L'expéditeur de chaque e-mail envoyé par Statup.

## Stockage et serveur

Avec Docker Compose, les quatre premiers appartiennent à l'image : `docker-compose.yml` les fixe.

### `DATABASE_URL`

- Par défaut : `./statup.db`, et `/data/statup.db` dans l'image
- Valeurs : un chemin vers un fichier SQLite

La base de données. Statup la crée au premier démarrage.

### `UPLOAD_DIR`

- Par défaut : `data/uploads`, et `/data/uploads` dans l'image
- Valeurs : un répertoire

L'endroit où sont conservés les icônes envoyées et le logo.

### `HOST`

- Par défaut : `0.0.0.0`
- Valeurs : une adresse IP

L'adresse sur laquelle Statup écoute.

### `PORT`

- Par défaut : `3000`
- Valeurs : un numéro de port, supérieur à 0

Le port sur lequel Statup écoute. Avec Docker Compose, changez plutôt le port publié sous `ports:`.

### `DB_MAX_CONNECTIONS`

- Par défaut : `10`
- Valeurs : un nombre, supérieur à 0

La taille du pool de connexions à la base de données.

### `BACKUP_DIR`

- Par défaut : un dossier `backups` à côté de la base, et `/data/backups` dans l'image
- Valeurs : un répertoire
- Depuis : 0.11.0

L'endroit où `statup backup` écrit ses fichiers, et où `statup restore` garde une copie des données qu'il remplace. Voir [Ligne de commande](command-line.md).

## Logs

### `LOG_LEVEL`

- Par défaut : `info`
- Valeurs : `trace`, `debug`, `info`, `warn`, `error`, `off`

Ce qu'affiche la sortie du serveur. Le [Journal](journal.md) conserve ses lignes quel que soit le niveau.

### `RUST_LOG`

- Par défaut : non défini
- Valeurs : un filtre, comme `statup=debug,tower_http=info`

Un filtre plus fin de la sortie du serveur. Quand il contient un filtre valide, il remplace `LOG_LEVEL`.
