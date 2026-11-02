-- Statup 0.2.0 database, made by the published image of 0.2.0 and a fixed data set, to test upgrades.
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
INSERT INTO _sqlx_migrations VALUES(20260917000001,'initial schema','2026-11-02 13:10:35',1,x'3a2a607b68a7ef7ef1cdff97ed9ea094a748244b88a42c58d0cdfb6886f832882ff69c29d8d672b16b2d514350f3f6fc',7177833);
INSERT INTO _sqlx_migrations VALUES(20260917000002,'announcement follows','2026-11-02 13:10:35',1,x'b506725f1f4bc84dcc2940b9a8e3824aae81f33f44673f2a9495b2d96230c8c47adb9f73e080f70a80c240704e56570a',1905625);
INSERT INTO _sqlx_migrations VALUES(20260923000001,'service base status','2026-11-02 13:10:35',1,x'28b8b6108bb27aba122e55241831e124123e1cdc5157c153d967747c29e93591cd411c3081e581699c0fa121d05f9080',1783667);
INSERT INTO _sqlx_migrations VALUES(20260923000002,'single degraded level','2026-11-02 13:10:35',1,x'02c2cf1e7b743d59d99032fb1d183a275ab31f90e92be509ed7a4304287c5aa972dd045a9e2a9150762301157b74a53b',360917);
INSERT INTO _sqlx_migrations VALUES(20260924000001,'maintenance without downtime','2026-11-02 13:10:35',1,x'2fb82608b90b7b2659d0e40ede2883b8480f283058f5218db82c70e0db05a59b26d3bd786ccf632c475e86cd13fb47aa',1280667);
INSERT INTO _sqlx_migrations VALUES(20260924000002,'single dashboard layout','2026-11-02 13:10:35',1,x'891f9bfed12b14013a088711b3453849ac67a2fc00f44899c3c0ded61a9d753006e19d267ae20d094a49c6b2b3e9e695',2580750);
INSERT INTO _sqlx_migrations VALUES(20260925000001,'temporary password expiry','2026-11-02 13:10:35',1,x'c84e68da87284a4a1a2194e78ac64924d2c4afa102b837343752911486230e5347341f93825d62e015d1f2c73df117a8',907834);
INSERT INTO _sqlx_migrations VALUES(20260925000002,'dashboard layout without user','2026-11-02 13:10:35',1,x'290c37e099559beeff960aa967f29c63a6284a06d241060ab4222449224eaae1422ec79717faee9179f0f981b8def185',2370041);
INSERT INTO _sqlx_migrations VALUES(20261004000001,'service monitoring','2026-11-02 13:10:35',1,x'b06a50bef906de5c1d160e0af74387f003a2c108c1685b5e59075e56252b2ad4d11e974191794ad7f01538dd2f29cb5d',2467500);
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
INSERT INTO users VALUES(1,'admin@upgrade.test','$argon2id$v=19$m=19456,t=2,p=1$4PssyUKIuAIfAC57MVSvjg$Nsj7CuhqbBRc0i71Hbjve3yu4IeEMUKvyGmjDs/Ej5A','admin','admin',1,'2026-10-25 10:00:00','2026-10-01 08:00:00','2026-10-25 10:00:00',NULL,0,NULL);
INSERT INTO users VALUES(2,'editor@upgrade.test','$argon2id$v=19$m=19456,t=2,p=1$4PssyUKIuAIfAC57MVSvjg$Nsj7CuhqbBRc0i71Hbjve3yu4IeEMUKvyGmjDs/Ej5A','editor','publisher',1,'2026-10-22 10:05:00','2026-10-01 08:10:00','2026-10-22 10:05:00','en',0,NULL);
INSERT INTO users VALUES(3,'reader@upgrade.test','$argon2id$v=19$m=19456,t=2,p=1$4PssyUKIuAIfAC57MVSvjg$Nsj7CuhqbBRc0i71Hbjve3yu4IeEMUKvyGmjDs/Ej5A','reader','reader',1,'2026-10-25 09:30:00','2026-10-01 08:20:00','2026-10-25 09:30:00',NULL,0,NULL);
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
INSERT INTO icons VALUES(1,'3f2a9c4e-6b1d-4e8a-9f70-2c5d8e1b4a63.svg','intranet.svg','image/svg+xml',412,1,'2026-10-02 09:00:00');
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
INSERT INTO services VALUES(1,'Fixture Mail','fixture-mail','Company mail and calendars.','operational','2026-10-02 09:10:00','2026-10-02 09:10:00',NULL,'envelope','operational','http','http://127.0.0.1:9/health',0,NULL);
INSERT INTO services VALUES(2,'Fixture VPN','fixture-vpn','Remote access for staff.','degraded','2026-10-02 09:11:00','2026-10-21 16:00:00',NULL,'lock-closed','degraded',NULL,NULL,0,NULL);
INSERT INTO services VALUES(3,'Fixture Intranet','fixture-intranet',NULL,'operational','2026-10-02 09:12:00','2026-10-12 03:41:00',NULL,'globe','operational','tcp','127.0.0.1:9',0,NULL);
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
INSERT INTO events VALUES(1,'incident','critical',0,'resolved',NULL,'Fixture incident: VPN drop','The VPN dropped every connection.',NULL,NULL,'2026-10-08 14:00:00','2026-10-08 15:20:00',NULL,1,'investigating','2026-10-08 14:02:00','2026-10-08 15:20:00',NULL,NULL,0);
INSERT INTO events VALUES(2,'incident','minor',0,'monitoring',NULL,'Fixture incident: mail delays','Mail is slow to arrive for some users.',NULL,NULL,'2026-10-20 08:30:00',NULL,NULL,2,'investigating','2026-10-20 08:32:00','2026-10-20 09:15:00',NULL,'2026-10-20 09:15:00',0);
INSERT INTO events VALUES(3,'maintenance',NULL,1,'scheduled',NULL,'Fixture maintenance: VPN upgrade','The VPN gateway gets its new firmware.','2027-03-01 22:00:00','2027-03-01 23:00:00',NULL,NULL,NULL,2,NULL,'2026-10-22 10:00:00','2026-10-22 10:00:00',NULL,NULL,0);
INSERT INTO events VALUES(4,'publication',NULL,0,NULL,'info','Fixture announcement: new intranet','The new intranet is open to everyone.',NULL,NULL,'2026-10-25 09:00:00',NULL,NULL,1,NULL,'2026-10-25 09:00:00','2026-10-25 09:00:00',NULL,NULL,0);
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
',1,'2026-10-08 15:20:00');
INSERT INTO event_updates VALUES(2,2,'<p>Investigating the mail queue.</p>
',2,'2026-10-20 08:40:00');
INSERT INTO event_updates VALUES(3,2,'<p>Fix deployed, watching.</p>
',2,'2026-10-20 09:15:00');
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
INSERT INTO event_templates VALUES(1,'Mail delays','Mail is slow to arrive for some users.','incident','minor',0,NULL,NULL,1,1,'2026-10-20 08:32:00','2026-10-05 10:00:00');
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
INSERT INTO events_fts_data VALUES(1,x'04101b');
INSERT INTO events_fts_data VALUES(10,x'000000000104040004010101020101030101040101');
INSERT INTO events_fts_data VALUES(137438953473,x'000000620b30636f6e6e656374696f6e0106010106010464726f70010205050370656401060101040105657665727901060101050107666978747572650102020108696e636964656e7401020301037468650106010102010376706e0108040101030411090a0c0c0d0a');
INSERT INTO events_fts_data VALUES(274877906945,x'0000007c07306172726976650206010106010664656c61797302020501076669787475726502020202026f7202060101070108696e636964656e74020203020173020601010301046d61696c0208040101020104736c6f77020601010402036f6d6502060101080102746f0206010105010575736572730206010109040d0b0c090d080c0b0a09');
INSERT INTO events_fts_data VALUES(412316860417,x'0000007a09306669726d7761726503060101080305787475726503020201076761746577617903060101040203657473030601010501036974730306010106010b6d61696e74656e616e636503020301036e6577030601010701037468650306010102010775706772616465030205010376706e030804010103040f0a0e0a0a100a0a0c');
INSERT INTO events_fts_data VALUES(549755813889,x'000000700d30616e6e6f756e63656d656e74040203010865766572796f6e6504060101080107666978747572650402020108696e7472616e6574040805010104020173040601010501036e657704080401010301046f70656e04060101060103746865040601010202016f040601010704110f0c10080b0b0a');
CREATE TABLE IF NOT EXISTS 'events_fts_idx'(segid, term, pgno, PRIMARY KEY(segid, term)) WITHOUT ROWID;
INSERT INTO events_fts_idx VALUES(1,x'',2);
INSERT INTO events_fts_idx VALUES(2,x'',2);
INSERT INTO events_fts_idx VALUES(3,x'',2);
INSERT INTO events_fts_idx VALUES(4,x'',2);
CREATE TABLE IF NOT EXISTS 'events_fts_docsize'(id INTEGER PRIMARY KEY, sz BLOB);
INSERT INTO events_fts_docsize VALUES(1,x'0405');
INSERT INTO events_fts_docsize VALUES(2,x'0408');
INSERT INTO events_fts_docsize VALUES(3,x'0407');
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
INSERT INTO dashboard_layouts VALUES(1,'recent_activity',30,1,'{"hide":["maintenance"]}','2026-10-06 09:00:00');
INSERT INTO dashboard_layouts VALUES(2,'scheduled_maintenances',40,1,'{"width":"narrow"}','2026-10-06 09:01:00');
INSERT INTO dashboard_layouts VALUES(3,'services',20,1,'{}','2026-10-01 08:00:00');
INSERT INTO dashboard_layouts VALUES(4,'status_banner',10,1,'{}','2026-10-01 08:00:00');
CREATE TABLE service_outages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    service_id INTEGER NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    CHECK (ended_at IS NULL OR ended_at >= started_at)
);
INSERT INTO service_outages VALUES(1,3,'2026-10-12 03:14:00','2026-10-12 03:41:00');
ANALYZE sqlite_schema;
INSERT INTO sqlite_stat1 VALUES('events_fts_config','events_fts_config','1 1');
INSERT INTO sqlite_stat1 VALUES('events_fts_data',NULL,'2');
INSERT INTO sqlite_stat1 VALUES('_sqlx_migrations','sqlite_autoindex__sqlx_migrations_1','9 1');
INSERT INTO sqlite_stat1 VALUES('dashboard_layouts','sqlite_autoindex_dashboard_layouts_1','4 1');
INSERT INTO sqlite_stat1 VALUES('users','idx_users_role_active','1 1 1');
INSERT INTO sqlite_stat1 VALUES('users','sqlite_autoindex_users_1','1 1');
INSERT INTO sqlite_stat1 VALUES('tower_sessions','idx_tower_sessions_expiry','3 1');
INSERT INTO sqlite_stat1 VALUES('tower_sessions','sqlite_autoindex_tower_sessions_1','3 1');
INSERT INTO sqlite_stat1 VALUES('settings','sqlite_autoindex_settings_1','3 1');
INSERT INTO sqlite_stat1 VALUES('service_outages','idx_service_outages_open','0 0');
INSERT INTO sqlite_stat1 VALUES('service_outages','idx_service_outages_service','1 1 1');
INSERT INTO sqlite_stat1 VALUES('event_templates','idx_event_templates_icon','1 1');
INSERT INTO sqlite_stat1 VALUES('event_templates','idx_event_templates_usage','1 1');
INSERT INTO sqlite_stat1 VALUES('event_templates','idx_event_templates_title','1 1');
INSERT INTO sqlite_stat1 VALUES('events_fts_idx','events_fts_idx','4 1 1');
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
INSERT INTO sqlite_sequence VALUES('service_outages',1);
INSERT INTO sqlite_sequence VALUES('event_templates',1);
INSERT INTO sqlite_sequence VALUES('events',4);
INSERT INTO sqlite_sequence VALUES('event_updates',3);
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
CREATE INDEX idx_service_outages_service ON service_outages(service_id, started_at);
CREATE UNIQUE INDEX idx_service_outages_open ON service_outages(service_id)
    WHERE ended_at IS NULL;
CREATE INDEX idx_tower_sessions_expiry ON tower_sessions (expiry_date);
PRAGMA writable_schema=OFF;
COMMIT;
