use serde::{Deserialize, Serialize};

use crate::RuntimePlayerIdentityKind;

/// Read-only projection of bans owned and persisted by the native game server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeBanRoster {
    pub instance_id: String,
    pub module_id: String,
    pub observed_at_unix_ms: u64,
    pub entries: Vec<NativeBanEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeBanEntry {
    pub account_id: String,
    pub display_name: Option<String>,
    pub identity_kind: RuntimePlayerIdentityKind,
}
