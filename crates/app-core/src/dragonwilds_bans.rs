use serde::{Deserialize, Serialize};

/// Persisted native bans, independent of the current online-player snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DragonwildsBanRoster {
    pub instance_id: String,
    pub observed_at_unix_ms: u64,
    pub entries: Vec<DragonwildsBanEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DragonwildsBanEntry {
    pub account_id: String,
    pub display_name: Option<String>,
}
