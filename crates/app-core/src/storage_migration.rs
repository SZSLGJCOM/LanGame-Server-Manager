use serde::{Deserialize, Serialize};

use crate::AppPathSettingsInput;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StorageMigrationPhase {
    Checking,
    Ready,
    Copying,
    Verifying,
    Applying,
    Cleaning,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StorageMigrationRoot {
    pub source: String,
    pub target: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StorageMigrationConflict {
    pub source: String,
    pub target: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StorageMigrationPreservedDestination {
    pub original_path: String,
    pub backup_path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StorageMigrationStatus {
    pub operation_id: String,
    pub phase: StorageMigrationPhase,
    pub input: AppPathSettingsInput,
    pub roots: Vec<StorageMigrationRoot>,
    pub total_files: u64,
    pub total_bytes: u64,
    pub completed_files: u64,
    pub completed_bytes: u64,
    pub reused_files: u64,
    pub conflicts: Vec<StorageMigrationConflict>,
    pub conflict_count: u64,
    pub can_cancel: bool,
    /// Storage writes remain blocked until the application completes recovery.
    #[serde(default)]
    pub recovery_required: bool,
    pub error: Option<String>,
    pub cleanup_warnings: Vec<String>,
    #[serde(default)]
    pub preserved_destinations: Vec<StorageMigrationPreservedDestination>,
}

impl StorageMigrationStatus {
    pub fn is_active(&self) -> bool {
        matches!(
            self.phase,
            StorageMigrationPhase::Checking
                | StorageMigrationPhase::Copying
                | StorageMigrationPhase::Verifying
                | StorageMigrationPhase::Applying
                | StorageMigrationPhase::Cleaning
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{StorageMigrationPreservedDestination, StorageMigrationStatus};

    #[test]
    fn migration_status_defaults_missing_backups_and_round_trips_retained_targets()
    -> Result<(), serde_json::Error> {
        let mut status: StorageMigrationStatus = serde_json::from_value(serde_json::json!({
            "operation_id": "migration-one", "phase": "completed",
            "input": { "games_root": "D:/games", "servers_root": "D:/instances",
                "archives_root": "D:/instances/.trash", "steamcmd_root": "D:/steamcmd" },
            "roots": [], "total_files": 0, "total_bytes": 0,
            "completed_files": 0, "completed_bytes": 0, "reused_files": 0,
            "conflicts": [], "conflict_count": 0, "can_cancel": false,
            "error": null, "cleanup_warnings": []
        }))?;
        assert!(status.preserved_destinations.is_empty());
        assert!(!status.recovery_required);
        status.recovery_required = true;
        status
            .preserved_destinations
            .push(StorageMigrationPreservedDestination {
                original_path: "D:/instances".to_owned(),
                backup_path: "D:/instances.previous-contents".to_owned(),
            });
        let serialized = serde_json::to_value(&status)?;
        assert_eq!(serialized["recovery_required"], true);
        assert!(
            serde_json::from_value::<StorageMigrationStatus>(serialized.clone())?.recovery_required
        );
        assert_eq!(
            serialized["preserved_destinations"][0],
            serde_json::json!({ "original_path": "D:/instances", "backup_path": "D:/instances.previous-contents" })
        );
        Ok(())
    }
}
