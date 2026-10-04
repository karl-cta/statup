// The first launch of a fresh instance, played once before every other
// journey: the owner's account, a page open to everyone and one service.
// The session it leaves behind is the one the other journeys sign in with.
import { expect, test } from "../fixtures.js";
import { OWNER, OWNER_SESSION } from "../owner.js";

test("a fresh instance is set up from its first account", async ({ page }) => {
    await page.goto("/");
    await expect(page).toHaveURL(/\/register$/);
    await expect(page.getByRole("heading", { level: 1 })).toHaveText("Bienvenue dans Statup");

    await page.getByLabel("Nom", { exact: true }).fill(OWNER.name);
    await page.getByLabel("Adresse e-mail").fill(OWNER.email);
    await page.getByRole("textbox", { name: "Mot de passe" }).fill(OWNER.password);
    await page.getByRole("button", { name: "Continuer" }).click();

    await expect(page).toHaveURL(/\/setup\/page$/);
    await page.getByLabel("Nom affiché").fill("Acme IT");
    await page.getByText("Tout le monde", { exact: true }).click();
    await expect(page.getByRole("radio", { name: /Tout le monde/ })).toBeChecked();
    await page.getByRole("button", { name: "Continuer" }).click();

    await expect(page).toHaveURL(/\/setup\/services$/);
    await page.getByLabel("Nom d'un service").fill("Intranet");
    await page.getByRole("button", { name: "Ajouter" }).click();
    await page.getByRole("button", { name: "Continuer" }).click();

    await expect(page).toHaveURL(/\/setup\/done$/);
    await expect(page.getByRole("heading", { level: 1 })).toHaveText("Votre page est prête");
    await page.context().storageState({ path: OWNER_SESSION });

    await page.getByRole("link", { name: "Ouvrir le tableau de bord" }).click();
    await expect(page.getByText("Intranet", { exact: true }).filter({ visible: true }).first()).toBeVisible();
});
