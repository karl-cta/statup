-- Icons library: shared image assets for services and events
CREATE TABLE IF NOT EXISTS icons (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    filename TEXT NOT NULL UNIQUE,
    original_name TEXT NOT NULL,
    mime_type TEXT NOT NULL CHECK (mime_type IN ('image/png', 'image/jpeg', 'image/webp', 'image/svg+xml')),
    size_bytes INTEGER NOT NULL,
    uploaded_by INTEGER NOT NULL REFERENCES users(id),
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_icons_created_at ON icons(created_at DESC);

-- Add optional icon reference to services
ALTER TABLE services ADD COLUMN icon_id INTEGER REFERENCES icons(id) ON DELETE SET NULL;

-- Add optional icon reference to events
ALTER TABLE events ADD COLUMN icon_id INTEGER REFERENCES icons(id) ON DELETE SET NULL;

-- Event templates: reusable presets for recurring events
CREATE TABLE IF NOT EXISTS event_templates (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    event_type TEXT NOT NULL CHECK (event_type IN ('incident', 'maintenance_scheduled', 'maintenance_urgent', 'changelog', 'info')),
    impact TEXT NOT NULL DEFAULT 'minor' CHECK (impact IN ('none', 'minor', 'major', 'critical')),
    icon_id INTEGER REFERENCES icons(id) ON DELETE SET NULL,
    created_by INTEGER NOT NULL REFERENCES users(id),
    usage_count INTEGER NOT NULL DEFAULT 0,
    last_used_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_event_templates_title ON event_templates(title);
CREATE INDEX idx_event_templates_usage ON event_templates(usage_count DESC);
