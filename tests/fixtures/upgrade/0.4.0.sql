-- Statup 0.4.0 database, made by the published image of 0.4.0 and a fixed data set, to test upgrades.
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
INSERT INTO _sqlx_migrations VALUES(20260917000001,'initial schema','2026-11-02 13:12:30',1,x'3a2a607b68a7ef7ef1cdff97ed9ea094a748244b88a42c58d0cdfb6886f832882ff69c29d8d672b16b2d514350f3f6fc',9260250);
INSERT INTO _sqlx_migrations VALUES(20260917000002,'announcement follows','2026-11-02 13:12:30',1,x'b506725f1f4bc84dcc2940b9a8e3824aae81f33f44673f2a9495b2d96230c8c47adb9f73e080f70a80c240704e56570a',1084500);
INSERT INTO _sqlx_migrations VALUES(20260923000001,'service base status','2026-11-02 13:12:30',1,x'28b8b6108bb27aba122e55241831e124123e1cdc5157c153d967747c29e93591cd411c3081e581699c0fa121d05f9080',1064333);
INSERT INTO _sqlx_migrations VALUES(20260923000002,'single degraded level','2026-11-02 13:12:30',1,x'02c2cf1e7b743d59d99032fb1d183a275ab31f90e92be509ed7a4304287c5aa972dd045a9e2a9150762301157b74a53b',281333);
INSERT INTO _sqlx_migrations VALUES(20260924000001,'maintenance without downtime','2026-11-02 13:12:30',1,x'2fb82608b90b7b2659d0e40ede2883b8480f283058f5218db82c70e0db05a59b26d3bd786ccf632c475e86cd13fb47aa',2505375);
INSERT INTO _sqlx_migrations VALUES(20260924000002,'single dashboard layout','2026-11-02 13:12:30',1,x'891f9bfed12b14013a088711b3453849ac67a2fc00f44899c3c0ded61a9d753006e19d267ae20d094a49c6b2b3e9e695',2093917);
INSERT INTO _sqlx_migrations VALUES(20260925000001,'temporary password expiry','2026-11-02 13:12:30',1,x'c84e68da87284a4a1a2194e78ac64924d2c4afa102b837343752911486230e5347341f93825d62e015d1f2c73df117a8',833500);
INSERT INTO _sqlx_migrations VALUES(20260925000002,'dashboard layout without user','2026-11-02 13:12:30',1,x'290c37e099559beeff960aa967f29c63a6284a06d241060ab4222449224eaae1422ec79717faee9179f0f981b8def185',2022709);
INSERT INTO _sqlx_migrations VALUES(20261004000001,'service monitoring','2026-11-02 13:12:30',1,x'b06a50bef906de5c1d160e0af74387f003a2c108c1685b5e59075e56252b2ad4d11e974191794ad7f01538dd2f29cb5d',2659166);
INSERT INTO _sqlx_migrations VALUES(20261006000001,'any supported locale','2026-11-02 13:12:30',1,x'5c829db39f507a37a8dc4f86e87f64283179d4e2983893d4e07236d5eb16a5ad94d6366305bb7be175958a15182d0456',2663459);
INSERT INTO _sqlx_migrations VALUES(20261007000001,'notifications','2026-11-02 13:12:30',1,x'6cd7afac804c148068190aceaabfa1ee5165344a710ed767b1a0ed08ac9128a1c4fa2463948e4232c630fe7b3084068a',1782083);
INSERT INTO _sqlx_migrations VALUES(20261008000001,'service position','2026-11-02 13:12:30',1,x'83dcfd7a5d0fccb74cd8b4896916428a4210283a2cd31a101ab24c010b46bdfcff8615448ba05f8119cfb479e87a3230',900750);
INSERT INTO _sqlx_migrations VALUES(20261008000002,'password resets','2026-11-02 13:12:30',1,x'2ffb03b78a46a460d534053e3ed035e6caba0a11ffb7d767cf814ff5093c29d6e630e9c183e446d84eca635dc05cc46a',1001458);
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
INSERT INTO users VALUES(1,'admin@upgrade.test','$argon2id$v=19$m=19456,t=2,p=1$aBvwpv0PtrOPEJO5ZZKRRg$p/ZIayl8MjXQBesvANe4QU82rI9sXB16it2HdVTMp74','admin','admin',1,'2026-10-25 11:05:00','2026-10-01 07:55:00','2026-10-25 11:05:00',0,NULL,NULL);
INSERT INTO users VALUES(2,'editor@upgrade.test','$argon2id$v=19$m=19456,t=2,p=1$aBvwpv0PtrOPEJO5ZZKRRg$p/ZIayl8MjXQBesvANe4QU82rI9sXB16it2HdVTMp74','Fixture Editor','publisher',1,'2026-10-20 09:31:00','2026-10-01 08:00:00','2026-10-01 08:10:00',0,NULL,'en');
INSERT INTO users VALUES(3,'reader@upgrade.test','$argon2id$v=19$m=19456,t=2,p=1$aBvwpv0PtrOPEJO5ZZKRRg$p/ZIayl8MjXQBesvANe4QU82rI9sXB16it2HdVTMp74','Fixture Reader','reader',1,'2026-10-28 13:55:00','2026-10-01 08:05:00','2026-10-01 08:05:00',0,NULL,NULL);
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
INSERT INTO icons VALUES(1,'a3f1c2d4-5b6e-4f70-8a9b-0c1d2e3f4a5b.svg','fixture-icon.svg','image/svg+xml',412,1,'2026-10-01 08:20:00');
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
    CHECK (detected_status IN ('major_outage')), position INTEGER NOT NULL DEFAULT 0);
