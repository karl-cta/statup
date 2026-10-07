// An admin adds a notification destination: the form says where to find the
// address of the chosen tool, a test reaches the tool before anything is
// saved, and the destination is listed afterwards.
import { createServer } from "node:http";

import { expect, test } from "../fixtures.js";

// A tool on this machine that accepts every post and keeps what it got.
function startTool() {
    const received = [];
    const server = createServer((request, response) => {
        let body = "";
        request.on("data", (chunk) => {
            body += chunk;
        });
        request.on("end", () => {
            received.push({ method: request.method, url: request.url, body });
            response.writeHead(200);
            response.end("ok");
        });
    });
    return new Promise((resolve) => {
        server.listen(0, "127.0.0.1", () => resolve({ server, received, port: server.address().port }));
    });
}

test("an admin adds a destination, tests it, and finds it listed", async ({ page }, testInfo) => {
    const tool = await startTool();
    try {
        const name = `Tableau n8n ${testInfo.project.name}`;

        await page.goto("/admin/settings");
        await page.getByRole("link", { name: "Gérer les notifications" }).click();
        await expect(page).toHaveURL(/\/admin\/notifications$/);

        const form = page.locator("form[data-destination-form]");
        if (!(await form.isVisible())) await page.getByText("Ajouter une destination").click();
        await page.getByRole("combobox", { name: "Outil" }).click();
        await page.getByRole("option", { name: "Autre outil (webhook)" }).click();
        await expect(form.getByText("Statup y envoie un JSON")).toBeVisible();

        await form.getByLabel("Nom", { exact: true }).fill(name);
        await form.getByLabel("Adresse du webhook").fill(`http://127.0.0.1:${tool.port}/hook`);
        await form.getByRole("button", { name: "Envoyer un essai" }).click();
        await expect(form.getByText("Message d'essai envoyé")).toBeVisible();
        expect(tool.received).toHaveLength(1);
        expect(tool.received[0].method).toBe("POST");
        expect(tool.received[0].body).toContain('"happening":"test"');

        await form.getByRole("button", { name: "Ajouter", exact: true }).click();
        await expect(page).toHaveURL(/\/admin\/notifications\?added=/);
        const row = page.getByRole("listitem").filter({ hasText: name });
        await expect(row.getByText("Aucun envoi pour l'instant")).toBeVisible();
    } finally {
        tool.server.close();
    }
});
