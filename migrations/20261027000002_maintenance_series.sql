-- Maintenances announced once that come back every week, or every month on
-- the Nth weekday. Each series keeps one scheduled session ahead in
-- `events`; `next_start` is when the session after it begins. The time of
-- day is read on the instance clock, `next_start` is UTC like every stored
-- time.
CREATE TABLE maintenance_series (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    rhythm TEXT NOT NULL
        CHECK (rhythm IN ('weekly', 'monthly')),
    -- 0 for Monday to 6 for Sunday.
    weekday INTEGER NOT NULL
        CHECK (weekday BETWEEN 0 AND 6),
    rank TEXT
        CHECK (rank IN ('first', 'second', 'third', 'fourth', 'last')),
    -- HH:MM on the instance clock.
    start_time TEXT NOT NULL,
    duration_minutes INTEGER NOT NULL
        CHECK (duration_minutes > 0),
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    keeps_services_up INTEGER NOT NULL DEFAULT 0
        CHECK (keeps_services_up IN (0, 1)),
    next_start TEXT NOT NULL,
    author_id INTEGER NOT NULL REFERENCES users(id),
    stopped_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),

    -- A monthly series names its rank, a weekly one does not.
    CHECK ((rhythm = 'weekly') = (rank IS NULL))
);

-- The services each session of a series concerns. A deleted service leaves
-- the series.
CREATE TABLE maintenance_series_services (
    series_id INTEGER NOT NULL REFERENCES maintenance_series(id) ON DELETE CASCADE,
    service_id INTEGER NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    PRIMARY KEY (series_id, service_id)
);

CREATE INDEX idx_maintenance_series_services_service
    ON maintenance_series_services(service_id);

ALTER TABLE events ADD COLUMN series_id INTEGER
    REFERENCES maintenance_series(id) ON DELETE SET NULL;

-- The schedule looks for active series without a session to come.
CREATE INDEX idx_events_series ON events(series_id, lifecycle)
    WHERE series_id IS NOT NULL;
