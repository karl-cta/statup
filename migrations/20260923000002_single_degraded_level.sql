-- "Slow" and "partial outage" read as the same thing to a visitor: they
-- become one level, degraded. The table checks still accept the old values,
-- which nothing writes any more.
UPDATE services SET status = 'degraded' WHERE status = 'partial_outage';
UPDATE services SET manual_status = 'degraded' WHERE manual_status = 'partial_outage';
UPDATE events SET severity = 'minor' WHERE severity = 'major';
UPDATE event_templates SET severity = 'minor' WHERE severity = 'major';
