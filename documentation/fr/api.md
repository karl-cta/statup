---
title: API
description: "Lire et modifier les services et les événements depuis un script ou un outil, avec une clé d'API."
---

Avec une clé, un script ou un outil fait ce que fait un éditeur : lire les services et les événements, ajouter un service et régler son état, publier un incident, une maintenance ou une annonce, puis publier ses mises à jour jusqu'à sa résolution. Un outil de supervision qui signale des pannes a sa propre adresse, voir [Outils de supervision](monitoring-tools.md).

## Les clés

Dans **Paramètres**, **API**, puis **Gérer les clés** : **Nouvelle clé**, un nom comme `Zabbix`, puis **Créer la clé**. La clé s'affiche une seule fois, masquée : **Afficher** la montre, **Copier** la copie. Statup n'en garde qu'une empreinte : une clé perdue ne peut pas être affichée de nouveau. **Régénérer** donne à la même clé un nouveau secret et arrête l'ancien aussitôt. **Retirer** l'arrête pour de bon.

Une clé agit comme un éditeur et ne fait rien de l'administration : ni équipe, ni paramètres, ni alertes, ni clés, aucune suppression, aucune modification d'un événement clos. Ce qu'elle écrit est signé de son nom, dans la chronologie de l'événement et dans le [Journal](journal.md). Une clé appartient à l'instance : elle continue de marcher quand l'administrateur qui l'a créée est désactivé, et la page des clés l'indique, **compte désactivé**, pour qu'un administrateur décide.

## Les requêtes

Chaque adresse commence par `/api/v1/`, sur l'adresse de votre page. La clé va dans l'en-tête `Authorization`, jamais dans l'adresse, qui finit dans les journaux du serveur et du proxy :

```bash
curl https://status.example.com/api/v1/services \
  -H "Authorization: Bearer $STATUP_KEY"
```

- Un corps est du JSON, envoyé avec `Content-Type: application/json`, de 64 Ko au plus. Un champ que l'API ne prend pas est refusé plutôt qu'ignoré.
- Les dates sont en UTC, à la seconde : `2026-10-07T09:00:00Z`.
- L'API répond en anglais et ne garde rien dans le navigateur : aucun cookie, et une session ouverte ne donne aucun accès.
- Elle compte ses requêtes avec celles des pages : 600 par minute depuis une même adresse.

## Les erreurs

Un refus répond avec son statut et un corps JSON :

```json
{"error": {"code": "service_name_too_long", "message": "The name is longer than 100 characters."}}
```

`code` reste le même d'une version à l'autre ; `message` peut changer. Les codes que toute adresse peut répondre :

| Statut | `code` | Quand |
|---|---|---|
| 400 | `invalid_data` | Le corps ou un paramètre ne peut pas être lu. |
| 401 | `unauthorized` | Pas de clé, une clé fausse ou une clé retirée. |
| 403 | `event_closed_admin_only` | L'événement est clos. |
| 404 | `not_found` | Rien n'existe à cette adresse. |
| 405 | `method_not_allowed` | L'adresse ne prend pas cette méthode. |
| 408 | `timeout` | Le serveur a mis plus de 30 secondes. |
| 413 | `payload_too_large` | Le corps dépasse 64 Ko. |
| 415 | `unsupported_media_type` | Le corps n'est pas envoyé en JSON. |
| 422 | `invalid_data` | Le JSON contient un champ que l'API ne prend pas, ou une valeur du mauvais type. |
| 429 | `rate_limited` | Trop de requêtes ; `Retry-After` dit combien de secondes attendre. |
| 500 | `internal` | Une erreur sur le serveur. |

Une valeur que les règles refusent répond 400 avec la règle pour code, celle que le formulaire afficherait : `service_name_required`, `title_empty`, `severity_required`, `invalid_transition`, `next_update`, etc.

## Les services

Un service, tel que toutes les adresses ci-dessous le répondent :

```json
{
  "id": 3,
  "name": "Mail",
  "slug": "mail",
  "description": "Outlook and the shared mailboxes",
  "status": "major_outage",
  "manual_status": "operational",
  "monitored": false,
  "updated_at": "2026-10-07T09:00:00Z"
}
```

- `status` est ce que la page affiche : le pire de l'état réglé à la main, des événements ouverts et d'une panne détectée.
- `manual_status` est l'état réglé à la main. Les deux prennent `operational`, `degraded`, `major_outage` ou `maintenance`.
- `monitored` vaut `true` quand Statup vérifie le service lui-même, voir [Surveillance](monitoring.md).

### Lister les services

`GET /api/v1/services` répond `{"services": [...]}`, dans l'ordre choisi sur la page Services.

### Lire un service

`GET /api/v1/services/3` répond le service, ou 404.

### Ajouter un service

`POST /api/v1/services`, avec un nom de 100 caractères au plus et une description facultative de 500 au plus :

```bash
curl https://status.example.com/api/v1/services \
  -H "Authorization: Bearer $STATUP_KEY" \
  -H "Content-Type: application/json" \
  -d '{"name": "Mail", "description": "Outlook and the shared mailboxes"}'
```

