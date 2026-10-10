use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValheimWorldRuleSource {
    Saved,
    NewWorld,
    MissingMetadata,
}

/// Only world modifier keys are exposed; seeds and player history stay local.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValheimWorldRules {
    pub instance_id: String,
    pub world_name: String,
    pub source: ValheimWorldRuleSource,
    pub world_version: Option<i32>,
    pub saved_keys: Vec<String>,
}
