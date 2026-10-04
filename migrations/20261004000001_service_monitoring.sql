-- What Statup checks on its own for a service: nothing, a web address that
-- answers, or a port that accepts a connection. The kind and its target are
-- set or cleared together.
ALTER TABLE services ADD COLUMN check_kind TEXT
    CHECK (check_kind IN ('http', 'tcp'));
ALTER TABLE services ADD COLUMN check_target TEXT
    CHECK ((check_kind IS NULL) = (check_target IS NULL));
-- Accept a certificate the instance cannot verify, for internal tools.
ALTER TABLE services ADD COLUMN check_internal_cert INTEGER NOT NULL DEFAULT 0
    CHECK (check_internal_cert IN (0, 1));

-- The state the checks found, a third source of the shown status next to the
-- one set by hand and the one open events give.
ALTER TABLE services ADD COLUMN detected_status TEXT
    CHECK (detected_status IN ('major_outage'));

-- One row per outage the checks detected, none per check: the availability
-- strip reads it.
CREATE TABLE service_outages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    service_id INTEGER NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    CHECK (ended_at IS NULL OR ended_at >= started_at)
);

CREATE INDEX idx_service_outages_service ON service_outages(service_id, started_at);

-- A service has at most one outage under way.
CREATE UNIQUE INDEX idx_service_outages_open ON service_outages(service_id)
    WHERE ended_at IS NULL;
