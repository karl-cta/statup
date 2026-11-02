-- Statup 0.5.0 database, made by the published image of 0.5.0 and a fixed data set, to test upgrades.
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
INSERT INTO _sqlx_migrations VALUES(20260917000001,'initial schema','2026-11-02 13:11:46',1,x'3a2a607b68a7ef7ef1cdff97ed9ea094a748244b88a42c58d0cdfb6886f832882ff69c29d8d672b16b2d514350f3f6fc',6222583);
INSERT INTO _sqlx_migrations VALUES(20260917000002,'announcement follows','2026-11-02 13:11:46',1,x'b506725f1f4bc84dcc2940b9a8e3824aae81f33f44673f2a9495b2d96230c8c47adb9f73e080f70a80c240704e56570a',2676292);
INSERT INTO _sqlx_migrations VALUES(20260923000001,'service base status','2026-11-02 13:11:46',1,x'28b8b6108bb27aba122e55241831e124123e1cdc5157c153d967747c29e93591cd411c3081e581699c0fa121d05f9080',1559000);
INSERT INTO _sqlx_migrations VALUES(20260923000002,'single degraded level','2026-11-02 13:11:46',1,x'02c2cf1e7b743d59d99032fb1d183a275ab31f90e92be509ed7a4304287c5aa972dd045a9e2a9150762301157b74a53b',474375);
INSERT INTO _sqlx_migrations VALUES(20260924000001,'maintenance without downtime','2026-11-02 13:11:46',1,x'2fb82608b90b7b2659d0e40ede2883b8480f283058f5218db82c70e0db05a59b26d3bd786ccf632c475e86cd13fb47aa',1376208);
INSERT INTO _sqlx_migrations VALUES(20260924000002,'single dashboard layout','2026-11-02 13:11:46',1,x'891f9bfed12b14013a088711b3453849ac67a2fc00f44899c3c0ded61a9d753006e19d267ae20d094a49c6b2b3e9e695',2501125);
INSERT INTO _sqlx_migrations VALUES(20260925000001,'temporary password expiry','2026-11-02 13:11:46',1,x'c84e68da87284a4a1a2194e78ac64924d2c4afa102b837343752911486230e5347341f93825d62e015d1f2c73df117a8',850208);
INSERT INTO _sqlx_migrations VALUES(20260925000002,'dashboard layout without user','2026-11-02 13:11:46',1,x'290c37e099559beeff960aa967f29c63a6284a06d241060ab4222449224eaae1422ec79717faee9179f0f981b8def185',2257917);
INSERT INTO _sqlx_migrations VALUES(20261004000001,'service monitoring','2026-11-02 13:11:46',1,x'b06a50bef906de5c1d160e0af74387f003a2c108c1685b5e59075e56252b2ad4d11e974191794ad7f01538dd2f29cb5d',1916417);
INSERT INTO _sqlx_migrations VALUES(20261006000001,'any supported locale','2026-11-02 13:11:46',1,x'5c829db39f507a37a8dc4f86e87f64283179d4e2983893d4e07236d5eb16a5ad94d6366305bb7be175958a15182d0456',2667458);
INSERT INTO _sqlx_migrations VALUES(20261007000001,'notifications','2026-11-02 13:11:46',1,x'6cd7afac804c148068190aceaabfa1ee5165344a710ed767b1a0ed08ac9128a1c4fa2463948e4232c630fe7b3084068a',2432709);
INSERT INTO _sqlx_migrations VALUES(20261008000001,'service position','2026-11-02 13:11:46',1,x'83dcfd7a5d0fccb74cd8b4896916428a4210283a2cd31a101ab24c010b46bdfcff8615448ba05f8119cfb479e87a3230',930791);
INSERT INTO _sqlx_migrations VALUES(20261008000002,'password resets','2026-11-02 13:11:46',1,x'2ffb03b78a46a460d534053e3ed035e6caba0a11ffb7d767cf814ff5093c29d6e630e9c183e446d84eca635dc05cc46a',693041);
INSERT INTO _sqlx_migrations VALUES(20261013000001,'next update','2026-11-02 13:11:46',1,x'8dbc0c2cec214dbb1fe9909b713a341b7b83984296e76f531e35e0525135f4cda2a3406928f9e0420702434000f5db85',853959);
INSERT INTO _sqlx_migrations VALUES(20261013000002,'member notices','2026-11-02 13:11:46',1,x'5d5524cb9abe57d421df3380904306814afff2793d5b18ed2e40e51d8b0649e354c291ff2cf79ae67de745468c98f843',1776458);
INSERT INTO _sqlx_migrations VALUES(20261013000003,'notice others','2026-11-02 13:11:46',1,x'92e28099c361043af7ac69cb8f9fa9e859660b55bada0dd709c735ba0c94a1358d0d162933ca1ce7da4aebe84fcb991b',2623833);
INSERT INTO _sqlx_migrations VALUES(20261014000001,'outage delay','2026-11-02 13:11:46',1,x'b2935542b5a6502e12caec1ea5309448fddb234745145c7d4c12023f64ceb6034405e5141eb6561cafcd902136fafe8a',905125);
INSERT INTO _sqlx_migrations VALUES(20261014000002,'event reviews','2026-11-02 13:11:46',1,x'e1385294c6f439b8b89302b60d560db9ca96fcd72fb8ec138f187f32db616044d9e66c3cbecc1ab076f1010f5fd33a8b',370292);
INSERT INTO _sqlx_migrations VALUES(20261014000003,'reviewed notifications','2026-11-02 13:11:46',1,x'37593174f514ee14c026a4c9f5bdb1816fad9f5b98258b896d812d885c88587a1fc185bca6ca6523d87b0b60a90b100c',1647958);
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
INSERT INTO users VALUES(1,'admin@upgrade.test','$argon2id$v=19$m=19456,t=2,p=1$zQ6u991JaN9c7mg4SxO7Aw$tkmjugOP8Z5razfK02LCXztKMNF7QZWJmKr1d3QOiwY','admin','admin',1,'2026-10-30 09:00:00','2026-10-01 08:00:00','2026-10-30 09:00:00',0,NULL,NULL);
INSERT INTO users VALUES(2,'editor@upgrade.test','$argon2id$v=19$m=19456,t=2,p=1$zQ6u991JaN9c7mg4SxO7Aw$tkmjugOP8Z5razfK02LCXztKMNF7QZWJmKr1d3QOiwY','Fixture Editor','publisher',1,'2026-10-29 17:45:00','2026-10-01 08:10:00','2026-10-29 17:45:00',0,NULL,'en');
INSERT INTO users VALUES(3,'reader@upgrade.test','$argon2id$v=19$m=19456,t=2,p=1$zQ6u991JaN9c7mg4SxO7Aw$tkmjugOP8Z5razfK02LCXztKMNF7QZWJmKr1d3QOiwY','Fixture Reader','reader',1,'2026-10-28 12:00:00','2026-10-01 08:12:00','2026-10-28 12:00:00',0,NULL,NULL);
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
INSERT INTO icons VALUES(1,'f33c5c33-3773-41d0-9977-85de703337a2.svg','intranet.svg','image/svg+xml',413,2,'2026-10-01 08:21:30');
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
    CHECK (detected_status IN ('major_outage')), position INTEGER NOT NULL DEFAULT 0, outage_after_minutes INTEGER NOT NULL DEFAULT 3
    CHECK (outage_after_minutes BETWEEN 1 AND 60));
