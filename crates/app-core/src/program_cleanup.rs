use serde::{Deserialize, Serialize};

use crate::InstallState;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProgramCleanupResult {
    pub removed_install_roots: Vec<String>,
    pub preserved_data_paths: Vec<String>,
    pub retained_installs: Vec<ProgramCleanupRetention>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramCleanupRetention {
    pub install_root: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleUninstallResult {
    pub module_id: String,
    pub install_state: InstallState,
    pub executable_exists: bool,
    pub cleanup: ProgramCleanupResult,
}
