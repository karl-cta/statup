// An editor announces a maintenance that repeats every week: the session's
// page names the rhythm, the dashboard lists the session without the banner
// announcing it ahead, and stopping the repetition cancels the session.
import { expect, test } from "../fixtures.js";

// The house calendar runs on the instance clock, set to UTC.
function weekAhead() {
    const now = new Date();
    const day = new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate() + 7));
    const label = new Intl.DateTimeFormat("fr", {
        weekday: "long",
        day: "numeric",
        month: "long",
        year: "numeric",
        timeZone: "UTC",
    }).format(day);
    return {
        label: label.charAt(0).toUpperCase() + label.slice(1),
        weekday: new Intl.DateTimeFormat("fr", { weekday: "long", timeZone: "UTC" }).format(day),
        inNextMonth: day.getUTCMonth() !== now.getUTCMonth(),
    };
}

async function announceWeekly(page, { service, title }) {
    const target = weekAhead();
    await page.goto("/events/new");
    await page.getByText("Maintenance", { exact: true }).click();
    await page.getByRole("button", { name: /^Début/ }).click();
    if (target.inNextMonth) await page.getByRole("button", { name: "Mois suivant" }).click();
    await page.getByRole("button", { name: target.label, exact: true }).click();
    await page.getByRole("combobox", { name: "Heure de début" }).fill("10:00");
    await page.getByLabel("Heures", { exact: true }).fill("2");
    await page.getByText(service, { exact: true }).click();
    await page.getByLabel("Répéter", { exact: true }).check();
    await page.getByText(`Chaque ${target.weekday}`, { exact: true }).click();
    await page.getByLabel("Titre").fill(title);
    await page.getByRole("button", { name: "Annoncer la maintenance" }).click();
    await expect(page.getByText("Événement publié.")).toBeVisible();
    return target;
}

test("a weekly maintenance is listed, kept out of the banner, and cancelled when its repetition stops", async ({ page }, testInfo) => {
    const service = `Sauvegardes ${testInfo.project.name}`;
    const title = `Rotation hebdomadaire ${testInfo.project.name}`;
    const details = () => page.getByRole("complementary", { name: "Détails" });

    await page.goto("/services/new");
    await page.getByLabel("Nom", { exact: true }).fill(service);
    await page.getByRole("button", { name: "Créer le service" }).click();
    await expect(page).toHaveURL(/\/services\?saved=/);

    const target = await announceWeekly(page, { service, title });
    const sessionUrl = page.url();
    await expect(details().getByText("Répétition", { exact: true })).toBeVisible();
    await expect(details().getByText(`Chaque ${target.weekday}`, { exact: true })).toBeVisible();

    await page.goto("/");
    const block = page.getByRole("region", { name: "Maintenances", exact: true }).filter({ visible: true });
    await expect(block.getByText(title)).toBeVisible();
    await expect(page.locator(".hero").filter({ visible: true })).not.toContainText(title);

    await page.goto(sessionUrl);
    await details().getByRole("button", { name: "Arrêter la répétition" }).click();
    await details().getByRole("button", { name: "Arrêter", exact: true }).click();
    await expect(details().getByText("Répétition", { exact: true })).toHaveCount(0);
    await expect(details().getByText("Annulée", { exact: true })).toBeVisible();
});
