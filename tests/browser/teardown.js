// Removes the throwaway instance's data folder once every journey has run.
import { rmSync } from "node:fs";

export default function teardown() {
    rmSync(process.env.STATUP_BROWSER_DATA, { recursive: true, force: true });
}
