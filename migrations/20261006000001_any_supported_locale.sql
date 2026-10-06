-- The preferred language no longer lists its values in the table: the
-- application checks them against the languages it ships, so adding one
-- needs no migration. SQLite cannot drop a CHECK in place; the column is
-- swapped for a copy without it, under the same name.
ALTER TABLE users ADD COLUMN locale TEXT;
UPDATE users SET locale = preferred_locale WHERE preferred_locale IS NOT NULL;
ALTER TABLE users DROP COLUMN preferred_locale;
ALTER TABLE users RENAME COLUMN locale TO preferred_locale;
