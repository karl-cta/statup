-- Add icon_name column for built-in icon selection (alternative to icon_id upload)
ALTER TABLE services ADD COLUMN icon_name TEXT;
