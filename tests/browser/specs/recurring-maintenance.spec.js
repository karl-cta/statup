// An editor announces a maintenance that repeats every week: the session's
// page names the rhythm, the dashboard lists the session without the banner
// announcing it ahead, and stopping the repetition cancels the session.
// A maintenance still to come offers its dates to a calendar.
import { readFile } from "node:fs/promises";

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

async function createService(page, name) {
    await page.goto("/services/new");
    await page.getByLabel("Nom", { exact: true }).fill(name);
    await page.getByRole("button", { name: "Créer le service" }).click();
    await expect(page).toHaveURL(/\/services\?saved=/);
}

async function announceMaintenance(page, { service, title, repeat }) {
    const target = weekAhead();
    await page.goto("/events/new");
    await page.getByText("Maintenance", { exact: true }).click();
    await page.getByRole("button", { name: /^Début/ }).click();
    if (target.inNextMonth) await page.getByRole("button", { name: "Mois suivant" }).click();
    await page.getByRole("button", { name: target.label, exact: true }).click();
    await page.getByRole("combobox", { name: "Heure de début" }).fill("10:00");
    await page.getByLabel("Heures", { exact: true }).fill("2");
    await page.getByText(service, { exact: true }).click();
    if (repeat) {
        await page.getByLabel("Répéter", { exact: true }).check();
        await page.getByText(`Chaque ${target.weekday}`, { exact: true }).click();
    }
    await page.getByLabel("Titre").fill(title);
    await page.getByRole("button", { name: "Annoncer la maintenance" }).click();
    await expect(page.getByText("Événement publié.")).toBeVisible();
    return target;
}

test("a weekly maintenance is listed, kept out of the banner, and cancelled when its repetition stops", async ({ page }, testInfo) => {
    const service = `Sauvegardes ${testInfo.project.name}`;
    const title = `Rotation hebdomadaire ${testInfo.project.name}`;
    const details = () => page.getByRole("complementary", { name: "Détails" });

    await createService(page, service);

    const target = await announceMaintenance(page, { service, title, repeat: true });
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

test("a maintenance still to come can be added to a calendar", async ({ page }, testInfo) => {
    const service = `Pare-feu ${testInfo.project.name}`;
    const title = `Mise à jour du pare-feu ${testInfo.project.name}`;
    const details = page.getByRole("complementary", { name: "Détails" });

    await createService(page, service);
    await announceMaintenance(page, { service, title, repeat: false });

    const summary = details.getByText("Ajouter à mon agenda", { exact: true });
    await expect(summary).toBeVisible();
    await summary.click();
    const outlook = details.getByRole("link", { name: "Outlook", exact: true });
    const apple = details.getByRole("link", { name: "Apple", exact: true });
    const google = details.getByRole("link", { name: "Google Agenda", exact: true });
    await expect(outlook).toBeVisible();
    await expect(apple).toBeVisible();
    await expect(google).toBeVisible();

    const downloading = page.waitForEvent("download");
    await outlook.click();
    const download = await downloading;
    expect(download.suggestedFilename()).toMatch(/\.ics$/);
    const file = await readFile(await download.path(), "utf8");
    expect(file).toContain("BEGIN:VCALENDAR");
    expect(file).toContain(title);

    // Google Calendar is an external site: its link opens a new tab.
    await expect(google).toHaveAttribute("target", "_blank");

    await page.keyboard.press("Escape");
    await expect(outlook).toBeHidden();
});
