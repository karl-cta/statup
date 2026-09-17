-- An announcement may follow a maintenance: what is new once the work is
-- done. The link survives the deletion of the maintenance as a plain
-- announcement.
ALTER TABLE events ADD COLUMN follows_event_id INTEGER REFERENCES events(id) ON DELETE SET NULL;

CREATE INDEX idx_events_follows ON events(follows_event_id);
