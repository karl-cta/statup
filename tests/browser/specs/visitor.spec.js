// A visitor without an account reads a page open to everyone, and finds
// where to sign in: in the masthead on a desktop, in the menu on a phone.
import { expect, test } from "../fixtures.js";

test.use({ storageState: { cookies: [], origins: [] } });

test("a visitor reads the page without signing in", async ({ page }, testInfo) => {
    await page.goto("/");
    await expect(page).toHaveURL(/\/$/);
    await expect(
        page.getByText("Intranet", { exact: true }).filter({ visible: true }).first(),
    ).toBeVisible();

    if (testInfo.project.name === "phone") {
        await page.getByRole("button", { name: "Menu" }).click();
    }
    await expect(page.getByRole("link", { name: "Connexion" }).filter({ visible: true })).toHaveCount(1);
});
