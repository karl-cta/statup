// On a phone, the masthead's menu opens over the page without moving it and
// lists full-width rows, and the masthead tucks away while the reader goes
// down a long page and comes back when they go up.
import { expect, test } from "../fixtures.js";

test.skip(({ isMobile }) => !isMobile, "phone only");

test("the menu opens over the page and closes with the same button", async ({ page }) => {
    await page.goto("/");
    const button = page.getByRole("button", { name: "Menu" });
    const menu = page.getByRole("navigation", { name: "Menu", exact: true });
    const main = page.getByRole("main");
    await expect(button).toHaveAttribute("aria-expanded", "false");
    const mainTop = (await main.boundingBox()).y;

    await button.click();
    await expect(button).toHaveAttribute("aria-expanded", "true");
    const dashboard = menu.getByRole("link", { name: "Tableau de bord" });
    await expect(dashboard).toBeVisible();
    await expect(page.getByRole("button", { name: "Déconnexion" })).toBeVisible();
    expect((await main.boundingBox()).y).toBe(mainTop);

    const viewportWidth = page.viewportSize().width;
    expect((await dashboard.boundingBox()).width).toBeGreaterThanOrEqual(viewportWidth * 0.9);

    await button.click();
    await expect(button).toHaveAttribute("aria-expanded", "false");
    await expect(dashboard).toBeHidden();
});

test("the masthead tucks away going down and comes back going up", async ({ page }) => {
    await page.goto("/events/new");
    const masthead = page.getByRole("banner");
    const scrollTo = (y) => page.evaluate((top) => window.scrollTo(0, top), y);
    const maxScroll = await page.evaluate(() => document.documentElement.scrollHeight - innerHeight);
    const mastHeight = (await masthead.boundingBox()).height;
    expect(maxScroll, "a page long enough to scroll past the masthead").toBeGreaterThan(mastHeight + 100);
    await expect(masthead).not.toHaveClass(/is-tucked/);

    await scrollTo(maxScroll);
    await expect(masthead).toHaveClass(/is-tucked/);
    await expect.poll(async () => (await masthead.boundingBox()).y).toBeLessThan(0);

    await scrollTo(maxScroll - 100);
    await expect(masthead).not.toHaveClass(/is-tucked/);
    await expect.poll(async () => (await masthead.boundingBox()).y).toBeGreaterThanOrEqual(0);
});
