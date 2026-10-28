// An administrator picks an accent colour and an ambiance in Settings: the page
// wears them at once, saving keeps them on every page (the masthead's line
// takes the accent, lighter in the dark theme), and the defaults write nothing.
import { expect, test } from "../fixtures.js";

const NAVY_LIGHT = "rgb(31, 61, 105)";
const NAVY_DARK = "rgb(112, 161, 226)";

async function saveGeneral(page) {
    await page.getByRole("button", { name: "Enregistrer" }).click();
    await expect(page.getByRole("status").filter({ hasText: "Paramètres enregistrés." })).toBeVisible();
}

// The inputs are visually hidden: the click goes to the swatch, named by its
// title, and to the drawn choice, as a reader's would.
async function choose(page, accent, ambiance) {
    await page.goto("/admin/settings");
    await page.getByTitle(accent, { exact: true }).click();
    await page.getByRole("radiogroup", { name: "Ambiance" }).getByText(ambiance, { exact: true }).click();
    await expect(page.getByRole("radio", { name: accent, exact: true })).toBeChecked();
    await expect(page.getByRole("radio", { name: ambiance, exact: true })).toBeChecked();
}

const mastLine = (page) => page.getByRole("banner").evaluate((mast) => getComputedStyle(mast).borderBottomColor);

test("a chosen colour and ambiance show before saving and stay on every page after", async ({ page }) => {
    const html = page.locator("html");
    try {
        await choose(page, "Marine", "Galet");
        await expect(html).toHaveAttribute("data-accent", "navy");
        await expect(html).toHaveAttribute("data-ambiance", "pebble");
        await expect(page.locator("[data-accent-name]")).toHaveText("Marine");
        await expect(page.locator("[data-unsaved-note]")).toBeVisible();

        await saveGeneral(page);
        await expect(page.locator("[data-unsaved-note]")).toBeHidden();

        await page.goto("/");
        await expect(html).toHaveAttribute("data-accent", "navy");
        await expect(html).toHaveAttribute("data-ambiance", "pebble");
        await expect.poll(() => mastLine(page), "the masthead line takes the accent").toBe(NAVY_LIGHT);

        await page.evaluate(() => window.localStorage.setItem("theme", "dark"));
        await page.reload();
        await expect(html).toHaveClass(/dark/);
        await expect.poll(() => mastLine(page), "the accent is lighter in the dark theme").toBe(NAVY_DARK);
    } finally {
        await page.evaluate(() => window.localStorage.removeItem("theme"));
        await choose(page, "Encre", "Sable");
        await saveGeneral(page);
    }
});

test("the default colour and ambiance write no attribute", async ({ page }) => {
    const html = page.locator("html");
    try {
        await choose(page, "Marine", "Galet");
        await saveGeneral(page);
        await expect(html).toHaveAttribute("data-accent", "navy");

        await choose(page, "Encre", "Sable");
        await saveGeneral(page);
        await page.goto("/");
        await expect(html).not.toHaveAttribute("data-accent", /.*/);
        await expect(html).not.toHaveAttribute("data-ambiance", /.*/);
    } finally {
        await choose(page, "Encre", "Sable");
        await saveGeneral(page);
    }
});
