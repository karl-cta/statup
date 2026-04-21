-- Phase 2, moteur de modules dashboard.
--
-- Une ligne par (contexte, user, module) décrit la place, l'état
-- activé/désactivé et la config spécifique d'un module dans le dashboard.
--
--   context  : 'public' ou 'admin'
--   user_id  : NULL pour le layout par défaut (admin-defined),
--              renseigné pour une préférence utilisateur (override).
--   module_id: id stable du module (registre Rust).
--   position : ordre d'affichage au sein du contexte/user.
--   enabled  : 1 si visible, 0 si masqué.
--   config   : JSON textuel, config spécifique au module.

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

-- Unicité par layout par défaut (user_id IS NULL).
CREATE UNIQUE INDEX idx_dashboard_layouts_default
    ON dashboard_layouts(context, module_id)
    WHERE user_id IS NULL;

-- Unicité par layout utilisateur (user_id renseigné).
CREATE UNIQUE INDEX idx_dashboard_layouts_user
    ON dashboard_layouts(context, user_id, module_id)
    WHERE user_id IS NOT NULL;

-- Lookup ordonné pour rendu.
CREATE INDEX idx_dashboard_layouts_lookup
    ON dashboard_layouts(context, user_id, position);
