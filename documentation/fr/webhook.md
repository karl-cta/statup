---
title: Corps du webhook
description: "Le JSON que reçoit un canal webhook pour chaque incident, maintenance, annonce et panne détectée."
---

Un canal de type **Webhook** reçoit un `POST` avec un corps JSON à l'URL que vous lui donnez : Zapier, Make, n8n, Power Automate, Home Assistant ou un script à vous. Pour les canaux eux-mêmes, voir [Alertes](alerts.md).

## Le corps, version 1

```json
{
  "version": 1,
  "happening": "opened",
  "occurred_at": "2026-10-07T09:00:00Z",
  "instance": "Example",
  "event": {
    "id": 7,
    "kind": "incident",
    "severity": "critical",
    "category": null,
    "state": "investigating",
    "title": "Network down at the head office",
    "services": ["Network", "VPN"],
    "planned_start": null,
    "planned_end": null,
    "url": "https://status.example.com/events/7"
  },
  "service": null,
  "message": {
    "mark": "outage",
    "headline": "Major incident · Network down at the head office",
    "summary": "Investigating · Network, VPN",
    "text": "The provider is on site.",
    "url": "https://status.example.com/events/7"
  }
}
```

## Champs

### `happening`

Ce qui s'est produit : `opened`, `updated`, `rescheduled`, `started`, `closed`, `published`, `reviewed`, `service_down`, `service_up` ou `test`. `reviewed` correspond au bilan écrit une fois un incident résolu, envoyé une seule fois ; `message.text` en contient le début. `test` est le message envoyé par **Envoyer un test**.

### `event`

- `kind` : `incident`, `maintenance` ou `publication` (une annonce).
- `severity` : `minor` ou `critical`, pour un incident.
- `category` : `changelog` ou `info`, pour une annonce.
- `state` : l'état vers lequel l'événement est passé.

`event` vaut `null` pour une panne trouvée par les vérifications ou signalée par un outil de supervision, et pour `test`, dont le corps n'a pas de liste `services` et un `service` à `null`. Le corps d'une panne porte alors, à son premier niveau à côté de `service`, une liste `services` avec une entrée par service de cette vague : `[{"name": "Mail", "down_since": "2026-10-07T08:57:00Z"}]`. Le corps d'un événement n'a pas cette liste : ses services sont les noms de `event.services`.

### `service`

Quand une telle panne ne concerne qu'un seul service, `service` contient cette même entrée. Il vaut `null` pour une vague de plusieurs services, et dans le corps d'un événement, comme dans l'exemple ci-dessus.

### `message`

Le texte que reçoivent les outils de discussion, dans la langue du canal. `mark` vaut `outage`, `degraded`, `maintenance`, `restored` ou `plain`.

## Stabilité

Les dates sont en UTC. Une version ultérieure du corps ajoutera des champs sans modifier ceux-ci.
