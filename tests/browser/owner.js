// The account the first launch creates. Throwaway values for a throwaway
// instance on 127.0.0.1.
import { join } from "node:path";

export const OWNER = {
    name: "Owner",
    email: "owner@example.com",
    password: "Browser_owner_12",
};

export const OWNER_SESSION = join(import.meta.dirname, ".auth", "owner.json");
