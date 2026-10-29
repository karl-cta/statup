// An administrator finds the Backups page from the settings, sets the
// schedule of the automatic backups in its panel (the day menu, the time typed
// as "22h15", the count), sees the panel fit its content, and downloads a
// backup on the spot.
import { readFileSync } from "node:fs";

import { expect, test } from "../fixtures.js";

const SUMMARY = ".journal-summary";

async function openPanel(page) {
    await page.goto("/admin/backups");
    await page.locator(".journal-edit").click();
    await expect(page.locator(".backup-panel")).toBeVisible();
}

async function chooseRhythm(page, name) {
    await page.locator(".backup-panel .journal-option").filter({ hasText: name }).click();
}

async function save(page) {
    await page.locator(".backup-panel").getByRole("button", { name: "Enregistrer" }).click();
    await expect(page).toHaveURL(/\/admin\/backups\?saved=1$/);
    await expect(page.getByText("Sauvegardes automatiques enregistrées.")).toBeVisible();
}

// The schedule every other journey expects: each day at 03:00, seven kept,
// one before each upgrade.
async function restoreDefaultSchedule(page) {
    await openPanel(page);
    await chooseRhythm(page, "Chaque jour");
    await page.locator('.backup-panel input[name="time"]').fill("03:00");
    await page.locator('.backup-panel input[name="keep"]').fill("7");
    await page.locator('.backup-panel input[name="before_upgrade"]').check();
    await save(page);
}

test("the settings lead to the backups page", async ({ page }) => {
    await page.goto("/admin/settings");
    await page.getByRole("link", { name: /Sauvegardes/ }).click();

    await expect(page).toHaveURL(/\/admin\/backups$/);
    await expect(page.getByRole("heading", { level: 1 })).toHaveText("Sauvegardes");
});

test("a weekly schedule is typed, saved and kept after a reload", async ({ page }) => {
    try {
        await openPanel(page);
        const day = page.locator(".backup-panel .cs-wrap");
        const time = page.locator('.backup-panel input[name="time"]');
        const keep = page.locator('.backup-panel input[name="keep"]');

        await expect(day).toBeHidden();
        await chooseRhythm(page, "Chaque semaine");
        await expect(day).toBeVisible();
        await expect(time).toBeVisible();
        await expect(keep).toBeVisible();

        await day.getByRole("combobox").click();
        await page.getByRole("option", { name: "vendredi" }).click();
        await expect(day.getByRole("combobox")).toContainText("vendredi");
        await time.fill("22h15");
        await keep.fill("14");
        await expect(time).toBeVisible();
        await expect(keep).toBeVisible();
        await save(page);

        await expect(page.locator(SUMMARY)).toContainText("Chaque vendredi à 22:15");
        await expect(page.locator(SUMMARY)).toContainText("14 gardées");

        await page.reload();
        await expect(page.locator(SUMMARY)).toContainText("Chaque vendredi à 22:15");
        await expect(page.locator(SUMMARY)).toContainText("14 gardées");
    } finally {
        await restoreDefaultSchedule(page);
    }
});

test("the panel and the day menu fit their content", async ({ page }) => {
    await openPanel(page);
    await chooseRhythm(page, "Chaque semaine");
    const panel = page.locator(".backup-panel");
    const day = page.locator(".backup-panel .cs-wrap");
    await expect(day).toBeVisible();

    const panelBox = await panel.boundingBox();
    const dayBox = await day.boundingBox();
    expect(panelBox.width).toBeLessThanOrEqual(360);
    expect(dayBox.width).toBeLessThanOrEqual(140);

    const viewportWidth = page.viewportSize().width;
    expect(panelBox.x).toBeGreaterThanOrEqual(0);
    expect(panelBox.x + panelBox.width).toBeLessThanOrEqual(viewportWidth);
    const overflow = await page.evaluate(
        () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
    );
    expect(overflow).toBeLessThanOrEqual(0);
});

test("a backup is downloaded on the spot as a database file", async ({ page }) => {
    await page.goto("/admin/backups");
    const [download] = await Promise.all([
        page.waitForEvent("download"),
        page.getByRole("button", { name: "Télécharger une sauvegarde" }).click(),
    ]);

    const name = download.suggestedFilename();
    expect(name).toMatch(/^statup-backup-.*\.db$/);
    const saved = readFileSync(await download.path());
    expect(saved.subarray(0, 16)).toEqual(Buffer.from("SQLite format 3\0", "latin1"));
});