INSERT INTO services VALUES(1,'Fixture Mail','fixture-mail','Company mail and calendars.','operational','2026-10-01 08:20:00','2026-10-28 10:30:00',NULL,'envelope','operational',NULL,NULL,0,NULL,1,3);
INSERT INTO services VALUES(2,'Fixture VPN','fixture-vpn','Remote access to the office network.','degraded','2026-10-01 08:21:00','2026-10-27 16:00:00',NULL,'lock-closed','degraded','http','https://127.0.0.1:9/',1,NULL,2,5);
INSERT INTO services VALUES(3,'Fixture Intranet','fixture-intranet','Internal pages and documents.','operational','2026-10-01 08:22:00','2026-10-20 03:25:00',1,NULL,'operational','tcp','127.0.0.1:9',0,NULL,3,3);
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
    CHECK (keeps_services_up IN (0, 1)), next_update_at TEXT, update_every_minutes INTEGER, next_update_by INTEGER REFERENCES users(id) ON DELETE SET NULL, next_update_reminded INTEGER NOT NULL DEFAULT 0,

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
INSERT INTO events VALUES(1,'incident','critical',0,'resolved',NULL,'Fixture incident: VPN drop','Remote access dropped for every user.',NULL,NULL,'2026-10-15 08:00:00','2026-10-15 09:15:00',NULL,2,'investigating','2026-10-15 08:00:00','2026-10-16 10:00:00',NULL,NULL,0,NULL,NULL,NULL,0);
INSERT INTO events VALUES(2,'incident','minor',0,'monitoring',NULL,'Fixture incident: mail delays','Messages take longer than usual to arrive.',NULL,NULL,'2026-10-28 09:00:00',NULL,NULL,2,'investigating','2026-10-28 09:00:00','2026-10-28 11:05:00',NULL,'2026-10-28 10:30:00',0,'2026-10-28 11:00:00',30,2,1);
INSERT INTO events VALUES(3,'maintenance',NULL,1,'scheduled',NULL,'Fixture maintenance: VPN upgrade','The VPN gateway moves to its new version. Remote access stops for an hour.','2027-03-01 22:00:00','2027-03-01 23:00:00',NULL,NULL,NULL,2,NULL,'2026-10-29 14:00:00','2026-10-29 14:00:00',NULL,NULL,0,NULL,NULL,NULL,0);
INSERT INTO events VALUES(4,'publication',NULL,0,NULL,'changelog','Fixture announcement: new intranet','The new intranet is open to everyone from today.',NULL,NULL,'2026-10-30 10:00:00',NULL,NULL,1,NULL,'2026-10-30 10:00:00','2026-10-30 10:00:00',NULL,NULL,0,NULL,NULL,NULL,0);
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
INSERT INTO event_updates VALUES(1,1,'<p>Resolved.</p>'||char(10),2,'2026-10-15 09:15:00');
INSERT INTO event_updates VALUES(2,2,'<p>Investigating the mail queue.</p>'||char(10),2,'2026-10-28 09:05:00');
INSERT INTO event_updates VALUES(3,2,'<p>Fix deployed, watching.</p>'||char(10),2,'2026-10-28 10:30:00');
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
INSERT INTO event_templates VALUES(1,'Fixture template: mail delays','Messages take longer than usual to arrive.','incident','minor',0,NULL,NULL,2,1,'2026-10-28 09:00:00','2026-10-10 11:00:00');
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
INSERT INTO events_fts_data VALUES(1,x'041024');
INSERT INTO events_fts_data VALUES(10,x'000000000101010001010101');
INSERT INTO events_fts_data VALUES(137438953473,x'000001cf07306163636573730106010103020601010b02016e030601010e030a6e6f756e63656d656e74040203020572726976650206010108010664656c6179730202050203726f700102050503706564010601010401056576657279010601010606036f6e65040601010801076669787475726501020201020201020201020202026f720106010105020601010d0203726f6d040601010901076761746577617903060101040104686f7572030601010f0108696e636964656e7401020301020303067472616e6574040805010104020173040601010502027473030601010701066c6f6e676572020601010401046d61696c02020404086e74656e616e6365030203020765737361676573020601010202046f766573030601010501036e6577030601010801080401010301046f70656e0406010106010672656d6f74650106010102020601010a010573746f7073030601010c010474616b650206010103020368616e02060101050301650306010102010601010202016f0206010107010601010601060101070303646179040601010a01077570677261646503020502037365720106010107030375616c0206010106010776657273696f6e03060101090202706e0102040208040101030412080f0c0b080a0c0a150e0a0e0b100e08090d090d0e0b100b120c0b0a0d120a0c0a0a0e');
CREATE TABLE IF NOT EXISTS 'events_fts_idx'(segid, term, pgno, PRIMARY KEY(segid, term)) WITHOUT ROWID;
INSERT INTO events_fts_idx VALUES(1,x'',2);
CREATE TABLE IF NOT EXISTS 'events_fts_docsize'(id INTEGER PRIMARY KEY, sz BLOB);
INSERT INTO events_fts_docsize VALUES(1,x'0406');
INSERT INTO events_fts_docsize VALUES(2,x'0407');
INSERT INTO events_fts_docsize VALUES(3,x'040e');
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
INSERT INTO dashboard_layouts VALUES(1,'recent_activity',30,1,'{"hide":["maintenance"]}','2026-10-12 09:01:00');
INSERT INTO dashboard_layouts VALUES(2,'scheduled_maintenances',40,1,'{}','2026-10-01 08:00:00');
INSERT INTO dashboard_layouts VALUES(3,'services',20,1,'{"width":"wide"}','2026-10-12 09:00:00');
INSERT INTO dashboard_layouts VALUES(4,'status_banner',10,1,'{}','2026-10-01 08:00:00');
CREATE TABLE service_outages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    service_id INTEGER NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    CHECK (ended_at IS NULL OR ended_at >= started_at)
);
INSERT INTO service_outages VALUES(1,3,'2026-10-20 03:04:00','2026-10-20 03:25:00');
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
INSERT INTO notification_channels VALUES(1,'Fixture webhook','webhook','http://127.0.0.1:9/hook','en',1,1,1,1,'2026-10-05 10:00:00','2026-10-05 10:00:00');
INSERT INTO notification_channels VALUES(2,'Fixture email','email','ops@upgrade.test','fr',1,1,0,0,'2026-10-06 09:00:00','2026-10-06 09:00:00');
CREATE TABLE password_resets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    secret_hash TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
INSERT INTO password_resets VALUES(1,3,'$argon2id$v=19$m=19456,t=2,p=1$zQ6u991JaN9c7mg4SxO7Aw$tkmjugOP8Z5razfK02LCXztKMNF7QZWJmKr1d3QOiwY','2026-10-22 13:00:00','2026-10-22 12:00:00');
CREATE TABLE member_notices (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind TEXT NOT NULL
        CHECK (kind IN ('update_overdue', 'outage_detected', 'delivery_failed')),
    event_id INTEGER REFERENCES events(id) ON DELETE CASCADE,
    service_id INTEGER REFERENCES services(id) ON DELETE CASCADE,
    channel_id INTEGER REFERENCES notification_channels(id) ON DELETE CASCADE,
    -- When the overdue update was announced for.
    due_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    read_at TEXT
, others INTEGER NOT NULL DEFAULT 0);
INSERT INTO member_notices VALUES(1,1,'outage_detected',NULL,3,NULL,NULL,'2026-10-20 03:07:00','2026-10-20 08:30:00',0);
INSERT INTO member_notices VALUES(2,2,'outage_detected',NULL,3,NULL,NULL,'2026-10-20 03:07:00',NULL,0);
INSERT INTO member_notices VALUES(3,2,'update_overdue',2,NULL,NULL,'2026-10-28 11:00:00','2026-10-28 11:05:00',NULL,0);
INSERT INTO member_notices VALUES(4,1,'delivery_failed',NULL,NULL,1,NULL,'2026-10-28 11:12:30',NULL,0);
CREATE TABLE event_reviews (
    event_id INTEGER PRIMARY KEY REFERENCES events(id) ON DELETE CASCADE,
    body TEXT NOT NULL,
    author_id INTEGER NOT NULL REFERENCES users(id),
    published_at TEXT NOT NULL DEFAULT (datetime('now'))
);
INSERT INTO event_reviews VALUES(1,'Fixture review: the VPN gateway rebooted.',2,'2026-10-16 10:00:00');
CREATE TABLE IF NOT EXISTS "notification_deliveries" (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    channel_id INTEGER NOT NULL REFERENCES notification_channels(id) ON DELETE CASCADE,
    happening TEXT NOT NULL
        CHECK (happening IN ('opened', 'updated', 'rescheduled', 'started', 'closed',
                             'published', 'reviewed', 'service_down', 'service_up', 'test')),
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
INSERT INTO notification_deliveries VALUES(1,1,'test',NULL,NULL,NULL,NULL,'sent',1,'2026-10-05 10:01:00',NULL,'2026-10-05 10:01:00','2026-10-05 10:01:01');
INSERT INTO notification_deliveries VALUES(2,2,'test',NULL,NULL,NULL,NULL,'sent',1,'2026-10-06 09:01:00',NULL,'2026-10-06 09:01:00','2026-10-06 09:01:02');
INSERT INTO notification_deliveries VALUES(3,1,'opened',1,NULL,'investigating',NULL,'sent',1,'2026-10-15 08:00:00',NULL,'2026-10-15 08:00:00','2026-10-15 08:00:02');
INSERT INTO notification_deliveries VALUES(4,2,'opened',1,NULL,'investigating',NULL,'sent',1,'2026-10-15 08:00:00',NULL,'2026-10-15 08:00:00','2026-10-15 08:00:03');
INSERT INTO notification_deliveries VALUES(5,1,'closed',1,1,'resolved',NULL,'sent',1,'2026-10-15 09:15:00',NULL,'2026-10-15 09:15:00','2026-10-15 09:15:01');
INSERT INTO notification_deliveries VALUES(6,2,'closed',1,1,'resolved',NULL,'sent',1,'2026-10-15 09:15:00',NULL,'2026-10-15 09:15:00','2026-10-15 09:15:02');
INSERT INTO notification_deliveries VALUES(7,1,'reviewed',1,NULL,'resolved',NULL,'sent',1,'2026-10-16 10:00:00',NULL,'2026-10-16 10:00:00','2026-10-16 10:00:01');
INSERT INTO notification_deliveries VALUES(8,2,'reviewed',1,NULL,'resolved',NULL,'sent',1,'2026-10-16 10:00:00',NULL,'2026-10-16 10:00:00','2026-10-16 10:00:02');
INSERT INTO notification_deliveries VALUES(9,1,'service_down',NULL,NULL,NULL,3,'sent',1,'2026-10-20 03:07:00',NULL,'2026-10-20 03:07:00','2026-10-20 03:07:01');
INSERT INTO notification_deliveries VALUES(10,1,'service_up',NULL,NULL,NULL,3,'sent',1,'2026-10-20 03:25:00',NULL,'2026-10-20 03:25:00','2026-10-20 03:25:01');
INSERT INTO notification_deliveries VALUES(11,1,'opened',2,NULL,'investigating',NULL,'sent',1,'2026-10-28 09:00:00',NULL,'2026-10-28 09:00:00','2026-10-28 09:00:01');
INSERT INTO notification_deliveries VALUES(12,2,'opened',2,NULL,'investigating',NULL,'sent',1,'2026-10-28 09:00:00',NULL,'2026-10-28 09:00:00','2026-10-28 09:00:02');
INSERT INTO notification_deliveries VALUES(13,1,'updated',2,2,'investigating',NULL,'sent',1,'2026-10-28 09:05:00',NULL,'2026-10-28 09:05:00','2026-10-28 09:05:01');
INSERT INTO notification_deliveries VALUES(14,2,'updated',2,2,'investigating',NULL,'sent',1,'2026-10-28 09:05:00',NULL,'2026-10-28 09:05:00','2026-10-28 09:05:02');
INSERT INTO notification_deliveries VALUES(15,1,'updated',2,3,'monitoring',NULL,'failed',4,'2026-10-28 11:12:30','refused','2026-10-28 10:30:00',NULL);
INSERT INTO notification_deliveries VALUES(16,2,'updated',2,3,'monitoring',NULL,'sent',1,'2026-10-28 10:30:00',NULL,'2026-10-28 10:30:00','2026-10-28 10:30:02');
INSERT INTO notification_deliveries VALUES(17,1,'opened',3,NULL,'scheduled',NULL,'sent',1,'2026-10-29 14:00:00',NULL,'2026-10-29 14:00:00','2026-10-29 14:00:01');
INSERT INTO notification_deliveries VALUES(18,2,'opened',3,NULL,'scheduled',NULL,'sent',1,'2026-10-29 14:00:00',NULL,'2026-10-29 14:00:00','2026-10-29 14:00:02');
INSERT INTO notification_deliveries VALUES(19,1,'published',4,NULL,NULL,NULL,'sent',1,'2026-10-30 10:00:00',NULL,'2026-10-30 10:00:00','2026-10-30 10:00:01');
ANALYZE sqlite_schema;
INSERT INTO sqlite_stat1 VALUES('events_fts_config','events_fts_config','1 1');
INSERT INTO sqlite_stat1 VALUES('events_fts_data',NULL,'2');
INSERT INTO sqlite_stat1 VALUES('_sqlx_migrations','sqlite_autoindex__sqlx_migrations_1','19 1');
INSERT INTO sqlite_stat1 VALUES('dashboard_layouts','sqlite_autoindex_dashboard_layouts_1','4 1');
INSERT INTO sqlite_stat1 VALUES('users','idx_users_role_active','1 1 1');
INSERT INTO sqlite_stat1 VALUES('users','sqlite_autoindex_users_1','1 1');
INSERT INTO sqlite_stat1 VALUES('member_notices','idx_member_notices_unread','3 2');
INSERT INTO sqlite_stat1 VALUES('member_notices','idx_member_notices_user','4 2 1');
INSERT INTO sqlite_stat1 VALUES('notification_deliveries','idx_notification_deliveries_created','19 2');
INSERT INTO sqlite_stat1 VALUES('notification_deliveries','idx_notification_deliveries_queue','19 10 7 1');
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
INSERT INTO sqlite_stat1 VALUES('services','idx_services_icon','3 2');
INSERT INTO sqlite_stat1 VALUES('services','idx_services_status','3 2');
INSERT INTO sqlite_stat1 VALUES('services','sqlite_autoindex_services_1','3 1');
INSERT INTO sqlite_stat1 VALUES('events','idx_events_update_reminders','0 0');
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
INSERT INTO sqlite_sequence VALUES('notification_deliveries',19);
INSERT INTO sqlite_sequence VALUES('users',3);
INSERT INTO sqlite_sequence VALUES('services',3);
INSERT INTO sqlite_sequence VALUES('service_outages',1);
INSERT INTO sqlite_sequence VALUES('events',4);
INSERT INTO sqlite_sequence VALUES('event_updates',3);
INSERT INTO sqlite_sequence VALUES('event_templates',1);
INSERT INTO sqlite_sequence VALUES('notification_channels',2);
INSERT INTO sqlite_sequence VALUES('member_notices',4);
INSERT INTO sqlite_sequence VALUES('password_resets',1);
INSERT INTO sqlite_sequence VALUES('icons',1);
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
CREATE INDEX idx_password_resets_user ON password_resets(user_id);
CREATE INDEX idx_member_notices_user ON member_notices(user_id, created_at);
CREATE INDEX idx_member_notices_unread ON member_notices(user_id) WHERE read_at IS NULL;
CREATE INDEX idx_events_update_reminders ON events(next_update_at)
    WHERE next_update_at IS NOT NULL AND next_update_reminded = 0;
CREATE INDEX idx_notification_deliveries_queue
    ON notification_deliveries(channel_id, status, id);
CREATE INDEX idx_notification_deliveries_created ON notification_deliveries(created_at);
CREATE INDEX idx_tower_sessions_expiry ON tower_sessions (expiry_date);
PRAGMA writable_schema=OFF;
COMMIT;
