// Every test fails when the page reports an error: a script that throws, a
// request refused by the content security policy, a missing file.
import { test as base, expect } from "@playwright/test";

export const test = base.extend({
    page: async ({ page }, use) => {
        const errors = [];
        page.on("console", (message) => {
            if (message.type() === "error") errors.push(message.text());
        });
        page.on("pageerror", (error) => errors.push(error.message));
        await use(page);
        expect(errors, "errors in the browser console").toEqual([]);
    },
});

export { expect };