INSERT INTO services VALUES(1,'Fixture Mail','fixture-mail','Company mail.','operational','2026-10-01 08:30:00','2026-10-20 09:30:00',NULL,'envelope','operational',NULL,NULL,0,NULL,1);
INSERT INTO services VALUES(2,'Fixture VPN','fixture-vpn','Remote access.','degraded','2026-10-01 08:31:00','2026-10-12 08:35:00',NULL,'lock-closed','degraded','tcp','127.0.0.1:9',0,NULL,2);
INSERT INTO services VALUES(3,'Fixture Intranet','fixture-intranet',NULL,'operational','2026-10-01 08:32:00','2026-10-15 03:20:00',NULL,'globe','operational','http','https://127.0.0.1:9/',1,NULL,3);
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
INSERT INTO events VALUES(1,'incident','critical',0,'resolved',NULL,'Fixture incident: VPN drop','The VPN gateway stopped answering.',NULL,NULL,'2026-10-12 06:55:00','2026-10-12 08:30:00',NULL,2,'investigating','2026-10-12 07:00:00','2026-10-12 08:30:00',NULL,NULL,0);
INSERT INTO events VALUES(2,'incident','minor',0,'monitoring',NULL,'Fixture incident: mail delays','Outgoing mail is delayed by up to an hour.',NULL,NULL,'2026-10-20 07:50:00',NULL,NULL,2,'investigating','2026-10-20 08:00:00','2026-10-20 09:30:00',NULL,'2026-10-20 09:30:00',0);
INSERT INTO events VALUES(3,'maintenance',NULL,1,'scheduled',NULL,'Fixture maintenance: VPN upgrade','The VPN gateway moves to its new firmware.','2027-03-01 22:00:00','2027-03-01 23:00:00',NULL,NULL,NULL,1,NULL,'2026-10-22 10:00:00','2026-10-22 10:00:00',NULL,NULL,0);
INSERT INTO events VALUES(4,'publication',NULL,0,NULL,'changelog','Fixture announcement: new intranet','The intranet opens at its new address on Monday.',NULL,NULL,'2026-10-25 11:00:00',NULL,NULL,1,NULL,'2026-10-25 11:00:00','2026-10-25 11:00:00',NULL,NULL,0);
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
INSERT INTO event_updates VALUES(1,1,'<p>Resolved.</p>'||char(10),2,'2026-10-12 08:30:00');
INSERT INTO event_updates VALUES(2,2,'<p>Investigating the mail queue.</p>'||char(10),2,'2026-10-20 08:15:00');
INSERT INTO event_updates VALUES(3,2,'<p>Fix deployed, watching.</p>'||char(10),1,'2026-10-20 09:30:00');
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
INSERT INTO event_templates VALUES(1,'Fixture template: mail outage','Mail is not delivered.','incident','critical',0,NULL,NULL,2,1,'2026-10-20 07:59:00','2026-10-05 09:00:00');
CREATE TABLE settings (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);
INSERT INTO settings VALUES('instance_name','Upgrade fixture');
INSERT INTO settings VALUES('public_mode','true');
INSERT INTO settings VALUES('time_zone','Europe/Dublin');
INSERT INTO settings VALUES('services_problems_first','true');
INSERT INTO settings VALUES('page_address','http://status.upgrade.test');
PRAGMA writable_schema=ON;
INSERT INTO sqlite_schema(type,name,tbl_name,rootpage,sql)VALUES('table','events_fts','events_fts',0,'CREATE VIRTUAL TABLE events_fts USING fts5(
    title,
    description,
    content=''events'',
    content_rowid=''id''
)');
CREATE TABLE IF NOT EXISTS 'events_fts_data'(id INTEGER PRIMARY KEY, block BLOB);
INSERT INTO events_fts_data VALUES(1,x'04101f');
INSERT INTO events_fts_data VALUES(10,x'000000000101010001010101');
INSERT INTO events_fts_data VALUES(137438953473,x'0000018c083061646472657373040601010802016e0206010109030a6e6f756e63656d656e74040203030773776572696e6701060101060201740406010105010262790206010106010764656c6179656402060101050601730202050203726f7001020501086669726d77617265030601010903057874757265010202010202010202010202010767617465776179010601010402060101040104686f7572020601010a0108696e636964656e7401020301020303067472616e65740408050101030201730206010104020274730306010107010601010601046d61696c02080401010304086e74656e616e636503020302056f6e646179040601010a0303766573030601010501036e6577030601010801080401010701026f6e0406010109020470656e73040601010402077574676f696e670206010102010773746f707065640106010105010374686501060101020206010102010601010202016f0206010108010601010601027570020601010703056772616465030205010376706e010804010103020804010103040e080f0e08090e06080f13130b100e080e0c0d0c0a10090b0e0e140d090a');
CREATE TABLE IF NOT EXISTS 'events_fts_idx'(segid, term, pgno, PRIMARY KEY(segid, term)) WITHOUT ROWID;
INSERT INTO events_fts_idx VALUES(1,x'',2);
CREATE TABLE IF NOT EXISTS 'events_fts_docsize'(id INTEGER PRIMARY KEY, sz BLOB);
INSERT INTO events_fts_docsize VALUES(1,x'0405');
INSERT INTO events_fts_docsize VALUES(2,x'0409');
INSERT INTO events_fts_docsize VALUES(3,x'0408');
INSERT INTO events_fts_docsize VALUES(4,x'0409');
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
INSERT INTO dashboard_layouts VALUES(1,'recent_activity',30,1,'{"hide":["maintenance"]}','2026-10-03 10:05:00');
INSERT INTO dashboard_layouts VALUES(2,'scheduled_maintenances',40,1,'{}','2026-10-01 07:55:00');
INSERT INTO dashboard_layouts VALUES(3,'services',20,1,'{"width":"wide"}','2026-10-03 10:00:00');
INSERT INTO dashboard_layouts VALUES(4,'status_banner',10,1,'{}','2026-10-01 07:55:00');
CREATE TABLE service_outages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    service_id INTEGER NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    CHECK (ended_at IS NULL OR ended_at >= started_at)
);
INSERT INTO service_outages VALUES(1,3,'2026-10-15 03:00:00','2026-10-15 03:20:00');
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
INSERT INTO notification_channels VALUES(1,'Fixture webhook','webhook','http://127.0.0.1:9/hook','en',1,1,1,1,'2026-10-10 09:00:00','2026-10-10 09:00:00');
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
INSERT INTO notification_deliveries VALUES(1,1,'test',NULL,NULL,NULL,NULL,'sent',1,'2026-10-10 09:01:00',NULL,'2026-10-10 09:01:00','2026-10-10 09:01:01');
INSERT INTO notification_deliveries VALUES(2,1,'opened',1,NULL,'investigating',NULL,'sent',1,'2026-10-12 07:00:00',NULL,'2026-10-12 07:00:00','2026-10-12 07:00:02');
INSERT INTO notification_deliveries VALUES(3,1,'closed',1,1,'resolved',NULL,'sent',1,'2026-10-12 08:30:00',NULL,'2026-10-12 08:30:00','2026-10-12 08:30:01');
INSERT INTO notification_deliveries VALUES(4,1,'service_down',NULL,NULL,NULL,3,'sent',1,'2026-10-15 03:00:00',NULL,'2026-10-15 03:00:00','2026-10-15 03:00:01');
INSERT INTO notification_deliveries VALUES(5,1,'service_up',NULL,NULL,NULL,3,'sent',1,'2026-10-15 03:20:00',NULL,'2026-10-15 03:20:00','2026-10-15 03:20:01');
INSERT INTO notification_deliveries VALUES(6,1,'opened',2,NULL,'investigating',NULL,'sent',1,'2026-10-20 08:00:00',NULL,'2026-10-20 08:00:00','2026-10-20 08:00:01');
INSERT INTO notification_deliveries VALUES(7,1,'updated',2,2,'investigating',NULL,'failed',4,'2026-10-20 08:57:30','refused','2026-10-20 08:15:00',NULL);
INSERT INTO notification_deliveries VALUES(8,1,'updated',2,3,'monitoring',NULL,'sent',1,'2026-10-20 09:30:00',NULL,'2026-10-20 09:30:00','2026-10-20 09:30:01');
INSERT INTO notification_deliveries VALUES(9,1,'opened',3,NULL,'scheduled',NULL,'sent',1,'2026-10-22 10:00:00',NULL,'2026-10-22 10:00:00','2026-10-22 10:00:01');
INSERT INTO notification_deliveries VALUES(10,1,'published',4,NULL,NULL,NULL,'sent',1,'2026-10-25 11:00:00',NULL,'2026-10-25 11:00:00','2026-10-25 11:00:01');
CREATE TABLE password_resets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    secret_hash TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
INSERT INTO password_resets VALUES(1,3,'$argon2id$v=19$m=19456,t=2,p=1$aBvwpv0PtrOPEJO5ZZKRRg$p/ZIayl8MjXQBesvANe4QU82rI9sXB16it2HdVTMp74','2026-10-28 15:00:00','2026-10-28 14:00:00');
ANALYZE sqlite_schema;
INSERT INTO sqlite_stat1 VALUES('events_fts_config','events_fts_config','1 1');
INSERT INTO sqlite_stat1 VALUES('events_fts_data',NULL,'2');
INSERT INTO sqlite_stat1 VALUES('_sqlx_migrations','sqlite_autoindex__sqlx_migrations_1','13 1');
INSERT INTO sqlite_stat1 VALUES('dashboard_layouts','sqlite_autoindex_dashboard_layouts_1','4 1');
INSERT INTO sqlite_stat1 VALUES('users','idx_users_role_active','1 1 1');
INSERT INTO sqlite_stat1 VALUES('users','sqlite_autoindex_users_1','1 1');
INSERT INTO sqlite_stat1 VALUES('notification_deliveries','idx_notification_deliveries_created','10 1');
INSERT INTO sqlite_stat1 VALUES('notification_deliveries','idx_notification_deliveries_queue','10 10 5 1');
INSERT INTO sqlite_stat1 VALUES('settings','sqlite_autoindex_settings_1','5 1');
INSERT INTO sqlite_stat1 VALUES('password_resets','idx_password_resets_user','1 1');
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
INSERT INTO sqlite_stat1 VALUES('tower_sessions','idx_tower_sessions_expiry','1 1');
INSERT INTO sqlite_stat1 VALUES('tower_sessions','sqlite_autoindex_tower_sessions_1','1 1');
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
INSERT INTO sqlite_sequence VALUES('service_outages',1);
INSERT INTO sqlite_sequence VALUES('event_templates',1);
INSERT INTO sqlite_sequence VALUES('events',4);
INSERT INTO sqlite_sequence VALUES('event_updates',3);
INSERT INTO sqlite_sequence VALUES('notification_channels',1);
INSERT INTO sqlite_sequence VALUES('notification_deliveries',10);
INSERT INTO sqlite_sequence VALUES('password_resets',1);
CREATE TRIGGER users_updated_at AFTER UPDATE ON users
BEGIN
    UPDATE users SET updated_at = datetime('now') WHERE id = NEW.id;
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
CREATE TRIGGER services_updated_at AFTER UPDATE ON services
WHEN OLD.position IS NEW.position
BEGIN
    UPDATE services SET updated_at = datetime('now') WHERE id = NEW.id;
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
CREATE INDEX idx_password_resets_user ON password_resets(user_id);
CREATE INDEX idx_tower_sessions_expiry ON tower_sessions (expiry_date);
PRAGMA writable_schema=OFF;
COMMIT;
