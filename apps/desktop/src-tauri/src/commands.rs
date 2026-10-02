use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
#[cfg(test)]
use std::net::UdpSocket;
use std::net::{IpAddr, Shutdown, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(test)]
use app_core::DEFAULT_LANGAME_SERVER_FILES_ROOT;
use app_core::{
    ActiveInstanceRun, AppPathSettingsInput, AppSettings, AppState, BackgroundJob, CpuCoreSnapshot,
    CreateInstanceInput, DEFAULT_LANGAME_INSTANCES_ROOT, InsertInstanceBroadcastEventInput,
    InstallSource, InstallState, InstanceBackupRestoreResult, InstanceBackupResult,
    InstanceBroadcastEvent, InstanceBroadcastPolicy, InstanceDetails, InstanceProvisioning,
    InstanceRuntimeOverview, InstanceStatus, InstanceSummary, JobKind, JobStatus, LaunchPlan,
    LogTailSnapshot, MemoryModuleSnapshot, ModuleBindAddressMode, ModuleBindAddressSpec,
    ModuleDetails, ModuleModsSpec, ModulePlayerActionSpec, ModulePlayerQuerySpec,
    ModuleShutdownSpec, ModuleSummary, NetworkAdapterSnapshot, OperatorServiceProbe,
    PalworldOperatorServiceProbe, PalworldOperatorSnapshot, PortBinding, ProcessIdentity,
    ProcessLaunchPlan, ProcessWindowPolicy, RuntimeDiagnosticSignal, RuntimePerformanceApplication,
    RuntimePerformancePolicy, RuntimePerformanceSnapshot, RuntimeProcessPerformanceSnapshot,
    RuntimeStartupQueueSnapshot, RuntimeStartupSchedule, RuntimeWindowSnapshot,
    RuntimeWindowSuppressionAttempt, RuntimeWindowSuppressionResult, SevenDaysOperatorSnapshot,
    StartInstanceResult, StopInstanceResult, StorageStatus, SystemSnapshot,
    UpdateInstanceBroadcastPolicyInput, UpdateInstanceInput,
};
use app_modules::{ModuleDescriptor, discover_modules};
use app_platform_win::{
    BindAddressCandidate, HostMetricsSnapshot, OverlayFamily, ProcessNetworkEndpoint,
    WindowInspectionTarget, WindowsHostMonitor, WindowsPlatform, read_process_memory_metrics,
};
#[cfg(test)]
use app_runtime::spawn_launch_plan;
use app_runtime::{
    ManagedInstance, ManagedProcess, StoppedManagedProcess, apply_runtime_performance_policy,
    build_launch_plan_with_override, inspect_process_identity, kill_process_by_pid,
    process_identities_match, process_is_running, process_matches_identity,
    resolve_runtime_performance_policy_for_instance,
    resolve_runtime_performance_policy_with_preview_for_instance, stabilize_spawned_process,
    stop_managed_instance, stop_spawned_process,
};
use app_steamcmd::{
    InstallProgressUpdate, ModuleInstallResult, SteamCmdStatus, SteamWorkshopDownloadResult,
    SteamWorkshopInstallationSnapshot, inspect_workshop_items,
    probe_module_install_state_with_override, remove_managed_steamcmd,
    steamcmd_status as read_steamcmd_status,
};
#[cfg(test)]
use app_storage::create_instance;
use app_storage::{
    ActiveInstanceRunEntry, GameInstallSyncRecord, StartedInstanceProcess, StorageBootstrap,
    bootstrap_storage,
    create_instance_auto_stop_backup as create_instance_auto_stop_backup_snapshot,
    create_instance_backup as create_instance_backup_snapshot,
    delete_instance_backup as delete_instance_backup_snapshot, initialize_database,
    insert_instance_broadcast_event, list_active_instance_runs,
    list_instance_backups as list_instance_backups_snapshot,
    list_instance_broadcast_events as list_instance_broadcast_events_snapshot, list_instances,
    mark_instance_process_started_with_identity, mark_instance_process_stopped,
    materialize_instance_configuration, normalize_query_host, read_active_instance_run,
    read_instance_broadcast_policy as read_instance_broadcast_policy_snapshot,
    read_instance_details, read_instance_log_document, read_instance_runtime_overview,
    read_log_path_snapshot, rename_instance_backup as rename_instance_backup_snapshot,
    resolve_module_install_root, restore_instance_backup as restore_instance_backup_snapshot,
    save_app_settings, sync_game_installs, sync_modules, update_instance,
    update_instance_if_current,
    upsert_instance_broadcast_policy as upsert_instance_broadcast_policy_snapshot,
};
use serde::de::Error as DeError;
use serde::{Deserialize, Serialize, de::Deserializer};
use serde_json::{Value, json};
use tauri::{Emitter, Manager};

use crate::assistant::{
    AssistantProviderSettings, AssistantRunInput, AssistantRunOutput, AssistantSecretDescriptor,
    AssistantSecretStatus, delete_secret, list_ollama_models, read_secret_status,
    redact_assistant_provider_text, run_assistant, run_assistant_with_system_prompt, store_secret,
    summarize_text,
};
use crate::dst_mods::{DstModConfigurationSpec, read_dst_mod_configuration_specs_with_roots};
use crate::project_zomboid_mods::{
    ProjectZomboidWorkshopModsSnapshot,
    read_project_zomboid_workshop_mods_snapshot as read_local_project_zomboid_workshop_mods_snapshot,
};
use crate::runtime_log_stream::{RuntimeLogTailState, read_runtime_log_delta_bounded};
use crate::runtime_transport::{
    battleye_rcon_exec, runtime_command_response_text, source_rcon_exec, telnet_exec,
    websocket_rcon_exec,
};
#[cfg(test)]
use crate::runtime_transport::{
    battleye_rcon_read_command_response, battleye_rcon_read_packet, battleye_rcon_wrap_payload,
    crc32_ieee, source_rcon_read_command_response, source_rcon_read_packet,
    source_rcon_write_packet, telnet_text_from_bytes, websocket_read_frame,
    websocket_read_http_headers, websocket_read_text_message,
};
#[path = "commands_ark_cluster_backups.rs"]
pub(crate) mod commands_ark_cluster_backups;
#[path = "commands_ark_clusters.rs"]
pub(crate) mod commands_ark_clusters;
#[path = "commands_ark_tools.rs"]
pub(crate) mod commands_ark_tools;
#[path = "commands_assistant_ops.rs"]
pub(crate) mod commands_assistant_ops;
#[path = "commands_autostart.rs"]
pub(crate) mod commands_autostart;
#[path = "commands_broadcast.rs"]
pub(crate) mod commands_broadcast;
#[path = "commands_configuration_icons.rs"]
pub(crate) mod commands_configuration_icons;
#[path = "commands_dst_import.rs"]
mod commands_dst_import;
#[path = "commands_dst_import_operation.rs"]
mod commands_dst_import_operation;
#[path = "commands_dst_import_validation.rs"]
mod commands_dst_import_validation;
#[path = "commands_dst_world_state.rs"]
pub(crate) mod commands_dst_world_state;
#[path = "commands_external_url.rs"]
mod commands_external_url;
#[path = "commands_global_player_counts.rs"]
mod commands_global_player_counts;
#[path = "commands_install_progress.rs"]
pub(crate) mod commands_install_progress;
#[path = "commands_instance_isolation.rs"]
pub(crate) mod commands_instance_isolation;
#[path = "commands_instance_network.rs"]
pub(crate) mod commands_instance_network;
#[path = "commands_instance_stop.rs"]
mod commands_instance_stop;
#[path = "commands_live_players.rs"]
pub(crate) mod commands_live_players;
#[path = "commands_local_paths.rs"]
mod commands_local_paths;
#[path = "commands_managed_save.rs"]
pub(crate) mod commands_managed_save;
#[path = "commands_manual_player_actions.rs"]
pub(crate) mod commands_manual_player_actions;
#[path = "commands_mod_staging.rs"]
mod commands_mod_staging;
#[path = "commands_mods.rs"]
mod commands_mods;
#[path = "commands_workshop_collection_removal.rs"]
pub(crate) mod commands_workshop_collection_removal;
#[path = "dst_import_mods.rs"]
mod dst_import_mods;
pub use commands_workshop_collection_removal::remove_instance_workshop_collection;
#[path = "commands_module_mutation_locks.rs"]
pub(super) mod commands_module_mutation_locks;
#[path = "commands_program_storage.rs"]
pub(crate) mod commands_program_storage;
pub use commands_program_storage::update_instance_program;
#[path = "commands_instance_reconciliation.rs"]
mod commands_instance_reconciliation;
#[path = "commands_instance_retirement.rs"]
pub(crate) mod commands_instance_retirement;
#[path = "commands_player_access.rs"]
pub(crate) mod commands_player_access;
#[path = "commands_player_counts.rs"]
mod commands_player_counts;
#[path = "commands_program_inventory.rs"]
pub(crate) mod commands_program_inventory;
#[path = "commands_runtime_actions.rs"]
mod commands_runtime_actions;
#[path = "commands_runtime_ark.rs"]
mod commands_runtime_ark;
#[path = "commands_runtime_lifecycle.rs"]
pub(crate) mod commands_runtime_lifecycle;
#[path = "commands_runtime_observability.rs"]
mod commands_runtime_observability;
#[path = "commands_runtime_prestart_update.rs"]
mod commands_runtime_prestart_update;
#[path = "commands_theforest_control.rs"]
mod commands_theforest_control;
#[path = "commands_runtime_exit.rs"]
mod runtime_exit;
use commands_runtime_prestart_update::*;

