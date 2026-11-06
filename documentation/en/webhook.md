---
title: Webhook body
description: "The JSON that a webhook channel receives for each incident, maintenance, announcement and detected outage."
---

A channel of type **Webhook** receives a `POST` with a JSON body at the URL you give it: Zapier, Make, n8n, Power Automate, Home Assistant or a script of your own. See [Alerts](alerts.md) for the channels themselves.

## The body, version 1

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

## Fields

### `happening`

What took place: `opened`, `updated`, `rescheduled`, `started`, `closed`, `published`, `reviewed`, `service_down`, `service_up` or `test`. `reviewed` is the report written once an incident is resolved, sent once; `message.text` holds its start. `test` is the message of **Send a test**.

### `event`

- `kind`: `incident`, `maintenance` or `publication` (an announcement).
- `severity`: `minor` or `critical`, for an incident.
- `category`: `changelog` or `info`, for an announcement.
- `state`: the state the event moved to.

`event` is `null` for an outage the checks found or a monitoring tool reported, and for `test`, whose body has no `services` list and a `null` `service`. The body of an outage then carries, at its top level beside `service`, a `services` list with one entry for each service of that wave: `[{"name": "Mail", "down_since": "2026-10-07T08:57:00Z"}]`. The body of an event has no such list: its services are the names in `event.services`.

### `service`

When such an outage is about one service alone, `service` holds that same entry. It is `null` for a wave of several services, and in the body of an event, as in the example above.

### `message`

The text the chat tools receive, in the language of the channel. `mark` is `outage`, `degraded`, `maintenance`, `restored` or `plain`.

## Stability

Dates are in UTC. A later version of the body adds fields without changing these.
