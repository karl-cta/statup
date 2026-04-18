-- Phase 1, refonte modèle événement.
--
-- Nouvelles dimensions :
--   kind       : incident | maintenance | publication
--   severity   : minor | major | critical (optionnel, NULL pour publication)
--   planned    : booléen (maintenance planifiée vs subie)
--   lifecycle  : workflow par kind (NULL pour publication)
--                incident    : investigating, in_progress, monitoring, resolved, cancelled
--                maintenance : scheduled, in_progress, completed, cancelled
--   category   : changelog | info (uniquement pour publication)
--
-- Migration destructive assumée en pré-v1. Les tables events, event_templates,
-- event_services, event_updates et events_fts sont reconstruites. Aucune donnée
-- n'est conservée (validé avec Karl, base locale propre).

PRAGMA foreign_keys = OFF;

-- Drop FTS triggers et table virtuelle
DROP TRIGGER IF EXISTS events_fts_insert;
DROP TRIGGER IF EXISTS events_fts_update;
DROP TRIGGER IF EXISTS events_fts_delete;
DROP TABLE IF EXISTS events_fts;

-- Drop trigger updated_at sur events
DROP TRIGGER IF EXISTS events_updated_at;

-- Drop tables dépendantes (event_updates et event_services référencent events.id)
DROP TABLE IF EXISTS event_updates;
DROP TABLE IF EXISTS event_services;

-- Drop events et event_templates (remplacés par le nouveau schéma)
DROP TABLE IF EXISTS events;
DROP TABLE IF EXISTS event_templates;

-- Nouvelle table events
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

    -- Cohérence lifecycle selon le kind. lifecycle IS NOT NULL explicite
    -- pour que NULL ne laisse pas passer via la sémantique trivaluée du SQL.
    CHECK (
        (kind = 'incident' AND lifecycle IS NOT NULL AND lifecycle IN ('investigating', 'in_progress', 'monitoring', 'resolved', 'cancelled'))
        OR (kind = 'maintenance' AND lifecycle IS NOT NULL AND lifecycle IN ('scheduled', 'in_progress', 'completed', 'cancelled'))
        OR (kind = 'publication' AND lifecycle IS NULL)
    ),

    -- Category uniquement pour publication, obligatoire dans ce cas
    CHECK (
        (kind = 'publication' AND category IS NOT NULL)
        OR (kind != 'publication' AND category IS NULL)
    ),

    -- Severity absente pour publication
    CHECK (
        kind != 'publication' OR severity IS NULL
    ),

    -- previous_lifecycle doit suivre les mêmes valeurs que lifecycle
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

-- Dashboard : maintenances planifiées à venir
CREATE INDEX idx_events_upcoming_maintenance
    ON events(kind, lifecycle, planned_start)
    WHERE kind = 'maintenance' AND planned = 1 AND lifecycle = 'scheduled';

-- Dashboard : événements actifs (lifecycle non terminal)
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

-- Nouvelle table event_templates, alignée sur les nouvelles dimensions
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

-- Recréation FTS sur events (title, description)
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
