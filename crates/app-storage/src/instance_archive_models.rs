use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceArchiveState {
    Archiving,
    Archived,
    Restoring,
    Restored,
    Purging,
    Purged,
    MissingMetadata,
    Unrecognized,
}

#[derive(Clone, Debug, Serialize)]
pub struct InstanceArchiveSummary {
    pub archive_id: String,
    pub instance_id: Option<String>,
    pub instance_name: Option<String>,
    pub module_id: Option<String>,
    pub deleted_at_unix_ms: Option<u64>,
    pub archived_instance_root: String,
    pub previous_instance_root: Option<String>,
    pub preserved_external_saves_path: Option<String>,
    pub state: InstanceArchiveState,
    pub can_restore: bool,
    pub can_purge: bool,
    pub issues: Vec<String>,
    pub program_storage: String,
    pub omitted_program_bytes: u64,
    pub omitted_program_files: usize,
    pub required_program_fingerprint: Option<String>,
    pub required_program_version: Option<String>,
    pub program_retention_reason: Option<String>,
    pub external_saves_backup_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PendingInstanceDeletion {
    pub operation_id: String,
    pub instance_id: String,
    pub instance_name: String,
    pub module_id: String,
    pub deleted_instance_root: String,
    pub started_at_unix_ms: Option<u64>,
    pub can_retry: bool,
    pub issues: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct InstanceArchiveList {
    pub archives: Vec<InstanceArchiveSummary>,
    pub pending_deletions: Vec<PendingInstanceDeletion>,
    pub issues: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct InstanceArchiveRestoreResult {
    pub archive_id: String,
    pub instance_id: String,
    pub instance_name: String,
    pub restored_instance_root: String,
    pub external_saves_backup_id: Option<String>,
    pub preserved_external_saves_path: Option<String>,
    pub external_saves_restore_required: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct InstanceArchivePurgeResult {
    pub archive_id: String,
    pub purged: bool,
}
