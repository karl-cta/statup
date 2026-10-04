// Browser tests: a fresh Statup instance on port 3100, driven by Chromium
// as a desktop and as a phone. Run them with scripts/browser-tests.sh.
import { execFileSync } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

import { defineConfig, devices } from "@playwright/test";

import { OWNER_SESSION } from "./owner.js";

const ROOT = resolve(import.meta.dirname, "../..");
const PORT = 3100;

// The config is loaded again by every worker: the instance's data folder and
// the binary's path are worked out once, by the main process, and handed
// down through the environment.
process.env.STATUP_BROWSER_DATA ??= mkdtempSync(join(tmpdir(), "statup-browser-"));
process.env.STATUP_BROWSER_BINARY ??= binaryPath();
const DATA = process.env.STATUP_BROWSER_DATA;

function binaryPath() {
    const metadata = execFileSync("cargo", ["metadata", "--format-version", "1", "--no-deps"], {
        cwd: ROOT,
        encoding: "utf8",
    });
    return join(JSON.parse(metadata).target_directory, "debug", "statup");
}

export default defineConfig({
    testDir: "./specs",
    globalTeardown: "./teardown.js",
    testMatch: /\.(spec|setup)\.js$/,
    workers: 1,
    retries: 0,
    forbidOnly: Boolean(process.env.CI),
    reporter: process.env.CI ? [["list"], ["html", { open: "never" }]] : "list",
    use: {
        baseURL: `http://127.0.0.1:${PORT}`,
        locale: "fr-FR",
        reducedMotion: "reduce",
        trace: "retain-on-failure",
        video: "retain-on-failure",
    },
    projects: [
        {
            name: "setup",
            testMatch: /\.setup\.js$/,
            use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 800 } },
        },
        {
            name: "desktop",
            dependencies: ["setup"],
            testIgnore: /\.setup\.js$/,
            use: {
                ...devices["Desktop Chrome"],
                viewport: { width: 1280, height: 800 },
                storageState: OWNER_SESSION,
            },
        },
        {
            name: "phone",
            dependencies: ["setup"],
            testIgnore: /\.setup\.js$/,
            use: { ...devices["Pixel 7"], storageState: OWNER_SESSION },
        },
    ],
    webServer: {
        command: process.env.STATUP_BROWSER_BINARY,
        cwd: ROOT,
        url: `http://127.0.0.1:${PORT}/health`,
        reuseExistingServer: false,
        env: {
            DATABASE_URL: join(DATA, "statup.db"),
            UPLOAD_DIR: join(DATA, "uploads"),
            HOST: "127.0.0.1",
            PORT: String(PORT),
            DEFAULT_LOCALE: "fr",
            UPDATE_CHECK: "false",
            ADMIN_EMAIL: "",
            ADMIN_PASSWORD: "",
            LOG_LEVEL: "warn",
        },
    },
});
