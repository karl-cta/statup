-- The state someone sets by hand, kept apart from the one open events give:
-- the page shows the worse of the two, so closing an incident or finishing a
-- maintenance never erases what the team declared.
ALTER TABLE services ADD COLUMN manual_status TEXT NOT NULL DEFAULT 'operational'
    CHECK (manual_status IN ('operational', 'degraded', 'partial_outage', 'major_outage', 'maintenance'));

-- A service with no open event holds the state someone set by hand.
UPDATE services SET manual_status = status
WHERE NOT EXISTS (
    SELECT 1 FROM event_services es INNER JOIN events e ON e.id = es.event_id
    WHERE es.service_id = services.id
      AND ((e.kind = 'incident' AND e.lifecycle IN ('investigating', 'in_progress'))
        OR (e.kind = 'maintenance' AND e.lifecycle = 'in_progress'))
);

-- When an incident's services came back, even if the team still watches:
-- the availability strip ends there rather than at the resolution.
ALTER TABLE events ADD COLUMN restored_at TEXT;

UPDATE events SET restored_at = updated_at
WHERE kind = 'incident' AND lifecycle = 'monitoring';
