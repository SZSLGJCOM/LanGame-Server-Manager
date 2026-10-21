CREATE TABLE instance_maintenance_schedules (
    instance_id TEXT NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
    action TEXT NOT NULL CHECK (action IN ('backup', 'restart')),
    enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    time_local TEXT NOT NULL CHECK (length(time_local) = 5),
    revision INTEGER NOT NULL DEFAULT 1,
    last_local_date TEXT,
    run_token TEXT,
    owner_session TEXT,
    run_status TEXT CHECK (run_status IN ('running', 'succeeded', 'failed', 'skipped', 'interrupted')),
    started_at_unix_ms INTEGER,
    completed_at_unix_ms INTEGER,
    detail TEXT,
    PRIMARY KEY (instance_id, action)
);
PRAGMA user_version = 6;
