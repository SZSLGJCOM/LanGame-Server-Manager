CREATE TABLE instance_archives (
    archive_id TEXT PRIMARY KEY,
    instance_id TEXT,
    instance_name TEXT,
    module_id TEXT,
    deleted_at_unix_ms INTEGER,
    original_root TEXT,
    archive_leaf TEXT NOT NULL,
    directory_identity_json TEXT,
    snapshot_json TEXT,
    snapshot_sha256 TEXT,
    preserved_external_saves_path TEXT,
    state TEXT NOT NULL CHECK (state IN (
        'archiving', 'archived', 'restoring', 'restored', 'purging', 'purged',
        'missing_metadata', 'unrecognized'
    )),
    problem TEXT,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_instance_archives_state ON instance_archives(state);
CREATE UNIQUE INDEX idx_instance_archives_active_leaf
    ON instance_archives(archive_leaf) WHERE state NOT IN ('restored', 'purged');
CREATE UNIQUE INDEX idx_instance_archives_pending_instance
    ON instance_archives(instance_id) WHERE state IN ('archiving', 'restoring');
PRAGMA user_version = 3;
