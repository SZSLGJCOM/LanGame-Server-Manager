CREATE TABLE program_removals (
    operation_id TEXT PRIMARY KEY,
    module_id TEXT NOT NULL,
    install_id INTEGER NOT NULL,
    source_root TEXT NOT NULL,
    phase TEXT NOT NULL CHECK (phase IN ('prepared', 'committed')),
    journal_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Recovery intent must survive removal or replacement of an installation row.
CREATE UNIQUE INDEX idx_program_removals_install_id ON program_removals(install_id);
PRAGMA user_version = 5;
