//! Game-owned Satisfactory HTTPS API state. These values are not INI settings.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SatisfactoryConnectionStatus {
    Stopped,
    Unclaimed,
    AuthorizationRequired,
    Ready,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SatisfactorySave {
    pub save_name: String,
    pub save_date_time: String,
    pub play_duration_seconds: i64,
    pub is_creative_mode_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SatisfactorySession {
    pub session_name: String,
    pub saves: Vec<SatisfactorySave>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SatisfactoryRuleOption {
    pub value: String,
    pub label_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SatisfactoryRuleDefinition {
    pub key: String,
    pub scope: String,
    pub kind: String,
    pub default_value: String,
    #[serde(default)]
    pub options: Vec<SatisfactoryRuleOption>,
    pub minimum: Option<i64>,
    pub maximum: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SatisfactoryWorldSnapshot {
    pub instance_id: String,
    pub connection_status: SatisfactoryConnectionStatus,
    pub revision: String,
    pub server_name: Option<String>,
    pub active_session_name: String,
    pub auto_load_session_name: String,
    pub is_game_running: bool,
    pub connected_players: u32,
    pub creative_mode_enabled: bool,
    pub advanced_game_settings: BTreeMap<String, String>,
    pub server_options: BTreeMap<String, String>,
    pub pending_server_options: BTreeMap<String, String>,
    pub sessions: Vec<SatisfactorySession>,
    pub rule_definitions: Vec<SatisfactoryRuleDefinition>,
    pub starting_locations: Vec<SatisfactoryRuleOption>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetupSatisfactoryServerInput {
    pub instance_id: String,
    pub server_name: String,
    /// Omitted to generate and store a strong password before claiming.
    pub admin_password: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizeSatisfactoryServerInput {
    pub instance_id: String,
    /// One-time authorization; not returned to the frontend or stored in settings.
    pub admin_password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriteSatisfactoryWorldRulesInput {
    pub instance_id: String,
    pub expected_revision: String,
    pub acknowledge_enable_advanced_settings: bool,
    pub advanced_game_settings: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateSatisfactoryWorldInput {
    pub instance_id: String,
    pub expected_revision: String,
    pub session_name: String,
    pub starting_location: String,
    pub skip_onboarding: bool,
    pub acknowledge_enable_advanced_settings: bool,
    pub game_mode_settings: BTreeMap<String, String>,
    pub advanced_game_settings: BTreeMap<String, String>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriteSatisfactoryRoomInput {
    pub instance_id: String,
    pub expected_revision: String,
    pub server_name: Option<String>,
    /// None leaves the current join password unchanged; Some("") removes it.
    pub client_password: Option<String>,
    pub auto_load_session_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoadSatisfactorySaveInput {
    pub instance_id: String,
    pub expected_revision: String,
    pub save_name: String,
}

/// Map changes are acknowledged by the API before the game finishes loading.
/// The caller refreshes state instead of resending a create/load request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SatisfactoryWorldOperationResult {
    pub instance_id: String,
    pub accepted: bool,
    pub session_name: String,
}
