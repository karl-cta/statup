-- The links that let a member who forgot their password choose a new one.
-- The link carries the row id and a secret; only the secret's hash is kept.
-- Only the newest link of an account opens it, for an hour, and every link
-- of the account is deleted once one is used. Rows stay a day so the number
-- of links sent in the last hour can be counted.
CREATE TABLE password_resets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    secret_hash TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_password_resets_user ON password_resets(user_id);