Il répond 201 avec le nouveau service. Un service ajouté ainsi est **Publique**, hors de tout dossier et non vérifié : son icône, sa visibilité, son dossier et sa vérification se règlent sur la page Services.

### Modifier un service

`PATCH /api/v1/services/3`, avec `name`, `description`, ou les deux. Un champ absent garde sa valeur ; `"description": ""` retire la description. L'icône ne change pas. Il répond le service.

### Régler l'état d'un service

`PUT /api/v1/services/3/status`, avec `{"status": "degraded"}` : l'état réglé à la main, comme sur la page Services. Il répond le service, dont `status` peut rester pire tant qu'un incident est ouvert.

## Les événements

Un événement, tel que les adresses ci-dessous le répondent :

```json
{
  "id": 7,
  "kind": "incident",
  "severity": "critical",
  "category": null,
  "state": "investigating",
  "title": "Mail is down",
  "description": "Messages are not sent or received.",
  "services": [{"id": 3, "name": "Mail"}],
  "planned_start": null,
  "planned_end": null,
  "started_at": "2026-10-07T09:00:00Z",
  "ended_at": null,
  "next_update_at": "2026-10-07T09:30:00Z",
  "created_at": "2026-10-07T09:00:00Z",
  "updated_at": "2026-10-07T09:00:00Z"
}
```

- `kind` : `incident`, `maintenance` ou `publication` (une annonce).
- `severity` : `minor` ou `critical`, pour un incident.
- `category` : `info` ou `changelog`, pour une annonce.
- `state` : pour un incident `investigating`, `in_progress`, `monitoring`, `resolved` ou `cancelled` ; pour une maintenance `scheduled`, `in_progress`, `completed` ou `cancelled` ; aucun pour une annonce.
- `description` est le Markdown tel qu'écrit.

### Lister les événements

`GET /api/v1/events` répond `{"events": [...], "total": 42}`, les plus récents d'abord, 20 à la fois. `total` compte tous les événements qui correspondent, quelle que soit la page. Paramètres, tous facultatifs :

- `kind` : `incident`, `maintenance` ou `publication`.
- `open` : `true` pour les événements en cours ou annoncés, `false` pour les clos. Une annonce n'a pas d'état : `open` la laisse de côté.
- `limit` : de 1 à 100, 20 par défaut.
- `offset` : combien en sauter, 0 par défaut.

```bash
curl "https://status.example.com/api/v1/events?kind=incident&open=true" \
  -H "Authorization: Bearer $STATUP_KEY"
```

### Lire un événement

`GET /api/v1/events/7` répond l'événement et ses `updates`, les plus récentes d'abord. Une mise à jour contient `id`, `message` en texte simple, `message_html` tel que la page l'affiche, `author`, le membre ou la clé qui l'a publiée, et `created_at`.

### Publier un événement

`POST /api/v1/events` publie aussitôt, avec les mêmes alertes que la page. Il répond 201 avec l'événement.

```bash
curl https://status.example.com/api/v1/events \
  -H "Authorization: Bearer $STATUP_KEY" \
  -H "Content-Type: application/json" \
  -d '{"kind": "incident", "severity": "critical", "title": "Mail is down",
       "services": [3], "next_update": "30 min"}'
```

| Champ | Pour | |
|---|---|---|
| `kind` | tous | Obligatoire. |
| `title` | tous | Obligatoire, 200 caractères au plus. |
| `description` | tous | Markdown. |
| `services` | tous | Les identifiants des services concernés. |
| `severity` | incident | Obligatoire : `minor` ou `critical`. |
| `state` | incident | `in_progress` ou `monitoring` quand l'équipe y travaille déjà ; `investigating` par défaut. Tout autre état répond 400 `invalid_transition`. |
| `started_at` | incident | Son début, pour un incident déclaré en retard. |
| `next_update` | incident | Quand la prochaine mise à jour est attendue : un délai comme `30 min` ou `1 h`, ou une heure comme `14:30`, dans le fuseau de la page. |
| `planned_start` | maintenance | Annonce la maintenance pour cette heure ; sans lui, la maintenance commence maintenant. |
| `planned_end` | maintenance | Sa fin prévue. |
| `keeps_services_up` | maintenance | `true` pour une maintenance marquée **Sans interruption**. |
| `category` | annonce | `info` par défaut, ou `changelog`. |

Un champ qui ne convient pas au type est laissé de côté, comme le fait le formulaire.

Une maintenance qui se répète se règle dans l'application ([Maintenances](maintenance.md#repeat)) ; l'API lit et modifie chacune de ses séances comme une maintenance ordinaire.

### Publier une mise à jour

`POST /api/v1/events/7/updates`, avec un `message` en Markdown, un nouvel `state`, ou les deux, et pour un incident un `next_update` :

```bash
curl https://status.example.com/api/v1/events/7/updates \
  -H "Authorization: Bearer $STATUP_KEY" \
  -H "Content-Type: application/json" \
  -d '{"message": "Mail works again.", "state": "resolved"}'
```

Il répond 201 avec l'événement et ses mises à jour, et envoie les alertes que la page envoie. Un événement clos répond 403 `event_closed_admin_only` : seul un administrateur, sur la page, peut encore le modifier.
