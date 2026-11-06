// An administrator makes an API key from Settings: it is shown once, masked,
// can be revealed and copied, and a reload no longer shows it. The key is then
// listed by its name and first characters, and withdrawing it asks first.
import { expect, test } from "../fixtures.js";

test("an admin makes a key, reveals and copies it, then withdraws it", async ({ page, context }, testInfo) => {
    await context.grantPermissions(["clipboard-read", "clipboard-write"]);
    // One name per project: a key left by a failed run on one never blocks the other.
    const name = `Zabbix ${testInfo.project.name}`;

    await page.goto("/admin/settings");
    await page.getByRole("link", { name: /^API/ }).click();
    await expect(page).toHaveURL(/\/admin\/api-keys$/);

    const form = page.locator("#add-key");
    await expect(form, "the add form is closed on arrival").toBeHidden();
    await page.getByRole("button", { name: "Nouvelle clé" }).click();
    await form.getByLabel("Nom", { exact: true }).fill(name);
    await form.getByRole("button", { name: "Créer la clé" }).click();

    const notice = page.getByRole("status").filter({ hasText: "Clé créée" });
    const key = notice.locator("#api-key");
    await expect(key, "the key arrives masked").toHaveAttribute("type", "password");
    await expect(key).toHaveValue(/^stp_/);
    const secret = await key.inputValue();

    await notice.getByRole("button", { name: "Afficher" }).click();
    await expect(key).toHaveAttribute("type", "text");
    await notice.getByRole("button", { name: "Masquer" }).click();
    await expect(key).toHaveAttribute("type", "password");

    await notice.getByRole("button", { name: "Copier" }).click();
    await expect(notice.getByRole("button", { name: "Copié" }), "the button confirms the copy").toBeVisible();
    expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(secret);
    await expect(key, "copying leaves the key masked").toHaveAttribute("type", "password");

    await expect(notice.getByRole("link", { name: "Documentation de l'API" })).toHaveAttribute(
        "href",
        "https://statup.dev/fr/docs/api/",
    );

    await page.reload();
    await expect(page).toHaveURL(/\/admin\/api-keys$/);
    await expect(page.locator("#api-key"), "no key after a reload").toHaveCount(0);
    const row = page.getByRole("listitem").filter({ hasText: name });
    await expect(row.getByText(`${secret.slice(0, 8)}…`)).toBeVisible();

    await row.getByLabel(`Gérer ${name}`).click();
    await expect(row.getByText("Les outils qui l'utilisent perdent l'accès.")).toBeVisible();
    await row.getByRole("button", { name: "Retirer", exact: true }).click();
    await expect(page.getByText("Clé retirée.")).toBeVisible();
    await expect(page.getByRole("listitem").filter({ hasText: name })).toHaveCount(0);
});
