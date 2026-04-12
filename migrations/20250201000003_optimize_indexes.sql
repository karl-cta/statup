-- Composite indexes for frequently used query patterns.

-- Dashboard: upcoming maintenance (event_type + status + scheduled_start)
CREATE INDEX IF NOT EXISTS idx_events_upcoming_maintenance
    ON events(event_type, status, scheduled_start)
    WHERE event_type = 'maintenance_scheduled' AND status = 'scheduled';

-- Dashboard: active incidents (status + event_type for NOT IN / IN filters)
CREATE INDEX IF NOT EXISTS idx_events_active
    ON events(status, event_type)
    WHERE status NOT IN ('resolved', 'cancelled');

-- Unread badge: count_since (created_at + event_type exclusion)
CREATE INDEX IF NOT EXISTS idx_events_created_type
    ON events(created_at DESC, event_type);

-- FK index on events.author_id (speeds up JOINs with users table)
CREATE INDEX IF NOT EXISTS idx_events_author ON events(author_id);

-- Admin: count_admins (role + is_active)
CREATE INDEX IF NOT EXISTS idx_users_role_active ON users(role, is_active);
