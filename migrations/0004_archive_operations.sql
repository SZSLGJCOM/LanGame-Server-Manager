ALTER TABLE instance_archives ADD COLUMN purpose TEXT NOT NULL DEFAULT 'archive'
    CHECK (purpose IN ('archive', 'delete'));
ALTER TABLE instance_archives ADD COLUMN instances_root_identity_json TEXT;
ALTER TABLE instance_archives ADD COLUMN archive_parent_identity_json TEXT;
ALTER TABLE instance_archives ADD COLUMN restore_staging_identity_json TEXT;
CREATE UNIQUE INDEX idx_instance_deletion_pending
    ON instance_archives(instance_id)
    WHERE purpose = 'delete' AND state NOT IN ('restored', 'purged');
PRAGMA user_version = 4;
