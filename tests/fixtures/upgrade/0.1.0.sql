-- Statup 0.1.0 database, made by the published image of 0.1.0 and a fixed data set, to test upgrades.
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
INSERT INTO _sqlx_migrations VALUES(20260917000001,'initial schema','2026-11-02 13:10:29',1,x'3a2a607b68a7ef7ef1cdff97ed9ea094a748244b88a42c58d0cdfb6886f832882ff69c29d8d672b16b2d514350f3f6fc',12099292);
INSERT INTO _sqlx_migrations VALUES(20260917000002,'announcement follows','2026-11-02 13:10:29',1,x'b506725f1f4bc84dcc2940b9a8e3824aae81f33f44673f2a9495b2d96230c8c47adb9f73e080f70a80c240704e56570a',808000);
INSERT INTO _sqlx_migrations VALUES(20260923000001,'service base status','2026-11-02 13:10:29',1,x'28b8b6108bb27aba122e55241831e124123e1cdc5157c153d967747c29e93591cd411c3081e581699c0fa121d05f9080',947875);
INSERT INTO _sqlx_migrations VALUES(20260923000002,'single degraded level','2026-11-02 13:10:29',1,x'02c2cf1e7b743d59d99032fb1d183a275ab31f90e92be509ed7a4304287c5aa972dd045a9e2a9150762301157b74a53b',319625);
INSERT INTO _sqlx_migrations VALUES(20260924000001,'maintenance without downtime','2026-11-02 13:10:29',1,x'2fb82608b90b7b2659d0e40ede2883b8480f283058f5218db82c70e0db05a59b26d3bd786ccf632c475e86cd13fb47aa',693791);
INSERT INTO _sqlx_migrations VALUES(20260924000002,'single dashboard layout','2026-11-02 13:10:29',1,x'891f9bfed12b14013a088711b3453849ac67a2fc00f44899c3c0ded61a9d753006e19d267ae20d094a49c6b2b3e9e695',1691875);
INSERT INTO _sqlx_migrations VALUES(20260925000001,'temporary password expiry','2026-11-02 13:10:29',1,x'c84e68da87284a4a1a2194e78ac64924d2c4afa102b837343752911486230e5347341f93825d62e015d1f2c73df117a8',480167);
INSERT INTO _sqlx_migrations VALUES(20260925000002,'dashboard layout without user','2026-11-02 13:10:29',1,x'290c37e099559beeff960aa967f29c63a6284a06d241060ab4222449224eaae1422ec79717faee9179f0f981b8def185',1473958);
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
    preferred_locale TEXT
        CHECK (preferred_locale IS NULL OR preferred_locale IN ('fr', 'en')),
    -- Set when someone else chose the password (an administrator adding a
    -- member), cleared when the person sets their own.
    must_change_password BOOLEAN NOT NULL DEFAULT 0
, temporary_password_expires_at TEXT);
INSERT INTO users VALUES(1,'admin@upgrade.test','$argon2id$v=19$m=19456,t=2,p=1$tb3iELLaNei6rwwkOGPRhg$a7cERfDeYQDVkkfLPybRaZL++FkADXBg/qGSqk9GxWk','admin','admin',1,'2026-10-25 11:05:00','2026-10-01 09:00:00','2026-10-25 11:05:00',NULL,0,NULL);
INSERT INTO users VALUES(2,'editor@upgrade.test','$argon2id$v=19$m=19456,t=2,p=1$tb3iELLaNei6rwwkOGPRhg$a7cERfDeYQDVkkfLPybRaZL++FkADXBg/qGSqk9GxWk','Fixture Editor','publisher',1,'2026-10-20 10:00:00','2026-10-01 09:10:00','2026-10-01 09:10:00','en',0,NULL);
INSERT INTO users VALUES(3,'reader@upgrade.test','$argon2id$v=19$m=19456,t=2,p=1$tb3iELLaNei6rwwkOGPRhg$a7cERfDeYQDVkkfLPybRaZL++FkADXBg/qGSqk9GxWk','Fixture Reader','reader',1,'2026-10-21 08:30:00','2026-10-01 09:11:00','2026-10-01 09:11:00','fr',0,NULL);
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
INSERT INTO icons VALUES(1,'3f6c1d2e-8a4b-4c5d-9e6f-7a8b9c0d1e2f.png','fixture-icon.png','image/png',1846,1,'2026-10-02 10:00:00');
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
    CHECK (manual_status IN ('operational', 'degraded', 'partial_outage', 'major_outage', 'maintenance')));
