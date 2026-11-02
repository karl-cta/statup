-- Statup 0.3.0 database, made by the published image of 0.3.0 and a fixed data set, to test upgrades.
-- Accounts admin@upgrade.test, editor@upgrade.test, reader@upgrade.test; password Fixture-Upgrade-2026! (a test value).
/* WARNING: Script requires that SQLITE_DBCONFIG_DEFENSIVE be disabled */
PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE _sqlx_migrations (
    version BIGINT PRIMARY KEY,
    description TEXT NOT NULL,
    installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    success BOOLEAN NOT NULL,
    checksum BLOB NOT NULL,
    execution_time BIGINT NOT NULL
);
INSERT INTO _sqlx_migrations VALUES(20260917000001,'initial schema','2026-11-02 13:10:18',1,x'3a2a607b68a7ef7ef1cdff97ed9ea094a748244b88a42c58d0cdfb6886f832882ff69c29d8d672b16b2d514350f3f6fc',8814792);
INSERT INTO _sqlx_migrations VALUES(20260917000002,'announcement follows','2026-11-02 13:10:18',1,x'b506725f1f4bc84dcc2940b9a8e3824aae81f33f44673f2a9495b2d96230c8c47adb9f73e080f70a80c240704e56570a',1457375);
INSERT INTO _sqlx_migrations VALUES(20260923000001,'service base status','2026-11-02 13:10:18',1,x'28b8b6108bb27aba122e55241831e124123e1cdc5157c153d967747c29e93591cd411c3081e581699c0fa121d05f9080',1805459);
INSERT INTO _sqlx_migrations VALUES(20260923000002,'single degraded level','2026-11-02 13:10:18',1,x'02c2cf1e7b743d59d99032fb1d183a275ab31f90e92be509ed7a4304287c5aa972dd045a9e2a9150762301157b74a53b',1364208);
INSERT INTO _sqlx_migrations VALUES(20260924000001,'maintenance without downtime','2026-11-02 13:10:18',1,x'2fb82608b90b7b2659d0e40ede2883b8480f283058f5218db82c70e0db05a59b26d3bd786ccf632c475e86cd13fb47aa',1289708);
INSERT INTO _sqlx_migrations VALUES(20260924000002,'single dashboard layout','2026-11-02 13:10:18',1,x'891f9bfed12b14013a088711b3453849ac67a2fc00f44899c3c0ded61a9d753006e19d267ae20d094a49c6b2b3e9e695',2132875);
INSERT INTO _sqlx_migrations VALUES(20260925000001,'temporary password expiry','2026-11-02 13:10:18',1,x'c84e68da87284a4a1a2194e78ac64924d2c4afa102b837343752911486230e5347341f93825d62e015d1f2c73df117a8',932292);
INSERT INTO _sqlx_migrations VALUES(20260925000002,'dashboard layout without user','2026-11-02 13:10:18',1,x'290c37e099559beeff960aa967f29c63a6284a06d241060ab4222449224eaae1422ec79717faee9179f0f981b8def185',2111583);
INSERT INTO _sqlx_migrations VALUES(20261004000001,'service monitoring','2026-11-02 13:10:18',1,x'b06a50bef906de5c1d160e0af74387f003a2c108c1685b5e59075e56252b2ad4d11e974191794ad7f01538dd2f29cb5d',1620375);
INSERT INTO _sqlx_migrations VALUES(20261006000001,'any supported locale','2026-11-02 13:10:18',1,x'5c829db39f507a37a8dc4f86e87f64283179d4e2983893d4e07236d5eb16a5ad94d6366305bb7be175958a15182d0456',1852375);
INSERT INTO _sqlx_migrations VALUES(20261007000001,'notifications','2026-11-02 13:10:18',1,x'6cd7afac804c148068190aceaabfa1ee5165344a710ed767b1a0ed08ac9128a1c4fa2463948e4232c630fe7b3084068a',593333);
CREATE TABLE users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    email TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT NOT NULL,
    display_name TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'reader'
        CHECK (role IN ('reader', 'publisher', 'admin')),
    is_active INTEGER NOT NULL DEFAULT 1,
    last_seen_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    must_change_password BOOLEAN NOT NULL DEFAULT 0