#[path = "commands_runtime_supervision.rs"]
pub(crate) mod commands_runtime_supervision;
#[path = "commands_stdin_dispatch.rs"]
pub(crate) mod commands_stdin_dispatch;
#[path = "commands_steamcmd_errors.rs"]
mod commands_steamcmd_errors;
#[path = "commands_steamcmd_preparation.rs"]
pub(crate) mod commands_steamcmd_preparation;
#[path = "commands_storage.rs"]
pub(crate) mod commands_storage;
#[path = "commands_storage_lifecycle.rs"]
mod commands_storage_lifecycle;
#[path = "commands_storage_management.rs"]
pub(crate) mod commands_storage_management;
pub use commands_instance_retirement::{archive_instance_record, delete_instance_record};
#[path = "dst_save_validation.rs"]
mod dst_save_validation;
#[path = "steam_store_about.rs"]
mod steam_store_about;
use crate::state::{
    DesktopState, RuntimeRestartScheduleEntry, RuntimeRestartScheduleRequest,
    RuntimeStartReservationAttempt, RuntimeStartReservationLease, RuntimeStartupReservation,
    StorageContextOperationGuard, StorageContextTaskLease, spawn_blocking_storage_context_task,
    spawn_storage_context_task,
};
use crate::steam_workshop::{
    SteamWorkshopLookupItem, SteamWorkshopSearchResult, lookup_public_workshop_items,
    search_public_workshop_items,
};
pub use commands_assistant_ops::{
    assistant_cancel_turn, assistant_clear_secret, assistant_confirm_operation,
    assistant_create_conversation, assistant_delete_conversation, assistant_execute_operation,
    assistant_get_conversation_state, assistant_list_conversations, assistant_list_ollama_models,
    assistant_resume_conversation, assistant_run, assistant_secret_status, assistant_store_secret,
    send_instance_gm_command, send_instance_runtime_command,
};
use commands_assistant_ops::{
    clear_assistant_pending_operations, dispatch_battleye_rcon_command,
    dispatch_source_rcon_command, dispatch_telnet_command, dispatch_websocket_rcon_command,
};
pub use commands_broadcast::{
    generate_instance_broadcast, list_instance_broadcast_events, read_instance_broadcast_policy,
    send_instance_broadcast, update_instance_broadcast_policy,
};
pub use commands_configuration_icons::read_module_configuration_icons;
use commands_dst_import::*;
use commands_dst_import_validation::*;
pub use commands_install_progress::cancel_installation_job;
pub use commands_instance_isolation::read_instance_isolation;
pub use commands_live_players::{
    execute_instance_player_action, read_instance_live_players, refresh_instance_live_players,
};
pub use commands_manual_player_actions::execute_instance_manual_player_action;
use commands_mods::{
    append_mod_install_note, download_steam_workshop_items_inner,
    install_manual_mod_references_inner, install_module_game_inner, module_mods_spec_from_manifest,
    normalize_manual_mod_references, prepare_dst_import_workshop_items,
    read_manual_mod_inventory_inner, resolve_manual_mod_references_inner,
    split_manual_mod_source_path_inputs, stage_manual_mod_files_inner, uninstall_module_game_inner,
    validate_module_game_inner,
};
#[cfg(test)]
use commands_mods::{
    cleanup_downloaded_mod_source, download_modrinth_mod_sources, extract_curseforge_mod_slug,
    extract_curseforge_project_id_from_reference, extract_modrinth_project_reference,
    extract_thunderstore_package_reference, manual_mod_path_stats, select_modrinth_download_file,
    select_thunderstore_payload_prefix,
};
pub use commands_player_access::apply_instance_player_access_mutation;
#[cfg(not(windows))]
pub use commands_runtime_lifecycle::request_app_restart_shutdown;
use commands_runtime_lifecycle::{
    RuntimeTransportRequest, dispatch_instance_runtime_transport, normalize_runtime_command_input,
    pending_start_console_log_snapshot, start_instance_process_after_reconcile,
};
pub use commands_runtime_lifecycle::{
    app_exit_shutdown_completed, preview_instance_launch, request_app_exit_shutdown,
    start_instance_process, stop_instance_process,
};
use commands_runtime_observability::{
    RuntimeRestartCandidate, build_palworld_operator_snapshot,
    build_runtime_performance_diagnostic, build_runtime_performance_snapshot,
    build_runtime_startup_queue_snapshot, build_sevendaystodie_operator_snapshot,
    collect_system_snapshot, extract_instance_player_capacity, json_bool,
    lightweight_system_snapshot, load_module_runtime_capability_map, now_unix_ms,
    runtime_performance_state_diagnostic_signal, runtime_restart_policy_from_settings,
    summarize_active_runtime_entries,
};
use commands_runtime_supervision::*;
pub use commands_runtime_supervision::{
    bind_address_candidates, log_frontend_event, overlay_families,
};
use commands_steamcmd_errors::{steamcmd_error_excerpt, steamcmd_error_message};
pub use commands_steamcmd_preparation::{
    cancel_steamcmd_preparation, ensure_steamcmd_ready, read_steamcmd_prepare_progress,
};
use commands_storage::create_instance_record_inner;
pub use commands_storage::{
    create_instance_backup, create_instance_record, delete_instance_backup, ensure_storage_ready,
    import_dontstarve_world_data, list_instance_backups, list_instances_from_storage,
    read_instance_details_from_storage, read_instance_log_document_from_storage,
    read_instance_runtime_overview_from_storage, read_instance_runtime_window_snapshot,
    read_palworld_operator_snapshot, read_sevendaystodie_operator_snapshot, rename_instance_backup,
    restore_instance_backup, suppress_instance_runtime_windows, sync_modules_to_storage,
    update_instance_record_if_current,
};
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;
const RUNTIME_WINDOW_MANUAL_SUPPRESSION_ACTION: &str = "instance.runtime_window.manual_suppression";
const RUNTIME_WINDOW_AUTO_SUPPRESSION_ACTION: &str = "instance.runtime_window.auto_suppression";
const RUNTIME_WINDOW_MANUAL_SUPPRESSION_FAILED_ACTION: &str =
    "instance.runtime_window.manual_suppression_failed";
const RUNTIME_WINDOW_AUTO_SUPPRESSION_FAILED_ACTION: &str =
    "instance.runtime_window.auto_suppression_failed";
