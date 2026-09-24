-- One layout for everyone: the members' arrangement is kept, the visitors'
-- one goes, and a row no longer says which page it belonged to.
CREATE TABLE dashboard_layouts_single (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER REFERENCES users(id) ON DELETE CASCADE,
    module_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    config TEXT NOT NULL DEFAULT '{}',
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

INSERT INTO dashboard_layouts_single (user_id, module_id, position, enabled, config, updated_at)
SELECT user_id, module_id, position, enabled, config, updated_at
FROM dashboard_layouts WHERE context = 'admin';

DROP TABLE dashboard_layouts;
ALTER TABLE dashboard_layouts_single RENAME TO dashboard_layouts;

CREATE UNIQUE INDEX idx_dashboard_layouts_default
    ON dashboard_layouts(module_id)
    WHERE user_id IS NULL;
CREATE UNIQUE INDEX idx_dashboard_layouts_user
    ON dashboard_layouts(user_id, module_id)
    WHERE user_id IS NOT NULL;
CREATE INDEX idx_dashboard_layouts_lookup
    ON dashboard_layouts(user_id, position);
