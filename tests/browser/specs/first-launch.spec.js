import { expect, test } from "../fixtures.js";

test("a fresh instance opens on the first account screen", async ({ page }) => {
    await page.goto("/");
    await expect(page).toHaveURL(/\/register$/);
    await expect(page.getByRole("heading", { level: 1 })).toHaveText("Bienvenue dans Statup");
    await expect(page.getByLabel("Adresse e-mail")).toBeVisible();
    await expect(page.getByRole("button", { name: "Continuer" })).toBeVisible();
});
