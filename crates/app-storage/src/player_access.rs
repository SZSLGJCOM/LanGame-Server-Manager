use std::collections::HashSet;

use app_core::{InstanceDetails, UpdateInstanceInput};
use app_modules::ModuleDescriptor;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::instance_settings_lock::acquire_instance_settings_mutation_lock;
use crate::instances::{materialize_instance_configuration_locked, update_instance_locked};
use crate::player_access_delimited::{
    DelimitedEntryRequirement, normalize_delimited_player_access_entry,
    player_access_text_accepts_comma, split_player_access_text_entries,
};
use crate::player_access_normalization::{
    is_humanitz_net_id, is_stored_humanitz_identity, normalize_ark_account_id,
    normalize_dst_klei_id, normalize_module_player_access_settings_strict,
    normalize_terraria_banlist_entry, normalize_uint64_id, normalize_valheim_platform_id,
};
use crate::save_paths::load_module_descriptor;
use crate::{StorageError, StoragePaths, read_instance_details};

#[path = "player_access_live_target.rs"]
mod live_target;

const PLAYER_ACCESS_CODEC_KEY: &str = "x-lsgm-player-access-codec";
const PLAYER_ACCESS_CONFLICTS_KEY: &str = "x-lsgm-player-access-conflicts-with";
const PLAYER_ACCESS_KIND_KEY: &str = "x-lsgm-player-access-kind";
const PLAYER_ACCESS_SYNC_KEY: &str = "x-lsgm-player-access-sync";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayerAccessMutationOperation {
    Add,
    Remove,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyInstancePlayerAccessMutationInput {
    pub instance_id: String,
    pub field_key: String,
    pub operation: PlayerAccessMutationOperation,
    pub value: Value,
    pub expected_value: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayerAccessPersistentStatus {
    Updated,
    Unchanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayerAccessSyncMode {
    Direct,
    Reload,
    Restart,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerAccessSyncMetadata {
    pub mode: PlayerAccessSyncMode,
    pub add_action_id: Option<String>,
    pub remove_action_id: Option<String>,
    pub action_id: Option<String>,
    pub verify_action_id: Option<String>,
    pub consume_action_ids: Vec<String>,
}

impl PlayerAccessSyncMetadata {
    pub fn mutation_action_id(&self, operation: PlayerAccessMutationOperation) -> Option<&str> {
        match self.mode {
            PlayerAccessSyncMode::Direct => match operation {
                PlayerAccessMutationOperation::Add => self.add_action_id.as_deref(),
                PlayerAccessMutationOperation::Remove => self.remove_action_id.as_deref(),
            },
            PlayerAccessSyncMode::Reload => self.action_id.as_deref(),
            PlayerAccessSyncMode::Restart => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlayerAccessPersistentMutationResult {
    pub details: InstanceDetails,
    pub field_key: String,
    pub operation: PlayerAccessMutationOperation,
    pub persistent_status: PlayerAccessPersistentStatus,
    pub live_target: String,
    pub sync: PlayerAccessSyncMetadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlayerAccessCodec {
    Plain,
    Steam64,
    HumanitzNetId,
    Uint64,
    PipeSteam64,
    CsvUuidName,
    MinecraftIpCsv,
    TerrariaBanlist,
    ArkAccountId,
    BarotraumaAccount,
    DstKleiId,
    ValheimPlatformId,
    ObjectIdentity,
}

impl PlayerAccessCodec {
    fn schema_name(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Steam64 => "steam64",
            Self::HumanitzNetId => "humanitz_net_id",
            Self::Uint64 => "uint64",
            Self::PipeSteam64 => "pipe_steam64",
            Self::CsvUuidName => "csv_uuid_name",
            Self::MinecraftIpCsv => "minecraft_ip_csv",
            Self::TerrariaBanlist => "terraria_banlist",
            Self::ArkAccountId => "ark_account_id",
            Self::BarotraumaAccount => "barotrauma_account",
            Self::DstKleiId => "dst_klei_id",
            Self::ValheimPlatformId => "valheim_platform_id",
            Self::ObjectIdentity => "object_identity",
        }
    }

    fn is_delimited(self) -> bool {
        matches!(
            self,
            Self::PipeSteam64 | Self::CsvUuidName | Self::MinecraftIpCsv | Self::BarotraumaAccount
        )
    }
}

#[derive(Debug, Clone)]
struct NormalizedEntry {
    stored: Value,
    canonical_identity: String,
    live_target: String,
}

#[derive(Debug)]
struct PatchedField {
    value: Value,
    changed: bool,
    matched_live_target: Option<String>,
}

pub async fn apply_instance_player_access_mutation(
    paths: &StoragePaths,
    input: ApplyInstancePlayerAccessMutationInput,
) -> Result<PlayerAccessPersistentMutationResult, StorageError> {
    let instance_id = input.instance_id.trim();
    let field_key = input.field_key.trim();
    if instance_id.is_empty() {
        return Err(invalid_player_access_input(
            "unknown",
            field_key,
            "instance id must not be empty",
        ));
    }
    if field_key.is_empty() {
        return Err(invalid_player_access_input(
            "unknown",
            field_key,
            "field key must not be empty",
        ));
    }

    let settings_lock = acquire_instance_settings_mutation_lock(paths, instance_id)?;
    let details = read_instance_details(paths, instance_id).await?;
    let module_id = details.summary.module_id.clone();
    let descriptor = load_module_descriptor(paths, &module_id)?.ok_or_else(|| {
        invalid_player_access_input(&module_id, field_key, "module descriptor is unavailable")
    })?;
    let schema = parse_module_schema(&descriptor, field_key)?;
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            invalid_player_access_input(
                &module_id,
                field_key,
                "module schema does not declare an object properties map",
            )
        })?;
    let field_schema = properties
        .get(field_key)
        .and_then(Value::as_object)
        .ok_or_else(|| {
            invalid_player_access_input(
                &module_id,
                field_key,
                "field is not declared by the module schema",
            )
        })?;

    validate_player_access_field(&module_id, field_key, field_schema)?;
    let codec = parse_codec(&module_id, field_key, field_schema)?;
    validate_codec_storage(&module_id, field_key, codec, field_schema)?;
    let mut sync = parse_sync_metadata(&module_id, field_key, field_schema)?;
    validate_sync_actions(&module_id, field_key, &descriptor, &sync)?;

    let normalized = normalize_entry(
        &module_id,
        field_key,
        codec,
        field_schema,
        input.operation,
        &input.value,
    )?;
    let mut settings: Value = serde_json::from_str(&details.settings_json)?;
    let settings = settings
        .as_object_mut()
        .ok_or(StorageError::InvalidSettingsRoot)?;
    normalize_module_player_access_settings_strict(&module_id, settings)?;
    validate_cross_roster_conflicts(
        &module_id,
        field_key,
        field_schema,
        properties,
        settings,
        input.operation,
        &normalized,
    )?;
    let current_value = settings
        .get(field_key)
        .cloned()
        .or_else(|| field_schema.get("default").cloned())
        .unwrap_or_else(|| {
            if field_schema.get("type").and_then(Value::as_str) == Some("array") {
                Value::Array(Vec::new())
            } else {
                Value::String(String::new())
            }
        });
    validate_scalar_expected_value(
        &module_id,
        field_key,
        codec,
        field_schema,
        &current_value,
        input.expected_value.as_ref(),
    )?;
    let patched = patch_field_value(
        &module_id,
        field_key,
        codec,
        field_schema,
        input.operation,
        &current_value,
        &normalized,
    )?;
    let live_target = if matches!(input.operation, PlayerAccessMutationOperation::Remove) {
        patched
            .matched_live_target
            .as_deref()
            .filter(|value| !value.is_empty())
            .unwrap_or(&normalized.live_target)
            .to_string()
    } else {
        normalized.live_target.clone()
    };

    if codec == PlayerAccessCodec::HumanitzNetId
        && input.operation == PlayerAccessMutationOperation::Remove
        && !is_humanitz_net_id(&live_target)
    {
        // Old Steam-only entries can be cleaned up, but must never become a
        // substring target for the current EOS-backed RCON player lookup.
        sync.mode = PlayerAccessSyncMode::Restart;
    }

    let (details, persistent_status) = if patched.changed {
        settings.insert(field_key.to_string(), patched.value);
        let settings_json = serde_json::to_string_pretty(&Value::Object(settings.clone()))?;
        let updated = update_instance_locked(
            paths,
            UpdateInstanceInput {
                id: details.summary.id.clone(),
                bind_ip: details.summary.bind_ip.clone(),
                auto_backup_on_stop: details.auto_backup_on_stop,
                backup_retention_count: details.backup_retention_count,
                settings_json,
                ports: details.ports.clone(),
            },
            &settings_lock,
        )
        .await?;
        (updated, PlayerAccessPersistentStatus::Updated)
    } else if matches!(sync.mode, PlayerAccessSyncMode::Reload) {
        let materialized =
            materialize_instance_configuration_locked(paths, &details.summary.id, &settings_lock)
                .await?;
        (materialized, PlayerAccessPersistentStatus::Unchanged)
    } else {
        (details, PlayerAccessPersistentStatus::Unchanged)
    };

    Ok(PlayerAccessPersistentMutationResult {
        details,
        field_key: field_key.to_string(),
        operation: input.operation,
        persistent_status,
        live_target,
        sync,
    })
}

fn validate_scalar_expected_value(
    module_id: &str,
    field_key: &str,
    codec: PlayerAccessCodec,
    field_schema: &Map<String, Value>,
    current_value: &Value,
    expected_value: Option<&Value>,
) -> Result<(), StorageError> {
    let scalar = field_schema.get("type").and_then(Value::as_str) == Some("string")
        && field_schema.get("format").and_then(Value::as_str) != Some("textarea");
    let expected_value = match (scalar, expected_value) {
        (true, Some(expected_value)) => expected_value,
        (true, None) => {
            return Err(invalid_player_access_input(
                module_id,
                field_key,
                "expectedValue is required for scalar player-access fields",
            ));
        }
        (false, Some(_)) => {
            return Err(invalid_player_access_input(
                module_id,
                field_key,
                "expectedValue is only valid for scalar player-access fields",
            ));
        }
        (false, None) => return Ok(()),
    };
    let values_match = current_value == expected_value
        || normalized_scalar_comparison_value(codec, field_schema, current_value)
            == normalized_scalar_comparison_value(codec, field_schema, expected_value);
    if values_match {
        Ok(())
    } else {
        Err(invalid_player_access_input(
            module_id,
            field_key,
            "player-access value changed while this edit was pending; reload and retry",
        ))
    }
}

fn normalized_scalar_comparison_value(
    codec: PlayerAccessCodec,
    field_schema: &Map<String, Value>,
    value: &Value,
) -> Value {
    let Some(text) = value.as_str() else {
        return value.clone();
    };
    if text.trim().is_empty() {
        return Value::String(String::new());
    }
    normalize_existing_entry(codec, field_schema, &Value::String(text.trim().to_string()))
        .map(|entry| entry.stored)
        .unwrap_or_else(|| Value::String(text.trim().to_string()))
}

fn validate_cross_roster_conflicts(
    module_id: &str,
    field_key: &str,
    field_schema: &Map<String, Value>,
    properties: &Map<String, Value>,
    settings: &Map<String, Value>,
    operation: PlayerAccessMutationOperation,
    normalized: &NormalizedEntry,
) -> Result<(), StorageError> {
    if !matches!(operation, PlayerAccessMutationOperation::Add) {
        return Ok(());
    }
    let Some(conflicts) = field_schema
        .get(PLAYER_ACCESS_CONFLICTS_KEY)
        .and_then(Value::as_array)
    else {
        return Ok(());
    };
    for conflict_field in conflicts {
        let conflict_field = conflict_field.as_str().map(str::trim).ok_or_else(|| {
            invalid_player_access_input(
                module_id,
                field_key,
                "player-access conflict metadata must contain field names",
            )
        })?;
        let conflict_schema = properties
            .get(conflict_field)
            .and_then(Value::as_object)
            .ok_or_else(|| {
                invalid_player_access_input(
                    module_id,
                    field_key,
                    &format!("conflict metadata references unknown field `{conflict_field}`"),
                )
            })?;
        let conflict_codec = parse_codec(module_id, conflict_field, conflict_schema)?;
        let current = settings
            .get(conflict_field)
            .or_else(|| conflict_schema.get("default"));
        let has_conflict = current.is_some_and(|current| {
            existing_field_entries(conflict_codec, conflict_schema, current)
                .into_iter()
                .any(|entry| entry.canonical_identity == normalized.canonical_identity)
        });
        if has_conflict {
            return Err(invalid_player_access_input(
                module_id,
                field_key,
                &format!(
                    "identity conflicts with `{conflict_field}`; remove it there before adding it to `{field_key}`"
                ),
            ));
        }
    }
    Ok(())
}

fn existing_field_entries(
    codec: PlayerAccessCodec,
    field_schema: &Map<String, Value>,
    current: &Value,
) -> Vec<NormalizedEntry> {
    match field_schema.get("type").and_then(Value::as_str) {
        Some("string")
            if field_schema.get("format").and_then(Value::as_str) == Some("textarea") =>
        {
            let Some(current) = current.as_str() else {
                return Vec::new();
            };
            split_player_access_text_entries(codec.schema_name(), field_schema, current)
                .into_iter()
                .filter_map(|entry| {
                    normalize_existing_entry(codec, field_schema, &Value::String(entry))
                })
                .collect()
        }
        Some("string") => normalize_existing_entry(codec, field_schema, current)
            .into_iter()
            .collect(),
        Some("array") => current
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| normalize_existing_entry(codec, field_schema, entry))
            .collect(),
        _ => Vec::new(),
    }
}

fn parse_module_schema(
    descriptor: &ModuleDescriptor,
    field_key: &str,
) -> Result<Value, StorageError> {
    let schema_json = descriptor.schema_json.as_deref().ok_or_else(|| {
        invalid_player_access_input(
            &descriptor.summary.id,
            field_key,
            "module does not declare a settings schema",
        )
    })?;
    serde_json::from_str(schema_json).map_err(StorageError::ConfigJson)
}

fn validate_player_access_field(
    module_id: &str,
    field_key: &str,
    field_schema: &Map<String, Value>,
) -> Result<(), StorageError> {
    let access_kind = field_schema
        .get(PLAYER_ACCESS_KIND_KEY)
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if !matches!(access_kind, "admin" | "allow" | "block" | "priority") {
        return Err(invalid_player_access_input(
            module_id,
            field_key,
            "field is not a supported player-access roster",
        ));
    }

    match field_schema.get("type").and_then(Value::as_str) {
        Some("string" | "array") => Ok(()),
        _ => Err(invalid_player_access_input(
            module_id,
            field_key,
            "player-access field must be a string or array",
        )),
    }
}

fn parse_codec(
    module_id: &str,
    field_key: &str,
    field_schema: &Map<String, Value>,
) -> Result<PlayerAccessCodec, StorageError> {
    let codec = field_schema
        .get(PLAYER_ACCESS_CODEC_KEY)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|codec| !codec.is_empty())
        .ok_or_else(|| {
            invalid_player_access_input(
                module_id,
                field_key,
                "x-lsgm-player-access-codec must explicitly declare a non-empty codec string",
            )
        })?;
    match codec {
        "plain" => Ok(PlayerAccessCodec::Plain),
        "steam64" => Ok(PlayerAccessCodec::Steam64),
        "humanitz_net_id" => Ok(PlayerAccessCodec::HumanitzNetId),
        "uint64" => Ok(PlayerAccessCodec::Uint64),
        "pipe_steam64" => Ok(PlayerAccessCodec::PipeSteam64),
        "csv_uuid_name" => Ok(PlayerAccessCodec::CsvUuidName),
        "minecraft_ip_csv" => Ok(PlayerAccessCodec::MinecraftIpCsv),
        "terraria_banlist" => Ok(PlayerAccessCodec::TerrariaBanlist),
        "ark_account_id" => Ok(PlayerAccessCodec::ArkAccountId),
        "barotrauma_account" => Ok(PlayerAccessCodec::BarotraumaAccount),
        "dst_klei_id" => Ok(PlayerAccessCodec::DstKleiId),
        "valheim_platform_id" => Ok(PlayerAccessCodec::ValheimPlatformId),
        "object_identity" => Ok(PlayerAccessCodec::ObjectIdentity),
        codec => Err(invalid_player_access_input(
            module_id,
            field_key,
            &format!("unsupported player-access codec `{codec}`"),
        )),
    }
}

fn validate_codec_storage(
    module_id: &str,
    field_key: &str,
    codec: PlayerAccessCodec,
    field_schema: &Map<String, Value>,
) -> Result<(), StorageError> {
    if field_schema.contains_key("x-lsgm-player-access-live-target")
        && codec != PlayerAccessCodec::ObjectIdentity
    {
        return Err(invalid_player_access_input(
            module_id,
            field_key,
            "explicit live target formats require an object_identity roster",
        ));
    }
    let field_type = field_schema.get("type").and_then(Value::as_str);
    let item_type = field_schema
        .get("items")
        .and_then(Value::as_object)
        .and_then(|items| items.get("type"))
        .and_then(Value::as_str);
    let compatible = match codec {
        PlayerAccessCodec::ObjectIdentity => {
            field_type == Some("array") && item_type == Some("object")
        }
        _ => {
            field_type == Some("string")
                || (field_type == Some("array") && item_type == Some("string"))
        }
    };
    if compatible {
        Ok(())
    } else {
        Err(invalid_player_access_input(
            module_id,
            field_key,
            "player-access codec is incompatible with the field storage shape",
        ))
    }
}

fn parse_sync_metadata(
    module_id: &str,
    field_key: &str,
    field_schema: &Map<String, Value>,
) -> Result<PlayerAccessSyncMetadata, StorageError> {
    let raw_sync = field_schema.get(PLAYER_ACCESS_SYNC_KEY).ok_or_else(|| {
        invalid_player_access_input(
            module_id,
            field_key,
            "x-lsgm-player-access-sync must explicitly declare synchronization metadata",
        )
    })?;
    let sync = raw_sync.as_object().ok_or_else(|| {
        invalid_player_access_input(
            module_id,
            field_key,
            "player-access sync metadata must be an object",
        )
    })?;
    let mode = match optional_non_empty_string(sync, "mode").as_deref() {
        Some("direct") => PlayerAccessSyncMode::Direct,
        Some("reload") => PlayerAccessSyncMode::Reload,
        Some("restart") => PlayerAccessSyncMode::Restart,
        Some(mode) => {
            return Err(invalid_player_access_input(
                module_id,
                field_key,
                &format!("unsupported player-access sync mode `{mode}`"),
            ));
        }
        None => {
            return Err(invalid_player_access_input(
                module_id,
                field_key,
                "player-access sync mode is missing",
            ));
        }
    };
    let consume_action_ids = match sync.get("consume_action_ids") {
        None => Vec::new(),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .ok_or_else(|| {
                        invalid_player_access_input(
                            module_id,
                            field_key,
                            "consume_action_ids must contain non-empty strings",
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => {
            return Err(invalid_player_access_input(
                module_id,
                field_key,
                "consume_action_ids must be an array",
            ));
        }
    };

    for key in [
        "add_action_id",
        "remove_action_id",
        "action_id",
        "verify_action_id",
    ] {
        if sync.contains_key(key) && optional_non_empty_string(sync, key).is_none() {
            return Err(invalid_player_access_input(
                module_id,
                field_key,
                "runtime action ids must be non-empty strings when declared",
            ));
        }
    }
    let metadata = PlayerAccessSyncMetadata {
        mode,
        add_action_id: optional_non_empty_string(sync, "add_action_id"),
        remove_action_id: optional_non_empty_string(sync, "remove_action_id"),
        action_id: optional_non_empty_string(sync, "action_id"),
        verify_action_id: optional_non_empty_string(sync, "verify_action_id"),
        consume_action_ids,
    };
    match metadata.mode {
        PlayerAccessSyncMode::Direct
            if (metadata.add_action_id.is_none() && metadata.remove_action_id.is_none())
                || metadata.action_id.is_some() =>
        {
            Err(invalid_player_access_input(
                module_id,
                field_key,
                "direct sync requires at least one of add_action_id or remove_action_id, without action_id",
            ))
        }
        PlayerAccessSyncMode::Reload
            if metadata.action_id.is_none()
                || metadata.add_action_id.is_some()
                || metadata.remove_action_id.is_some() =>
        {
            Err(invalid_player_access_input(
                module_id,
                field_key,
                "reload sync requires action_id only",
            ))
        }
        PlayerAccessSyncMode::Restart
            if metadata.add_action_id.is_some()
                || metadata.remove_action_id.is_some()
                || metadata.action_id.is_some()
                || metadata.verify_action_id.is_some() =>
        {
            Err(invalid_player_access_input(
                module_id,
                field_key,
                "restart sync must not declare runtime action ids",
            ))
        }
        _ => Ok(metadata),
    }
}

fn optional_non_empty_string(values: &Map<String, Value>, key: &str) -> Option<String> {
    values
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn validate_sync_actions(
    module_id: &str,
    field_key: &str,
    descriptor: &ModuleDescriptor,
    sync: &PlayerAccessSyncMetadata,
) -> Result<(), StorageError> {
    let known_action_ids = descriptor
        .runtime
        .player_actions
        .iter()
        .map(|action| action.id.as_str())
        .collect::<HashSet<_>>();
    let declared_ids = [
        sync.add_action_id.as_deref(),
        sync.remove_action_id.as_deref(),
        sync.action_id.as_deref(),
        sync.verify_action_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    .chain(sync.consume_action_ids.iter().map(String::as_str));

    for action_id in declared_ids {
        if !known_action_ids.contains(action_id) {
            return Err(invalid_player_access_input(
                module_id,
                field_key,
                &format!("sync metadata references unknown runtime action `{action_id}`"),
            ));
        }
    }
    Ok(())
}

fn normalize_entry(
    module_id: &str,
    field_key: &str,
    codec: PlayerAccessCodec,
    field_schema: &Map<String, Value>,
    operation: PlayerAccessMutationOperation,
    raw_value: &Value,
) -> Result<NormalizedEntry, StorageError> {
    if matches!(codec, PlayerAccessCodec::ObjectIdentity) {
        return normalize_object_identity(module_id, field_key, field_schema, raw_value);
    }
    let raw_value = raw_value.as_str().ok_or_else(|| {
        invalid_player_access_input(
            module_id,
            field_key,
            "player-access value must be a string for this roster codec",
        )
    })?;
    if raw_value.chars().any(char::is_control) {
        return Err(invalid_player_access_input(
            module_id,
            field_key,
            "player-access value must not contain control characters",
        ));
    }
    let value = raw_value.trim();
    if value.is_empty() {
        return Err(invalid_player_access_input(
            module_id,
            field_key,
            "player-access value must not be empty",
        ));
    }
    validate_string_property(module_id, field_key, field_schema, value)?;

    if codec.is_delimited() {
        let requirement = match operation {
            PlayerAccessMutationOperation::Add => DelimitedEntryRequirement::MutationAdd,
            PlayerAccessMutationOperation::Remove => DelimitedEntryRequirement::MutationRemove,
        };
        let normalized = normalize_delimited_player_access_entry(
            codec.schema_name(),
            field_schema,
            value,
            requirement,
        )
        .map_err(|message| invalid_player_access_input(module_id, field_key, &message))?;
        let live_target = if matches!(codec, PlayerAccessCodec::CsvUuidName) {
            normalized
                .parts
                .get(1)
                .cloned()
                .unwrap_or_else(|| normalized.identity.clone())
        } else {
            normalized.identity.clone()
        };
        return Ok(NormalizedEntry {
            stored: Value::String(normalized.stored),
            canonical_identity: canonical_text(&normalized.identity),
            live_target,
        });
    }

    match codec {
        PlayerAccessCodec::Plain => Ok(normalized_string_entry(value, value)),
        PlayerAccessCodec::HumanitzNetId => {
            let valid = if operation == PlayerAccessMutationOperation::Add {
                is_humanitz_net_id(value)
            } else {
                is_stored_humanitz_identity(value)
            };
            if !valid {
                return Err(invalid_player_access_input(
                    module_id,
                    field_key,
                    "enter the complete HumanitZ NetID (EpicAccountId|ProductUserId or |ProductUserId), with 32 hexadecimal digits per nonempty part; old Steam IDs can only be retained or removed",
                ));
            }
            Ok(NormalizedEntry {
                stored: Value::String(value.to_owned()),
                canonical_identity: value.to_owned(),
                live_target: value.to_owned(),
            })
        }
        PlayerAccessCodec::Steam64 => {
            validate_steam64(module_id, field_key, value)?;
            Ok(normalized_string_entry(value, value))
        }
        PlayerAccessCodec::Uint64 => {
            let id = normalize_uint64_id(value).ok_or_else(|| {
                invalid_player_access_input(
                    module_id,
                    field_key,
                    "value must contain only decimal digits in 0..18446744073709551615",
                )
            })?;
            Ok(normalized_string_entry(&id, &id))
        }
        PlayerAccessCodec::PipeSteam64
        | PlayerAccessCodec::CsvUuidName
        | PlayerAccessCodec::MinecraftIpCsv
        | PlayerAccessCodec::BarotraumaAccount => unreachable!(),
        PlayerAccessCodec::TerrariaBanlist => {
            let entry = normalize_terraria_banlist_entry(value).ok_or_else(|| {
                invalid_player_access_input(
                    module_id,
                    field_key,
                    "terraria_banlist entries must be at most 128 UTF-8 bytes and contain no template or control syntax",
                )
            })?;
            Ok(normalized_string_entry(&entry, &entry))
        }
        PlayerAccessCodec::ArkAccountId => {
            let account_id = normalize_ark_account_id(value).ok_or_else(|| {
                invalid_player_access_input(
                    module_id,
                    field_key,
                    "ark_account_id must be one safe account ID token of at most 128 characters",
                )
            })?;
            Ok(normalized_string_entry(&account_id, &account_id))
        }
        PlayerAccessCodec::DstKleiId => {
            let user_id = normalize_dst_klei_id(value).ok_or_else(|| {
                invalid_player_access_input(
                    module_id,
                    field_key,
                    "dst_klei_id must be one safe KU_ Klei user ID token",
                )
            })?;
            Ok(normalized_string_entry(&user_id, &user_id))
        }
        PlayerAccessCodec::ValheimPlatformId => {
            let platform_id = normalize_valheim_platform_id(value).ok_or_else(|| {
                invalid_player_access_input(
                    module_id,
                    field_key,
                    "valheim_platform_id must be one platform ID token without whitespace, comma, or pipe delimiters",
                )
            })?;
            Ok(normalized_string_entry(&platform_id, &platform_id))
        }
        PlayerAccessCodec::ObjectIdentity => unreachable!(),
    }
}

fn normalized_string_entry(stored: &str, identity: &str) -> NormalizedEntry {
    NormalizedEntry {
        stored: Value::String(stored.to_string()),
        canonical_identity: canonical_text(identity),
        live_target: identity.to_string(),
    }
}

fn normalize_object_identity(
    module_id: &str,
    field_key: &str,
    field_schema: &Map<String, Value>,
    raw_value: &Value,
) -> Result<NormalizedEntry, StorageError> {
    let item_schema = field_schema
        .get("items")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            invalid_player_access_input(
                module_id,
                field_key,
                "object_identity requires an array item schema",
            )
        })?;
    let properties = item_schema
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            invalid_player_access_input(
                module_id,
                field_key,
                "object_identity requires item properties",
            )
        })?;
    let identity_keys = find_object_identity_keys(properties);
    if identity_keys.is_empty() {
        return Err(invalid_player_access_input(
            module_id,
            field_key,
            "object_identity has no writable string identity property",
        ));
    }
    let raw_object = raw_value.as_object().ok_or_else(|| {
        invalid_player_access_input(
            module_id,
            field_key,
            "object_identity mutations require a structured object value",
        )
    })?;

    let access_kind = field_schema
        .get(PLAYER_ACCESS_KIND_KEY)
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut stored = Map::new();
    for (key, property) in properties {
        let property = property.as_object();
        let mut value = if let Some(value) = raw_object.get(key) {
            value.clone()
        } else if let Some(default) = property.and_then(|property| property.get("default")) {
            default.clone()
        } else {
            default_object_property_value(key, property, access_kind)
        };
        if property
            .and_then(|property| property.get("type"))
            .and_then(Value::as_str)
            == Some("string")
        {
            let text = value.as_str().ok_or_else(|| {
                invalid_player_access_input(
                    module_id,
                    field_key,
                    &format!("object property `{key}` must be a string"),
                )
            })?;
            if text.chars().any(char::is_control) {
                return Err(invalid_player_access_input(
                    module_id,
                    field_key,
                    &format!("object property `{key}` must not contain control characters"),
                ));
            }
            let text = text.trim();
            validate_string_property(module_id, field_key, property.expect("string schema"), text)?;
            value = Value::String(text.to_string());
        }
        stored.insert(key.clone(), value);
    }

    let mut canonical_parts = Vec::with_capacity(identity_keys.len());
    for identity_key in &identity_keys {
        let identity_schema = properties
            .get(*identity_key)
            .and_then(Value::as_object)
            .ok_or_else(|| {
                invalid_player_access_input(
                    module_id,
                    field_key,
                    "object identity property must be a schema object",
                )
            })?;
        let identity = stored
            .get(*identity_key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                invalid_player_access_input(
                    module_id,
                    field_key,
                    &format!(
                        "object identity property `{identity_key}` must be a non-empty string"
                    ),
                )
            })?
            .to_string();
        if let Some(platform_field) = identity_schema
            .get("x-lsgm-player-access-platform-field")
            .and_then(Value::as_str)
        {
            let platform = stored
                .get(platform_field)
                .and_then(Value::as_str)
                .unwrap_or_default();
            crate::settings_value_formats::validate_platform_account_id(platform, &identity)
                .map_err(|message| invalid_player_access_input(module_id, field_key, &message))?;
        }
        validate_object_identity_property(
            module_id,
            field_key,
            identity_key,
            identity_schema,
            &identity,
        )?;
        stored.insert((*identity_key).to_string(), Value::String(identity.clone()));
        canonical_parts.push(((*identity_key).to_string(), canonical_text(&identity)));
    }
    let canonical_identity = serde_json::to_string(&canonical_parts)?;
    let explicit_live_target = live_target::explicit_object_live_target(
        module_id,
        field_key,
        field_schema,
        properties,
        &identity_keys,
        raw_object,
        &stored,
    )?;
    let live_target = explicit_live_target.unwrap_or_else(|| {
        identity_keys
            .iter()
            .find(|key| {
                properties
                    .get(**key)
                    .and_then(Value::as_object)
                    .is_some_and(|property| {
                        property
                            .get("x-lsgm-player-access-platform-field")
                            .and_then(Value::as_str)
                            .is_some()
                    })
            })
            .or_else(|| identity_keys.last())
            .and_then(|key| stored.get(*key))
            .and_then(Value::as_str)
            .expect("validated identity remains a string")
            .to_string()
    });

    Ok(NormalizedEntry {
        stored: Value::Object(stored),
        canonical_identity,
        live_target,
    })
}

fn validate_string_property(
    module_id: &str,
    field_key: &str,
    schema: &Map<String, Value>,
    value: &str,
) -> Result<(), StorageError> {
    let valid_format = match schema.get("format").and_then(Value::as_str) {
        Some("date") => crate::settings_value_formats::is_iso_calendar_date(value),
        Some("date-or-local-datetime") => {
            crate::settings_value_formats::is_date_or_local_datetime(value)
        }
        _ => true,
    };
    if !valid_format {
        return Err(invalid_player_access_input(
            module_id,
            field_key,
            "value has an invalid calendar date or local time",
        ));
    }
    let length = value.chars().count() as u64;
    if schema
        .get("minLength")
        .and_then(Value::as_u64)
        .is_some_and(|minimum| length < minimum)
    {
        return Err(invalid_player_access_input(
            module_id,
            field_key,
            "value is shorter than the schema minimum",
        ));
    }
    if schema
        .get("maxLength")
        .and_then(Value::as_u64)
        .is_some_and(|maximum| length > maximum)
    {
        return Err(invalid_player_access_input(
            module_id,
            field_key,
            "value is longer than the schema maximum",
        ));
    }
    if let Some(pattern) = schema.get("pattern").and_then(Value::as_str) {
        let regex = Regex::new(pattern).map_err(|error| {
            invalid_player_access_input(
                module_id,
                field_key,
                &format!("string pattern is invalid: {error}"),
            )
        })?;
        if !regex.is_match(value) {
            return Err(invalid_player_access_input(
                module_id,
                field_key,
                "value does not match the roster field schema pattern",
            ));
        }
    }
    Ok(())
}

fn validate_object_identity_property(
    module_id: &str,
    field_key: &str,
    identity_key: &str,
    identity_schema: &Map<String, Value>,
    identity: &str,
) -> Result<(), StorageError> {
    if let Some(values) = identity_schema.get("enum").and_then(Value::as_array)
        && !values.iter().any(|value| value.as_str() == Some(identity))
    {
        return Err(invalid_player_access_input(
            module_id,
            field_key,
            &format!("object identity property `{identity_key}` is not an allowed enum value"),
        ));
    }
    if let Some(pattern) = identity_schema.get("pattern").and_then(Value::as_str) {
        let regex = Regex::new(pattern).map_err(|error| {
            invalid_player_access_input(
                module_id,
                field_key,
                &format!("identity pattern is invalid: {error}"),
            )
        })?;
        if !regex.is_match(identity) {
            return Err(invalid_player_access_input(
                module_id,
                field_key,
                &format!("object identity property `{identity_key}` does not match its pattern"),
            ));
        }
    }
    Ok(())
}

fn default_object_property_value(
    key: &str,
    property: Option<&Map<String, Value>>,
    access_kind: &str,
) -> Value {
    match property
        .and_then(|property| property.get("type"))
        .and_then(Value::as_str)
    {
        Some("integer" | "number") => {
            let value = if key.ends_with("permission_level") && access_kind != "admin" {
                1000
            } else {
                0
            };
            Value::Number(value.into())
        }
        Some("boolean") => Value::Bool(false),
        _ if key.to_ascii_lowercase().contains("reason") => Value::String(String::from("LanGame")),
        _ => Value::String(String::new()),
    }
}

fn validate_steam64(module_id: &str, field_key: &str, value: &str) -> Result<(), StorageError> {
    if value.len() == 17 && value.bytes().all(|byte| byte.is_ascii_digit()) {
        Ok(())
    } else {
        Err(invalid_player_access_input(
            module_id,
            field_key,
            "value must be a 17-digit Steam64 ID",
        ))
    }
}

fn patch_field_value(
    module_id: &str,
    field_key: &str,
    codec: PlayerAccessCodec,
    field_schema: &Map<String, Value>,
    operation: PlayerAccessMutationOperation,
    current_value: &Value,
    normalized: &NormalizedEntry,
) -> Result<PatchedField, StorageError> {
    match field_schema.get("type").and_then(Value::as_str) {
        Some("string")
            if field_schema.get("format").and_then(Value::as_str) == Some("textarea") =>
        {
            patch_string_lines(
                module_id,
                field_key,
                codec,
                field_schema,
                operation,
                current_value,
                normalized,
            )
        }
        Some("string") => patch_scalar_string(
            module_id,
            field_key,
            codec,
            field_schema,
            operation,
            current_value,
            normalized,
        ),
        Some("array") => patch_array(
            module_id,
            field_key,
            codec,
            field_schema,
            operation,
            current_value,
            normalized,
        ),
        _ => Err(invalid_player_access_input(
            module_id,
            field_key,
            "unsupported player-access field storage type",
        )),
    }
}

fn patch_string_lines(
    module_id: &str,
    field_key: &str,
    codec: PlayerAccessCodec,
    field_schema: &Map<String, Value>,
    operation: PlayerAccessMutationOperation,
    current_value: &Value,
    normalized: &NormalizedEntry,
) -> Result<PatchedField, StorageError> {
    let current = current_value.as_str().ok_or_else(|| {
        invalid_player_access_input(module_id, field_key, "stored roster value must be a string")
    })?;
    let normalize_declared_separators =
        player_access_text_accepts_comma(codec.schema_name(), field_schema);
    let entries = if normalize_declared_separators {
        let mut seen = HashSet::new();
        split_player_access_text_entries(codec.schema_name(), field_schema, current)
            .into_iter()
            .filter_map(|entry| {
                normalize_existing_entry(codec, field_schema, &Value::String(entry))
            })
            .filter(|entry| seen.insert(entry.canonical_identity.clone()))
            .filter_map(|entry| entry.stored.as_str().map(String::from))
            .collect::<Vec<_>>()
    } else {
        current
            .lines()
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    let canonical_current = normalize_declared_separators.then(|| entries.join("\n"));
    let matched_live_target = entries.iter().find_map(|entry| {
        normalize_existing_entry(codec, field_schema, &Value::String(entry.clone()))
            .filter(|existing| existing.canonical_identity == normalized.canonical_identity)
            .map(|entry| entry.live_target)
    });
    let has_match = matched_live_target.is_some();

    match operation {
        PlayerAccessMutationOperation::Add if has_match => Ok(PatchedField {
            value: canonical_current
                .as_ref()
                .map(|value| Value::String(value.clone()))
                .unwrap_or_else(|| current_value.clone()),
            changed: canonical_current
                .as_ref()
                .is_some_and(|value| value != current),
            matched_live_target,
        }),
        PlayerAccessMutationOperation::Add => {
            let stored = normalized.stored.as_str().ok_or_else(|| {
                invalid_player_access_input(
                    module_id,
                    field_key,
                    "string roster codec did not produce a string value",
                )
            })?;
            let mut next = entries;
            next.push(stored.to_string());
            Ok(PatchedField {
                value: Value::String(next.join("\n")),
                changed: true,
                matched_live_target: None,
            })
        }
        PlayerAccessMutationOperation::Remove if !has_match => Ok(PatchedField {
            value: canonical_current
                .as_ref()
                .map(|value| Value::String(value.clone()))
                .unwrap_or_else(|| current_value.clone()),
            changed: canonical_current
                .as_ref()
                .is_some_and(|value| value != current),
            matched_live_target: None,
        }),
        PlayerAccessMutationOperation::Remove => {
            let next = entries
                .into_iter()
                .filter(|entry| {
                    normalize_existing_entry(codec, field_schema, &Value::String(entry.clone()))
                        .map(|existing| {
                            existing.canonical_identity != normalized.canonical_identity
                        })
                        .unwrap_or(true)
                })
                .collect::<Vec<_>>();
            Ok(PatchedField {
                value: Value::String(next.join("\n")),
                changed: true,
                matched_live_target,
            })
        }
    }
}

fn patch_scalar_string(
    module_id: &str,
    field_key: &str,
    codec: PlayerAccessCodec,
    field_schema: &Map<String, Value>,
    operation: PlayerAccessMutationOperation,
    current_value: &Value,
    normalized: &NormalizedEntry,
) -> Result<PatchedField, StorageError> {
    let current = current_value.as_str().ok_or_else(|| {
        invalid_player_access_input(module_id, field_key, "stored roster value must be a string")
    })?;
    let existing = if current.trim().is_empty() {
        None
    } else {
        normalize_existing_entry(
            codec,
            field_schema,
            &Value::String(current.trim().to_string()),
        )
    };
    let is_match = existing
        .as_ref()
        .map(|entry| entry.canonical_identity == normalized.canonical_identity)
        .unwrap_or(false);
    let matched_live_target = existing
        .filter(|entry| entry.canonical_identity == normalized.canonical_identity)
        .map(|entry| entry.live_target);

    match operation {
        PlayerAccessMutationOperation::Add if is_match => Ok(PatchedField {
            value: current_value.clone(),
            changed: false,
            matched_live_target,
        }),
        PlayerAccessMutationOperation::Add => Ok(PatchedField {
            value: normalized.stored.clone(),
            changed: current_value != &normalized.stored,
            matched_live_target: None,
        }),
        PlayerAccessMutationOperation::Remove if is_match => Ok(PatchedField {
            value: Value::String(String::new()),
            changed: true,
            matched_live_target,
        }),
        PlayerAccessMutationOperation::Remove => Ok(PatchedField {
            value: current_value.clone(),
            changed: false,
            matched_live_target: None,
        }),
    }
}

fn patch_array(
    module_id: &str,
    field_key: &str,
    codec: PlayerAccessCodec,
    field_schema: &Map<String, Value>,
    operation: PlayerAccessMutationOperation,
    current_value: &Value,
    normalized: &NormalizedEntry,
) -> Result<PatchedField, StorageError> {
    let current = current_value.as_array().ok_or_else(|| {
        invalid_player_access_input(module_id, field_key, "stored roster value must be an array")
    })?;
    let matched_live_target = current.iter().find_map(|entry| {
        normalize_existing_entry(codec, field_schema, entry)
            .filter(|existing| existing.canonical_identity == normalized.canonical_identity)
            .map(|entry| entry.live_target)
    });
    let has_match = matched_live_target.is_some();

    match operation {
        PlayerAccessMutationOperation::Add
            if has_match && matches!(codec, PlayerAccessCodec::ObjectIdentity) =>
        {
            let mut replaced = false;
            let mut next = Vec::with_capacity(current.len());
            for entry in current {
                let is_match =
                    normalize_existing_entry(codec, field_schema, entry).is_some_and(|existing| {
                        existing.canonical_identity == normalized.canonical_identity
                    });
                if is_match {
                    if !replaced {
                        next.push(normalized.stored.clone());
                        replaced = true;
                    }
                } else {
                    next.push(entry.clone());
                }
            }
            let value = Value::Array(next);
            Ok(PatchedField {
                changed: value != *current_value,
                value,
                matched_live_target,
            })
        }
        PlayerAccessMutationOperation::Add if has_match => Ok(PatchedField {
            value: current_value.clone(),
            changed: false,
            matched_live_target,
        }),
        PlayerAccessMutationOperation::Add => {
            let mut next = current.clone();
            next.push(normalized.stored.clone());
            Ok(PatchedField {
                value: Value::Array(next),
                changed: true,
                matched_live_target: None,
            })
        }
        PlayerAccessMutationOperation::Remove if !has_match => Ok(PatchedField {
            value: current_value.clone(),
            changed: false,
            matched_live_target: None,
        }),
        PlayerAccessMutationOperation::Remove => {
            let next = current
                .iter()
                .filter(|entry| {
                    normalize_existing_entry(codec, field_schema, entry)
                        .map(|existing| {
                            existing.canonical_identity != normalized.canonical_identity
                        })
                        .unwrap_or(true)
                })
                .cloned()
                .collect::<Vec<_>>();
            Ok(PatchedField {
                value: Value::Array(next),
                changed: true,
                matched_live_target,
            })
        }
    }
}

fn normalize_existing_entry(
    codec: PlayerAccessCodec,
    field_schema: &Map<String, Value>,
    value: &Value,
) -> Option<NormalizedEntry> {
    if matches!(codec, PlayerAccessCodec::ObjectIdentity) {
        return normalize_object_identity("stored", "stored", field_schema, value).ok();
    }

    let value = value.as_str()?.trim();
    if value.is_empty() {
        return None;
    }
    if codec.is_delimited() {
        let normalized = normalize_delimited_player_access_entry(
            codec.schema_name(),
            field_schema,
            value,
            DelimitedEntryRequirement::Stored,
        )
        .ok()?;
        let live_target = if matches!(codec, PlayerAccessCodec::CsvUuidName) {
            normalized.parts.get(1)?.clone()
        } else {
            normalized.identity.clone()
        };
        return Some(NormalizedEntry {
            stored: Value::String(normalized.stored),
            canonical_identity: canonical_text(&normalized.identity),
            live_target,
        });
    }
    match codec {
        PlayerAccessCodec::Plain => Some(normalized_string_entry(value, value)),
        PlayerAccessCodec::HumanitzNetId => {
            is_stored_humanitz_identity(value).then(|| NormalizedEntry {
                stored: Value::String(value.to_owned()),
                canonical_identity: value.to_owned(),
                live_target: value.to_owned(),
            })
        }
        PlayerAccessCodec::Steam64 => (value.len() == 17
            && value.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| normalized_string_entry(value, value)),
        PlayerAccessCodec::Uint64 => {
            normalize_uint64_id(value).map(|id| normalized_string_entry(&id, &id))
        }
        PlayerAccessCodec::PipeSteam64
        | PlayerAccessCodec::CsvUuidName
        | PlayerAccessCodec::MinecraftIpCsv
        | PlayerAccessCodec::BarotraumaAccount => unreachable!(),
        PlayerAccessCodec::TerrariaBanlist => {
            let entry = normalize_terraria_banlist_entry(value)?;
            Some(normalized_string_entry(&entry, &entry))
        }
        PlayerAccessCodec::ArkAccountId => {
            let account_id = normalize_ark_account_id(value)?;
            Some(normalized_string_entry(&account_id, &account_id))
        }
        PlayerAccessCodec::DstKleiId => {
            let user_id = normalize_dst_klei_id(value)?;
            Some(normalized_string_entry(&user_id, &user_id))
        }
        PlayerAccessCodec::ValheimPlatformId => {
            let platform_id = normalize_valheim_platform_id(value)?;
            Some(normalized_string_entry(&platform_id, &platform_id))
        }
        PlayerAccessCodec::ObjectIdentity => unreachable!(),
    }
}

fn find_object_identity_keys(properties: &Map<String, Value>) -> Vec<&str> {
    let declared = properties
        .iter()
        .filter_map(|(key, value)| {
            value
                .as_object()
                .is_some_and(|property| {
                    property
                        .get("x-lsgm-player-access-identity")
                        .and_then(Value::as_bool)
                        == Some(true)
                        && property.get("type").and_then(Value::as_str) == Some("string")
                })
                .then_some(key.as_str())
        })
        .collect::<Vec<_>>();
    if !declared.is_empty() {
        return declared;
    }

    const PREFERRED_KEYS: &[&str] = &[
        "steam_id",
        "steamid",
        "steam64_id",
        "steam64",
        "account_id",
        "user_id",
        "userid",
        "player_id",
        "playerid",
        "uuid",
        "xuid",
        "id",
        "name",
    ];
    PREFERRED_KEYS
        .iter()
        .copied()
        .find_map(|preferred_key| {
            properties.iter().find_map(|(key, value)| {
                (key.eq_ignore_ascii_case(preferred_key)
                    && value
                        .as_object()
                        .and_then(|property| property.get("type"))
                        .and_then(Value::as_str)
                        == Some("string"))
                .then_some(key.as_str())
            })
        })
        .or_else(|| {
            properties.iter().find_map(|(key, value)| {
                (value
                    .as_object()
                    .and_then(|property| property.get("type"))
                    .and_then(Value::as_str)
                    == Some("string"))
                .then_some(key.as_str())
            })
        })
        .into_iter()
        .collect()
}

fn canonical_text(value: &str) -> String {
    value.trim().to_lowercase()
}

fn invalid_player_access_input(module_id: &str, field_key: &str, message: &str) -> StorageError {
    StorageError::InvalidModuleSetting {
        module_id: module_id.to_string(),
        field: field_key.to_string(),
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn field_schema(value: Value) -> Map<String, Value> {
        value.as_object().expect("schema object").clone()
    }

    #[test]
    fn humanitz_net_id_keeps_native_spelling_and_retired_ids_are_remove_only() {
        let schema = field_schema(json!({"type":"string","format":"textarea"}));
        let full = "0123456789abcdef0123456789ABCDEF|FEDCBA9876543210fedcba9876543210";
        let entry = normalize_entry(
            "humanitz",
            "admin_steam_ids",
            PlayerAccessCodec::HumanitzNetId,
            &schema,
            PlayerAccessMutationOperation::Add,
            &json!(full),
        )
        .unwrap();
        assert_eq!(entry.stored, json!(full));
        assert_eq!(entry.canonical_identity, full);
        assert_eq!(entry.live_target, full);
        let old = json!("76561198000000001");
        assert!(
            normalize_entry(
                "humanitz",
                "admin_steam_ids",
                PlayerAccessCodec::HumanitzNetId,
                &schema,
                PlayerAccessMutationOperation::Add,
                &old
            )
            .is_err()
        );
        assert!(
            normalize_entry(
                "humanitz",
                "admin_steam_ids",
                PlayerAccessCodec::HumanitzNetId,
                &schema,
                PlayerAccessMutationOperation::Remove,
                &old
            )
            .is_ok()
        );
    }

    #[test]
    fn steam64_lines_are_deduplicated_by_canonical_identity() {
        let schema = field_schema(json!({"type":"string","format":"textarea"}));
        let normalized = normalize_entry(
            "humanitz",
            "banned_player_steam_ids",
            PlayerAccessCodec::Steam64,
            &schema,
            PlayerAccessMutationOperation::Add,
            &json!("76561198000000001"),
        )
        .expect("normalize");
        let patched = patch_field_value(
            "humanitz",
            "banned_player_steam_ids",
            PlayerAccessCodec::Steam64,
            &schema,
            PlayerAccessMutationOperation::Add,
            &json!("76561198000000001"),
            &normalized,
        )
        .expect("patch");

        assert!(!patched.changed);
        assert_eq!(patched.value, json!("76561198000000001"));
    }

    #[test]
    fn pipe_codec_removes_all_metadata_variants_for_same_steam_id() {
        let schema = field_schema(json!({
            "type":"string",
            "format":"textarea",
            "x-lsgm-player-access-delimited-fields":[
                {"name":"steam_id","format":"steam64","required":true},
                {"name":"reason","format":"text","required":false,"maxLength":512}
            ]
        }));
        let normalized = normalize_entry(
            "rust",
            "banned_entries",
            PlayerAccessCodec::PipeSteam64,
            &schema,
            PlayerAccessMutationOperation::Remove,
            &json!("76561198000000001"),
        )
        .expect("normalize");
        let patched = patch_field_value(
            "rust",
            "banned_entries",
            PlayerAccessCodec::PipeSteam64,
            &schema,
            PlayerAccessMutationOperation::Remove,
            &json!("76561198000000001|first\n76561198000000002|keep\n76561198000000001|duplicate"),
            &normalized,
        )
        .expect("patch");

        assert!(patched.changed);
        assert_eq!(patched.value, json!("76561198000000002|keep"));
        assert_eq!(
            patched.matched_live_target.as_deref(),
            Some("76561198000000001")
        );
    }

    #[test]
    fn minecraft_codec_uses_uuid_for_identity_and_name_for_live_target() {
        let schema = field_schema(json!({
            "type":"string",
            "format":"textarea",
            "x-lsgm-player-access-delimited-fields":[
                {"name":"uuid","format":"minecraft_uuid","required":true},
                {"name":"name","format":"minecraft_name","required":true},
                {"name":"reason","format":"text","required":false,"maxLength":512}
            ]
        }));
        let normalized = normalize_entry(
            "minecraft",
            "banned_player_entries",
            PlayerAccessCodec::CsvUuidName,
            &schema,
            PlayerAccessMutationOperation::Add,
            &json!("A0EebC99-9C0B-4EF8-BB6D-6BB9BD380A11,Alex,LanGame"),
        )
        .expect("normalize");

        assert_eq!(
            normalized.canonical_identity,
            "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11"
        );
        assert_eq!(normalized.live_target, "Alex");
        assert_eq!(
            normalized.stored,
            json!("a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11,Alex,LanGame")
        );
    }

    #[test]
    fn minecraft_uuid_and_name_codec_enforces_renderer_boundaries() {
        let schema = field_schema(json!({
            "type":"string",
            "format":"textarea",
            "x-lsgm-player-access-delimited-fields":[
                {"name":"uuid","format":"minecraft_uuid","required":true},
                {"name":"name","format":"minecraft_name","required":true},
                {"name":"reason","format":"text","required":false,"maxLength":512}
            ]
        }));
        let normalized = normalize_entry(
            "minecraft",
            "banned_player_entries",
            PlayerAccessCodec::CsvUuidName,
            &schema,
            PlayerAccessMutationOperation::Add,
            &json!("A0EEBC999C0B4EF8BB6D6BB9BD380A11,abcdefghijklmnop"),
        )
        .expect("compact UUID and 16-character name are valid");

        assert_eq!(
            normalized.stored,
            json!("a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11,abcdefghijklmnop")
        );
        for invalid in [
            "A0EEBC999C0B4EF8BB6D6BB9BD380A1,Alex",
            "G0EEBC999C0B4EF8BB6D6BB9BD380A11,Alex",
            "A-0EEBC999C0B4EF8BB6D6BB9BD380A11,Alex",
            "A0EEBC999C0B4EF8BB6D6BB9BD380A11,abcdefghijklmnopq",
            "A0EEBC999C0B4EF8BB6D6BB9BD380A11,Player-One",
            "A0EEBC999C0B4EF8BB6D6BB9BD380A11,\u{73a9}\u{5bb6}",
            "A0EEBC999C0B4EF8BB6D6BB9BD380A11,Alex,Reason,Ignored",
        ] {
            assert!(
                normalize_entry(
                    "minecraft",
                    "banned_player_entries",
                    PlayerAccessCodec::CsvUuidName,
                    &schema,
                    PlayerAccessMutationOperation::Add,
                    &json!(invalid),
                )
                .is_err(),
                "invalid UUID/name entry was accepted: {invalid}",
            );
        }
    }

    #[test]
    fn minecraft_ip_codec_uses_shared_ip_normalization() {
        let schema = field_schema(json!({
            "type":"string",
            "format":"textarea",
            "x-lsgm-player-access-delimited-fields":[
                {"name":"ip","format":"ip","required":true},
                {"name":"reason","format":"text","required":false,"maxLength":512}
            ]
        }));
        let normalized = normalize_entry(
            "minecraft",
            "banned_ip_entries",
            PlayerAccessCodec::MinecraftIpCsv,
            &schema,
            PlayerAccessMutationOperation::Add,
            &json!("2001:0db8:0000:0000:0000:0000:0000:0001,LanGame"),
        )
        .expect("IPv6 entry");

        assert_eq!(normalized.canonical_identity, "2001:db8::1");
        assert_eq!(normalized.live_target, "2001:db8::1");
        assert_eq!(normalized.stored, json!("2001:db8::1,LanGame"));
        for invalid in [
            "999.1.1.1",
            "192.000.2.42",
            "localhost",
            "127.0.0.1:25565",
            "2001:db8::zz",
        ] {
            assert!(
                normalize_entry(
                    "minecraft",
                    "banned_ip_entries",
                    PlayerAccessCodec::MinecraftIpCsv,
                    &schema,
                    PlayerAccessMutationOperation::Add,
                    &json!(invalid),
                )
                .is_err(),
                "invalid IP entry was accepted: {invalid}",
            );
        }
    }

    #[test]
    fn terraria_codec_uses_shared_banlist_boundaries() {
        let schema = field_schema(json!({"type":"string","format":"textarea"}));
        let maximum = "a".repeat(128);
        let normalized = normalize_entry(
            "terraria",
            "banlist_entries",
            PlayerAccessCodec::TerrariaBanlist,
            &schema,
            PlayerAccessMutationOperation::Add,
            &json!(maximum),
        )
        .expect("128-byte Terraria entry");
        assert_eq!(normalized.stored, json!(maximum));

        normalize_entry(
            "terraria",
            "banlist_entries",
            PlayerAccessCodec::TerrariaBanlist,
            &schema,
            PlayerAccessMutationOperation::Add,
            &json!("你".repeat(42)),
        )
        .expect("126-byte Unicode Terraria entry");

        for invalid in [
            "a".repeat(129),
            "你".repeat(43),
            String::from("{{player}}"),
            String::from("line\nbreak"),
        ] {
            assert!(
                normalize_entry(
                    "terraria",
                    "banlist_entries",
                    PlayerAccessCodec::TerrariaBanlist,
                    &schema,
                    PlayerAccessMutationOperation::Add,
                    &json!(invalid),
                )
                .is_err(),
                "invalid Terraria entry was accepted",
            );
        }
    }

    #[test]
    fn object_identity_uses_all_declared_identity_properties_and_updates_metadata() {
        let schema = field_schema(json!({
            "type":"array",
            "x-lsgm-player-access-kind":"block",
            "items":{"type":"object","properties":{
                "platform":{
                    "type":"string",
                    "enum":["Steam","EOS"],
                    "default":"Steam",
                    "x-lsgm-player-access-identity":true
                },
                "userid":{
                    "type":"string",
                    "pattern":"^[A-Za-z0-9._:-]{1,64}$",
                    "x-lsgm-player-access-platform-field":"platform",
                    "x-lsgm-player-access-identity":true
                },
                "name":{"type":"string"},
                "unbandate":{"type":"string","default":"9999-12-31"},
                "reason":{"type":"string","default":"LanGame"}
            }}
        }));
        let normalized = normalize_entry(
            "sevendaystodie",
            "blacklist_entries",
            PlayerAccessCodec::ObjectIdentity,
            &schema,
            PlayerAccessMutationOperation::Add,
            &json!({
                "platform":"Steam",
                "userid":"76561198000000001",
                "name":"Updated"
            }),
        )
        .expect("normalize");
        assert_eq!(normalized.live_target, "76561198000000001");
        assert_eq!(normalized.stored["reason"], json!("LanGame"));
        assert_eq!(normalized.stored["unbandate"], json!("9999-12-31"));

        let trimmed = normalize_entry(
            "sevendaystodie",
            "blacklist_entries",
            PlayerAccessCodec::ObjectIdentity,
            &schema,
            PlayerAccessMutationOperation::Add,
            &json!({
                "platform":"Steam",
                "userid":"76561198000000002",
                "name":"  Alice  ",
                "reason":"  Trusted  "
            }),
        )
        .expect("trim object metadata");
        assert_eq!(trimmed.stored["name"], json!("Alice"));
        assert_eq!(trimmed.stored["reason"], json!("Trusted"));
        for key in ["name", "reason"] {
            let mut unsafe_entry = json!({
                "platform":"Steam",
                "userid":"76561198000000002",
                "name":"Alice",
                "reason":"Trusted"
            });
            unsafe_entry[key] = json!("unsafe\u{85}");
            assert!(
                normalize_entry(
                    "sevendaystodie",
                    "blacklist_entries",
                    PlayerAccessCodec::ObjectIdentity,
                    &schema,
                    PlayerAccessMutationOperation::Add,
                    &unsafe_entry,
                )
                .is_err(),
                "object metadata control character was accepted for {key}",
            );
        }

        let updated = patch_field_value(
            "sevendaystodie",
            "blacklist_entries",
            PlayerAccessCodec::ObjectIdentity,
            &schema,
            PlayerAccessMutationOperation::Add,
            &json!([{
                "platform":"Steam",
                "userid":"76561198000000001",
                "name":"Old",
                "unbandate":"2027-01-01",
                "reason":"Old reason"
            }]),
            &normalized,
        )
        .expect("update metadata");
        assert!(updated.changed);
        assert_eq!(updated.value.as_array().map(Vec::len), Some(1));
        assert_eq!(updated.value[0]["name"], json!("Updated"));
        assert_eq!(updated.value[0]["reason"], json!("LanGame"));

        let patched = patch_field_value(
            "sevendaystodie",
            "blacklist_entries",
            PlayerAccessCodec::ObjectIdentity,
            &schema,
            PlayerAccessMutationOperation::Remove,
            &json!([normalized.stored.clone()]),
            &normalized,
        )
        .expect("patch");
        assert!(patched.changed);
        assert_eq!(patched.value, json!([]));

        let eos = normalize_entry(
            "sevendaystodie",
            "blacklist_entries",
            PlayerAccessCodec::ObjectIdentity,
            &schema,
            PlayerAccessMutationOperation::Add,
            &json!({"platform":"EOS","userid":"76561198000000001"}),
        )
        .expect("EOS identity");
        assert_ne!(eos.canonical_identity, normalized.canonical_identity);
    }

    #[test]
    fn strict_string_codecs_share_renderer_normalization() {
        let schema = field_schema(json!({
            "type":"string",
            "format":"textarea",
            "x-lsgm-player-access-delimited-fields":[
                {"name":"account_id","format":"barotrauma_account","required":true},
                {"name":"display_name","format":"text","required":false,"maxLength":128}
            ]
        }));
        for (codec, input, expected) in [
            (
                PlayerAccessCodec::ArkAccountId,
                "  EOS_Account-1  ",
                "EOS_Account-1",
            ),
            (
                PlayerAccessCodec::BarotraumaAccount,
                "76561198000000001,Captain",
                "STEAM_1:1:19867136,Captain",
            ),
            (PlayerAccessCodec::DstKleiId, "ku_Player-1", "KU_Player-1"),
            (
                PlayerAccessCodec::ValheimPlatformId,
                "Xbox_12345",
                "Xbox_12345",
            ),
        ] {
            let normalized = normalize_entry(
                "fixture",
                "roster",
                codec,
                &schema,
                PlayerAccessMutationOperation::Add,
                &json!(input),
            )
            .expect("strict codec value");
            assert_eq!(normalized.stored, json!(expected));
        }

        for (codec, invalid) in [
            (PlayerAccessCodec::ArkAccountId, "bad account"),
            (PlayerAccessCodec::BarotraumaAccount, "not-steam"),
            (PlayerAccessCodec::DstKleiId, "OU_player"),
            (PlayerAccessCodec::ValheimPlatformId, "id,other"),
        ] {
            assert!(
                normalize_entry(
                    "fixture",
                    "roster",
                    codec,
                    &schema,
                    PlayerAccessMutationOperation::Add,
                    &json!(invalid),
                )
                .is_err(),
                "invalid strict codec input was accepted: {invalid}",
            );
        }
    }

    #[test]
    fn non_object_codecs_reject_unicode_controls_and_plain_honors_schema_pattern() {
        let plain_schema = field_schema(json!({
            "type":"string",
            "pattern":"^[^-\"\\r\\n]*$"
        }));
        normalize_entry(
            "necesse",
            "owner_name",
            PlayerAccessCodec::Plain,
            &plain_schema,
            PlayerAccessMutationOperation::Add,
            &json!("Alice_1"),
        )
        .expect("valid Necesse owner");
        for invalid in ["Alice-1", "Alice\"1", "Alice\u{85}"] {
            assert!(
                normalize_entry(
                    "necesse",
                    "owner_name",
                    PlayerAccessCodec::Plain,
                    &plain_schema,
                    PlayerAccessMutationOperation::Add,
                    &json!(invalid),
                )
                .is_err(),
                "invalid Necesse owner was accepted: {invalid:?}",
            );
        }
    }

    #[test]
    fn declared_steam64_separators_normalize_and_deduplicate_entries() {
        let schema = field_schema(json!({
            "type":"string",
            "format":"textarea",
            "x-lsgm-player-access-entry-separators":["newline","comma"]
        }));
        let normalized = normalize_entry(
            "vrising",
            "ban_list",
            PlayerAccessCodec::Steam64,
            &schema,
            PlayerAccessMutationOperation::Add,
            &json!("76561198000000002"),
        )
        .expect("normalize Steam64");
        let patched = patch_field_value(
            "vrising",
            "ban_list",
            PlayerAccessCodec::Steam64,
            &schema,
            PlayerAccessMutationOperation::Add,
            &json!("76561198000000001, 76561198000000002\n76561198000000001"),
            &normalized,
        )
        .expect("canonicalize comma roster");

        assert!(patched.changed);
        assert_eq!(patched.value, json!("76561198000000001\n76561198000000002"));
    }

    #[test]
    fn sync_metadata_requires_at_least_one_direct_action() {
        let schema = field_schema(json!({
            "x-lsgm-player-access-sync": {"mode":"direct"}
        }));
        let error = parse_sync_metadata("fixture", "bans", &schema)
            .expect_err("direct sync without either operation must fail");
        assert!(
            error
                .to_string()
                .contains("at least one of add_action_id or remove_action_id")
        );
    }

    #[test]
    fn roster_fields_require_explicit_codec_and_sync_metadata() {
        for shape in [
            json!({"type":"string","format":"textarea"}),
            json!({"type":"array","items":{"type":"object"}}),
        ] {
            let mut schema = field_schema(shape);
            schema.insert(String::from("x-lsgm-player-access-kind"), json!("block"));

            let codec_error = parse_codec("fixture", "blocklist", &schema)
                .expect_err("roster shape must not infer the missing codec");
            assert!(codec_error.to_string().contains(PLAYER_ACCESS_CODEC_KEY));
            let sync_error = parse_sync_metadata("fixture", "blocklist", &schema)
                .expect_err("missing sync metadata must not imply a restart policy");
            assert!(sync_error.to_string().contains(PLAYER_ACCESS_SYNC_KEY));
        }
    }

    #[test]
    fn roster_codec_rejects_empty_or_non_string_declarations() {
        for codec in [Value::Null, json!(""), json!(" "), json!(true), json!([])] {
            let schema = field_schema(json!({
                "type":"string",
                "x-lsgm-player-access-codec":codec
            }));
            let error = parse_codec("fixture", "blocklist", &schema)
                .expect_err("invalid codec declarations must fail");
            assert!(error.to_string().contains(PLAYER_ACCESS_CODEC_KEY));
        }
    }

    #[test]
    fn roster_sync_requires_an_explicit_mode() {
        for sync in [
            json!({}),
            json!({"mode":null}),
            json!({"mode":""}),
            json!({"mode":" "}),
        ] {
            let schema = field_schema(json!({"x-lsgm-player-access-sync":sync}));
            let error = parse_sync_metadata("fixture", "blocklist", &schema)
                .expect_err("empty synchronization metadata must not imply restart");
            assert!(error.to_string().contains("sync mode is missing"));
        }
    }

    #[test]
    fn roster_fields_accept_explicit_plain_restart_metadata() {
        let schema = field_schema(json!({
            "type":"string",
            "format":"textarea",
            "x-lsgm-player-access-kind":"block",
            "x-lsgm-player-access-codec":"plain",
            "x-lsgm-player-access-sync":{"mode":"restart"}
        }));

        assert_eq!(
            parse_codec("fixture", "blocklist", &schema).expect("codec"),
            PlayerAccessCodec::Plain
        );
        assert_eq!(
            parse_sync_metadata("fixture", "blocklist", &schema)
                .expect("sync")
                .mode,
            PlayerAccessSyncMode::Restart
        );
    }
}

#[cfg(test)]
#[path = "player_access_operation_sync_tests.rs"]
mod operation_sync_tests;

#[cfg(test)]
#[path = "player_access_uint64_tests.rs"]
mod uint64_tests;