const RUNTIME_PERFORMANCE_REFRESH_INTERVAL: Duration = Duration::from_secs(30);
const RUNTIME_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const SYSTEM_SNAPSHOT_CACHE_TTL: Duration = Duration::from_secs(15);
const BIND_ADDRESS_CACHE_TTL: Duration = Duration::from_secs(30);
const STARTUP_WINDOW_GUARD_ATTEMPTS: usize = 150;
const STARTUP_WINDOW_GUARD_INTERVAL_MS: u64 = 200;
const DESKTOP_APP_LOG_TAIL_LINE_LIMIT: usize = 256;
const DESKTOP_APP_LOG_TAIL_BYTE_LIMIT: u64 = 64 * 1024;
const MANUAL_MOD_SITE_DOWNLOAD_MAX_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateInstanceBroadcastInput {
    pub instance_id: String,
    pub settings: AssistantProviderSettings,
    pub intent: String,
    #[serde(default)]
    pub tone: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub rule_id: Option<String>,
    #[serde(default)]
    pub initiator: Option<String>,
    #[serde(default)]
    pub policy_snapshot_json: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateInstanceBroadcastOutput {
    pub message: String,
    pub provider: String,
    pub model: String,
    pub endpoint_url: String,
    pub event: InstanceBroadcastEvent,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendInstanceBroadcastInput {
    pub instance_id: String,
    pub message: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub rule_id: Option<String>,
    #[serde(default)]
    pub ai_provider: Option<String>,
    #[serde(default)]
    pub ai_model: Option<String>,
    #[serde(default)]
    pub initiator: Option<String>,
    #[serde(default)]
    pub policy_snapshot_json: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendInstanceBroadcastOutput {
    pub event: InstanceBroadcastEvent,
    pub action_id: String,
    pub transport: String,
    pub command_preview: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssistantRequestInput {
    pub settings: AssistantProviderSettings,
    #[serde(default)]
    pub conversation_id: Option<String>,
    pub prompt: String,
    #[serde(skip)]
    pub prior_requests: Vec<String>,
    #[serde(skip)]
    pub conversation_messages: Vec<AssistantConversationMessage>,
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub selected_instance_id: Option<String>,
    #[serde(default)]
    pub selected_module_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(
    tag = "role",
    content = "content",
    rename_all = "lowercase",
    deny_unknown_fields
)]
pub enum AssistantConversationMessage {
    User(String),
    Assistant(String),
}

// Only the core constructs an operation scope after interpreting a request.
// IPC callers cannot supply goals or change a continuation's task contract.
#[derive(Debug, Clone)]
pub(super) struct AssistantExecuteOperationInput {
    pub settings: AssistantProviderSettings,
    pub prompt: String,
    pub task: commands_assistant_ops::AssistantTaskRequest,
    pub context: Option<String>,
    pub selected_instance_id: Option<String>,
    pub selected_module_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantConfirmOperationInput {
    pub settings: AssistantProviderSettings,
    #[serde(default)]
    pub conversation_id: Option<String>,
    pub confirmation_token: String,
    pub plan_summary: String,
    #[serde(default)]
    pub continue_task: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssistantOperationAction {
    #[serde(alias = "start")]
    #[serde(alias = "startServer")]
    StartServer,
    StopServer,
    RestartServer,
    CreateBackup,
    RestoreBackup,
    CreateServer,
    #[serde(alias = "install")]
    #[serde(alias = "installServer")]
    InstallServer,
    ValidateServer,
    #[serde(alias = "applyBeginnerConfig")]
    ApplyBeginnerConfig,
    #[serde(alias = "customizeConfig")]
    CustomizeConfig,
    PatchInstanceText,
    PatchInstanceFiles,
    #[serde(alias = "installFunMod")]
    InstallFunMod,
    #[serde(alias = "installSiteMod")]
    InstallSiteMod,
    #[serde(alias = "repairPorts")]
    RepairPorts,
    #[serde(alias = "runGmCommand")]
    RunGmCommand,
    #[serde(alias = "broadcast")]
    Broadcast,
    #[serde(other)]
    None,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AssistantOperationPlan {
    pub action: AssistantOperationAction,
    #[serde(default)]
    pub backup_id: Option<String>,
    #[serde(default)]
    pub task_requirements: Option<commands_assistant_ops::AssistantTaskRequirements>,
    #[serde(default, alias = "instance_id")]
    pub instance_id: Option<String>,
    #[serde(default, alias = "module_id")]
    pub module_id: Option<String>,
    #[serde(default, alias = "settings_patch")]
    pub settings_patch: Option<Value>,
    #[serde(default)]
    pub text_patch: Option<app_storage::InstanceTextPatch>,
    #[serde(default)]
    pub file_patches: Vec<app_storage::InstanceFileEdits>,
    #[serde(default, alias = "port_patch")]
    pub port_patch: Option<Value>,
    #[serde(
        default,
        alias = "workshop_item_ids",
        deserialize_with = "deserialize_assistant_workshop_item_ids"
    )]
    pub workshop_item_ids: Vec<String>,
    #[serde(default, alias = "mod_references")]
    pub mod_references: Vec<String>,
    #[serde(default, alias = "source_paths")]
    pub source_paths: Vec<String>,
    #[serde(default, alias = "broadcast_intent")]
    pub broadcast_intent: Option<String>,
    #[serde(default, alias = "runtime_commands")]
    pub runtime_commands: Vec<String>,
    #[serde(default, alias = "process_key")]
    pub process_key: Option<String>,
    #[serde(default)]
    pub transport: Option<String>,
    #[serde(default, alias = "port_name")]
    pub port_name: Option<String>,
    #[serde(default, alias = "password_setting_key")]
    pub password_setting_key: Option<String>,
    #[serde(default, alias = "enabled_setting_key")]
    pub enabled_setting_key: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

fn deserialize_assistant_workshop_item_ids<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Value::deserialize(deserializer)?;
    assistant_workshop_item_ids(raw).map_err(DeError::custom)
}

fn assistant_workshop_item_ids(raw: Value) -> Result<Vec<String>, String> {
    let entries = match raw {
        Value::Array(entries) => entries,
        Value::String(value) => {
            let mut split = value
                .split(',')
                .map(|entry| entry.trim())
                .filter(|entry| !entry.is_empty())
                .map(|entry| Value::String(entry.to_string()))
                .collect::<Vec<_>>();
            if split.is_empty() && !value.trim().is_empty() {
                split.push(Value::String(value.trim().to_string()));
            }
            split
        }
        Value::Null => return Ok(Vec::new()),
        _ => {
            return Err("workshopItemIds must be an array or string ID list".into());
        }
    };

    entries
        .into_iter()
        .map(|entry| match entry {
            Value::String(value) => {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    return Err("workshopItemIds entries must be non-empty string or number".into());
                }
                Ok(trimmed.to_string())
            }
            Value::Number(value) => Ok(value.to_string()),
            _ => Err("workshopItemIds entries must be string or number".into()),
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantOperationConfigDocument {
    pub path: String,
    pub content: String,
    pub truncated: bool,
}

#[derive(Debug, Clone)]
struct AssistantSettingsPatchMerge {
    pub settings: Value,
    pub applied_keys: Vec<String>,
    pub rejected_keys: Vec<String>,
}

#[derive(Debug, Clone)]
struct AssistantPortPatchMerge {
    pub ports: Vec<PortBinding>,
    pub applied_names: Vec<String>,
    pub rejected_names: Vec<String>,
}

#[derive(Debug, Clone)]
struct AssistantTextListSettingMerge {
    pub settings: Value,
    pub applied_keys: Vec<String>,
    pub added_values: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantExecuteOperationOutput {
    pub completed_operations: Vec<commands_assistant_ops::AssistantCompletedOperation>,
    pub conversation_id: Option<String>,
    pub conversation_revision: Option<u64>,
    pub continuation: Option<commands_assistant_ops::AssistantRunPause>,
    pub handled: bool,
    pub action: AssistantOperationAction,
    pub message: String,
    pub requires_confirmation: bool,
    pub confirmation_token: Option<String>,
    pub confirmation_expires_at_unix_ms: Option<u64>,
    pub plan_summary: Option<String>,
    pub instance_id: Option<String>,
    pub module_id: Option<String>,
    pub applied_settings_keys: Vec<String>,
    pub rejected_settings_keys: Vec<String>,
    pub applied_port_names: Vec<String>,
    pub rejected_port_names: Vec<String>,
    pub workshop_item_ids: Vec<String>,
    pub mod_references: Vec<String>,
    pub resolved_mod_ids: Vec<String>,
    pub source_paths: Vec<String>,
    pub runtime_commands: Vec<String>,
    pub runtime_response_texts: Vec<String>,
    pub config_document_count: usize,
    pub file_change_preview: Option<app_storage::InstanceTextPatchPreview>,
    pub file_change_result: Option<app_storage::InstanceFilePatchResult>,
    pub file_change_previews: Vec<app_storage::InstanceFileEditsPreview>,
    pub file_changes_result: Option<app_storage::InstanceFilePatchesResult>,
    pub assistant_reason: Option<String>,
    pub verification: Option<commands_assistant_ops::AssistantOperationVerification>,
    pub task: Option<commands_assistant_ops::AssistantTaskReceipt>,
    pub follow_up: Option<Box<AssistantExecuteOperationOutput>>,
    #[serde(skip)]
    runtime_start: Option<StartInstanceResult>,
    #[serde(skip)]
    runtime_start_failure: Option<commands_runtime_lifecycle::RuntimeStartFailure>,
    #[serde(skip)]
    restored_instance: Option<Box<InstanceDetails>>,
}

pub fn spawn_runtime_heartbeat(app_handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut heartbeat = tokio::time::interval(RUNTIME_HEARTBEAT_INTERVAL);
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            heartbeat.tick().await;
            let state = app_handle.state::<DesktopState>();
            if !state.is_storage_ready() {
                continue;
            }

            if let Err(error) = reconcile_runtime_state(&state).await
                && let Ok(storage) = bootstrap_storage()
            {
                append_desktop_app_log(
                    &storage,
                    "warn",
                    "runtime.heartbeat.reconcile_failed",
                    &error,
                    json!({
                        "interval_ms": RUNTIME_HEARTBEAT_INTERVAL.as_millis(),
                    }),
                );
            }
            if let Err(error) = commands_autostart::spawn_pending_autostart(&app_handle)
                && let Ok(storage) = bootstrap_storage()
            {
                append_desktop_app_log(
                    &storage,
                    "error",
                    "instance.autostart.dispatch_failed",
                    &error,
                    json!({}),
                );
            }
        }
    });
}

#[derive(Debug, Serialize)]
pub struct BootstrapResponse {
    pub booted_at_unix_ms: u128,
    pub state: AppState,
}

#[derive(Debug, Serialize)]
pub struct SteamNewsItem {
    pub gid: String,
    pub title: String,
    pub url: String,
    pub author: Option<String>,
    pub feed_label: Option<String>,
    pub excerpt: String,
    pub published_at_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SteamReviewSummary {
    pub app_id: u64,
    pub review_score: Option<i64>,
    pub review_score_desc: String,
    pub total_positive: u64,
    pub total_negative: u64,
    pub total_reviews: u64,
    pub positive_percent: u64,
    pub source_url: String,
}

#[derive(Debug, Deserialize)]
struct SteamNewsEnvelope {
    appnews: SteamNewsAppNews,
}

#[derive(Debug, Deserialize)]
struct SteamNewsAppNews {
    newsitems: Vec<SteamNewsApiItem>,
}

#[derive(Debug, Deserialize)]
struct SteamNewsApiItem {
    gid: String,
    title: String,
    url: String,
    author: Option<String>,
    contents: String,
    feedlabel: Option<String>,
    date: u64,
}

#[derive(Debug, Deserialize)]
struct SteamAppReviewsEnvelope {
    query_summary: Option<SteamAppReviewsQuerySummary>,
}

#[derive(Debug, Clone, Deserialize)]
struct SteamAppReviewsQuerySummary {
    #[serde(default)]
    review_score: Option<i64>,
    #[serde(default)]
    review_score_desc: Option<String>,
    #[serde(default)]
    total_positive: Option<u64>,
    #[serde(default)]
    total_negative: Option<u64>,
    #[serde(default)]
    total_reviews: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DstWorldImportResult {
    pub instance_id: String,
    pub source_cluster_path: String,
    pub target_cluster_path: String,
    pub safeguard_path: String,
    pub imported_master: bool,
    pub imported_caves: bool,
    pub imported_shards: Vec<String>,
    pub imported_workshop_mod_ids: Vec<String>,
    pub copied_file_count: usize,
    pub copied_total_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManualModStageItem {
    pub source_path: String,
    pub target_path: String,
    pub status: String,
    pub message: Option<String>,
    pub file_count: usize,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManualModStageResult {
    pub instance_id: String,
    pub module_id: String,
    pub source_label: String,
    pub target_label: String,
    pub target_path: String,
    pub affected_root_names: Vec<String>,
    pub items: Vec<ManualModStageItem>,
    pub copied_file_count: usize,
    pub copied_total_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManualModInventoryItem {
    pub name: String,
    pub path: String,
    pub item_type: String,
    pub inferred_id: Option<String>,
    pub file_count: usize,
    pub total_bytes: u64,
    pub modified_unix_ms: Option<u128>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManualModInventoryResult {
    pub instance_id: String,
    pub module_id: String,
    pub source_label: String,
    pub target_label: String,
    pub target_path: String,
    pub target_exists: bool,
    pub items: Vec<ManualModInventoryItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManualModReferenceItem {
    pub reference: String,
    pub status: String,
    pub resolved_id: Option<String>,
    pub title: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManualModReferenceResolveResult {
    pub instance_id: String,
    pub module_id: String,
    pub source_label: String,
    pub setting_key: String,
    pub setting_label: String,
    pub items: Vec<ManualModReferenceItem>,
    pub resolved_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ModuleModsManifest {
    mods: Option<ModuleModsSpec>,
}

#[derive(Debug, Deserialize)]
struct CurseToolsSearchResponse {
    #[serde(default)]
    data: Vec<CurseToolsMod>,
}

#[derive(Debug, Deserialize)]
struct CurseToolsMod {
    id: u64,
    name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ThunderstorePackageReference {
    namespace: String,
    package: String,
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ThunderstorePackageMetadata {
    full_name: String,
    latest: ThunderstorePackageVersion,
    community_listings: Vec<ThunderstoreCommunityListing>,
}

#[derive(Debug, Deserialize)]
struct ThunderstoreCommunityListing {
    community: String,
}

#[derive(Debug, Deserialize)]
struct ThunderstorePackageVersion {
    version_number: String,
    download_url: String,
    dependencies: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModrinthProjectReference {
    project: String,
    loaders: Vec<String>,
    game_versions: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ModrinthProjectVersion {
    project_id: String,
    name: String,
    version_number: String,
    #[serde(default)]
    version_type: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    files: Vec<ModrinthVersionFile>,
    dependencies: Vec<ModrinthDependency>,
}

#[derive(Debug, Deserialize)]
struct ModrinthDependency {
    dependency_type: String,
    project_id: Option<String>,
    version_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct ModrinthVersionFile {
    url: String,
    filename: String,
    size: u64,
    hashes: HashMap<String, String>,
    #[serde(default)]
    primary: bool,
    #[serde(default)]
    file_type: Option<String>,
}

#[derive(Debug, Clone)]
struct DownloadedManualModSource {
    path: PathBuf,
    label: String,
    identity: OnlineModIdentity,
}

#[derive(Debug, Clone)]
struct OnlineModIdentity {
    provider: String,
    project: String,
    historical_names: Vec<String>,
    version: String,
    dependencies: Vec<String>,
}

struct ResolvedManualModTarget {
    instance_id: String,
    module_id: String,
    source_label: String,
    target_label: String,
    target_path: PathBuf,
    accepts: Vec<String>,
    id_strategy: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstanceRuntimeCommandResult {
    pub instance_id: String,
    pub process_key: String,
    pub display_name: String,
    pub pid: u32,
    pub command: String,
    pub response_text: Option<String>,
    pub write_confirmation_pending: bool,
    pub submitted_at_unix_ms: u128,
}

#[tauri::command]
pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[tauri::command]
pub async fn bootstrap(
    app: tauri::AppHandle,
    state: tauri::State<'_, DesktopState>,
    include_system_snapshot: Option<bool>,
) -> Result<BootstrapResponse, String> {
    let mut latest = state
        .app_state
        .read()
        .map_err(|_| String::from("desktop state lock poisoned"))?
        .clone();
    let booted_at_unix_ms = state
        .booted_at
        .duration_since(UNIX_EPOCH)
        .map_err(|_| String::from("failed to compute boot timestamp"))?
        .as_millis();

    if include_system_snapshot.unwrap_or(false) {
        latest.snapshot = collect_system_snapshot(&app, &state, &latest).await?;
    } else {
        let cached = state
            .system_snapshot_cache
            .lock()
            .map_err(|_| String::from("system snapshot cache lock poisoned"))?
            .latest();
        latest.snapshot = lightweight_system_snapshot(&latest, cached);
    }

    Ok(BootstrapResponse {
        booted_at_unix_ms,
        state: latest,
    })
}

#[tauri::command]
pub fn read_background_jobs(
    state: tauri::State<'_, DesktopState>,
) -> Result<Vec<BackgroundJob>, String> {
    Ok(state
        .app_state
        .read()
        .map_err(|_| String::from("desktop state lock poisoned"))?
        .jobs
        .clone())
}

#[tauri::command]
pub async fn refresh_modules(
    state: tauri::State<'_, DesktopState>,
    include_preserved_program_counts: Option<bool>,
) -> Result<Vec<ModuleSummary>, String> {
    let _storage_context_operation =
        state.begin_storage_context_operation("module state refresh")?;
    let requested = state
        .app_state
        .read()
        .map_err(|_| String::from("desktop state lock poisoned"))?
        .modules
        .clone();
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    let modules =
        discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
    let include_counts = include_preserved_program_counts.unwrap_or(true);
    let mut summaries = if include_counts {
        load_module_summaries_with_install_state(&storage, &modules).await?
    } else {
        load_module_installation_summaries(&storage, &modules).await?
    };

    let mut state_guard = state
        .app_state
        .write()
        .map_err(|_| String::from("desktop state lock poisoned"))?;
    merge_module_refresh(
        &mut summaries,
        &state_guard.modules,
        &requested,
        include_counts,
    );
    state_guard.modules = summaries.clone();

    Ok(summaries)
}

fn merge_module_refresh(
    incoming: &mut [ModuleSummary],
    current: &[ModuleSummary],
    requested: &[ModuleSummary],
    include_counts: bool,
) {
    for summary in incoming {
        let Some(latest) = current.iter().find(|item| item.id == summary.id) else {
            continue;
        };
        if !include_counts {
            summary.instance_program_count = latest.instance_program_count;
            summary.archived_program_count = latest.archived_program_count;
        }
        // Installation tasks own transient states and completions which happened
        // while this probe waited for files or the archive inventory.
        if matches!(
            latest.install_state,
            InstallState::Installing | InstallState::Updating | InstallState::Uninstalling
        ) || requested
            .iter()
            .find(|item| item.id == summary.id)
            .is_some_and(|before| before.install_state != latest.install_state)
        {
            summary.install_state = latest.install_state.clone();
        }
    }
}

#[tauri::command]
pub async fn read_module_details(
    module_id: String,
    include_preserved_program_counts: Option<bool>,
) -> Result<ModuleDetails, String> {
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    let descriptors =
        discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
    let descriptor = find_descriptor(&descriptors, &module_id)?;

    load_module_details_with_install_state(
        &storage,
        descriptor,
        include_preserved_program_counts.unwrap_or(true),
    )
    .await
}

#[tauri::command]
pub async fn lookup_steam_workshop_items(
    ids: Vec<String>,
    locale: Option<String>,
) -> Result<Vec<SteamWorkshopLookupItem>, String> {
    lookup_public_workshop_items(
        ids,
        app_network::SourcePreference::from_locale(locale.as_deref()),
    )
    .await
}

#[tauri::command]
pub async fn read_steam_workshop_item_details(
    id: String,
    locale: Option<String>,
) -> Result<SteamWorkshopLookupItem, String> {
    crate::steam_workshop::read_public_workshop_item_details(id, locale.as_deref()).await
}

#[tauri::command]
pub async fn search_steam_workshop_items(
    app_id: u32,
    query: Option<String>,
    sort: Option<String>,
    page: Option<u32>,
    locale: Option<String>,
    browse_kind: Option<String>,
) -> Result<SteamWorkshopSearchResult, String> {
    search_public_workshop_items(app_id, query, sort, page, locale, browse_kind).await
}

#[tauri::command]
pub async fn read_steam_workshop_installation_status(
    instance_id: String,
    ids: Vec<String>,
) -> Result<SteamWorkshopInstallationSnapshot, String> {
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    let instance = read_instance_details(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?;
    let descriptors =
        discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
    let descriptor = find_descriptor(&descriptors, &instance.summary.module_id)?;
    let workshop = descriptor.workshop.as_ref().ok_or_else(|| {
        format!(
            "module `{}` does not declare [workshop]",
            descriptor.summary.id
        )
    })?;
    if !workshop.provider.eq_ignore_ascii_case("steam") {
        return Err(format!(
            "module `{}` declares unsupported Workshop provider `{}`",
            descriptor.summary.id, workshop.provider
        ));
    }
    let consumer_app_id = workshop.consumer_app_id.ok_or_else(|| {
        format!(
            "module `{}` does not declare workshop.consumer_app_id",
            descriptor.summary.id
        )
    })?;
    let install_root = PathBuf::from(commands_runtime_lifecycle::private_runtime_install_root(
        &instance,
    )?);

    let dst_ugc_roots = if instance.summary.module_id == "dontstarve" {
        Path::new(&instance.config_file_path)
            .parent()
            .and_then(Path::parent)
            .map(|instance_root| {
                ["Master", "Caves"]
                    .map(|shard| {
                        instance_root
                            .join("data/ugc")
                            .join(shard)
                            .join("content/322330")
                    })
                    .to_vec()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    tokio::task::spawn_blocking(move || {
        app_steamcmd::inspect_workshop_items_with_dst_ugc_roots(
            &storage.settings,
            consumer_app_id,
            &install_root,
            &dst_ugc_roots,
            &ids,
        )
        .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Workshop inspection worker failed: {error}"))?
}

#[tauri::command]
pub async fn read_dontstarve_mod_configuration_specs(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    ids: Vec<String>,
    locale: Option<String>,
) -> Result<Vec<DstModConfigurationSpec>, String> {
    let storage_context_operation =
        state.begin_storage_context_operation("Don't Starve Mod configuration read")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    let instance = read_instance_details(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?;
    if instance.summary.module_id != "dontstarve" {
        return Err(format!(
            "instance `{instance_id}` is not a Don't Starve Together server"
        ));
    }

    let instance_root = Path::new(&instance.config_file_path)
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| String::from("instance config path has no instance root"))?;
    let install_root = app_storage::resolve_instance_private_runtime_root(instance_root)
        .map_err(|error| error.to_string())?;
    let normalized_locale = locale.unwrap_or_else(|| String::from("en"));
    let steamcmd_root = PathBuf::from(read_steamcmd_status(&storage.settings).root);
    let extra_roots = vec![instance_root.join("data"), steamcmd_root];

    spawn_blocking_storage_context_task(&storage_context_operation, move || {
        read_dst_mod_configuration_specs_with_roots(
            &install_root,
            &extra_roots,
            &ids,
            &normalized_locale,
        )
    })
    .await
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn read_project_zomboid_workshop_mods_snapshot(
    instance_id: String,
    ids: Vec<String>,
) -> Result<ProjectZomboidWorkshopModsSnapshot, String> {
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;

    let instance = read_instance_details(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?;
    if instance.summary.module_id != "projectzomboid" {
        return Err(format!(
            "instance `{instance_id}` is not a Project Zomboid server"
        ));
    }

    let descriptors =
        discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
    let descriptor = find_descriptor(&descriptors, &instance.summary.module_id)?;
    let install_root = PathBuf::from(commands_runtime_lifecycle::private_runtime_install_root(
        &instance,
    )?);
    let consumer_app_id = descriptor
        .workshop
        .as_ref()
        .and_then(|workshop| workshop.consumer_app_id)
        .unwrap_or(108600);
    let steamcmd_root = storage.paths.steamcmd_root.clone();

    tokio::task::spawn_blocking(move || {
        read_local_project_zomboid_workshop_mods_snapshot(
            &install_root,
            &steamcmd_root,
            consumer_app_id,
            &ids,
        )
    })
    .await
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn fetch_steam_news_for_app(
    app_id: u64,
    count: Option<usize>,
    locale: Option<String>,
) -> Result<Vec<SteamNewsItem>, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent(format!(
            "LanGame Server Manager/{}",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|error| error.to_string())?;
    let clamped_count = count.unwrap_or(3).clamp(1, 6);
    let request = client
        .get("https://api.steampowered.com/ISteamNews/GetNewsForApp/v2/")
        .query(&[
            ("appid", app_id.to_string()),
            ("count", clamped_count.to_string()),
            ("maxlength", String::from("360")),
            ("feeds", String::from("steam_community_announcements")),
            ("format", String::from("json")),
        ])
        .build()
        .map_err(|error| error.to_string())?;
    let response = app_network::read_public_bytes(
        &client,
        request,
        Duration::from_secs(8),
        8 * 1024 * 1024,
        app_network::SourcePreference::from_locale(locale.as_deref()),
    )
    .await
    .map_err(|error| error.to_string())?;
    let payload = serde_json::from_slice::<SteamNewsEnvelope>(&response.bytes)
        .map_err(|error| error.to_string())?;
    app_network::record_success(response.url.as_str());

    Ok(payload
        .appnews
        .newsitems
        .into_iter()
        .map(map_steam_news_item)
        .collect())
}

#[tauri::command]
pub async fn fetch_steam_store_about(
    app_id: u64,
    locale: Option<String>,
) -> Result<Option<String>, String> {
    steam_store_about::fetch(app_id, locale.as_deref()).await
}

#[tauri::command]
pub async fn fetch_steam_review_summary(
    app_id: u64,
    locale: Option<String>,
) -> Result<Option<SteamReviewSummary>, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent(format!(
            "LanGame Server Manager/{}",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|error| error.to_string())?;

    let requested_locale = locale.unwrap_or_default().to_lowercase();
    let interface_language = if requested_locale.starts_with("zh") {
        "schinese"
    } else {
        "english"
    };
    let request = client
        .get(format!(
            "https://store.steampowered.com/appreviews/{app_id}"
        ))
        .query(&[
            ("json", "1"),
            ("filter", "all"),
            ("language", "all"),
            ("purchase_type", "all"),
            ("num_per_page", "0"),
            ("l", interface_language),
        ])
        .build()
        .map_err(|error| error.to_string())?;
    let response = app_network::read_public_bytes(
        &client,
        request,
        Duration::from_secs(8),
        8 * 1024 * 1024,
        app_network::SourcePreference::from_locale(Some(&requested_locale)),
    )
    .await
    .map_err(|error| error.to_string())?;
    let payload = serde_json::from_slice::<SteamAppReviewsEnvelope>(&response.bytes)
        .map_err(|error| error.to_string())?;
    app_network::record_success(response.url.as_str());

    Ok(payload
        .query_summary
        .and_then(|summary| map_steam_review_summary(app_id, summary)))
}

#[tauri::command]
pub async fn open_external_url(url: String) -> Result<(), String> {
    commands_external_url::open(&url).await
}

#[tauri::command]
pub fn open_local_path(path: String) -> Result<(), String> {
    let open_target = commands_local_paths::local_directory_to_open(&path)?;

    #[cfg(windows)]
    {
        let mut command = ProcessCommand::new("explorer.exe");
        apply_no_window(&mut command);
        command
            .arg(open_target.as_os_str())
            .spawn()
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    #[cfg(not(windows))]
    {
        let _ = open_target;
        Err(String::from(
            "opening local paths is unsupported on this platform",
        ))
    }
}

#[tauri::command]
pub fn probe_steamcmd_status() -> Result<SteamCmdStatus, String> {
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    Ok(read_steamcmd_status(&storage.settings))
}

#[tauri::command]
pub async fn uninstall_steamcmd(
    state: tauri::State<'_, DesktopState>,
) -> Result<SteamCmdStatus, String> {
    let storage_context_operation = state.begin_storage_context_operation("SteamCMD removal")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    let settings = storage.settings.clone();
    spawn_storage_context_task(&storage_context_operation, async move {
        remove_managed_steamcmd(&settings).await
    })
    .await
    .map_err(|error| format!("SteamCMD removal task failed: {error}"))?
    .map_err(|error| {
        append_desktop_app_log(
            &storage,
            "error",
            "steamcmd.uninstall.failed",
            &error.to_string(),
            json!({ "output_excerpt": steamcmd_error_excerpt(&error) }),
        );
        steamcmd_error_message(&error)
    })
}

#[tauri::command]
pub async fn update_app_settings(
    state: tauri::State<'_, DesktopState>,
    input: AppPathSettingsInput,
) -> Result<app_core::AppSettings, String> {
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    let next_settings = normalize_app_path_settings(&storage.settings, &input);
    let runtime_roots_will_change = runtime_roots_changed(&storage.settings, &next_settings);
    let any_path_changed = app_paths_changed(&storage.settings, &next_settings);
    if !any_path_changed {
        ensure_storage_context_snapshot_current(&state, &storage, "path update")?;
        return Ok(storage.settings);
    }
    let _storage_context_transition = state.begin_storage_context_transition()?;
    ensure_storage_context_snapshot_current(&state, &storage, "path update")?;

    append_desktop_app_log(
        &storage,
        "info",
        "settings.paths.update.request",
        "Saving app path settings",
        json!({
            "games_root": next_settings.games_root,
            "servers_root": next_settings.servers_root,
            "archives_root": next_settings.archives_root,
            "steamcmd_root": next_settings.steamcmd_root,
        }),
    );

    let blockers =
        collect_app_path_update_blockers(&state, &storage, runtime_roots_will_change).await?;
    if let Err(message) = ensure_app_path_update_allowed(&blockers, runtime_roots_will_change) {
        append_desktop_app_log(
            &storage,
            "warn",
            "settings.paths.update.blocked",
            &message,
            json!({
                "runtime_roots_changed": runtime_roots_will_change,
                "active_run_instance_ids": blockers.active_run_instance_ids,
                "tracked_instance_ids": blockers.tracked_instance_ids,
                "pending_start_instance_ids": blockers.pending_start_instance_ids,
                "active_job_ids": blockers.active_job_ids,
                "pending_restart_count": blockers.pending_restart_count,
                "shutdown_in_progress": blockers.shutdown_in_progress,
            }),
        );
        return Err(message);
    }

    // Retain the inventory lock through persistence so another operation cannot
    // create an archive between the emptiness check and the root change.
    let _archive_root_guard =
        app_storage::guard_archive_root_settings_update(&storage.paths, &next_settings)
            .await
            .map_err(|error| error.to_string())?;
    let saved = save_app_settings(next_settings).map_err(|error| {
        let message = error.to_string();
        append_desktop_app_log(
            &storage,
            "error",
            "settings.paths.update.failed",
            &message,
            json!({}),
        );
        logged_error_message(&storage, message)
    })?;

    if let Err(message) = clear_assistant_pending_operations() {
        append_desktop_app_log(
            &storage,
            "warn",
            "settings.paths.update.assistant_confirmations_clear_failed",
            &message,
            json!({}),
        );
    }

    mutate_app_state(&state, |app_state| {
        app_state.settings = saved.clone();
    })?;

    Ok(saved)
}

fn normalize_app_path_settings(current: &AppSettings, input: &AppPathSettingsInput) -> AppSettings {
    let servers_root = normalize_app_path_setting(&input.servers_root, &current.servers_root);
    AppSettings {
        archives_root: normalize_app_path_setting(
            &input.archives_root,
            &Path::new(&servers_root).join(".trash").to_string_lossy(),
        ),
        servers_root,
        games_root: normalize_app_path_setting(&input.games_root, &current.games_root),
        modules_root: current.modules_root.clone(),
        steamcmd_root: normalize_app_path_setting(&input.steamcmd_root, &current.steamcmd_root),
    }
}

fn normalize_app_path_setting(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    PathBuf::from(if trimmed.is_empty() {
        fallback
    } else {
        trimmed
    })
    .to_string_lossy()
    .into_owned()
}

fn app_paths_changed(current: &AppSettings, next: &AppSettings) -> bool {
    runtime_roots_changed(current, next)
        || Path::new(&current.steamcmd_root) != Path::new(&next.steamcmd_root)
}

fn runtime_roots_changed(current: &AppSettings, next: &AppSettings) -> bool {
    Path::new(&current.servers_root) != Path::new(&next.servers_root)
        || Path::new(&current.archives_root) != Path::new(&next.archives_root)
        || Path::new(&current.games_root) != Path::new(&next.games_root)
}

fn storage_context_snapshot_is_current(
    state: &DesktopState,
    captured: &AppSettings,
) -> Result<bool, String> {
    let current = state
        .app_state
        .read()
        .map_err(|_| String::from("desktop state lock poisoned"))?;
    Ok(!app_paths_changed(&current.settings, captured))
}

fn ensure_storage_context_snapshot_current(
    state: &DesktopState,
    storage: &StorageBootstrap,
    operation: &str,
) -> Result<(), String> {
    if storage_context_snapshot_is_current(state, &storage.settings)? {
        return Ok(());
    }

    Err(format!(
        "{operation} captured application paths that have since changed; retry the operation"
    ))
}

#[derive(Debug, Default)]
struct AppPathUpdateBlockers {
    active_run_instance_ids: Vec<String>,
    tracked_instance_ids: Vec<String>,
    pending_start_instance_ids: Vec<String>,
    active_job_ids: Vec<String>,
    pending_restart_count: usize,
    shutdown_in_progress: bool,
}

async fn collect_app_path_update_blockers(
    state: &DesktopState,
    storage: &StorageBootstrap,
    include_active_runs: bool,
) -> Result<AppPathUpdateBlockers, String> {
    let mut pending_start_instance_ids = state.pending_runtime_start_instance_ids()?;
    pending_start_instance_ids.sort();

    let mut tracked_instance_ids = if include_active_runs {
        state
            .runtime_supervisor
            .lock()
            .map_err(|_| String::from("runtime supervisor lock poisoned"))?
            .tracked_instances()
            .into_iter()
            .map(|instance| instance.summary.id)
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    tracked_instance_ids.sort();
    tracked_instance_ids.dedup();

    let (mut active_job_ids, shutdown_in_progress) = {
        let app_state = state
            .app_state
            .read()
            .map_err(|_| String::from("desktop state lock poisoned"))?;
        let active_job_ids = app_state
            .jobs
            .iter()
            .filter(|job| matches!(&job.status, JobStatus::Pending | JobStatus::Running))
            .map(|job| job.id.clone())
            .collect::<Vec<_>>();
        (
            active_job_ids,
            state.shutdown_in_progress.load(Ordering::SeqCst),
        )
    };
    active_job_ids.sort();

    let pending_restart_count = state
        .runtime_restart_scheduler
        .lock()
        .map_err(|_| String::from("runtime restart scheduler lock poisoned"))?
        .pending_count();

    let mut active_run_instance_ids = if include_active_runs {
        list_active_instance_runs(&storage.paths)
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|run| run.instance_id)
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    active_run_instance_ids.sort();
    active_run_instance_ids.dedup();

    Ok(AppPathUpdateBlockers {
        active_run_instance_ids,
        tracked_instance_ids,
        pending_start_instance_ids,
        active_job_ids,
        pending_restart_count,
        shutdown_in_progress,
    })
}

fn ensure_app_path_update_allowed(
    blockers: &AppPathUpdateBlockers,
    runtime_roots_changed: bool,
) -> Result<(), String> {
    let runtime_is_active = runtime_roots_changed
        && (!blockers.active_run_instance_ids.is_empty()
            || !blockers.tracked_instance_ids.is_empty());
    if !runtime_is_active
        && blockers.pending_start_instance_ids.is_empty()
        && blockers.active_job_ids.is_empty()
        && blockers.pending_restart_count == 0
        && !blockers.shutdown_in_progress
    {
        return Ok(());
    }

    let mut reasons = Vec::new();
    if !blockers.active_run_instance_ids.is_empty() {
        reasons.push(format!(
            "active runs: {}",
            blockers.active_run_instance_ids.join(", ")
        ));
    }
    if !blockers.tracked_instance_ids.is_empty() {
        reasons.push(format!(
            "supervised runtimes: {}",
            blockers.tracked_instance_ids.join(", ")
        ));
    }
    if !blockers.pending_start_instance_ids.is_empty() {
        reasons.push(format!(
            "pending starts: {}",
            blockers.pending_start_instance_ids.join(", ")
        ));
    }
    if !blockers.active_job_ids.is_empty() {
        reasons.push(format!(
            "background jobs: {}",
            blockers.active_job_ids.join(", ")
        ));
    }
    if blockers.pending_restart_count > 0 {
        reasons.push(format!(
            "pending automatic restarts: {}",
            blockers.pending_restart_count
        ));
    }
    if blockers.shutdown_in_progress {
        reasons.push(String::from("application shutdown is in progress"));
    }

    Err(format!(
        "application paths cannot be changed until runtime and background activity finishes ({})",
        reasons.join("; ")
    ))
}

#[tauri::command]
pub fn pick_directory_path(current_path: Option<String>) -> Result<Option<String>, String> {
    show_directory_picker(current_path.as_deref())
}

#[tauri::command]
pub async fn install_module_game(
    state: tauri::State<'_, DesktopState>,
    module_id: String,
) -> Result<ModuleInstallResult, String> {
    install_module_game_inner(state, module_id).await
}

#[tauri::command]
pub async fn validate_module_game(
    state: tauri::State<'_, DesktopState>,
    module_id: String,
) -> Result<ModuleInstallResult, String> {
    validate_module_game_inner(state, module_id).await
}

#[tauri::command]
pub async fn uninstall_module_game<R: tauri::Runtime>(
    app_handle: tauri::AppHandle<R>,
    module_id: String,
) -> Result<app_core::ModuleUninstallResult, String> {
    uninstall_module_game_inner(app_handle, module_id).await
}

#[tauri::command]
pub async fn download_steam_workshop_items<R: tauri::Runtime>(
    app_handle: tauri::AppHandle<R>,
    instance_id: String,
    ids: Vec<String>,
    missing_only: Option<bool>,
    locale: Option<String>,
) -> Result<SteamWorkshopDownloadResult, String> {
    let operation = app_handle
        .state::<DesktopState>()
        .begin_storage_context_operation("Steam Workshop download")?;
    spawn_storage_context_task(&operation, async move {
        download_steam_workshop_items_inner(
            app_handle.state::<DesktopState>(),
            instance_id,
            ids,
            missing_only.unwrap_or(false),
            app_network::SourcePreference::from_locale(locale.as_deref()),
        )
        .await
    })
    .await
    .map_err(|error| format!("Workshop download task failed: {error}"))?
}

#[tauri::command]
pub async fn stage_manual_mod_files(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    source_paths: Vec<String>,
) -> Result<ManualModStageResult, String> {
    let storage_context_operation = state.begin_storage_context_operation("manual mod staging")?;
    stage_manual_mod_files_inner(
        &state,
        &storage_context_operation,
        instance_id,
        source_paths,
    )
    .await
}

#[tauri::command]
pub async fn install_manual_mod_references(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    references: Vec<String>,
) -> Result<ManualModStageResult, String> {
    let storage_context_operation =
        state.begin_storage_context_operation("manual mod installation")?;
    install_manual_mod_references_inner(&state, &storage_context_operation, instance_id, references)
        .await
}

#[tauri::command]
pub async fn read_manual_mod_inventory(
    instance_id: String,
) -> Result<ManualModInventoryResult, String> {
    read_manual_mod_inventory_inner(instance_id).await
}

#[tauri::command]
pub async fn resolve_manual_mod_references(
    instance_id: String,
    references: Vec<String>,
) -> Result<ManualModReferenceResolveResult, String> {
    resolve_manual_mod_references_inner(instance_id, references).await
}
#[cfg(windows)]
fn apply_no_window(command: &mut ProcessCommand) {
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn apply_no_window(_command: &mut ProcessCommand) {}

fn find_descriptor<'a>(
    descriptors: &'a [ModuleDescriptor],
    module_id: &str,
) -> Result<&'a ModuleDescriptor, String> {
    descriptors
        .iter()
        .find(|descriptor| descriptor.summary.id == module_id)
        .ok_or_else(|| format!("module `{module_id}` not found in modules"))
}

async fn running_instance_labels_for_module(
    paths: &app_storage::StoragePaths,
    module_id: &str,
) -> Result<Vec<String>, String> {
    let instances = list_instances(paths)
        .await
        .map_err(|error| error.to_string())?;
    let active_instance_ids = list_active_instance_runs(paths)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|run| run.instance_id)
        .collect::<HashSet<_>>();

    let mut labels = instances
        .into_iter()
        .filter(|instance| instance.module_id == module_id)
        .filter(|instance| {
            active_instance_ids.contains(&instance.id)
                || instance.active_process_count > 0
                || matches!(
                    instance.status,
                    app_core::InstanceStatus::Starting
                        | app_core::InstanceStatus::Running
                        | app_core::InstanceStatus::Stopping
                )
        })
        .map(|instance| format!("{} ({})", instance.name, instance.id))
        .collect::<Vec<_>>();
    labels.sort();
    Ok(labels)
}

fn build_game_install_sync_record(
    settings: &app_core::AppSettings,
    descriptor: &ModuleDescriptor,
    mark_verified: bool,
    install_root_override: Option<&str>,
) -> GameInstallSyncRecord {
    let probe = probe_module_install_state_with_override(
        settings,
        &descriptor.summary.id,
        descriptor.summary.steam_app_id,
        descriptor.install.as_ref(),
        descriptor.process.as_ref(),
        install_root_override,
    );

    GameInstallSyncRecord {
        module_id: descriptor.summary.id.clone(),
        install_root: probe.install_root,
        install_state: probe.install_state,
        current_version: probe.current_version,
        mark_verified,
    }
}

async fn load_module_install_root_override(
    paths: &app_storage::StoragePaths,
    module_id: &str,
) -> Option<String> {
    resolve_module_install_root(paths, module_id)
        .await
        .ok()
        .flatten()
}

async fn load_module_summaries_with_install_state(
    storage: &app_storage::StorageBootstrap,
    descriptors: &[ModuleDescriptor],
) -> Result<Vec<ModuleSummary>, String> {
    // Keep archive inventory futures out of callers' nested desktop poll frames.
    let (instance_counts, archive_counts) =
        Box::pin(load_preserved_program_counts(storage)).await?;
    let mut summaries = load_module_installation_summaries(storage, descriptors).await?;
    for summary in &mut summaries {
        summary.instance_program_count = instance_counts.get(&summary.id).copied().unwrap_or(0);
        summary.archived_program_count = archive_counts.get(&summary.id).copied().unwrap_or(0);
    }
    Ok(summaries)
}

async fn load_module_installation_summaries(
    storage: &app_storage::StorageBootstrap,
    descriptors: &[ModuleDescriptor],
) -> Result<Vec<ModuleSummary>, String> {
    let mut summaries = Vec::with_capacity(descriptors.len());
    for descriptor in descriptors {
        let install_root_override =
            load_module_install_root_override(&storage.paths, &descriptor.summary.id).await;
        let summary = module_summary_with_install_state(
            &storage.settings,
            descriptor,
            install_root_override.as_deref(),
        );
        summaries.push(summary);
    }

    Ok(summaries)
}

async fn load_module_details_with_install_state(
    storage: &app_storage::StorageBootstrap,
    descriptor: &ModuleDescriptor,
    include_preserved_program_counts: bool,
) -> Result<ModuleDetails, String> {
    let install_root_override =
        load_module_install_root_override(&storage.paths, &descriptor.summary.id).await;
    let mut details = map_module_details_with_install_state(
        &storage.settings,
        descriptor,
        install_root_override.as_deref(),
    );
    // Configuration and installation capabilities do not inspect archived
    // programs. Their reads must remain available during archive mutations.
    if include_preserved_program_counts {
        let (instance_counts, archive_counts) =
            Box::pin(load_preserved_program_counts(storage)).await?;
        details.summary.instance_program_count = instance_counts
            .get(&descriptor.summary.id)
            .copied()
            .unwrap_or(0);
        details.summary.archived_program_count = archive_counts
            .get(&descriptor.summary.id)
            .copied()
            .unwrap_or(0);
    }
    Ok(details)
}

async fn load_preserved_program_counts(
    storage: &StorageBootstrap,
) -> Result<
    (
        std::collections::BTreeMap<String, u32>,
        std::collections::BTreeMap<String, u32>,
    ),
    String,
> {
    let mut instance_counts = std::collections::BTreeMap::new();
    for record in app_storage::read_all_instance_program_installs(&storage.paths)
        .await
        .map_err(|error| error.to_string())?
    {
        if record.runtime_mode == "independent"
            && record.install.owner_instance_id.as_deref() == Some(record.instance_id.as_str())
            && let Some(root) = record.install.install_root.parent()
            && app_storage::resolve_instance_runtime_root(root).is_ok()
        {
            *instance_counts.entry(record.install.module_id).or_default() += 1;
        }
    }
    let mut archive_counts = std::collections::BTreeMap::new();
    for source in app_storage::read_archived_program_sources(&storage.paths)
        .await
        .map_err(|error| error.to_string())?
    {
        *archive_counts.entry(source.module_id).or_default() += 1;
    }
    Ok((instance_counts, archive_counts))
}

async fn persist_descriptor_install_states(
    storage: &app_storage::StorageBootstrap,
    descriptors: &[ModuleDescriptor],
) -> Result<(), String> {
    let mut records = Vec::with_capacity(descriptors.len());

    for descriptor in descriptors {
        let install_root_override =
            load_module_install_root_override(&storage.paths, &descriptor.summary.id).await;
        records.push(build_game_install_sync_record(
            &storage.settings,
            descriptor,
            false,
            install_root_override.as_deref(),
        ));
    }

    sync_game_installs(&storage.paths, &records)
        .await
        .map_err(|error| error.to_string())
}

async fn persist_install_result(
    storage: &app_storage::StorageBootstrap,
    result: &ModuleInstallResult,
    mark_verified: bool,
) -> Result<(), String> {
    let record = GameInstallSyncRecord {
        module_id: result.module_id.clone(),
        install_root: result.install_root.clone(),
        install_state: result.install_state.clone(),
        current_version: result.current_version.clone(),
        mark_verified,
    };

    sync_game_installs(&storage.paths, &[record])
        .await
        .map_err(|error| error.to_string())
}

fn module_summary_with_install_state(
    settings: &app_core::AppSettings,
    descriptor: &ModuleDescriptor,
    install_root_override: Option<&str>,
) -> ModuleSummary {
    let mut summary = descriptor.summary.clone();
    summary.install_state = probe_module_install_state_with_override(
        settings,
        &summary.id,
        summary.steam_app_id,
        descriptor.install.as_ref(),
        descriptor.process.as_ref(),
        install_root_override,
    )
    .install_state;
    summary
}

fn map_module_details_with_install_state(
    settings: &app_core::AppSettings,
    descriptor: &ModuleDescriptor,
    install_root_override: Option<&str>,
) -> ModuleDetails {
    ModuleDetails {
        summary: module_summary_with_install_state(settings, descriptor, install_root_override),
        schema_json: descriptor.schema_json.clone(),
        default_ports: descriptor.default_ports.clone(),
        install: descriptor.install.clone(),
        process: descriptor.process.clone(),
        workshop: descriptor.workshop.clone(),
        mods: module_mods_spec_from_manifest(&descriptor.manifest_toml),
        runtime: descriptor.runtime.clone(),
    }
}

fn map_steam_news_item(item: SteamNewsApiItem) -> SteamNewsItem {
    SteamNewsItem {
        gid: item.gid,
        title: item.title,
        url: item.url,
        author: item.author,
        feed_label: item.feedlabel,
        excerpt: clean_steam_news_excerpt(&item.contents),
        published_at_unix_ms: item.date.saturating_mul(1000),
    }
}

fn map_steam_review_summary(
    app_id: u64,
    summary: SteamAppReviewsQuerySummary,
) -> Option<SteamReviewSummary> {
    let total_positive = summary.total_positive.unwrap_or(0);
    let total_negative = summary.total_negative.unwrap_or(0);
    let derived_total = total_positive.saturating_add(total_negative);
    let total_reviews = summary
        .total_reviews
        .filter(|total| *total > 0)
        .unwrap_or(derived_total);
    let review_score_desc = summary
        .review_score_desc
        .unwrap_or_default()
        .trim()
        .to_string();

    if total_reviews == 0 && review_score_desc.is_empty() {
        return None;
    }

    let positive_percent = if total_reviews > 0 {
        ((total_positive as f64 / total_reviews as f64) * 100.0).round() as u64
    } else {
        0
    }
    .min(100);

    Some(SteamReviewSummary {
        app_id,
        review_score: summary.review_score,
        review_score_desc,
        total_positive,
        total_negative,
        total_reviews,
        positive_percent,
        source_url: format!("https://store.steampowered.com/app/{app_id}/#app_reviews_hash"),
    })
}

fn clean_steam_news_excerpt(contents: &str) -> String {
    let normalized = contents
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n");
    let without_html = strip_delimited(&normalized, '<', '>');
    let without_bbcode = strip_delimited(&without_html, '[', ']');
    let decoded = without_bbcode
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ");
    let collapsed = decoded.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_plain_text(collapsed.trim(), 220)
}

fn strip_delimited(value: &str, open: char, close: char) -> String {
    let mut output = String::with_capacity(value.len());
    let mut depth = 0usize;

    for ch in value.chars() {
        if ch == open {
            depth += 1;
            continue;
        }
        if ch == close && depth > 0 {
            depth -= 1;
            continue;
        }
        if depth == 0 {
            output.push(ch);
        }
    }

    output
}

fn truncate_plain_text(value: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (count, ch) in value.chars().enumerate() {
        if count >= max_chars {
            out.push_str("...");
            break;
        }
        out.push(ch);
    }

    out
}

fn current_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn new_background_job_id(kind: &str, target_id: &str) -> String {
    format!("{kind}:{target_id}:{}", uuid::Uuid::new_v4().simple())
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;

#[cfg(test)]
include!("commands_assistant_session_test_support.rs");

#[cfg(test)]
#[path = "commands_settings_tests.rs"]
mod settings_tests;

#[cfg(test)]
#[path = "commands_dst_import_tests.rs"]
mod dst_import_tests;

#[cfg(all(test, windows))]
#[path = "commands_local_path_tests.rs"]
mod local_path_tests;
