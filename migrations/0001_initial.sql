PRAGMA foreign_keys = ON;

CREATE TABLE app_settings (
    key TEXT PRIMARY KEY,
    value_json TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE modules (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    description TEXT,
    steam_app_id INTEGER,
    manifest_toml TEXT,
    schema_json TEXT,
    supported_platforms TEXT NOT NULL DEFAULT 'windows',
    root_path TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE game_installs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    module_id TEXT NOT NULL,
    install_root TEXT NOT NULL,
    install_state TEXT NOT NULL DEFAULT 'not_installed',
    current_version TEXT,
    last_verified_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (module_id) REFERENCES modules(id) ON DELETE CASCADE
);

CREATE TABLE instances (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    module_id TEXT NOT NULL,
    bind_ip TEXT NOT NULL DEFAULT '0.0.0.0',
    install_id INTEGER,
    status TEXT NOT NULL DEFAULT 'stopped',
    data_path TEXT NOT NULL,
    config_path TEXT NOT NULL,
    logs_path TEXT NOT NULL,
    saves_path TEXT NOT NULL,
    env_json TEXT NOT NULL DEFAULT '{}',
    args_json TEXT NOT NULL DEFAULT '[]',
    autostart INTEGER NOT NULL DEFAULT 0,
    crash_restart_limit INTEGER NOT NULL DEFAULT 5,
    auto_backup_on_stop INTEGER NOT NULL DEFAULT 0,
    backup_retention_count INTEGER NOT NULL DEFAULT 10,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (module_id) REFERENCES modules(id) ON DELETE CASCADE,
    FOREIGN KEY (install_id) REFERENCES game_installs(id) ON DELETE SET NULL
);

CREATE TABLE instance_ports (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    instance_id TEXT NOT NULL,
    name TEXT,
    port INTEGER NOT NULL,
    protocol TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (instance_id) REFERENCES instances(id) ON DELETE CASCADE,
    UNIQUE (port, protocol)
);

CREATE TABLE instance_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    instance_id TEXT NOT NULL,
    pid INTEGER,
    status TEXT NOT NULL,
    started_at TEXT,
    stopped_at TEXT,
    exit_code INTEGER,
    crash_flag INTEGER NOT NULL DEFAULT 0,
    log_path TEXT,
    session_id TEXT,
    process_key TEXT,
    display_name TEXT,
    is_primary INTEGER NOT NULL DEFAULT 1,
    process_creation_time TEXT,
    process_image_path TEXT,
    FOREIGN KEY (instance_id) REFERENCES instances(id) ON DELETE CASCADE
);

CREATE TABLE instance_broadcast_policies (
    instance_id TEXT PRIMARY KEY,
    enabled INTEGER NOT NULL DEFAULT 0,
    rules_json TEXT NOT NULL,
    updated_at_unix_ms INTEGER NOT NULL,
    FOREIGN KEY(instance_id) REFERENCES instances(id) ON DELETE CASCADE
);

CREATE TABLE instance_broadcast_events (
    event_id TEXT PRIMARY KEY,
    instance_id TEXT NOT NULL,
    module_id TEXT NOT NULL,
    source TEXT NOT NULL,
    rule_id TEXT,
    message TEXT NOT NULL,
    ai_provider TEXT,
    ai_model TEXT,
    action_id TEXT,
    transport TEXT,
    command_preview TEXT,
    status TEXT NOT NULL,
    response_text TEXT,
    error_message TEXT,
    created_at_unix_ms INTEGER NOT NULL,
    initiator TEXT,
    policy_snapshot_json TEXT,
    FOREIGN KEY(instance_id) REFERENCES instances(id) ON DELETE CASCADE
);

CREATE INDEX idx_game_installs_module_id ON game_installs(module_id);

CREATE INDEX idx_instance_broadcast_events_instance_created
ON instance_broadcast_events(instance_id, created_at_unix_ms DESC);

CREATE INDEX idx_instance_ports_instance_id ON instance_ports(instance_id);

CREATE INDEX idx_instance_runs_instance_id ON instance_runs(instance_id);

CREATE INDEX idx_instance_runs_instance_session ON instance_runs(instance_id, session_id, id);

CREATE INDEX idx_instance_runs_status ON instance_runs(status, instance_id);

CREATE INDEX idx_instances_module_id ON instances(module_id);

CREATE INDEX idx_instances_status ON instances(status);

PRAGMA user_version = 1;
