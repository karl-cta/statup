-- Services keep the order chosen on the Services page. Existing ones start
-- in the alphabetical order they were shown in; a service inserted without
-- a position sorts by name among the others at 0.
--
-- Moving a service is not a change to it: its last update time stays. The
-- trigger is replaced first so the numbering below leaves it alone too.
DROP TRIGGER services_updated_at;

ALTER TABLE services ADD COLUMN position INTEGER NOT NULL DEFAULT 0;

UPDATE services SET position = ranked.rank
FROM (SELECT id, ROW_NUMBER() OVER (ORDER BY name COLLATE NOCASE, id) AS rank FROM services) AS ranked
WHERE services.id = ranked.id;

CREATE TRIGGER services_updated_at AFTER UPDATE ON services
WHEN OLD.position IS NEW.position
BEGIN
    UPDATE services SET updated_at = datetime('now') WHERE id = NEW.id;
END;
