-- Phase 2, dashboard module engine.
--
-- One row per (context, user, module) describes the slot, the enabled/disabled
-- state, and the module-specific config for the dashboard.
--
--   context  : 'public' or 'admin'.
--   user_id  : NULL for the admin-defined default layout, set for a
--              per-user preference (override).
--   module_id: stable module id (from the Rust registry).
--   position : display order within (context, user).
--   enabled  : 1 if visible, 0 if hidden.
--   config   : JSON blob, module-specific config.

CREATE TABLE dashboard_layouts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    context TEXT NOT NULL CHECK (context IN ('public', 'admin')),
    user_id INTEGER REFERENCES users(id) ON DELETE CASCADE,
    module_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    config TEXT NOT NULL DEFAULT '{}',
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Uniqueness for the default layout (user_id IS NULL).
CREATE UNIQUE INDEX idx_dashboard_layouts_default
    ON dashboard_layouts(context, module_id)
    WHERE user_id IS NULL;

-- Uniqueness for per-user layouts (user_id set).
CREATE UNIQUE INDEX idx_dashboard_layouts_user
    ON dashboard_layouts(context, user_id, module_id)
    WHERE user_id IS NOT NULL;

-- Ordered lookup for rendering.
CREATE INDEX idx_dashboard_layouts_lookup
    ON dashboard_layouts(context, user_id, position);
