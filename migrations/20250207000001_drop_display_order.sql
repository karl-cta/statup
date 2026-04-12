-- Remove display_order column from services table.
-- Services are now sorted alphabetically by name.
ALTER TABLE services DROP COLUMN display_order;
