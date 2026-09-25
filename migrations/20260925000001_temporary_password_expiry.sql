-- A password chosen by someone else stops opening the account after a week:
-- set with the temporary password, cleared when the person sets their own.
-- Temporary passwords issued before this column get their week from now.
ALTER TABLE users ADD COLUMN temporary_password_expires_at TEXT;
UPDATE users SET temporary_password_expires_at = datetime('now', '+7 days')
    WHERE must_change_password = 1;
