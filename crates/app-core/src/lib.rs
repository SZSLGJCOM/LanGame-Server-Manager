use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

mod live_players;
pub use live_players::*;
mod install_progress;
pub use install_progress::{InstallPhase, InstallProgress};
mod program_cleanup;
pub use program_cleanup::{ModuleUninstallResult, ProgramCleanupResult, ProgramCleanupRetention};
mod download_integrity;
pub use download_integrity::DownloadIntegritySpec;
pub mod ark_cluster;
pub mod ark_maps;
pub mod dst_shards;
mod system_snapshot;
pub use system_snapshot::SystemSnapshot;
mod system_telemetry;
pub use system_telemetry::{DiskVolumeSnapshot, SystemTelemetry, TelemetryStatus};

mod runtime_resources;
pub use runtime_resources::RuntimeResourceLimits;
mod program_update;
pub use program_update::InstanceProgramUpdatePolicy;

pub const DEFAULT_LANGAME_DATA_ROOT: &str = "D:/LanGame";
pub const DEFAULT_LANGAME_INSTANCES_ROOT: &str = "D:/LanGame/instances";
pub const DEFAULT_LANGAME_SERVER_FILES_ROOT: &str = "D:/LanGame/server-files";
pub const DEFAULT_LANGAME_STEAMCMD_ROOT: &str = "D:/LanGame/cmd/steamcmd";
pub const INSTANCE_CREATION_DEFAULT_BIND_IP: &str = "0.0.0.0";
pub const INSTANCE_CREATION_DEFAULT_AUTOSTART: bool = false;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub servers_root: String,
    /// Empty only in pre-archive-root settings; resolve against servers_root.
    #[serde(default)]
    pub archives_root: String,
    pub games_root: String,
    pub modules_root: String,
    pub steamcmd_root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppPathSettingsInput {
    pub servers_root: String,
    pub archives_root: String,
    pub games_root: String,
    pub steamcmd_root: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            servers_root: String::from(DEFAULT_LANGAME_INSTANCES_ROOT),
            archives_root: String::new(),
            games_root: String::from(DEFAULT_LANGAME_SERVER_FILES_ROOT),
            modules_root: String::from("./modules"),
            steamcmd_root: String::from(DEFAULT_LANGAME_STEAMCMD_ROOT),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_instance_input_serializes_only_instance_identity() {
        let input: CreateInstanceInput = serde_json::from_value(serde_json::json!({
            "name": "Managed Palworld",
            "module_id": "palworld"
        }))
        .expect("identity-only create input should deserialize");

        assert_eq!(
            serde_json::to_value(input).unwrap(),
            serde_json::json!({
                "name": "Managed Palworld",
                "module_id": "palworld"
            })
        );
    }

    #[test]
    fn create_instance_input_rejects_retired_runtime_fields() {
        let error = serde_json::from_value::<CreateInstanceInput>(serde_json::json!({
            "name": "Legacy Palworld",
            "module_id": "palworld",
            "bind_ip": "192.168.1.8",
            "autostart": true
        }))
        .expect_err("retired runtime fields must not be accepted by create");

        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn default_settings_use_langame_runtime_layout() {
        let settings = AppSettings::default();

        assert_eq!(settings.servers_root, DEFAULT_LANGAME_INSTANCES_ROOT);
        assert_eq!(settings.games_root, DEFAULT_LANGAME_SERVER_FILES_ROOT);
        assert_eq!(settings.steamcmd_root, DEFAULT_LANGAME_STEAMCMD_ROOT);
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StorageStatus {
    pub database_path: String,
    pub migrations_path: String,
    pub app_log_path: String,
    pub database_exists: bool,
    pub schema_version: i64,
    pub migrations_applied: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum InstallState {
    #[default]
    NotInstalled,
    Installing,
    Incomplete,
    Installed,
    Updating,
    Uninstalling,
    Corrupted,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum InstanceStatus {
    #[default]
    Stopped,
    Starting,
    Running,
    Stopping,
    Error,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum JobKind {
    #[default]
    InstallSteamCmd,
    DownloadGame,
    DownloadWorkshop,
    ValidateGame,
    UninstallGame,
    StartInstance,
    StopInstance,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum JobStatus {
    #[default]
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleSummary {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub steam_app_id: Option<u32>,
    pub install_state: InstallState,
    #[serde(default)]
    pub instance_program_count: u32,
    #[serde(default)]
    pub archived_program_count: u32,
    pub supported_platforms: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortBinding {
    pub name: String,
    pub protocol: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModulePortGroupSpec {
    pub id: String,
    pub members: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_offsets: Option<BTreeMap<String, u16>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstallSource {
    MinecraftJava,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinecraftJavaInstallSpec {
    pub version: String,
    pub manifest_url: Option<String>,
    pub server_jar: String,
    pub java_policy: String,
    pub default_distribution: String,
    pub distributions: Vec<MinecraftDistributionSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinecraftDistributionSpec {
    pub id: String,
    pub label: String,
    pub server_jar: String,
    pub source: String,
    pub entrypoint_kind: String,
    pub supports_mods: bool,
    pub supports_plugins: bool,
    pub supports_datapacks: bool,
    pub supports_resource_packs: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallSpec {
    pub shared_game_dir: String,
    pub download_url_windows: Option<String>,
    pub download_integrity_windows: Option<DownloadIntegritySpec>,
    pub source: Option<InstallSource>,
    pub verification_path: Option<String>,
    pub minecraft: Option<MinecraftJavaInstallSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProcessWindowPolicy {
    #[default]
    Background,
    External,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProcessHostSurface {
    #[default]
    ManagedTerminal,
    ManagedPseudoConsole,
    ManagedNativeWindow,
    ExternalWindow,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessSpec {
    pub executable: String,
    pub args_template: Vec<String>,
    #[serde(default)]
    pub environment_template: BTreeMap<String, String>,
    pub working_directory_template: Option<String>,
    #[serde(default)]
    pub window_policy: ProcessWindowPolicy,
    #[serde(default)]
    pub host_surface: ProcessHostSurface,
    pub host_notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkshopSpec {
    pub provider: String,
    pub consumer_app_id: Option<u32>,
    pub supports_collections: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleModSourceSpec {
    pub provider: String,
    pub label: String,
    pub url: String,
    #[serde(default)]
    pub loaders: Vec<String>,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub install_note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleManualModStagingSpec {
    pub target_template: String,
    pub target_label: String,
    #[serde(default)]
    pub accepts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleModEnablementSpec {
    pub setting_key: String,
    pub setting_label: String,
    #[serde(default)]
    pub id_strategy: Option<String>,
    #[serde(default)]
    pub reference_strategy: Option<String>,
    #[serde(default)]
    pub reference_game_id: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleModsSpec {
    pub source: Option<ModuleModSourceSpec>,
    pub manual_staging: Option<ModuleManualModStagingSpec>,
    pub enablement: Option<ModuleModEnablementSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModulePlayerQuerySpec {
    pub protocol: String,
    #[serde(default)]
    pub port_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModulePlayerActionSpec {
    pub id: String,
    #[serde(default)]
    pub kind: Option<String>,
    pub label: String,
    #[serde(default)]
    pub label_zh_cn: Option<String>,
    #[serde(default = "default_player_action_transport")]
    pub transport: String,
    pub command_template: String,
    #[serde(default)]
    pub target_label: Option<String>,
    #[serde(default)]
    pub target_label_zh_cn: Option<String>,
    #[serde(default)]
    pub target_placeholder: Option<String>,
    #[serde(default)]
    pub target_placeholder_zh_cn: Option<String>,
    #[serde(default)]
    pub target_required: bool,
    #[serde(default)]
    pub target_encoding: Option<String>,
    #[serde(default)]
    pub role_values: Vec<String>,
    #[serde(default)]
    pub process_key: Option<String>,
    #[serde(default)]
    pub port_name: Option<String>,
    #[serde(default)]
    pub password_setting_key: Option<String>,
    #[serde(default)]
    pub enabled_setting_key: Option<String>,
    #[serde(default)]
    pub destructive: bool,
}

fn default_player_action_transport() -> String {
    String::from("stdin")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModulePlayerManagementSpec {
    pub status: String,
    pub planned_surface: String,
    pub reason: String,
    pub verification: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleShutdownSpec {
    #[serde(default)]
    pub commands: Vec<ModuleShutdownCommandSpec>,
    #[serde(default = "default_shutdown_grace_period_ms")]
    pub grace_period_ms: u64,
}

fn default_shutdown_grace_period_ms() -> u64 {
    10_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleShutdownCommandSpec {
    #[serde(default = "default_player_action_transport")]
    pub transport: String,
    #[serde(default)]
    pub fallback_transport: Option<String>,
    pub command: String,
    #[serde(default)]
    pub process_key: Option<String>,
    #[serde(default)]
    pub port_name: Option<String>,
    #[serde(default)]
    pub password_setting_key: Option<String>,
    #[serde(default)]
    pub enabled_setting_key: Option<String>,
    #[serde(default)]
    pub wait_after_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RuntimePriorityClass {
    Idle,
    BelowNormal,
    #[default]
    Normal,
    AboveNormal,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimePerformancePolicy {
    #[serde(default)]
    pub resource_limits: RuntimeResourceLimits,
    #[serde(default)]
    pub priority_class: RuntimePriorityClass,
    #[serde(default)]
    pub cpu_affinity_mask: Option<u64>,
    #[serde(default = "default_apply_to_child_processes")]
    pub apply_to_child_processes: bool,
    #[serde(default = "default_startup_stagger_ms")]
    pub startup_stagger_ms: u64,
    #[serde(default = "default_child_process_stagger_ms")]
    pub child_process_stagger_ms: u64,
}

impl Default for RuntimePerformancePolicy {
    fn default() -> Self {
        Self {
            resource_limits: RuntimeResourceLimits::default(),
            priority_class: RuntimePriorityClass::AboveNormal,
            cpu_affinity_mask: None,
            apply_to_child_processes: default_apply_to_child_processes(),
            startup_stagger_ms: default_startup_stagger_ms(),
            child_process_stagger_ms: default_child_process_stagger_ms(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimePerformancePolicyPreview {
    pub summary: String,
    pub priority_source: String,
    pub cpu_affinity_source: String,
    pub cpu_affinity_preset: Option<String>,
    pub logical_cpu_count: usize,
}

impl Default for RuntimePerformancePolicyPreview {
    fn default() -> Self {
        Self {
            summary: String::from("Runtime performance policy will use the module defaults."),
            priority_source: String::from("module_default"),
            cpu_affinity_source: String::from("all_cpus"),
            cpu_affinity_preset: None,
            logical_cpu_count: 0,
        }
    }
}

fn default_apply_to_child_processes() -> bool {
    true
}

fn default_startup_stagger_ms() -> u64 {
    1500
}

fn default_child_process_stagger_ms() -> u64 {
    500
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ModulePortRole {
    Player,
    Service,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModulePortRoleSpec {
    pub role: ModulePortRole,
    #[serde(default)]
    pub port_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModuleJoinProfile {
    SteamConnect {
        client_app_id: u32,
        join_port_name: String,
        query_port_name: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModuleRuntimeSpec {
    #[serde(default)]
    pub program_sharing: InstanceProgramMode,
    #[serde(default)]
    pub player_count_source: ModulePlayerCountSource,
    #[serde(default)]
    pub bind_address: ModuleBindAddressSpec,
    #[serde(default)]
    pub port_roles: Vec<ModulePortRoleSpec>,
    #[serde(default)]
    pub port_groups: Vec<ModulePortGroupSpec>,
    #[serde(default)]
    pub player_query: Option<ModulePlayerQuerySpec>,
    #[serde(default)]
    pub join: Option<ModuleJoinProfile>,
    #[serde(default)]
    pub player_actions: Vec<ModulePlayerActionSpec>,
    #[serde(default)]
    pub player_list: Option<ModulePlayerListSpec>,
    #[serde(default)]
    pub player_management: Option<ModulePlayerManagementSpec>,
    #[serde(default)]
    pub shutdown: Option<ModuleShutdownSpec>,
    #[serde(default)]
    pub performance: RuntimePerformancePolicy,
    #[serde(default)]
    pub requires_admin: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstanceProgramMode {
    Shared,
    #[default]
    Independent,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstanceProgramSource {
    #[default]
    Verified,
    Local,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModuleBindAddressMode {
    #[default]
    Unsupported,
    Strict,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModuleBindAddressSpec {
    #[serde(default)]
    pub mode: ModuleBindAddressMode,
    #[serde(default)]
    pub port_names: Vec<String>,
    #[serde(default)]
    pub required_setting_key: Option<String>,
    #[serde(default = "default_bind_address_startup_timeout_ms")]
    pub startup_timeout_ms: u64,
}

impl Default for ModuleBindAddressSpec {
    fn default() -> Self {
        Self {
            mode: ModuleBindAddressMode::Unsupported,
            port_names: Vec::new(),
            required_setting_key: None,
            startup_timeout_ms: default_bind_address_startup_timeout_ms(),
        }
    }
}

fn default_bind_address_startup_timeout_ms() -> u64 {
    60_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleDetails {
    pub summary: ModuleSummary,
    pub schema_json: Option<String>,
    pub default_ports: Vec<PortBinding>,
    pub install: Option<InstallSpec>,
    pub process: Option<ProcessSpec>,
    pub workshop: Option<WorkshopSpec>,
    pub mods: Option<ModuleModsSpec>,
    #[serde(default)]
    pub runtime: ModuleRuntimeSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceSummary {
    pub id: String,
    pub name: String,
    pub module_id: String,
    pub status: InstanceStatus,
    #[serde(default)]
    pub active_process_count: usize,
    pub bind_ip: String,
    pub port_count: usize,
    pub autostart: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateInstanceInput {
    pub name: String,
    pub module_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceProvisioning {
    pub summary: InstanceSummary,
    pub config_file_path: String,
    pub ports: Vec<PortBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceDetails {
    pub summary: InstanceSummary,
    pub config_file_path: String,
    pub saves_path: String,
    #[serde(default)]
    pub backup_uses_declared_saves_path: bool,
    pub auto_backup_on_stop: bool,
    pub backup_retention_count: u32,
    pub settings_json: String,
    pub ports: Vec<PortBinding>,
    pub active_run: Option<ActiveInstanceRun>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstanceBroadcastRule {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceBroadcastPeriodicRule {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_broadcast_periodic_interval_minutes")]
    pub interval_minutes: u32,
    #[serde(default)]
    pub prompt: Option<String>,
}

fn default_broadcast_periodic_interval_minutes() -> u32 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceBroadcastRules {
    #[serde(default)]
    pub startup: InstanceBroadcastRule,
    #[serde(default)]
    pub shutdown: InstanceBroadcastRule,
    #[serde(default)]
    pub runtime_health: InstanceBroadcastRule,
    #[serde(default)]
    pub periodic: InstanceBroadcastPeriodicRule,
    #[serde(default = "default_broadcast_tone")]
    pub tone: String,
    #[serde(default = "default_broadcast_cooldown_minutes")]
    pub cooldown_minutes: u32,
}

fn default_broadcast_tone() -> String {
    String::from("short")
}

fn default_broadcast_cooldown_minutes() -> u32 {
    10
}

impl Default for InstanceBroadcastPeriodicRule {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_minutes: default_broadcast_periodic_interval_minutes(),
            prompt: None,
        }
    }
}

impl Default for InstanceBroadcastRules {
    fn default() -> Self {
        Self {
            startup: InstanceBroadcastRule::default(),
            shutdown: InstanceBroadcastRule::default(),
            runtime_health: InstanceBroadcastRule::default(),
            periodic: InstanceBroadcastPeriodicRule::default(),
            tone: default_broadcast_tone(),
            cooldown_minutes: default_broadcast_cooldown_minutes(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceBroadcastPolicy {
    pub instance_id: String,
    pub enabled: bool,
    pub rules: InstanceBroadcastRules,
    pub updated_at_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInstanceBroadcastPolicyInput {
    pub instance_id: String,
    pub enabled: bool,
    pub rules: InstanceBroadcastRules,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceBroadcastEvent {
    pub event_id: String,
    pub instance_id: String,
    pub module_id: String,
    pub source: String,
    #[serde(default)]
    pub rule_id: Option<String>,
    pub message: String,
    #[serde(default)]
    pub ai_provider: Option<String>,
    #[serde(default)]
    pub ai_model: Option<String>,
    #[serde(default)]
    pub action_id: Option<String>,
    #[serde(default)]
    pub transport: Option<String>,
    #[serde(default)]
    pub command_preview: Option<String>,
    pub status: String,
    #[serde(default)]
    pub response_text: Option<String>,
    #[serde(default)]
    pub error_message: Option<String>,
    #[serde(default)]
    pub initiator: Option<String>,
    #[serde(default)]
    pub policy_snapshot_json: Option<String>,
    pub created_at_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsertInstanceBroadcastEventInput {
    pub instance_id: String,
    pub module_id: String,
    pub source: String,
    #[serde(default)]
    pub rule_id: Option<String>,
    pub message: String,
    #[serde(default)]
    pub ai_provider: Option<String>,
    #[serde(default)]
    pub ai_model: Option<String>,
    #[serde(default)]
    pub action_id: Option<String>,
    #[serde(default)]
    pub transport: Option<String>,
    #[serde(default)]
    pub command_preview: Option<String>,
    pub status: String,
    #[serde(default)]
    pub response_text: Option<String>,
    #[serde(default)]
    pub error_message: Option<String>,
    #[serde(default)]
    pub initiator: Option<String>,
    #[serde(default)]
    pub policy_snapshot_json: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstanceBackupKind {
    #[default]
    Manual,
    AutoStop,
    PreRestore,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceBackupResult {
    pub backup_id: String,
    pub instance_id: String,
    #[serde(default)]
    pub backup_kind: InstanceBackupKind,
    #[serde(default)]
    pub display_name: Option<String>,
    pub created_at_unix_ms: u128,
    pub backup_path: String,
    pub saves_path: String,
    pub file_count: usize,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceBackupRestoreResult {
    pub instance_id: String,
    pub backup_id: String,
    pub restored_at_unix_ms: u128,
    pub saves_path: String,
    pub restored_file_count: usize,
    pub restored_total_bytes: u64,
    pub safeguard_backup_id: String,
    pub safeguard_backup_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceDeletionResult {
    pub instance_id: String,
    pub instance_name: String,
    pub module_id: String,
    pub deleted_at_unix_ms: u128,
    pub deleted_instance_root: String,
    pub preserved_external_saves_path: Option<String>,
    #[serde(default)]
    pub program_cleanup: ProgramCleanupResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceArchiveResult {
    pub archive_id: String,
    pub instance_id: String,
    pub instance_name: String,
    pub module_id: String,
    pub deleted_at_unix_ms: u128,
    pub previous_instance_root: String,
    pub archived_instance_root: Option<String>,
    pub effective_saves_path: String,
    pub saves_archived_with_instance_root: bool,
    pub preserved_external_saves_path: Option<String>,
    pub external_saves_backup_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInstanceInput {
    pub id: String,
    pub bind_ip: String,
    pub auto_backup_on_stop: bool,
    pub backup_retention_count: u32,
    pub settings_json: String,
    pub ports: Vec<PortBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchValidationIssue {
    pub code: String,
    pub severity: String,
    pub message: String,
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub context: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchPlan {
    pub instance_id: String,
    pub instance_name: String,
    pub module_id: String,
    pub install_root: String,
    pub install_state: InstallState,
    pub uses_private_runtime: bool,
    pub working_directory: String,
    pub executable_path: String,
    pub executable_exists: bool,
    pub ready_to_launch: bool,
    pub validation_issues: Vec<LaunchValidationIssue>,
    pub args: Vec<String>,
    pub environment: BTreeMap<String, String>,
    pub command_line: String,
    pub window_policy: ProcessWindowPolicy,
    pub uses_script_entrypoint: bool,
    pub requires_admin: bool,
    pub host_surface: ProcessHostSurface,
    pub host_notes: Option<String>,
    pub performance_policy: RuntimePerformancePolicy,
    pub performance_preview: RuntimePerformancePolicyPreview,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessIdentity {
    /// Opaque OS start token. Together with the image path this distinguishes
    /// a newly created process that has reused an old PID.
    pub creation_time: u64,
    pub image_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceProcessState {
    pub run_id: i64,
    pub session_id: Option<String>,
    pub process_key: String,
    pub display_name: String,
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_identity: Option<ProcessIdentity>,
    pub status: String,
    pub started_at: Option<String>,
    pub stopped_at: Option<String>,
    pub exit_code: Option<i32>,
    pub crash_flag: bool,
    pub log_path: Option<String>,
    pub is_primary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveInstanceRun {
    pub run_id: i64,
    pub session_id: Option<String>,
    pub pid: Option<u32>,
    pub log_path: Option<String>,
    pub process_count: usize,
    pub processes: Vec<InstanceProcessState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceRunRecord {
    pub run_id: i64,
    pub session_id: Option<String>,
    pub status: String,
    pub pid: Option<u32>,
    pub started_at: Option<String>,
    pub stopped_at: Option<String>,
    pub exit_code: Option<i32>,
    pub crash_flag: bool,
    pub log_path: Option<String>,
    pub process_count: usize,
    pub processes: Vec<InstanceProcessState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogTailSnapshot {
    pub source_path: Option<String>,
    pub lines: Vec<String>,
    pub total_lines: usize,
    pub truncated: bool,
    pub read_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeHealth {
    pub status: String,
    pub summary: String,
    pub reason: RuntimeHealthReason,
    pub matched_line: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeHealthReason {
    pub code: String,
    pub params: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeDiagnosticSignal {
    pub code: String,
    pub severity: String,
    pub summary: String,
    pub matched_line: Option<String>,
    pub actionable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimePlayerQueryState {
    pub status: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimePlayerSnapshot {
    pub current_players: Option<usize>,
    pub max_players: Option<usize>,
    pub query: RuntimePlayerQueryState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimePerformanceApplication {
    pub pid: u32,
    pub priority_class: RuntimePriorityClass,
    pub cpu_affinity_mask: Option<u64>,
    pub apply_to_child_processes: bool,
    pub targeted_process_count: usize,
    pub priority_applied_count: usize,
    pub affinity_applied_count: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStartupSchedule {
    pub delay_ms: u64,
    pub instance_stagger_ms: u64,
    pub effective_stagger_ms: u64,
    pub child_process_stagger_ms: u64,
    pub running_instance_count: usize,
    pub active_process_count: usize,
    pub process_count: usize,
    pub queued_start_count: usize,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimePendingRestartSnapshot {
    pub instance_id: String,
    pub instance_name: String,
    pub delay_ms: u64,
    pub recent_crash_count: usize,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeProcessPerformanceSnapshot {
    pub process_key: String,
    pub display_name: String,
    pub pid: u32,
    pub policy: RuntimePerformancePolicy,
    pub application: Option<RuntimePerformanceApplication>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimePerformanceSnapshot {
    #[serde(default)]
    pub applied_resource_limits: Option<RuntimeResourceLimits>,
    pub status: String,
    pub summary: String,
    pub policy: RuntimePerformancePolicy,
    pub preview: RuntimePerformancePolicyPreview,
    pub process_count: usize,
    pub processes: Vec<RuntimeProcessPerformanceSnapshot>,
}

impl Default for RuntimePerformanceSnapshot {
    fn default() -> Self {
        Self {
            applied_resource_limits: None,
            status: String::from("unavailable"),
            summary: String::from("Runtime performance policy snapshot is not available yet."),
            policy: RuntimePerformancePolicy::default(),
            preview: RuntimePerformancePolicyPreview::default(),
            process_count: 0,
            processes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStartupQueueSnapshot {
    pub status: String,
    pub summary: String,
    pub active_run_count: usize,
    pub active_process_count: usize,
    pub tracked_instance_count: usize,
    pub tracked_process_count: usize,
    pub next_start_delay_ms: u64,
    pub projected_effective_stagger_ms: u64,
    pub projected_queued_start_count: usize,
    pub projected_process_count: usize,
    pub pending_restart_count: usize,
    pub next_restart_delay_ms: u64,
    pub next_restart: Option<RuntimePendingRestartSnapshot>,
    pub last_startup_schedule: Option<RuntimeStartupSchedule>,
}

impl Default for RuntimeStartupQueueSnapshot {
    fn default() -> Self {
        Self {
            status: String::from("unknown"),
            summary: String::from("Runtime startup queue snapshot is not available yet."),
            active_run_count: 0,
            active_process_count: 0,
            tracked_instance_count: 0,
            tracked_process_count: 0,
            next_start_delay_ms: 0,
            projected_effective_stagger_ms: 0,
            projected_queued_start_count: 0,
            projected_process_count: 0,
            pending_restart_count: 0,
            next_restart_delay_ms: 0,
            next_restart: None,
            last_startup_schedule: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStabilitySnapshot {
    pub status: String,
    pub summary: String,
    pub recent_crash_count: usize,
    pub last_exit_code: Option<i32>,
    pub restart_policy_enabled: bool,
    pub restart_limit: usize,
    pub restart_backoff_ms: u64,
    pub pending_restart: Option<RuntimePendingRestartSnapshot>,
}

impl Default for RuntimeStabilitySnapshot {
    fn default() -> Self {
        Self {
            status: String::from("unknown"),
            summary: String::from("Runtime stability snapshot is not available yet."),
            recent_crash_count: 0,
            last_exit_code: None,
            restart_policy_enabled: false,
            restart_limit: 0,
            restart_backoff_ms: 0,
            pending_restart: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeWindowSurface {
    pub process_key: String,
    pub display_name: String,
    pub relation: String,
    pub pid: u32,
    pub process_name: String,
    pub window_handle: String,
    pub title: String,
    pub class_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeWindowSnapshot {
    pub instance_id: String,
    pub observed_at_unix_ms: u128,
    pub status: String,
    pub summary: String,
    pub inspected_process_count: usize,
    pub last_suppression_attempt: Option<RuntimeWindowSuppressionAttempt>,
    pub windows: Vec<RuntimeWindowSurface>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeWindowSuppressionResult {
    pub instance_id: String,
    pub source: String,
    pub attempted_at_unix_ms: u128,
    pub inspected_process_count: usize,
    pub visible_window_count_before: usize,
    pub suppressed_window_count: usize,
    pub remaining_visible_window_count: usize,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeWindowSuppressionAttempt {
    pub instance_id: String,
    pub source: String,
    pub status: String,
    pub attempted_at_unix_ms: u128,
    pub inspected_process_count: usize,
    pub visible_window_count_before: usize,
    pub suppressed_window_count: usize,
    pub remaining_visible_window_count: usize,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceRuntimeOverview {
    pub recent_runs: Vec<InstanceRunRecord>,
    pub log_tail: LogTailSnapshot,
    pub health: RuntimeHealth,
    pub diagnostics: Vec<RuntimeDiagnosticSignal>,
    pub players: RuntimePlayerSnapshot,
    pub performance: RuntimePerformanceSnapshot,
    pub startup_queue: RuntimeStartupQueueSnapshot,
    pub stability: RuntimeStabilitySnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorServiceProbe {
    pub key: String,
    pub transport: String,
    pub enabled: bool,
    pub configured: bool,
    pub endpoint: Option<String>,
    pub reachable: bool,
    pub status: String,
    pub detail: String,
    pub http_status: Option<u16>,
}

pub type PalworldOperatorServiceProbe = OperatorServiceProbe;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PalworldOperatorSnapshot {
    pub instance_id: String,
    pub checked_at_unix_ms: u128,
    pub services: Vec<OperatorServiceProbe>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SevenDaysOperatorSnapshot {
    pub instance_id: String,
    pub checked_at_unix_ms: u128,
    pub operator_host: String,
    pub services: Vec<OperatorServiceProbe>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessLaunchPlan {
    pub process_key: String,
    pub display_name: String,
    pub log_path: String,
    pub launch_plan: LaunchPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartedProcess {
    pub run_id: i64,
    pub process_key: String,
    pub display_name: String,
    pub pid: u32,
    pub log_path: String,
    pub performance: RuntimePerformanceApplication,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartInstanceResult {
    pub summary: InstanceSummary,
    pub run_id: i64,
    pub session_id: Option<String>,
    pub pid: u32,
    pub log_path: String,
    pub launch_plan: LaunchPlan,
    pub process_count: usize,
    pub processes: Vec<StartedProcess>,
    pub launch_plans: Vec<ProcessLaunchPlan>,
    pub startup_schedule: RuntimeStartupSchedule,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoppedProcess {
    pub run_id: i64,
    pub process_key: String,
    pub display_name: String,
    pub pid: Option<u32>,
    pub log_path: Option<String>,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopInstanceResult {
    pub summary: InstanceSummary,
    pub run_id: i64,
    pub session_id: Option<String>,
    pub pid: Option<u32>,
    pub log_path: Option<String>,
    pub exit_code: Option<i32>,
    pub process_count: usize,
    pub processes: Vec<StoppedProcess>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundJob {
    pub id: String,
    pub kind: JobKind,
    pub label: String,
    pub status: JobStatus,
    pub progress_percent: f32,
    #[serde(default)]
    pub install_progress: Option<InstallProgress>,
    #[serde(default)]
    pub cancellable: bool,
    #[serde(default)]
    pub cancel_requested: bool,
    pub target_id: Option<String>,
    pub detail: Option<String>,
    pub output_excerpt: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CpuCoreSnapshot {
    pub name: String,
    pub utility_percent: f32,
    pub performance_percent: f32,
    pub frequency_mhz: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryModuleSnapshot {
    pub bank_label: String,
    pub device_locator: String,
    pub manufacturer: String,
    pub part_number: String,
    pub capacity_bytes: u64,
    pub speed_mts: u32,
    pub configured_clock_mts: u32,
    pub configured_voltage_mv: u32,
    pub memory_type: String,
    pub inferred_cas_latency: Option<u32>,
    pub timing_summary: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetworkAdapterSnapshot {
    pub rate_status: TelemetryStatus,
    pub name: String,
    pub description: String,
    pub status: String,
    pub family_name: Option<String>,
    pub ipv4_addresses: Vec<String>,
    pub mac_address: Option<String>,
    pub link_speed_bps: u64,
    pub received_bytes: u64,
    pub transmitted_bytes: u64,
    pub receive_bps: u64,
    pub transmit_bps: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppState {
    pub settings: AppSettings,
    pub storage: StorageStatus,
    pub modules: Vec<ModuleSummary>,
    pub instances: Vec<InstanceSummary>,
    pub jobs: Vec<BackgroundJob>,
    pub snapshot: SystemSnapshot,
}

impl AppState {
    pub fn bootstrap_default() -> Self {
        Self::default()
    }
}
