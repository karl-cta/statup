// An editor creates a service, finds it last, moves it up with the arrows
// of the Services page, and the order holds after a reload.
import { expect, test } from "../fixtures.js";

test("services keep the order chosen with the arrows", async ({ page }, testInfo) => {
    const last = `Ordre ${testInfo.project.name}`;
    const names = () =>
        page
            .locator("[data-service-order] > li")
            .evaluateAll((rows) => rows.map((row) => row.dataset.serviceName));

    await page.goto("/services/new");
    await page.getByLabel("Nom", { exact: true }).fill(last);
    await page.getByRole("button", { name: "Créer le service" }).click();
    await expect(page).toHaveURL(/\/services\?saved=/);
    const before = await names();
    expect(before.at(-1), "a new service goes last").toBe(last);

    await page.getByRole("button", { name: "Changer l'ordre" }).click();
    await expect(page.getByRole("button", { name: `Descendre ${last}` })).toBeDisabled();
    await page.getByRole("button", { name: `Monter ${last}` }).click();
    const moved = [...before.slice(0, -2), last, before.at(-2)];
    await expect.poll(names).toEqual(moved);
    // Either arrow of the moved row: at the top, its up arrow is disabled.
    await expect(page.locator(":focus")).toHaveAttribute("aria-label", new RegExp(last));

    const saved = page.waitForResponse((response) => response.url().endsWith("/services/order"));
    await page.getByRole("button", { name: "Terminer" }).click();
    expect((await saved).status()).toBe(204);

    await page.reload();
    expect(await names()).toEqual(moved);
});
