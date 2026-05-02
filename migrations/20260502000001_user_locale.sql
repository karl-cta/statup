-- Per-user preferred locale, persisted across sessions and devices.
-- NULL means "no preference saved", the runtime falls back to cookie or Accept-Language.

ALTER TABLE users ADD COLUMN preferred_locale TEXT
    CHECK (preferred_locale IS NULL OR preferred_locale IN ('fr', 'en'));
