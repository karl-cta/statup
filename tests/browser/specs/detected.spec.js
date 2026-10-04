// The checks find a service down: the banner says the outage was detected
// automatically, and an editor opens its incident from there, filled in.
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

import { expect, test } from "../fixtures.js";

// What the background task records after three failed rounds; waiting for
// them would take three minutes.
function recordDetectedOutage(serviceName) {
    const db = new DatabaseSync(join(process.env.STATUP_BROWSER_DATA, "statup.db"));
    try {
        const { id } = db.prepare("SELECT id FROM services WHERE name = ?").get(serviceName);
        db.prepare("UPDATE services SET detected_status = 'major_outage', status = 'major_outage' WHERE id = ?").run(id);
        db.prepare("INSERT INTO service_outages (service_id, started_at) VALUES (?, datetime('now', '-5 minutes'))").run(id);
        return id;
    } finally {
        db.close();
    }
}

test("an outage the checks found leads an editor to its incident", async ({ page }, testInfo) => {
    const service = `Wiki interne ${testInfo.project.name}`;

    await page.goto("/services/new");
    await page.getByLabel("Nom", { exact: true }).fill(service);
    await page.getByText("Port", { exact: true }).click();
    await page.getByLabel("Adresse et port").fill("127.0.0.1:1");
    await page.getByRole("button", { name: "Créer le service" }).click();
    await expect(page).toHaveURL(/\/services\?saved=/);

    const id = recordDetectedOutage(service);

    await page.goto("/");
    const bannerRow = page
        .getByRole("listitem")
        .filter({ hasText: service })
        .filter({ hasText: "Détecté automatiquement" });
    await expect(bannerRow.getByText("Déclarer un incident")).toBeVisible();

    await bannerRow.getByRole("link").click();
    await expect(page).toHaveURL(new RegExp(`/events/new\\?service=${id}$`));
    await expect(page.getByRole("checkbox", { name: service })).toBeChecked();
});
