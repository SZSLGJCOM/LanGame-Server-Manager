CREATE TABLE player_history_cursors (
    run_id INTEGER PRIMARY KEY REFERENCES instance_runs(id) ON DELETE CASCADE,
    byte_offset INTEGER NOT NULL DEFAULT 0,
    generation INTEGER NOT NULL DEFAULT 0,
    last_elapsed_ms INTEGER,
    source_identity TEXT,
    completed INTEGER NOT NULL DEFAULT 0,
    polled_at INTEGER NOT NULL DEFAULT 0,
    issue TEXT
);

CREATE TABLE player_history_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    instance_id TEXT NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
    run_id INTEGER NOT NULL REFERENCES instance_runs(id) ON DELETE CASCADE,
    process_key TEXT NOT NULL,
    generation INTEGER NOT NULL,
    byte_offset INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('join', 'leave', 'chat')),
    player_name TEXT NOT NULL,
    account_id TEXT,
    message TEXT,
    log_time TEXT NOT NULL,
    elapsed_ms INTEGER NOT NULL,
    estimated_at_unix_ms INTEGER,
    duration_ms INTEGER,
    paired_event_id INTEGER REFERENCES player_history_events(id) ON DELETE SET NULL,
    session_ambiguous INTEGER NOT NULL DEFAULT 0,
    UNIQUE(run_id, generation, byte_offset)
);
CREATE INDEX player_history_instance_page ON player_history_events(instance_id, id DESC);
CREATE INDEX player_history_session ON player_history_events(run_id, generation, player_name, kind, paired_event_id);
PRAGMA user_version = 7;
