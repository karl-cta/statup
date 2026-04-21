-- Phase 1, event model refactor.
--
-- New dimensions:
--   kind       : incident | maintenance | publication
--   severity   : minor | major | critical (optional, NULL for publication)
--   planned    : boolean (scheduled vs unplanned maintenance)
--   lifecycle  : workflow per kind (NULL for publication)
--                incident    : investigating, in_progress, monitoring, resolved, cancelled
--                maintenance : scheduled, in_progress, completed, cancelled
--   category   : changelog | info (publication only)
--
-- Destructive migration accepted in pre-v1. Tables events, event_templates,
-- event_services, event_updates, and events_fts are rebuilt. No data is
-- preserved.

PRAGMA foreign_keys = OFF;

-- Drop FTS triggers and virtual table
DROP TRIGGER IF EXISTS events_fts_insert;
DROP TRIGGER IF EXISTS events_fts_update;
DROP TRIGGER IF EXISTS events_fts_delete;
DROP TABLE IF EXISTS events_fts;

-- Drop updated_at trigger on events
DROP TRIGGER IF EXISTS events_updated_at;

-- Drop dependent tables (event_updates and event_services reference events.id)
DROP TABLE IF EXISTS event_updates;
DROP TABLE IF EXISTS event_services;

-- Drop events and event_templates (replaced by the new schema)
DROP TABLE IF EXISTS events;
DROP TABLE IF EXISTS event_templates;

-- New events table
CREATE TABLE events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL
        CHECK (kind IN ('incident', 'maintenance', 'publication')),
    severity TEXT
        CHECK (severity IN ('minor', 'major', 'critical')),
    planned INTEGER NOT NULL DEFAULT 0
        CHECK (planned IN (0, 1)),
    lifecycle TEXT,
    category TEXT
        CHECK (category IN ('changelog', 'info')),
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    planned_start TEXT,
    planned_end TEXT,
    started_at TEXT,
    ended_at TEXT,
    icon_id INTEGER REFERENCES icons(id) ON DELETE SET NULL,
    author_id INTEGER NOT NULL REFERENCES users(id),
    previous_lifecycle TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),

    -- Lifecycle consistency with kind. `lifecycle IS NOT NULL` is explicit so
    -- that NULL does not slip through SQL's three-valued logic.
    CHECK (
        (kind = 'incident' AND lifecycle IS NOT NULL AND lifecycle IN ('investigating', 'in_progress', 'monitoring', 'resolved', 'cancelled'))
        OR (kind = 'maintenance' AND lifecycle IS NOT NULL AND lifecycle IN ('scheduled', 'in_progress', 'completed', 'cancelled'))
        OR (kind = 'publication' AND lifecycle IS NULL)
    ),

    -- Category only for publication, mandatory in that case
    CHECK (
        (kind = 'publication' AND category IS NOT NULL)
        OR (kind != 'publication' AND category IS NULL)
    ),

    -- No severity for publications
    CHECK (
        kind != 'publication' OR severity IS NULL
    ),

    -- previous_lifecycle must use the same value set as lifecycle
    CHECK (
        previous_lifecycle IS NULL
        OR previous_lifecycle IN ('investigating', 'in_progress', 'monitoring', 'resolved', 'cancelled', 'scheduled', 'completed')
    )
);

-- Indexes events
CREATE INDEX idx_events_kind ON events(kind);
CREATE INDEX idx_events_lifecycle ON events(lifecycle);
CREATE INDEX idx_events_created_at ON events(created_at DESC);
CREATE INDEX idx_events_author ON events(author_id);

-- Dashboard: upcoming scheduled maintenances
CREATE INDEX idx_events_upcoming_maintenance
    ON events(kind, lifecycle, planned_start)
    WHERE kind = 'maintenance' AND planned = 1 AND lifecycle = 'scheduled';

-- Dashboard: active events (non-terminal lifecycle)
CREATE INDEX idx_events_active
    ON events(lifecycle, kind)
    WHERE lifecycle IS NOT NULL AND lifecycle NOT IN ('resolved', 'completed', 'cancelled');

-- Unread badge : count_since (date + kind)
CREATE INDEX idx_events_created_kind
    ON events(created_at DESC, kind);

-- Trigger updated_at
CREATE TRIGGER events_updated_at AFTER UPDATE ON events
BEGIN
    UPDATE events SET updated_at = datetime('now') WHERE id = NEW.id;
END;

-- Table de jonction event_services
CREATE TABLE event_services (
    event_id INTEGER NOT NULL REFERENCES events(id) ON DELETE CASCADE,
    service_id INTEGER NOT NULL REFERENCES services(id) ON DELETE RESTRICT,
    PRIMARY KEY (event_id, service_id)
);

CREATE INDEX idx_event_services_service ON event_services(service_id);

-- Event updates
CREATE TABLE event_updates (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id INTEGER NOT NULL REFERENCES events(id) ON DELETE CASCADE,
    message TEXT NOT NULL,
    author_id INTEGER NOT NULL REFERENCES users(id),
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_event_updates_event ON event_updates(event_id);

-- New event_templates table aligned on the new dimensions
CREATE TABLE event_templates (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    kind TEXT NOT NULL
        CHECK (kind IN ('incident', 'maintenance', 'publication')),
    severity TEXT
        CHECK (severity IN ('minor', 'major', 'critical')),
    planned INTEGER NOT NULL DEFAULT 0
        CHECK (planned IN (0, 1)),
    category TEXT
        CHECK (category IN ('changelog', 'info')),
    icon_id INTEGER REFERENCES icons(id) ON DELETE SET NULL,
    created_by INTEGER NOT NULL REFERENCES users(id),
    usage_count INTEGER NOT NULL DEFAULT 0,
    last_used_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),

    CHECK (
        (kind = 'publication' AND category IS NOT NULL)
        OR (kind != 'publication' AND category IS NULL)
    ),
    CHECK (
        kind != 'publication' OR severity IS NULL
    )
);

CREATE INDEX idx_event_templates_title ON event_templates(title);
CREATE INDEX idx_event_templates_usage ON event_templates(usage_count DESC);

-- Rebuild FTS on events (title, description)
CREATE VIRTUAL TABLE events_fts USING fts5(
    title,
    description,
    content='events',
    content_rowid='id'
);

CREATE TRIGGER events_fts_insert AFTER INSERT ON events BEGIN
    INSERT INTO events_fts(rowid, title, description)
    VALUES (NEW.id, NEW.title, NEW.description);
END;

CREATE TRIGGER events_fts_delete AFTER DELETE ON events BEGIN
    INSERT INTO events_fts(events_fts, rowid, title, description)
    VALUES ('delete', OLD.id, OLD.title, OLD.description);
END;

CREATE TRIGGER events_fts_update AFTER UPDATE ON events BEGIN
    INSERT INTO events_fts(events_fts, rowid, title, description)
    VALUES ('delete', OLD.id, OLD.title, OLD.description);
    INSERT INTO events_fts(rowid, title, description)
    VALUES (NEW.id, NEW.title, NEW.description);
END;

PRAGMA foreign_keys = ON;
