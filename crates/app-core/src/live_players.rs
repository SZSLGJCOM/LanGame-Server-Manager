use serde::{Deserialize, Serialize};

/// The native EOS NetID is an optional Epic account ID plus a required product ID.
pub fn is_humanitz_net_id(value: &str) -> bool {
    let Some((epic, product)) = value.split_once('|') else {
        return false;
    };
    let hex_id = |part: &str| part.len() == 32 && part.bytes().all(|byte| byte.is_ascii_hexdigit());
    (epic.is_empty() || hex_id(epic)) && hex_id(product)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModulePlayerCountSource {
    #[default]
    PlayerQuery,
    PlayerList,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModulePlayerListScope {
    Online,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModulePlayerListSource {
    RuntimeAction,
    StructuredLog,
    HttpApi,
    ServerQuery,
    ConsoleLog,
    TcpConsole,
    NativeConsole,
    FileIpc,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModulePlayerListCodec {
    DstClientTableV1,
    RustPlayerList,
    ArkListPlayers,
    ConanListPlayers,
    HumanitzPlayers,
    ZomboidPlayers,
    SevenDaysPlayers,
    SquadListPlayers,
    PalworldPlayers,
    NightingalePlayers,
    MinecraftPlayers,
    A2sPlayers,
    NecessePlayers,
    RomesteadPlayers,
    TerrariaPlayers,
    AstroneerPlayers,
    SoulmaskPlayers,
    SatisfactoryFrmPlayers,
    BarotraumaPlayers,
    ReturnToMoriaPlayers,
    WindrosePlayers,
    DragonwildsPlayers,
    ScumPlayers,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimePlayerIdentityKind {
    KleiUserId,
    SteamId,
    ArkAccountId,
    ConanUserId,
    EosId,
    PlayerName,
    SessionId,
    PalworldUserId,
    MinecraftUuid,
    AstroneerGuid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModulePlayerListSpec {
    pub scope: ModulePlayerListScope,
    pub source: ModulePlayerListSource,
    pub action_id: Option<String>,
    pub player_action_ids: Vec<String>,
    pub response_codec: ModulePlayerListCodec,
    pub identity_kind: RuntimePlayerIdentityKind,
    pub refresh_interval_ms: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeLivePlayerStatus {
    Ready,
    Refreshing,
    Stopped,
    Unsupported,
    Misconfigured,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeLivePlayerSnapshot {
    pub snapshot_id: String,
    pub instance_id: String,
    pub status: RuntimeLivePlayerStatus,
    pub source: Option<ModulePlayerListSource>,
    pub observed_at_unix_ms: Option<u64>,
    pub expires_at_unix_ms: Option<u64>,
    pub complete: bool,
    pub truncated: bool,
    pub stale: bool,
    pub current_players: Option<usize>,
    pub max_players: Option<usize>,
    pub entries: Vec<RuntimeLivePlayerEntry>,
    pub issue: Option<RuntimeLivePlayerIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeLivePlayerEntry {
    pub player_key: String,
    pub display_name: String,
    pub identifiers: Vec<RuntimeLivePlayerIdentifier>,
    pub available_action_ids: Vec<String>,
    pub ping_ms: Option<u32>,
    pub session_started_at_unix_ms: Option<u64>,
    pub role: Option<String>,
    pub attributes: Vec<RuntimeLivePlayerAttribute>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeLivePlayerIdentifier {
    pub kind: RuntimePlayerIdentityKind,
    pub value: String,
    pub stable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeLivePlayerAttribute {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeLivePlayerIssueCode {
    ProcessUnavailable,
    ProcessUntracked,
    LogUnavailable,
    CollectionTimeout,
    ProtocolIncomplete,
    CaptureLimit,
    IoFailed,
    RuntimeActionUnavailable,
    AdapterUnavailable,
    NamesUnavailable,
    QueryUnavailable,
    AuthenticationFailed,
    ExtensionUnavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeLivePlayerIssue {
    pub code: RuntimeLivePlayerIssueCode,
    pub setting_keys: Vec<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecuteInstancePlayerActionInput {
    pub instance_id: String,
    pub snapshot_id: String,
    pub player_key: String,
    pub action_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecuteInstanceManualPlayerActionInput {
    pub instance_id: String,
    pub action_id: String,
    pub target: String,
    pub role: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeLivePlayerActionStatus {
    Sent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteInstancePlayerActionResult {
    pub action_id: String,
    pub status: RuntimeLivePlayerActionStatus,
    pub executed_at_unix_ms: u64,
    pub summary: String,
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    #[test]
    fn runtime_live_player_snapshot_serializes_public_fields_only() {
        let statuses = [
            RuntimeLivePlayerStatus::Ready,
            RuntimeLivePlayerStatus::Refreshing,
            RuntimeLivePlayerStatus::Stopped,
            RuntimeLivePlayerStatus::Unsupported,
            RuntimeLivePlayerStatus::Misconfigured,
            RuntimeLivePlayerStatus::Failed,
        ];
        let snapshot = RuntimeLivePlayerSnapshot {
            snapshot_id: String::from("snapshot-1"),
            instance_id: String::from("instance-1"),
            status: RuntimeLivePlayerStatus::Ready,
            source: Some(ModulePlayerListSource::StructuredLog),
            observed_at_unix_ms: Some(1_700_000_000_000),
            expires_at_unix_ms: Some(1_700_000_030_000),
            complete: true,
            truncated: false,
            stale: false,
            current_players: Some(1),
            max_players: Some(6),
            entries: vec![RuntimeLivePlayerEntry {
                player_key: String::from("KU_abc"),
                display_name: String::from("Wanda"),
                identifiers: vec![RuntimeLivePlayerIdentifier {
                    kind: RuntimePlayerIdentityKind::KleiUserId,
                    value: String::from("KU_abc"),
                    stable: true,
                }],
                available_action_ids: vec![String::from("kick_userid")],
                ping_ms: Some(42),
                session_started_at_unix_ms: None,
                role: Some(String::from("admin")),
                attributes: vec![RuntimeLivePlayerAttribute {
                    key: String::from("prefab"),
                    value: String::from("wanda"),
                }],
            }],
            issue: None,
        };

        let value = serde_json::to_value(snapshot).expect("snapshot serializes");
        assert_eq!(value["snapshot_id"], "snapshot-1");
        assert_eq!(value["instance_id"], "instance-1");
        assert_eq!(value["complete"], true);
        assert_eq!(value["truncated"], false);
        assert_eq!(value["stale"], false);
        assert_eq!(value["entries"][0]["player_key"], "KU_abc");
        assert_eq!(
            value["entries"][0]["identifiers"][0]["kind"],
            "klei_user_id"
        );
        assert_eq!(value["entries"][0]["identifiers"][0]["value"], "KU_abc");
        assert_eq!(value["entries"][0]["identifiers"][0]["stable"], true);
        assert_eq!(
            value["entries"][0]["available_action_ids"],
            serde_json::json!(["kick_userid"])
        );
        assert_eq!(value["entries"][0]["attributes"][0]["key"], "prefab");
        assert_eq!(value["entries"][0]["attributes"][0]["value"], "wanda");
        assert_eq!(
            statuses
                .iter()
                .map(serde_json::to_value)
                .collect::<Result<Vec<_>, _>>()
                .expect("all statuses serialize"),
            vec![
                Value::String(String::from("ready")),
                Value::String(String::from("refreshing")),
                Value::String(String::from("stopped")),
                Value::String(String::from("unsupported")),
                Value::String(String::from("misconfigured")),
                Value::String(String::from("failed")),
            ]
        );

        for forbidden in [
            "target",
            "command",
            "transport",
            "password",
            "credential",
            "raw_response",
            "log_lines",
        ] {
            assert!(
                !contains_key(&value, forbidden),
                "serialized player snapshot must not expose {forbidden}"
            );
        }
    }

    #[test]
    fn runtime_live_player_action_input_rejects_unknown_fields() {
        for injected_field in ["target", "command", "reason", "duration"] {
            let input = format!(
                concat!(
                    "{{\"instance_id\":\"instance-1\",",
                    "\"snapshot_id\":\"snapshot-1\",",
                    "\"player_key\":\"KU_abc\",",
                    "\"action_id\":\"kick_userid\",",
                    "\"{}\":\"injected\"}}"
                ),
                injected_field
            );

            assert!(
                serde_json::from_str::<ExecuteInstancePlayerActionInput>(&input).is_err(),
                "{injected_field} must not be accepted in the runtime player action input"
            );
        }
    }

    #[test]
    fn manual_player_action_input_rejects_dispatch_metadata() {
        for injected_field in [
            "command",
            "transport",
            "process_key",
            "password_setting_key",
        ] {
            let input = format!(
                concat!(
                    "{{\"instance_id\":\"instance-1\",",
                    "\"action_id\":\"kick_player\",",
                    "\"target\":\"Player1\",",
                    "\"role\":null,",
                    "\"{}\":\"injected\"}}"
                ),
                injected_field
            );
            assert!(
                serde_json::from_str::<ExecuteInstanceManualPlayerActionInput>(&input).is_err(),
                "{injected_field} must not be accepted in the manual player-action input"
            );
        }
    }

    fn contains_key(value: &Value, expected_key: &str) -> bool {
        match value {
            Value::Object(object) => object
                .iter()
                .any(|(key, value)| key == expected_key || contains_key(value, expected_key)),
            Value::Array(values) => values.iter().any(|value| contains_key(value, expected_key)),
            _ => false,
        }
    }
}
