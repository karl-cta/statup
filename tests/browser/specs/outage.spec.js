// An editor declares a critical incident on a service, sees the service go
// down on the dashboard, resolves the incident and sees it operational again.
import { expect, test } from "../fixtures.js";

test("an outage takes a service down until the incident is resolved", async ({ page }, testInfo) => {
    const service = `Paie ${testInfo.project.name}`;
    const serviceRow = () =>
        page
            .getByRole("region", { name: "Services", exact: true })
            .filter({ visible: true })
            .getByRole("listitem")
            .filter({ hasText: service });

    await page.goto("/services/new");
    await page.getByLabel("Nom", { exact: true }).fill(service);
    await page.getByRole("button", { name: "Créer le service" }).click();
    await expect(page).toHaveURL(/\/services\?saved=/);

    await page.goto("/events/new");
    await page.getByText("Incident", { exact: true }).click();
    await page.getByText("Majeur", { exact: true }).click();
    await page.getByLabel("Titre").fill(`Panne de la paie ${testInfo.project.name}`);
    await page.getByLabel("Description").fill("La paie ne répond plus.");
    await page.getByText(service, { exact: true }).click();
    await page.getByRole("button", { name: "Publier l'incident" }).click();
    await expect(page.getByText("Publié, visible dès maintenant.")).toBeVisible();

    const incidentUrl = page.url();
    await page.goto("/");
    await expect(serviceRow().getByText("En panne", { exact: true })).toBeVisible();

    await page.goto(incidentUrl);
    await page.getByRole("link", { name: "Publier une mise à jour" }).click();
    await page.getByLabel("Nouvelle mise à jour").fill("Le service est rétabli.");
    await page.getByRole("combobox", { name: "Avancement" }).click();
    await page.getByRole("option", { name: "Résolu" }).click();
    await page.getByRole("button", { name: "Publier", exact: true }).click();
    await expect(page.getByText("Le service est rétabli.")).toBeVisible();

    await page.goto("/");
    await expect(serviceRow().getByText("Opérationnel", { exact: true })).toBeVisible();
});