, temporary_password_expires_at TEXT, preferred_locale TEXT);
INSERT INTO users VALUES(1,'admin@upgrade.test','$argon2id$v=19$m=19456,t=2,p=1$efDS7hvUTCFntOZFoBeXAg$w7TCccOXL4SrCnDgTOXbUdjFV0JcG8zKX0kdbSoxsew','admin','admin',1,'2026-10-25 11:05:00','2026-10-05 06:50:00','2026-10-05 06:50:00',0,NULL,NULL);
INSERT INTO users VALUES(2,'editor@upgrade.test','$argon2id$v=19$m=19456,t=2,p=1$efDS7hvUTCFntOZFoBeXAg$w7TCccOXL4SrCnDgTOXbUdjFV0JcG8zKX0kdbSoxsew','Fixture Editor','publisher',1,'2026-10-22 14:05:00','2026-10-05 07:00:00','2026-10-05 07:00:00',0,NULL,'en');
INSERT INTO users VALUES(3,'reader@upgrade.test','$argon2id$v=19$m=19456,t=2,p=1$efDS7hvUTCFntOZFoBeXAg$w7TCccOXL4SrCnDgTOXbUdjFV0JcG8zKX0kdbSoxsew','Fixture Reader','reader',1,'2026-10-25 12:00:00','2026-10-05 07:01:00','2026-10-05 07:01:00',0,NULL,NULL);
CREATE TABLE icons (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    filename TEXT NOT NULL UNIQUE,
    original_name TEXT NOT NULL,
    mime_type TEXT NOT NULL
        CHECK (mime_type IN ('image/png', 'image/jpeg', 'image/webp', 'image/svg+xml')),
    size_bytes INTEGER NOT NULL,
    uploaded_by INTEGER NOT NULL REFERENCES users(id),
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
INSERT INTO icons VALUES(1,'3f0c9a52-7b1e-4d2a-9c64-5e8f1a2b3c4d.svg','fixture-mark.svg','image/svg+xml',412,1,'2026-10-05 07:05:00');
CREATE TABLE services (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    slug TEXT NOT NULL UNIQUE,
    description TEXT,
    status TEXT NOT NULL DEFAULT 'operational'
        CHECK (status IN ('operational', 'degraded', 'partial_outage', 'major_outage', 'maintenance')),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    icon_id INTEGER REFERENCES icons(id) ON DELETE SET NULL,
    icon_name TEXT
, manual_status TEXT NOT NULL DEFAULT 'operational'
    CHECK (manual_status IN ('operational', 'degraded', 'partial_outage', 'major_outage', 'maintenance')), check_kind TEXT
    CHECK (check_kind IN ('http', 'tcp')), check_target TEXT
    CHECK ((check_kind IS NULL) = (check_target IS NULL)), check_internal_cert INTEGER NOT NULL DEFAULT 0
    CHECK (check_internal_cert IN (0, 1)), detected_status TEXT
    CHECK (detected_status IN ('major_outage')));
INSERT INTO services VALUES(1,'Fixture Mail','fixture-mail','Mail and calendars for every office.','operational','2026-10-05 07:10:00','2026-10-20 10:30:00',NULL,'envelope','operational',NULL,NULL,0,NULL);
INSERT INTO services VALUES(2,'Fixture VPN','fixture-vpn','Remote access to the office network.','degraded','2026-10-05 07:10:00','2026-10-24 08:00:00',NULL,'lock-closed','degraded','tcp','127.0.0.1:9',0,NULL);
INSERT INTO services VALUES(3,'Fixture Intranet','fixture-intranet','The company intranet.','operational','2026-10-05 07:10:00','2026-10-12 03:42:00',NULL,'globe','operational','http','https://127.0.0.1:9/',1,NULL);
CREATE TABLE events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL
        CHECK (kind IN ('incident', 'maintenance', 'publication')),
    severity TEXT
        CHECK (severity IN ('minor', 'major', 'critical')),
    planned INTEGER NOT NULL DEFAULT 0
        CHECK (planned IN (0, 1)),
    lifecycle TEXT,
    category TEXT
        CHECK (category IN ('changelog', 'info')),
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    planned_start TEXT,
    planned_end TEXT,
    started_at TEXT,
    ended_at TEXT,
    icon_id INTEGER REFERENCES icons(id) ON DELETE SET NULL,
    author_id INTEGER NOT NULL REFERENCES users(id),
    -- The state before the last change, for a one step undo.
    previous_lifecycle TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')), follows_event_id INTEGER REFERENCES events(id) ON DELETE SET NULL, restored_at TEXT, keeps_services_up INTEGER NOT NULL DEFAULT 0
    CHECK (keeps_services_up IN (0, 1)),

    -- `lifecycle IS NOT NULL` is spelled out so that NULL does not slip
    -- through three-valued logic.
    CHECK (
        (kind = 'incident' AND lifecycle IS NOT NULL AND lifecycle IN ('investigating', 'in_progress', 'monitoring', 'resolved', 'cancelled'))
        OR (kind = 'maintenance' AND lifecycle IS NOT NULL AND lifecycle IN ('scheduled', 'in_progress', 'completed', 'cancelled'))
        OR (kind = 'publication' AND lifecycle IS NULL)
    ),
    CHECK (
        (kind = 'publication' AND category IS NOT NULL)
        OR (kind != 'publication' AND category IS NULL)
    ),
    CHECK (
        kind != 'publication' OR severity IS NULL
    ),
    CHECK (
        previous_lifecycle IS NULL
        OR previous_lifecycle IN ('investigating', 'in_progress', 'monitoring', 'resolved', 'cancelled', 'scheduled', 'completed')
    )
);
INSERT INTO events VALUES(1,'incident','critical',0,'resolved',NULL,'Fixture incident: VPN drop','VPN connections dropped for remote staff.',NULL,NULL,'2026-10-05 07:30:00','2026-10-05 08:15:00',NULL,1,'investigating','2026-10-05 07:30:00','2026-10-05 08:15:00',NULL,NULL,0);
INSERT INTO events VALUES(2,'incident','minor',0,'monitoring',NULL,'Fixture incident: mail delays','Outgoing mail is queued for several minutes.',NULL,NULL,'2026-10-20 09:00:00',NULL,NULL,2,'investigating','2026-10-20 09:00:00','2026-10-20 10:30:00',NULL,'2026-10-20 10:30:00',0);
INSERT INTO events VALUES(3,'maintenance',NULL,1,'scheduled',NULL,'Fixture maintenance: VPN upgrade','The VPN gateway moves to a new version.','2027-03-01 22:00:00','2027-03-01 23:00:00',NULL,NULL,NULL,2,NULL,'2026-10-22 14:00:00','2026-10-22 14:00:00',NULL,NULL,0);
INSERT INTO events VALUES(4,'publication',NULL,0,NULL,'info','Fixture announcement: new intranet','The new intranet is open to every team.',NULL,NULL,'2026-10-25 11:00:00',NULL,NULL,1,NULL,'2026-10-25 11:00:00','2026-10-25 11:00:00',NULL,NULL,0);
CREATE TABLE event_services (
    event_id INTEGER NOT NULL REFERENCES events(id) ON DELETE CASCADE,
    service_id INTEGER NOT NULL REFERENCES services(id) ON DELETE RESTRICT,
    PRIMARY KEY (event_id, service_id)
);
INSERT INTO event_services VALUES(1,2);
INSERT INTO event_services VALUES(2,1);
INSERT INTO event_services VALUES(3,2);
INSERT INTO event_services VALUES(4,3);
CREATE TABLE event_updates (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id INTEGER NOT NULL REFERENCES events(id) ON DELETE CASCADE,
    message TEXT NOT NULL,
    author_id INTEGER NOT NULL REFERENCES users(id),
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
INSERT INTO event_updates VALUES(1,1,'<p>Resolved.</p>
',1,'2026-10-05 08:15:00');
INSERT INTO event_updates VALUES(2,2,'<p>Investigating the mail queue.</p>
',2,'2026-10-20 09:05:00');
INSERT INTO event_updates VALUES(3,2,'<p>Fix deployed, watching.</p>
',2,'2026-10-20 10:30:00');
CREATE TABLE event_templates (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    kind TEXT NOT NULL
        CHECK (kind IN ('incident', 'maintenance', 'publication')),
    severity TEXT
        CHECK (severity IN ('minor', 'major', 'critical')),
    planned INTEGER NOT NULL DEFAULT 0
        CHECK (planned IN (0, 1)),
    category TEXT
        CHECK (category IN ('changelog', 'info')),
    icon_id INTEGER REFERENCES icons(id) ON DELETE SET NULL,
    created_by INTEGER NOT NULL REFERENCES users(id),
    usage_count INTEGER NOT NULL DEFAULT 0,
    last_used_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),

    CHECK (
        (kind = 'publication' AND category IS NOT NULL)
        OR (kind != 'publication' AND category IS NULL)
    ),
    CHECK (
        kind != 'publication' OR severity IS NULL
    )
);
INSERT INTO event_templates VALUES(1,'Fixture template: mail slowdown','Mail is slower than usual.','incident','minor',0,NULL,NULL,2,1,'2026-10-20 09:00:00','2026-10-06 10:00:00');
CREATE TABLE settings (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);
INSERT INTO settings VALUES('instance_name','Upgrade fixture');
INSERT INTO settings VALUES('public_mode','true');
INSERT INTO settings VALUES('time_zone','Europe/Dublin');
INSERT INTO settings VALUES('page_address','http://status.upgrade.test');
PRAGMA writable_schema=ON;
INSERT INTO sqlite_schema(type,name,tbl_name,rootpage,sql)VALUES('table','events_fts','events_fts',0,'CREATE VIRTUAL TABLE events_fts USING fts5(
    title,
    description,
    content=''events'',
    content_rowid=''id''
)');
CREATE TABLE IF NOT EXISTS 'events_fts_data'(id INTEGER PRIMARY KEY, block BLOB);
INSERT INTO events_fts_data VALUES(1,x'04101d');
INSERT INTO events_fts_data VALUES(10,x'000000000101010001010101');
INSERT INTO events_fts_data VALUES(137438953473,x'000001900230610306010107020b6e6e6f756e63656d656e74040203010b636f6e6e656374696f6e730106010103010664656c6179730202050203726f700102050503706564010601010401056576657279040601010801076669787475726501020201020201020201020202026f720106010105010601010601076761746577617903060101040108696e636964656e7401020301020303067472616e65740408050101040201730206010104020601010501046d61696c02080401010304086e74656e616e63650302030206696e75746573020601010802046f766573030601010501036e6577030601010801080401010301046f70656e040601010602077574676f696e67020601010201067175657565640206010105010672656d6f7465010601010601077365766572616c0206010107020474616666010601010701047465616d0406010109020268650306010102010601010202016f03060101060106010107010775706772616465030205010776657273696f6e03060101090202706e010804010102020804010103040810120b080a0c150e0e100e0d0c0d0d0b100b0e0d0d0e0b0b0e0d0c0e');
CREATE TABLE IF NOT EXISTS 'events_fts_idx'(segid, term, pgno, PRIMARY KEY(segid, term)) WITHOUT ROWID;
INSERT INTO events_fts_idx VALUES(1,x'',2);
CREATE TABLE IF NOT EXISTS 'events_fts_docsize'(id INTEGER PRIMARY KEY, sz BLOB);
INSERT INTO events_fts_docsize VALUES(1,x'0406');
INSERT INTO events_fts_docsize VALUES(2,x'0407');
INSERT INTO events_fts_docsize VALUES(3,x'0408');
INSERT INTO events_fts_docsize VALUES(4,x'0408');
CREATE TABLE IF NOT EXISTS 'events_fts_config'(k PRIMARY KEY, v) WITHOUT ROWID;
INSERT INTO events_fts_config VALUES('version',4);
CREATE TABLE IF NOT EXISTS "dashboard_layouts" (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    module_id TEXT NOT NULL UNIQUE,
    position INTEGER NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    config TEXT NOT NULL DEFAULT '{}',
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
INSERT INTO dashboard_layouts VALUES(1,'recent_activity',30,1,'{"hide":["maintenance"]}','2026-10-06 09:00:00');
INSERT INTO dashboard_layouts VALUES(2,'scheduled_maintenances',40,1,'{}','2026-10-05 06:50:00');
INSERT INTO dashboard_layouts VALUES(3,'services',20,1,'{"width":"wide"}','2026-10-06 09:01:00');
INSERT INTO dashboard_layouts VALUES(4,'status_banner',10,1,'{}','2026-10-05 06:50:00');
CREATE TABLE service_outages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    service_id INTEGER NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    CHECK (ended_at IS NULL OR ended_at >= started_at)
);
INSERT INTO service_outages VALUES(1,3,'2026-10-12 03:10:00','2026-10-12 03:42:00');
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
INSERT INTO notification_channels VALUES(1,'Fixture webhook','webhook','http://127.0.0.1:9/hook','en',1,1,1,1,'2026-10-05 07:20:00','2026-10-05 07:20:00');
INSERT INTO notification_channels VALUES(2,'Fixture mail list','email','ops@upgrade.test','fr',1,1,1,0,'2026-10-15 16:00:00','2026-10-15 16:00:00');
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
INSERT INTO notification_deliveries VALUES(1,1,'opened',1,NULL,'investigating',NULL,'failed',4,'2026-10-05 08:12:30','refused','2026-10-05 07:30:00',NULL);
INSERT INTO notification_deliveries VALUES(2,1,'closed',1,1,'resolved',NULL,'failed',4,'2026-10-05 08:57:30','refused','2026-10-05 08:15:00',NULL);
INSERT INTO notification_deliveries VALUES(3,1,'service_down',NULL,NULL,NULL,3,'failed',4,'2026-10-12 03:54:30','refused','2026-10-12 03:12:00',NULL);
INSERT INTO notification_deliveries VALUES(4,1,'service_up',NULL,NULL,NULL,3,'failed',0,'2026-10-12 03:54:30','refused','2026-10-12 03:42:00',NULL);
INSERT INTO notification_deliveries VALUES(5,1,'opened',2,NULL,'investigating',NULL,'failed',4,'2026-10-20 09:42:30','refused','2026-10-20 09:00:00',NULL);
INSERT INTO notification_deliveries VALUES(6,2,'opened',2,NULL,'investigating',NULL,'sent',1,'2026-10-20 09:00:00',NULL,'2026-10-20 09:00:00','2026-10-20 09:00:02');
INSERT INTO notification_deliveries VALUES(7,1,'updated',2,2,'investigating',NULL,'failed',0,'2026-10-20 09:42:30','refused','2026-10-20 09:05:00',NULL);
INSERT INTO notification_deliveries VALUES(8,2,'updated',2,2,'investigating',NULL,'sent',1,'2026-10-20 09:05:00',NULL,'2026-10-20 09:05:00','2026-10-20 09:05:03');
INSERT INTO notification_deliveries VALUES(9,1,'updated',2,3,'monitoring',NULL,'failed',4,'2026-10-20 11:12:30','refused','2026-10-20 10:30:00',NULL);
INSERT INTO notification_deliveries VALUES(10,2,'updated',2,3,'monitoring',NULL,'sent',1,'2026-10-20 10:30:00',NULL,'2026-10-20 10:30:00','2026-10-20 10:30:02');
INSERT INTO notification_deliveries VALUES(11,1,'opened',3,NULL,'scheduled',NULL,'failed',4,'2026-10-22 14:42:30','refused','2026-10-22 14:00:00',NULL);
INSERT INTO notification_deliveries VALUES(12,2,'opened',3,NULL,'scheduled',NULL,'sent',1,'2026-10-22 14:00:00',NULL,'2026-10-22 14:00:00','2026-10-22 14:00:02');
INSERT INTO notification_deliveries VALUES(13,1,'published',4,NULL,NULL,NULL,'failed',4,'2026-10-25 11:42:30','refused','2026-10-25 11:00:00',NULL);
INSERT INTO notification_deliveries VALUES(14,2,'published',4,NULL,NULL,NULL,'sent',1,'2026-10-25 11:00:00',NULL,'2026-10-25 11:00:00','2026-10-25 11:00:03');
ANALYZE sqlite_schema;
INSERT INTO sqlite_stat1 VALUES('events_fts_config','events_fts_config','1 1');
INSERT INTO sqlite_stat1 VALUES('events_fts_data',NULL,'2');
INSERT INTO sqlite_stat1 VALUES('_sqlx_migrations','sqlite_autoindex__sqlx_migrations_1','11 1');
INSERT INTO sqlite_stat1 VALUES('dashboard_layouts','sqlite_autoindex_dashboard_layouts_1','4 1');
INSERT INTO sqlite_stat1 VALUES('users','idx_users_role_active','1 1 1');
INSERT INTO sqlite_stat1 VALUES('users','sqlite_autoindex_users_1','1 1');
INSERT INTO sqlite_stat1 VALUES('notification_deliveries','idx_notification_deliveries_created','14 2');
INSERT INTO sqlite_stat1 VALUES('notification_deliveries','idx_notification_deliveries_queue','14 7 7 1');
INSERT INTO sqlite_stat1 VALUES('settings','sqlite_autoindex_settings_1','4 1');
INSERT INTO sqlite_stat1 VALUES('service_outages','idx_service_outages_open','0 0');
INSERT INTO sqlite_stat1 VALUES('service_outages','idx_service_outages_service','1 1 1');
INSERT INTO sqlite_stat1 VALUES('event_templates','idx_event_templates_icon','1 1');
INSERT INTO sqlite_stat1 VALUES('event_templates','idx_event_templates_usage','1 1');
INSERT INTO sqlite_stat1 VALUES('event_templates','idx_event_templates_title','1 1');
INSERT INTO sqlite_stat1 VALUES('events_fts_idx','events_fts_idx','1 1 1');
INSERT INTO sqlite_stat1 VALUES('icons','idx_icons_created_at','1 1');
INSERT INTO sqlite_stat1 VALUES('icons','sqlite_autoindex_icons_1','1 1');
INSERT INTO sqlite_stat1 VALUES('services','idx_services_icon','3 3');
INSERT INTO sqlite_stat1 VALUES('services','idx_services_status','3 2');
INSERT INTO sqlite_stat1 VALUES('services','sqlite_autoindex_services_1','3 1');
INSERT INTO sqlite_stat1 VALUES('events','idx_events_follows','4 4');
INSERT INTO sqlite_stat1 VALUES('events','idx_events_active','2 1 1');
INSERT INTO sqlite_stat1 VALUES('events','idx_events_upcoming_maintenance','1 1 1 1');
INSERT INTO sqlite_stat1 VALUES('events','idx_events_created_kind','4 1 1');
INSERT INTO sqlite_stat1 VALUES('events','idx_events_updated_at','4 1');
INSERT INTO sqlite_stat1 VALUES('events','idx_events_icon','4 4');
INSERT INTO sqlite_stat1 VALUES('events','idx_events_author','4 2');
INSERT INTO sqlite_stat1 VALUES('events','idx_events_lifecycle','4 1');
INSERT INTO sqlite_stat1 VALUES('events','idx_events_kind','4 2');
INSERT INTO sqlite_stat1 VALUES('event_services','idx_event_services_service','4 2');
INSERT INTO sqlite_stat1 VALUES('event_services','sqlite_autoindex_event_services_1','4 1 1');
INSERT INTO sqlite_stat1 VALUES('event_updates','idx_event_updates_created_at','3 1');
INSERT INTO sqlite_stat1 VALUES('event_updates','idx_event_updates_event','3 2');
INSERT INTO sqlite_stat1 VALUES('tower_sessions','idx_tower_sessions_expiry','3 1');
INSERT INTO sqlite_stat1 VALUES('tower_sessions','sqlite_autoindex_tower_sessions_1','3 1');
ANALYZE sqlite_schema;
CREATE TABLE tower_sessions
            (
                id text primary key not null,
                data blob not null,
                expiry_date integer not null
            );
CREATE TABLE IF NOT EXISTS sqlite_sequence(name,seq);
DELETE FROM sqlite_sequence;
INSERT INTO sqlite_sequence VALUES('dashboard_layouts',8);
INSERT INTO sqlite_sequence VALUES('users',3);
INSERT INTO sqlite_sequence VALUES('icons',1);
INSERT INTO sqlite_sequence VALUES('services',3);
INSERT INTO sqlite_sequence VALUES('event_templates',1);
INSERT INTO sqlite_sequence VALUES('events',4);
INSERT INTO sqlite_sequence VALUES('event_updates',3);
INSERT INTO sqlite_sequence VALUES('service_outages',1);
INSERT INTO sqlite_sequence VALUES('notification_channels',2);
INSERT INTO sqlite_sequence VALUES('notification_deliveries',14);
CREATE TRIGGER users_updated_at AFTER UPDATE ON users
BEGIN
    UPDATE users SET updated_at = datetime('now') WHERE id = NEW.id;
END;
CREATE TRIGGER services_updated_at AFTER UPDATE ON services
BEGIN
    UPDATE services SET updated_at = datetime('now') WHERE id = NEW.id;
END;
CREATE TRIGGER events_updated_at AFTER UPDATE ON events
WHEN NEW.updated_at IS OLD.updated_at
BEGIN
    UPDATE events SET updated_at = datetime('now') WHERE id = NEW.id;
END;
CREATE TRIGGER events_fts_insert AFTER INSERT ON events BEGIN
    INSERT INTO events_fts(rowid, title, description)
    VALUES (NEW.id, NEW.title, NEW.description);
END;
CREATE TRIGGER events_fts_delete AFTER DELETE ON events BEGIN
    INSERT INTO events_fts(events_fts, rowid, title, description)
    VALUES ('delete', OLD.id, OLD.title, OLD.description);
END;
CREATE TRIGGER events_fts_update AFTER UPDATE OF title, description ON events BEGIN
    INSERT INTO events_fts(events_fts, rowid, title, description)
    VALUES ('delete', OLD.id, OLD.title, OLD.description);
    INSERT INTO events_fts(rowid, title, description)
    VALUES (NEW.id, NEW.title, NEW.description);
END;
CREATE TRIGGER notification_channels_updated_at AFTER UPDATE ON notification_channels
BEGIN
    UPDATE notification_channels SET updated_at = datetime('now') WHERE id = NEW.id;
END;
CREATE INDEX idx_users_role_active ON users(role, is_active);
CREATE INDEX idx_icons_created_at ON icons(created_at DESC);
CREATE INDEX idx_services_status ON services(status);
CREATE INDEX idx_services_icon ON services(icon_id);
CREATE INDEX idx_events_kind ON events(kind);
CREATE INDEX idx_events_lifecycle ON events(lifecycle);
CREATE INDEX idx_events_author ON events(author_id);
CREATE INDEX idx_events_icon ON events(icon_id);
CREATE INDEX idx_events_updated_at ON events(updated_at);
CREATE INDEX idx_events_created_kind ON events(created_at DESC, kind);
CREATE INDEX idx_events_upcoming_maintenance
    ON events(kind, lifecycle, planned_start)
    WHERE kind = 'maintenance' AND planned = 1 AND lifecycle = 'scheduled';
CREATE INDEX idx_events_active
    ON events(lifecycle, kind)
    WHERE lifecycle IS NOT NULL AND lifecycle NOT IN ('resolved', 'completed', 'cancelled');
CREATE INDEX idx_event_services_service ON event_services(service_id);
CREATE INDEX idx_event_updates_event ON event_updates(event_id);
CREATE INDEX idx_event_updates_created_at ON event_updates(created_at);
CREATE INDEX idx_event_templates_title ON event_templates(title);
CREATE INDEX idx_event_templates_usage ON event_templates(usage_count DESC);
CREATE INDEX idx_event_templates_icon ON event_templates(icon_id);
CREATE INDEX idx_events_follows ON events(follows_event_id);
CREATE INDEX idx_service_outages_service ON service_outages(service_id, started_at);
CREATE UNIQUE INDEX idx_service_outages_open ON service_outages(service_id)
    WHERE ended_at IS NULL;
CREATE INDEX idx_notification_deliveries_queue
    ON notification_deliveries(channel_id, status, id);
CREATE INDEX idx_notification_deliveries_created ON notification_deliveries(created_at);
CREATE INDEX idx_tower_sessions_expiry ON tower_sessions (expiry_date);
PRAGMA writable_schema=OFF;
COMMIT;
