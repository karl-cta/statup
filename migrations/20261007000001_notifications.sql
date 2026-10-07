-- Where Statup posts what happens on the page: a team chat channel, a phone
-- topic, a list of email addresses, or any tool that takes a webhook. The
-- target is the webhook address, or the addresses of an email destination.
CREATE TABLE notification_channels (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    kind TEXT NOT NULL
        CHECK (kind IN ('teams', 'slack', 'google_chat', 'discord', 'mattermost',
                        'ntfy', 'email', 'webhook')),
    target TEXT NOT NULL,
    -- The language of the messages, checked against the shipped ones by the
    -- application, like the users' preferred language.
    locale TEXT NOT NULL,
    -- What the destination receives. None ticked pauses it.
    on_incidents INTEGER NOT NULL DEFAULT 1 CHECK (on_incidents IN (0, 1)),
    on_maintenances INTEGER NOT NULL DEFAULT 1 CHECK (on_maintenances IN (0, 1)),
    on_publications INTEGER NOT NULL DEFAULT 1 CHECK (on_publications IN (0, 1)),
    on_detected INTEGER NOT NULL DEFAULT 0 CHECK (on_detected IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TRIGGER notification_channels_updated_at AFTER UPDATE ON notification_channels
BEGIN
    UPDATE notification_channels SET updated_at = datetime('now') WHERE id = NEW.id;
END;

-- One message for one destination, waiting, sent or failed. It keeps what
-- happened, not the text: the message is written in the destination's
-- language when it goes out, and an event deleted before then sends nothing.
--   happening : what the message says happened.
--   lifecycle : the state the event moved to, when it moved.
--   failure   : why the last attempt failed, a code the application words.
CREATE TABLE notification_deliveries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    channel_id INTEGER NOT NULL REFERENCES notification_channels(id) ON DELETE CASCADE,
    happening TEXT NOT NULL
        CHECK (happening IN ('opened', 'updated', 'rescheduled', 'started', 'closed',
                             'published', 'service_down', 'service_up', 'test')),
    event_id INTEGER REFERENCES events(id) ON DELETE CASCADE,
    update_id INTEGER REFERENCES event_updates(id) ON DELETE CASCADE,
    lifecycle TEXT,
    service_id INTEGER REFERENCES services(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'sent', 'failed')),
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TEXT NOT NULL DEFAULT (datetime('now')),
    failure TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    sent_at TEXT,
    CHECK ((happening IN ('service_down', 'service_up')) = (service_id IS NOT NULL)),
    CHECK ((happening IN ('service_down', 'service_up', 'test')) = (event_id IS NULL))
);

-- The queue of each destination, oldest first.
CREATE INDEX idx_notification_deliveries_queue
    ON notification_deliveries(channel_id, status, id);
CREATE INDEX idx_notification_deliveries_created ON notification_deliveries(created_at);
