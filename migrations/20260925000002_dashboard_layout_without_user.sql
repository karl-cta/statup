-- The layout belongs to the instance, nobody keeps one of their own: the
-- rows once kept for a person, read by nothing since, go with the column.
CREATE TABLE dashboard_layouts_shared (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    module_id TEXT NOT NULL UNIQUE,
    position INTEGER NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    config TEXT NOT NULL DEFAULT '{}',
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

INSERT INTO dashboard_layouts_shared (id, module_id, position, enabled, config, updated_at)
SELECT id, module_id, position, enabled, config, updated_at
FROM dashboard_layouts WHERE user_id IS NULL;

DROP TABLE dashboard_layouts;
ALTER TABLE dashboard_layouts_shared RENAME TO dashboard_layouts;