INSERT INTO services VALUES(1,'Fixture Mail','fixture-mail','Company mail and calendars.','operational','2026-10-01 09:20:00','2026-10-20 10:00:00',NULL,'envelope','operational');
INSERT INTO services VALUES(2,'Fixture VPN','fixture-vpn','Remote access to the office network.','degraded','2026-10-01 09:21:00','2026-10-22 14:00:00',NULL,'lock-closed','degraded');
INSERT INTO services VALUES(3,'Fixture Intranet','fixture-intranet',NULL,'operational','2026-10-01 09:22:00','2026-10-01 09:22:00',NULL,'globe','operational');
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
INSERT INTO events VALUES(1,'incident','critical',0,'resolved',NULL,'Fixture incident: VPN drop','Remote users lost the VPN connection.',NULL,NULL,'2026-10-12 08:00:00','2026-10-12 09:30:00',NULL,2,'investigating','2026-10-12 08:00:00','2026-10-12 09:30:00',NULL,NULL,0);
INSERT INTO events VALUES(2,'incident','minor',0,'monitoring',NULL,'Fixture incident: mail delays','Outgoing mail is delayed by up to an hour.',NULL,NULL,'2026-10-20 09:00:00',NULL,NULL,2,'investigating','2026-10-20 09:00:00','2026-10-20 10:00:00',NULL,'2026-10-20 10:00:00',0);
INSERT INTO events VALUES(3,'maintenance',NULL,1,'scheduled',NULL,'Fixture maintenance: VPN upgrade','The VPN gateway moves to a new version.','2027-03-01 22:00:00','2027-03-01 23:00:00',NULL,NULL,NULL,1,NULL,'2026-10-22 15:00:00','2026-10-22 15:00:00',NULL,NULL,0);
INSERT INTO events VALUES(4,'publication',NULL,0,NULL,'info','Fixture announcement: new intranet','The new intranet is open to everyone.',NULL,NULL,'2026-10-25 11:00:00',NULL,NULL,1,NULL,'2026-10-25 11:00:00','2026-10-25 11:00:00',NULL,NULL,0);
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
',2,'2026-10-12 09:30:00');
INSERT INTO event_updates VALUES(2,2,'<p>Investigating the mail queue.</p>
',2,'2026-10-20 09:05:00');
INSERT INTO event_updates VALUES(3,2,'<p>Fix deployed, watching.</p>
',2,'2026-10-20 10:00:00');
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
INSERT INTO event_templates VALUES(1,'Mail delays','Outgoing mail is delayed.','incident','minor',0,NULL,1,1,1,'2026-10-20 09:00:00','2026-10-05 10:00:00');
CREATE TABLE settings (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);
INSERT INTO settings VALUES('instance_name','Upgrade fixture');
INSERT INTO settings VALUES('public_mode','true');
INSERT INTO settings VALUES('time_zone','Europe/Dublin');
PRAGMA writable_schema=ON;
INSERT INTO sqlite_schema(type,name,tbl_name,rootpage,sql)VALUES('table','events_fts','events_fts',0,'CREATE VIRTUAL TABLE events_fts USING fts5(
    title,
    description,
    content=''events'',
    content_rowid=''id''
)');
CREATE TABLE IF NOT EXISTS 'events_fts_data'(id INTEGER PRIMARY KEY, block BLOB);
INSERT INTO events_fts_data VALUES(1,x'04101e');
INSERT INTO events_fts_data VALUES(10,x'000000000104040004010101020101030101040101');
INSERT INTO events_fts_data VALUES(137438953473,x'000000700b30636f6e6e656374696f6e0106010107010464726f700102050107666978747572650102020108696e636964656e7401020301046c6f73740106010104010672656d6f7465010601010201037468650106010105010575736572730106010103010376706e0108040101060411090c0d0b0d0a0c');
INSERT INTO events_fts_data VALUES(274877906945,x'000000830330616e0206010109010262790206010106010764656c6179656402060101050601730202050107666978747572650202020104686f7572020601010a0108696e636964656e74020203020173020601010401046d61696c02080401010301086f7574676f696e6702060101020102746f02060101080102757002060101070409090e060c0b0d080c0f09');
INSERT INTO events_fts_data VALUES(412316860417,x'0000008102306103060101070107666978747572650302020107676174657761790306010104010b6d61696e74656e616e636503020302046f766573030601010501036e657703060101080103746865030601010202016f0306010106010775706772616465030205010776657273696f6e03060101090202706e03080401010304080c0e100b0a0a080c0e');
INSERT INTO events_fts_data VALUES(549755813889,x'000000700d30616e6e6f756e63656d656e74040203010865766572796f6e6504060101080107666978747572650402020108696e7472616e6574040805010104020173040601010501036e657704080401010301046f70656e04060101060103746865040601010202016f040601010704110f0c10080b0b0a');
CREATE TABLE IF NOT EXISTS 'events_fts_idx'(segid, term, pgno, PRIMARY KEY(segid, term)) WITHOUT ROWID;
INSERT INTO events_fts_idx VALUES(1,x'',2);
INSERT INTO events_fts_idx VALUES(2,x'',2);
INSERT INTO events_fts_idx VALUES(3,x'',2);
INSERT INTO events_fts_idx VALUES(4,x'',2);
CREATE TABLE IF NOT EXISTS 'events_fts_docsize'(id INTEGER PRIMARY KEY, sz BLOB);
INSERT INTO events_fts_docsize VALUES(1,x'0406');
INSERT INTO events_fts_docsize VALUES(2,x'0409');
INSERT INTO events_fts_docsize VALUES(3,x'0408');
INSERT INTO events_fts_docsize VALUES(4,x'0407');
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
INSERT INTO dashboard_layouts VALUES(1,'recent_activity',30,1,'{"hide":["maintenance"]}','2026-10-03 12:01:00');
INSERT INTO dashboard_layouts VALUES(2,'scheduled_maintenances',40,1,'{}','2026-10-01 09:00:00');
INSERT INTO dashboard_layouts VALUES(3,'services',20,1,'{"width":"wide"}','2026-10-03 12:00:00');
INSERT INTO dashboard_layouts VALUES(4,'status_banner',10,1,'{}','2026-10-01 09:00:00');
ANALYZE sqlite_schema;
INSERT INTO sqlite_stat1 VALUES('events_fts_config','events_fts_config','1 1');
INSERT INTO sqlite_stat1 VALUES('events_fts_data',NULL,'2');
INSERT INTO sqlite_stat1 VALUES('_sqlx_migrations','sqlite_autoindex__sqlx_migrations_1','8 1');
INSERT INTO sqlite_stat1 VALUES('event_templates','idx_event_templates_icon','1 1');
INSERT INTO sqlite_stat1 VALUES('event_templates','idx_event_templates_usage','1 1');
INSERT INTO sqlite_stat1 VALUES('event_templates','idx_event_templates_title','1 1');
INSERT INTO sqlite_stat1 VALUES('tower_sessions','idx_tower_sessions_expiry','1 1');
INSERT INTO sqlite_stat1 VALUES('tower_sessions','sqlite_autoindex_tower_sessions_1','1 1');
INSERT INTO sqlite_stat1 VALUES('users','idx_users_role_active','3 1 1');
INSERT INTO sqlite_stat1 VALUES('users','sqlite_autoindex_users_1','3 1');
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
ANALYZE sqlite_schema;
CREATE TABLE tower_sessions
            (
                id text primary key not null,
                data blob not null,
                expiry_date integer not null
            );
CREATE TABLE IF NOT EXISTS sqlite_sequence(name,seq);
DELETE FROM sqlite_sequence;
INSERT INTO sqlite_sequence VALUES('dashboard_layouts',12);
INSERT INTO sqlite_sequence VALUES('users',3);
INSERT INTO sqlite_sequence VALUES('icons',1);
INSERT INTO sqlite_sequence VALUES('services',3);
INSERT INTO sqlite_sequence VALUES('events',4);
INSERT INTO sqlite_sequence VALUES('event_updates',3);
INSERT INTO sqlite_sequence VALUES('event_templates',1);
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
CREATE INDEX idx_tower_sessions_expiry ON tower_sessions (expiry_date);
PRAGMA writable_schema=OFF;
COMMIT;
