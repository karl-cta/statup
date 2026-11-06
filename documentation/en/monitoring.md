---
title: Monitoring
description: "Let Statup check that a website or a server answers, and show an outage on its own when it stops."
---

A service can be checked by Statup itself, every minute, from the server it runs on, so internal addresses work. A monitoring tool you already run can report outages instead, see [Monitoring tools](monitoring-tools.md).

## Set a check

In the service's form, under **Monitoring**, choose:

- **None**, the default: no check.
- **Web**, for an address that should answer. **URL** is a full `http://` or `https://` address, such as `https://intranet.example.com`, of up to 2,000 characters. Tick **Accept an internal certificate** for a tool whose certificate no public authority signed.
- **Port**, for a host and its port, such as `192.168.1.1` and `443`: a server, or a router or a firewall by its administration port. **Port** is a number from 1 to 65535.

An address that holds a user name or a password is refused: the checks keep no credentials.

**Test** runs the check at once, without saving, and says why it fails: the port is closed, there is no answer within 10 seconds, the name is unknown, no name server answers, there is no network, the certificate is refused, the server answers with an error, the address redirects in a loop, or the connection fails.

## How a check runs

- Every minute, up to 16 checks at a time, each given 10 seconds.
- **Web** sends a `GET` with the user agent `statup` and follows up to 5 redirects. Any status below 500 counts as an answer, even a refusal such as 401 or 404.
- **Port** opens a TCP connection and sends nothing. An accepted connection is an answer.
- The checks only reach the addresses you set.

## From failures to an outage

When a service fails as many checks in a row as its delay, three by default, it shows **Outage**, marked **Detected automatically**, dated from its first failed check. The first check that answers ends it, and the state set by hand comes back.

Each service sets its delay under **Monitoring**: "Declare an outage after 3 min without an answer", from 1 to 60. A critical server can show its outage after a minute, a printer after fifteen. Changing the delay, or saving the form with the same check, leaves an outage already shown as it is.

Statup never writes an incident itself. **Declare an incident**, on the banner or on the Services page, opens a new incident with the impact **Major**, the service and the start already filled in. See [Incidents](incidents.md).

## When no outage is declared

- **During a maintenance under way** that names the service, unless the maintenance is marked **No downtime**. Failures are counted but raise nothing. If the service still fails when the maintenance ends, the outage is declared at the next failed check, dated from then. A check that answers during the maintenance still ends an outage.
- **When Statup is off the network.** If every service checked in a round fails, three at least, all because Statup has no route to them or no name server answers, the checks pause for as long as that lasts. The Journal records one entry when the pause starts and one when the network is back. If every service on the page relies on the same name server and it goes down, Statup cannot tell that from its own cut and pauses too.
- **When Statup loses its connection another way.** If at least three services that were answering all fail in the same round, for any other reason, the round is ignored, for at most five rounds in a row. Two services failing together are judged as usual.

## Alerts

Services that fall in the same minute, such as the services of a server being restarted, are told in one message per channel ("3 services are not answering", with their names), and their return in one more. A service that an open incident already covers sends no alert. Declaring a maintenance before a planned restart keeps the checks quiet. See [Alerts](alerts.md).

## In the history

A detected outage of fifteen minutes or more colours its days in the service's availability history, unless an incident already covers that day, and the day reads "No answer for" and the length. A shorter one leaves the day green. See [Services](services.md#availability-history).

On a computer, a click on one of these days, on the dashboard or in a service's side panel, shows the outages of that day under **Detected outages**: when the service stopped answering and for how long. Members also read why, in the words of **Test**, or that a monitoring tool reported it; administrators get a **Journal** link to the entries of those days. Outages recorded before version 0.9.0 keep no reason.

**It was not an outage**, for editors and administrators, erases an ended outage. The box ticked under it erases with it the ended outages of the other services that began within five minutes, as when Statup or its server loses the network. The days turn green again, and the Journal records one entry for each service. An outage under way cannot be erased.

Changing the kind of check, the address, or **Accept an internal certificate** clears an outage under way, such as a false one caused by a wrong address. Outages already ended stay in the history until they are erased.

## Turn the checks off

[`MONITORING=false`](configuration.md#monitoring) stops the checks that run every minute. The addresses stay saved, and **Test** still checks an address when you press it.
