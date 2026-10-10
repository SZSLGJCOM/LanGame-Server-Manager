use serde::{Deserialize, Serialize};

/// A native descriptive slot, grouped from ordinary `.savegame` filenames.
/// Timestamps describe the filenames; they do not certify a successful load.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AstroneerSaveEntry {
    pub descriptive_name: String,
    pub latest_saved_at: String,
    pub versions: u32,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AstroneerSaveCatalog {
    pub instance_id: String,
    pub configured_name: String,
    pub entries: Vec<AstroneerSaveEntry>,
}
