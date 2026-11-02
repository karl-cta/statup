---
title: Status widget
description: "A small box to paste into an intranet or a wiki, which repeats the banner of the status page and refreshes itself."
---

The widget is a box 68 px high that fills the width it is given, on two lines. The whole box is a link that opens the status page in a new tab (**View**). Administrators get its code from **Settings**, **Widget**, between **Alerts** and **API**.

## What it shows

The widget repeats the banner of the [status page](status-page.md), with the same rules:

- Everything works: **All services are working normally**, and under it the page name and **Service status**.
- A disruption: the title of the most serious open incident, **+ 1 more** when other problems are open, and under it the service and its state, or **2 services disrupted** when several are. The line also says when the incident was last updated. The box takes the tint of the state, as the banner does.
- A maintenance under way: its title, the service and until when it runs, on a blue tint.
- A maintenance announced within the next seven days, when nothing is disrupted: **Maintenance planned for** and its date. A repeating maintenance is not announced here.
- A service that the checks found down without an incident: the service, its state and since when.

The widget refreshes itself every minute, without any script. In a narrow column the text is cut with an ellipsis; **+ 1 more** and **View** stay.

The widget has a request limit of its own, apart from the pages: 6,000 a minute from one address, enough for a whole office behind one company address to keep it open.

## Get the code

**Settings**, **Widget**, has three parts:

- **Preview** shows the widget with the current state, as a member sees it, in **Light**, **Dark** or **Match the device**. The theme chosen changes the address and the code below.
- **To paste** gives the **Address**, with **Copy the address**, and the suggested height, 68 px. **HTML code** holds an `iframe` that points to the address, with **Copy the code**.
- **Change the link**, in red at the foot, is described in [The private link](#the-private-link).

The address starts with [`PUBLIC_URL`](configuration.md#public_url) when it is set, otherwise with the **Page URL** of the [Alerts](alerts.md#page-url) page, otherwise with the address the administrator's browser used. Set one of the first two before copying, so the widget points to the address your colleagues reach.

## Paste it

A page that accepts HTML takes the **HTML code**:

```html
<iframe src="https://status.example.com/embed?key=YOUR-PRIVATE-KEY&amp;lang=en" title="Service status" width="100%" height="68" style="border:0"></iframe>
```

A tool that asks for an address and a height, such as an iframe block of a wiki, takes the **Address** and 68 px.

In Notion, type `/embed`, choose **Embed**, paste the **Address** and confirm with **Embed link**. Notion keeps its blocks taller than the widget, which stays at the top of its block. Choose **Match the device** before copying, so the widget follows Notion's dark mode.

Only the widget can be shown inside another site: every other page of Statup refuses to be framed.

> [!NOTE]
> Some intranets only show a frame from an address their administrator allowed. If the widget stays blank, ask them to allow the Statup address.

An intranet on the internet, such as Notion, with Statup on the company network:

- Give Statup an `https` address: a page served in `https` refuses a frame in `http`.
- Chrome and Edge treat the widget as a request to the local network. They can ask each reader to allow it, and the widget stays blank until they do. An administrator allows it for every reader with the [`LocalNetworkAccessAllowedForUrls`](https://chromeenterprise.google/policies/local-network-access-allowed-for-urls/) policy, set to the address of the intranet.

## The private link

The address and the code always carry the private link of the instance, the `key` of the address. With it the widget shows what any member sees: the services open to everyone and to members, and never a service restricted to groups.

Without the link, or with an old one, the widget shows what a visitor sees. On a page set to **Members only**, that is no state at all: just **Service status** and **Copy this widget's link again from Statup**.

The intranet where the widget is pasted is usually behind the company sign-in, so the link stays among colleagues. If it leaks, click **Change** under **Change the link** and confirm: the old link stops working at once wherever it is pasted, and a line is added to the [Journal](journal.md). Paste the new address or code again where the widget is.

## Theme and language

The theme is part of the address: `theme=dark` for **Dark**, `theme=auto` for **Match the device**, nothing for **Light**.

The address also carries the language Statup was in when it was copied, `lang=en` for example, and the widget speaks it in every tool. To paste it in another language, switch Statup to that language before copying. Without `lang`, the widget speaks the language the reader's browser asks for.
