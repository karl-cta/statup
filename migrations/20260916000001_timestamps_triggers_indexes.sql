-- One text format for every stored timestamp, the one datetime('now') writes,
-- so that comparisons between columns and bound values hold on the same day.
-- Triggers are dropped first: rewriting old values must not count as an edit.

DROP TRIGGER IF EXISTS events_updated_at;
DROP TRIGGER IF EXISTS events_fts_update;

UPDATE events SET planned_start = datetime(planned_start)
    WHERE datetime(planned_start) IS NOT NULL AND planned_start IS NOT datetime(planned_start);
UPDATE events SET planned_end = datetime(planned_end)
    WHERE datetime(planned_end) IS NOT NULL AND planned_end IS NOT datetime(planned_end);
UPDATE events SET started_at = datetime(started_at)
    WHERE datetime(started_at) IS NOT NULL AND started_at IS NOT datetime(started_at);
UPDATE events SET ended_at = datetime(ended_at)
    WHERE datetime(ended_at) IS NOT NULL AND ended_at IS NOT datetime(ended_at);

CREATE TRIGGER events_updated_at AFTER UPDATE ON events
WHEN NEW.updated_at IS OLD.updated_at
BEGIN
    UPDATE events SET updated_at = datetime('now') WHERE id = NEW.id;
END;

-- The search index only changes with the text it indexes.
CREATE TRIGGER events_fts_update AFTER UPDATE OF title, description ON events BEGIN
    INSERT INTO events_fts(events_fts, rowid, title, description)
    VALUES ('delete', OLD.id, OLD.title, OLD.description);
    INSERT INTO events_fts(rowid, title, description)
    VALUES (NEW.id, NEW.title, NEW.description);
END;

-- Latest activity and recently finished maintenances.
CREATE INDEX IF NOT EXISTS idx_events_updated_at ON events(updated_at);
CREATE INDEX IF NOT EXISTS idx_event_updates_created_at ON event_updates(created_at);

-- Foreign keys checked when an icon is deleted.
CREATE INDEX IF NOT EXISTS idx_events_icon ON events(icon_id);
CREATE INDEX IF NOT EXISTS idx_services_icon ON services(icon_id);
CREATE INDEX IF NOT EXISTS idx_event_templates_icon ON event_templates(icon_id);

-- Covered by other indexes.
DROP INDEX IF EXISTS idx_events_created_at;
DROP INDEX IF EXISTS idx_users_email;
DROP INDEX IF EXISTS idx_services_slug;

-- Sessions live in the table the session store creates itself.
DROP INDEX IF EXISTS idx_sessions_expiry;
DROP TABLE IF EXISTS sessions;
