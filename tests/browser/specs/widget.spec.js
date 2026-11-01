// An administrator opens the status widget from Settings, picks its theme and
// copies its address, then frames it on another page the way an intranet
// would. Changing the link gives a new address, and the old one stops being
// the members' view. The test changes the one link of the shared instance, so
// it runs once, not on a phone too.
import { createServer } from "node:http";

import { expect, test } from "../fixtures.js";

test.skip(({ isMobile }) => isMobile, "the same page on a phone, checked once");

const ADDRESS = /^https?:\/\/[^/]+\/embed\?key=([A-Za-z0-9]{32})&lang=fr(&theme=(dark|auto))?$/;

// An intranet page served by another site on this machine: Chrome blocks a
// page it takes for public from framing a local address, so the page that
// frames the widget is local too, at another origin.
async function framedWidget(context, address) {
    // Escaped like the code the page offers to paste.
    const src = address.replaceAll("&", "&amp;");
    const body = `<!doctype html><title>Intranet</title><iframe src="${src}" title="Widget" width="600" height="68"></iframe>`;
    const server = createServer((_, response) => {
        response.writeHead(200, { "content-type": "text/html; charset=utf-8" });
        response.end(body);
    });
    await new Promise((ready) => server.listen(0, "127.0.0.1", ready));
    const page = await context.newPage();
    await page.goto(`http://localhost:${server.address().port}/`);
    page.on("close", () => server.close());
    return page.frameLocator("iframe").locator("a.embed");
}

// The reply for a private link is kept private; the one for no valid link is not.
async function cacheControl(page, address) {
    const answer = await page.request.get(address);
    expect(answer.status()).toBe(200);
    return answer.headers()["cache-control"];
}

test("an admin copies the widget address, frames it, then changes the link", async ({ page, context }) => {
    await context.grantPermissions(["clipboard-read", "clipboard-write"]);

    await page.goto("/admin/settings");
    await page.getByRole("link", { name: /^Widget/ }).click();
    await expect(page).toHaveURL(/\/admin\/embed$/);
    await expect(page.getByRole("heading", { level: 1 })).toHaveText("Widget");

    const address = page.locator("#embed-address");
    await expect(address).toHaveValue(/^https?:\/\/[^/]+\/embed\?key=[A-Za-z0-9]{32}&lang=fr$/);
    const light = await address.inputValue();
    const oldKey = light.match(ADDRESS)[1];

    const themes = page.getByRole("navigation", { name: "Thème" });
    await expect(themes.getByRole("link", { name: "Clair" })).toHaveAttribute("aria-current", "true");
    await expect(page.locator(".embed-preview")).not.toHaveClass(/\bdark\b/);

    await themes.getByRole("link", { name: "Sombre" }).click();
    await expect(page).toHaveURL(/\/admin\/embed\?theme=dark$/);
    await expect(themes.getByRole("link", { name: "Sombre" })).toHaveAttribute("aria-current", "true");
    await expect(page.locator(".embed-preview")).toHaveClass(/\bdark\b/);
    await expect(address).toHaveValue(`${light}&theme=dark`);
    await expect(page.locator("#embed-code")).toHaveValue(/<iframe src=".*&amp;theme=dark" title="État des services"/);

    await themes.getByRole("link", { name: "Selon l'appareil" }).click();
    await expect(page).toHaveURL(/\/admin\/embed\?theme=auto$/);
    await expect(address).toHaveValue(`${light}&theme=auto`);

    await themes.getByRole("link", { name: "Sombre" }).click();
    await page.getByRole("button", { name: "Copier l'adresse" }).click();
    await expect(page.getByRole("button", { name: "Adresse copiée" }), "the button confirms the copy").toBeVisible();
    const copied = await page.evaluate(() => navigator.clipboard.readText());
    expect(copied).toBe(`${light}&theme=dark`);

    const widget = await framedWidget(context, copied);
    await expect(widget, "the widget shows in a frame of another page").toBeVisible();
    await expect(widget).toContainText("Voir");
    expect(await cacheControl(page, copied), "the old link is a private one").toBe("private, no-cache");

    await page.getByRole("button", { name: "Changer", exact: true }).click();
    const confirmation = page.getByRole("group", { name: "Confirmer le changement du lien" });
    await expect(confirmation).toContainText(/Changer le lien\s*\?/);
    await confirmation.getByRole("button", { name: "Changer" }).click();

    await expect(
        page.getByText("Lien changé. Recollez l'adresse ou le code partout où le widget est collé."),
    ).toBeVisible();
    await expect(address).toHaveValue(ADDRESS);
    const renewed = await address.inputValue();
    expect(renewed.match(ADDRESS)[1], "the key is new").not.toBe(oldKey);

    const fresh = await framedWidget(context, renewed);
    await expect(fresh).toContainText("Voir");
    expect(await cacheControl(page, renewed), "the new link is a private one").toBe("private, no-cache");

    // The instance is open to everyone, so the old address still shows a
    // widget, the visitor's: it is no longer the members' view.
    const stale = await framedWidget(context, copied);
    await expect(stale).toContainText("Voir");
    await expect(stale).not.toContainText("Lien du widget à recopier depuis Statup");
    expect(await cacheControl(page, copied), "the old link no longer opens the members' view").toBe("no-cache");
});
