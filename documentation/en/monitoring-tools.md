---
title: Monitoring tools
description: "Let Zabbix, Prometheus, Grafana, Uptime Kuma or a script tell Statup that a service is down or back up."
---

A monitoring tool already watches your servers. Set it once to tell Statup when a service goes down and when it comes back, and Statup shows it as it shows an outage its own [checks](monitoring.md) find.

## How a report works

Every tool posts to one address, `/api/v1/reports` on the address of your page, with an [API key](api.md#keys): make one per tool, named after it, so its reports are signed with its name.

- The service is named as in Statup, by its name or its slug, whatever the case. Name the tool's monitor, label or host the same way. Two services of the same name: the first in the order of the Services page takes the report, so name the other by its slug.
- **Down**: the service shows **Outage**, marked **Detected automatically**, from the moment the report arrives. The channels that receive **Outages detected automatically** are told, and editors find **Declare an incident** with the incident filled in. Statup never publishes an incident itself.
- **Up**: the outage ends and the state set by hand comes back.
- A service that an open incident covers sends no alert. Services reported down together in one message are told in one alert per channel.

A report is ignored, and a line written in the [Journal](journal.md), for a service Statup does not know, a service Statup checks itself (its own checks would undo the report the next minute), and a service reported down during a maintenance under way that names it, unless that maintenance is marked **No downtime**.

> [!IMPORTANT]
> A tool does not send again a report refused during a maintenance: a service still down when the maintenance ends would show as working. Schedule the same window in the tool, ending no earlier than Statup's: a maintenance in Uptime Kuma or Zabbix, a silence in Alertmanager or Grafana. Once its window is over, the tool reports a service still down after its usual delay: the next check in Uptime Kuma, about a minute in Zabbix, the group interval in Alertmanager and Grafana (5 minutes by default).

## Statup's format

A script, or a tool whose message you write, sends the name, the state and an optional message, in JSON or as a form:

```bash
curl https://status.example.com/api/v1/reports \
  -H "Authorization: Bearer $STATUP_KEY" \
  -H "Content-Type: application/json" \
  -d '{"service": "Mail", "state": "down", "message": "SMTP does not answer"}'
```

- `service`: a name or a slug.
- `state`: `down` or `up`, whatever the case.
- `message`: shown in the Journal line.

A tool that cannot set a header sends the key as a `key` field of the body instead, never in the address:

```bash
curl https://status.example.com/api/v1/reports \
  --data-urlencode "key=$STATUP_KEY" \
  --data-urlencode "service=Mail" \
  --data-urlencode "state=up"
```

## The answer

A report answers 200 with what it changed and what it left:

```json
{"applied": [{"service": "Mail", "state": "down"}], "ignored": [{"service": "VPN", "reason": "unchanged"}]}
```

| `reason` | |
|---|---|
| `unchanged` | The service is already down, or already up. |
| `unknown_service` | No service has this name or slug. |
| `monitored_by_statup` | Statup checks this service itself. |
| `under_maintenance` | A maintenance under way covers the service, and is not marked **No downtime**. |
| `no_service_label` | An Alertmanager or Grafana alert without a `service` label. |
| `no_monitor_name` | An Uptime Kuma heartbeat whose monitor has no name. |
| `not_down_or_up` | A state that is neither, such as pending. |

In Statup's format, the three refusals answer an error instead, so a script sees it failed: 404 `unknown_service`, 409 `monitored_by_statup`, 409 `under_maintenance`. A missing name answers 400 `service_required`, a state other than `down` or `up` 400 `invalid_state`, a body in no format Statup reads 400 `unknown_format`. Without a valid key, the answer is 401 and nothing is written in the Journal: the tool's own log shows the refusal. A body Statup cannot read answers 400 `invalid_data`, even when it holds the key. Of the reasons above, `unknown_service`, `monitored_by_statup` and `under_maintenance` also leave a line in the Journal; the others do not. See [API](api.md#errors) for the others.

## Tools that send their own format

Statup reads the message of these tools as they send it.

### Uptime Kuma

Statup finds a service by the name of its monitor. To create the services under the same names, import them first: on the Services page, **New**, then **Import from Uptime Kuma**.

1. **Status page URL**: the address of a Kuma status page as a browser shows it, such as `https://kuma.example.com/status/infra`. For a status page on its own domain, write the address of the Kuma server with `/status/` and the page's slug: the domain alone does not name the page. Tick **Accept an internal certificate** for a Kuma whose certificate Statup cannot verify.
2. **Read the page**: each group of the page shows its monitors, how many are to import, how many already exist in Statup, and the names over 100 characters, which Statup cannot take as they are.
3. Tick the groups, then **Import**. Each group becomes a folder of the same name, or fills the folder that already has it, whose visibility its new services take. A group whose name is over 100 characters or empty shows **no folder**: its services are created outside any folder. Each monitor becomes a service under its name in Kuma, in the order of the page.

Statup sees only the monitors of a status page: Kuma shows the others to its own users only. Kuma keeps a page for 5 minutes, so a monitor just added shows after up to 5 minutes. When Kuma gains monitors, import again: the URL is kept, only what is new is created, and nothing is renamed, moved or deleted.

Then send Kuma's alerts to Statup:

1. In **Settings**, **Notifications**, **Set Up Notification**, choose **Webhook**.
2. **Post URL**: `https://status.example.com/api/v1/reports`, with **HTTP Method** left at `POST`.
3. **Request Body**: choose **Preset - application/json**. The multipart preset is refused, and a custom body goes out as a form, which Statup does not read as JSON.
4. Turn on **Additional Headers** and write:

```json
{"Authorization": "Bearer stp_your_key"}
```

5. Turn the notification on for each monitor whose name is a service in Statup, under the monitor's **Notifications**, or at once with **Apply on all existing monitors**.

The notification's **Test** answers 200 without changing anything. A monitor that is pending or in maintenance changes nothing either.

A monitor that comes back up during a maintenance in Kuma says nothing when it ends: leave out of that maintenance a monitor already down when it begins, or Statup never hears it is back.

### Prometheus

Prometheus sends its alerts through Alertmanager, its alert manager: Statup reads what Alertmanager sends. Give each alert rule a `service` label that names the service. `probe_success` comes from a probe of the blackbox exporter; for a target Prometheus scrapes itself, write `up{job="mail"} == 0`.

```yaml
groups:
  - name: services
    rules:
      - alert: MailDown
        expr: probe_success{job="mail"} == 0
        for: 2m
        labels:
          service: Mail
        annotations:
          summary: SMTP does not answer
```

Then send those alerts to Statup, grouped by service, with the key in a file. In a setup that already has its receivers, add Statup as a child route of the existing one.

```yaml
route:
  receiver: statup
  group_by: [service]

receivers:
  - name: statup
    webhook_configs:
      - url: https://status.example.com/api/v1/reports
        http_config:
          authorization:
            credentials_file: /etc/alertmanager/statup-key
```

The key file must be readable by the user Alertmanager runs as, `65534` in the official image, and by no one else; otherwise every message fails with `permission denied` in Alertmanager's log, while it starts without a word:

```bash
chown 65534 /etc/alertmanager/statup-key
chmod 400 /etc/alertmanager/statup-key
```

Resolved alerts are sent by default, and bring the service back up. `summary` goes in the Journal line of the outage, else the alert's name. An alert without a `service` label, such as the Watchdog, is left out, without a Journal line. A body is 64 KB at most: grouping by `service` keeps each one small. Alertmanager repeats an alert every four hours by default: a label that names no service leaves a Journal line each time, until it is fixed. With a wrong key, Alertmanager's log shows `unexpected status code 401`, and it tries again at the next group interval.

### Grafana

1. In each alert rule, under **Add folder and labels**, add the label `service` with the name of the service. Under **Set evaluation behavior**, set **Alert state if no data or all values are null** and **Alert state if execution error or timeout** to **Keep Last State**: otherwise Statup shows an outage whenever Grafana cannot read its data source.
2. In **Alerting**, **Notification configuration**, tab **Contact points**, add a contact point with the integration **Webhook** and the URL `https://status.example.com/api/v1/reports`.
3. In **Optional Webhook settings**, leave **Authorization Header - Scheme** at `Bearer` and put the key in **Authorization Header - Credentials**. Keep the URL without the key: Grafana stores it in plain text.
4. Send the rules that name a service to this contact point: in each rule, **Select contact point**, or a notification policy that matches `service =~ .+`. Rules without the label would reach Statup for nothing.

Grafana sends resolved alerts by default; keep it so, or services never come back up. A service comes back when the resolved message leaves, at the policy's **Group interval**, five minutes by default: shorten it in the policy's **Timing options** if that is too slow. `summary` goes in the Journal line of the outage. The contact point's **Test** changes nothing, unless a custom test carries a `service` label: then it marks that service down for real.

## Tools with a message you write

Zabbix posts a body you write: write it in Statup's format.

### Zabbix

In **Alerts**, **Media types**, create a media type of type **Webhook**. Remove the parameters Zabbix fills in, then add these six:

| Name | Value |
|---|---|
| `url` | `https://status.example.com/api/v1/reports` |
| `key` | Your key |
| `service` | `{HOST.NAME}` |
| `value` | `{EVENT.VALUE}` |
| `update` | `{EVENT.UPDATE.STATUS}` |
| `problem` | `{EVENT.NAME}` |

and this script, through the pencil next to **Script**, then **Apply**:

```text
var params = JSON.parse(value);
if (params.update === '1') {
    return 'Not a problem nor a recovery';
}
var request = new HttpRequest();
request.addHeader('Authorization: Bearer ' + params.key);
request.addHeader('Content-Type: application/json');
request.post(params.url, JSON.stringify({
    service: params.service,
    state: params.value === '1' ? 'down' : 'up',
    message: params.value === '1' ? params.problem : ''
}));
if (request.getStatus() !== 200) {
    throw 'Statup answered ' + request.getStatus();
}
return 'OK';
```

Then:

1. In the media type's **Message templates**, add one for **Problem** and one for **Problem recovery**; Zabbix's default texts do.
2. Give a user a media of this type: **Users**, **Users**, the user, tab **Media**, **Add**, this type, any text in **Send to**, enabled. Take a user of its own rather than one of **Zabbix administrators**: the default action that tells them every problem would send every host to Statup.
3. In **Alerts**, **Actions**, **Trigger actions**, **Create action** on the triggers that should tell Statup. In its **Operations** tab, add an operation that sends to that user with this media type, and a **Recovery operation** that does the same.

Keep **Pause operations for suppressed problems** checked in the action's **Operations** tab, as it is by default: a problem raised during a maintenance in Zabbix then waits for the end of the maintenance instead of reaching Statup during it.

Here the service is named after the host's visible name: write another macro in `service` to name it otherwise. `{EVENT.UPDATE.STATUS}` keeps an acknowledgement from being taken for an outage. The media type's **Test**, with its default values, fails with `Statup answered 404` and leaves a "report ignored" line in the Journal: Statup knows no service called `{HOST.NAME}`.

## Any other tool

The tools above are the ones whose steps were tried on real instances. Any other tool reaches Statup the same way, in Statup's format.

### A tool that calls a web address

If it can post to an address when an alert starts and when it ends, give it the address `https://status.example.com/api/v1/reports`, the header `Authorization: Bearer` followed by the key, and a body in [Statup's format](#statups-format): the name of the service, `down` or `up`, and a message if you like. A tool that can set no header puts the key in a `key` field of the body, sent as a form.

### A tool that runs a script

If it runs a command on each notification, as Nagios and its relatives do, this script tells Statup. Save the key's header in a file only the monitoring user reads, so the key never shows in the list of processes:

```bash
printf 'Authorization: Bearer %s\n' "stp_your_key" > /etc/statup/header
chmod 600 /etc/statup/header
```

Then save this script as `/usr/local/bin/statup-report`, executable:

```bash
#!/bin/sh
# Tells Statup that a service is down or back up.
# Usage: statup-report <service> <PROBLEM|RECOVERY> [message]
case "$2" in
    PROBLEM) state=down ;;
    RECOVERY) state=up ;;
    *) exit 0 ;;
esac
curl -fsS --max-time 10 https://status.example.com/api/v1/reports \
    -H @/etc/statup/header \
    --data-urlencode "service=$1" \
    --data-urlencode "state=$state" \
    --data-urlencode "message=$3"
```

Have the tool call it with the name of the service, `PROBLEM` or `RECOVERY`, and a message: any other second word leaves Statup as it is. A cron job can call it the same way.
