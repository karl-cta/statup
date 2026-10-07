// On a phone, the masthead's menu opens over the page without moving it and
// lists full-width rows, the masthead tucks away while the reader goes down a
// long page and comes back when they go up, and the event filters fold behind
// one button beside the search.
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
    await page.goto("/");
    // A page long enough to scroll, whatever the dashboard holds today.
    await page.getByRole("main").evaluate((main) => {
        const room = document.createElement("div");
        room.style.height = "3000px";
        main.append(room);
    });
    const masthead = page.getByRole("banner");
    const scrollTo = (y) => page.evaluate((top) => window.scrollTo(0, top), y);
    const maxScroll = await page.evaluate(() => document.documentElement.scrollHeight - innerHeight);
    await expect(masthead).not.toHaveClass(/is-tucked/);

    await scrollTo(maxScroll);
    await expect(masthead).toHaveClass(/is-tucked/);
    await expect.poll(async () => (await masthead.boundingBox()).y).toBeLessThan(0);

    await scrollTo(maxScroll - 100);
    await expect(masthead).not.toHaveClass(/is-tucked/);
    await expect.poll(async () => (await masthead.boundingBox()).y).toBeGreaterThanOrEqual(0);
});

test("the event filters fold behind one button beside the search", async ({ page }) => {
    // A search shows the filters even on an instance without events.
    await page.goto("/events?q=zzz");
    const button = page.getByRole("button", { name: "Filtres" });
    const kind = page.locator("#filters-more").getByText("Type", { exact: true });
    await expect(button).toHaveAttribute("aria-expanded", "false");
    await expect(kind).toBeHidden();
    const search = await page.getByRole("searchbox").boundingBox();
    const box = await button.boundingBox();
    expect(Math.abs(box.y + box.height - (search.y + search.height))).toBeLessThan(2);

    await button.click();
    await expect(button).toHaveAttribute("aria-expanded", "true");
    await expect(kind).toBeVisible();

    await button.click();
    await expect(kind).toBeHidden();
});
