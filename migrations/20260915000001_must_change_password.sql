-- Set when someone else chose the password (an administrator adding a member),
-- cleared when the person sets their own. Existing accounts are left alone.

ALTER TABLE users ADD COLUMN must_change_password BOOLEAN NOT NULL DEFAULT 0;
