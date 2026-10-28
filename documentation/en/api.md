---
title: API
description: "Read and change services and events from a script or a tool, with an API key."
---

With a key, a script or a tool does what an editor does: read the services and the events, add a service and set its state, publish an incident, a maintenance or an announcement, then post its updates until it is resolved. A monitoring tool that reports outages has its own address, see [Monitoring tools](monitoring-tools.md).

## Keys

In **Settings**, **API**, then **Manage the keys**: **New key**, a name such as `Zabbix`, then **Create the key**. The key is shown once, masked: **Show** reveals it, **Copy** copies it. Statup keeps only a hash of it, so a lost key cannot be shown again: **Regenerate** gives the same key a new secret and stops the old one at once. **Revoke** stops it for good.

A key acts as an editor and does nothing of the administration: no team, settings, alerts or keys, no deletion, no change to a closed event. What it writes is signed with its name, in the event's timeline and in the [Journal](journal.md). A key belongs to the instance: it keeps working when the administrator who made it is disabled, and the keys page says **account disabled** next to it, for an administrator to decide.

## Requests

Every address starts with `/api/v1/`, on the address of your page. The key goes in the `Authorization` header, never in the address, which ends up in the logs of the server and of the proxy:

```bash
curl https://status.example.com/api/v1/services \
  -H "Authorization: Bearer $STATUP_KEY"
```

- A body is JSON, sent with `Content-Type: application/json`, of up to 64 KB. A field the API does not take is refused rather than ignored.
- Dates are in UTC, to the second: `2026-10-07T09:00:00Z`.
- The API answers in English and stores nothing in the browser: no cookie, and a signed-in session opens nothing.
- It counts its requests with the pages: 600 a minute from one address.

## Errors

A refusal answers with its status and a JSON body:

```json
{"error": {"code": "service_name_too_long", "message": "The name is longer than 100 characters."}}
```

`code` stays the same from one version to the next; `message` may change. The codes every address can answer:

| Status | `code` | When |
|---|---|---|
| 400 | `invalid_data` | The body or a parameter cannot be read. |
| 401 | `unauthorized` | No key, a wrong key, or a revoked one. |
| 403 | `event_closed_admin_only` | The event is closed. |
| 404 | `not_found` | Nothing exists at this address. |
| 405 | `method_not_allowed` | The address does not take this method. |
| 408 | `timeout` | The server took more than 30 seconds. |
| 413 | `payload_too_large` | The body is over 64 KB. |
| 415 | `unsupported_media_type` | The body is not sent as JSON. |
| 422 | `invalid_data` | The JSON holds a field the API does not take, or a value of the wrong type. |
| 429 | `rate_limited` | Too many requests; `Retry-After` says how many seconds to wait. |
| 500 | `internal` | An error on the server. |

A value the rules refuse answers 400 with the rule as its code, the one the form would show: `service_name_required`, `title_empty`, `severity_required`, `invalid_transition`, `next_update`, and the like.

## Services

A service, as every address below answers it:

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

- `status` is what the page shows: the worst of the state set by hand, the open events and an outage detected.
- `manual_status` is the state set by hand. Both take `operational`, `degraded`, `major_outage` or `maintenance`.
- `monitored` is `true` when Statup checks the service itself, see [Monitoring](monitoring.md).

### List the services

`GET /api/v1/services` answers `{"services": [...]}`, in the order arranged on the Services page.

### Read a service

`GET /api/v1/services/3` answers the service, or 404.

### Add a service

`POST /api/v1/services`, with a name of up to 100 characters and a description of up to 500, the description being optional:

```bash
curl https://status.example.com/api/v1/services \
  -H "Authorization: Bearer $STATUP_KEY" \
  -H "Content-Type: application/json" \
  -d '{"name": "Mail", "description": "Outlook and the shared mailboxes"}'
```

It answers 201 with the new service. A service added this way is **Public**, outside any folder and not checked: its icon, visibility, folder and check are set on the Services page.

### Change a service

`PATCH /api/v1/services/3`, with `name`, `description`, or both. A field left out keeps its value; `"description": ""` removes the description. The icon stays as it is. It answers the service.

### Set the state of a service

`PUT /api/v1/services/3/status`, with `{"status": "degraded"}`: the state set by hand, as on the Services page. It answers the service, whose `status` an open incident may keep worse.

## Events

An event, as the addresses below answer it:

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

- `kind`: `incident`, `maintenance` or `publication` (an announcement).
- `severity`: `minor` or `critical`, for an incident.
- `category`: `info` or `changelog`, for an announcement.
- `state`: for an incident `investigating`, `in_progress`, `monitoring`, `resolved` or `cancelled`; for a maintenance `scheduled`, `in_progress`, `completed` or `cancelled`; none for an announcement.
- `description` is the Markdown as written.

### List the events

`GET /api/v1/events` answers `{"events": [...], "total": 42}`, the newest first, 20 at a time. `total` counts every event that matches, whatever the page. Parameters, all optional:

- `kind`: `incident`, `maintenance` or `publication`.
- `open`: `true` for the events under way or announced, `false` for the closed ones. An announcement has no state, so `open` leaves it out.
- `limit`: from 1 to 100, 20 by default.
- `offset`: how many to skip, 0 by default.

```bash
curl "https://status.example.com/api/v1/events?kind=incident&open=true" \
  -H "Authorization: Bearer $STATUP_KEY"
```

### Read an event

`GET /api/v1/events/7` answers the event and its `updates`, the newest first. An update holds `id`, `message` as plain text, `message_html` as the page shows it, `author`, the member or the key that posted it, and `created_at`.

### Publish an event

`POST /api/v1/events` publishes at once, with the same alerts as the page. It answers 201 with the event.

```bash
curl https://status.example.com/api/v1/events \
  -H "Authorization: Bearer $STATUP_KEY" \
  -H "Content-Type: application/json" \
  -d '{"kind": "incident", "severity": "critical", "title": "Mail is down",
       "services": [3], "next_update": "30 min"}'
```

| Field | For | |
|---|---|---|
| `kind` | all | Required. |
| `title` | all | Required, up to 200 characters. |
| `description` | all | Markdown. |
| `services` | all | The ids of the services it concerns. |
| `severity` | incident | Required: `minor` or `critical`. |
| `state` | incident | `in_progress` or `monitoring` when the team already works on it; `investigating` by default. Any other state answers 400 `invalid_transition`. |
| `started_at` | incident | When it began, for an incident declared late. |
| `next_update` | incident | When the next update is due: a delay such as `30 min` or `1 h`, or a time of day such as `14:30`, in the page's time zone. |
| `planned_start` | maintenance | Announces the maintenance for this time; without it, the maintenance starts now. |
| `planned_end` | maintenance | When it should end. |
| `keeps_services_up` | maintenance | `true` for a maintenance marked **No downtime**. |
| `category` | announcement | `info` by default, or `changelog`. |

A field that does not fit the kind is left out, as the form does.

A maintenance that repeats is set in the app ([Maintenance](maintenance.md#repeat)); the API reads and changes each of its sessions as an ordinary maintenance.

### Post an update

`POST /api/v1/events/7/updates`, with a `message` in Markdown, a new `state`, or both, and for an incident a `next_update`:

```bash
curl https://status.example.com/api/v1/events/7/updates \
  -H "Authorization: Bearer $STATUP_KEY" \
  -H "Content-Type: application/json" \
  -d '{"message": "Mail works again.", "state": "resolved"}'
```

It answers 201 with the event and its updates, and sends the alerts the page sends. A closed event answers 403 `event_closed_admin_only`: only an administrator, on the page, can still change it.
