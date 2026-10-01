
import type { SystemDiskVolume, SystemTelemetry, TelemetrySampleStatus } from "./system-resource-types";

export type ViewKey = "system" | "servers" | "library";

export type ThemeMode = "light" | "dark";

export type ServerWorkspaceSection = "overview" | "settings";

export interface AppSettings {
  servers_root: string;
  games_root: string;
  archives_root: string;
  modules_root: string;
  steamcmd_root: string;
}

export interface AppPathSettingsInput {
  servers_root: string;
  games_root: string;
  archives_root: string;
  steamcmd_root: string;
}
export interface StorageStatus {
  database_path: string;
  migrations_path: string;
  app_log_path: string;
  database_exists: boolean;
  schema_version: number;
  migrations_applied: boolean;
}

export type InstallState = "NotInstalled" | "Installing" | "Incomplete" | "Installed" | "Updating" | "Uninstalling" | "Corrupted" | string;
export type InstanceStatus = "Stopped" | "Starting" | "Running" | "Stopping" | "Error" | string;

export interface ModuleSummary {
  id: string;
  name: string;
  version: string;
  description?: string | null;
  steam_app_id?: number | null;
  install_state: InstallState;
  instance_program_count?: number;
  archived_program_count?: number;
  supported_platforms: string[];
}

export interface PortBinding {
  name: string;
  port: number;
  protocol: string;
}

export interface DefaultPortDescriptor {
  name: string;
  port: number;
  protocol: string;
}

export interface ModuleInstallDetails {
  shared_game_dir?: string | null;
  download_url_windows?: string | null;
  download_integrity_windows?: { sha256: string; size: number } | null;
  source?: "minecraft_java" | string | null;
  verification_path?: string | null;
  minecraft?: {
    version?: string | null;
    manifest_url?: string | null;
    server_jar?: string | null;
    java_policy?: string | null;
    default_distribution?: string | null;
    distributions?: ModuleMinecraftDistributionDetails[] | null;
  } | null;
}

export interface ModuleMinecraftDistributionDetails {
  id: string;
  label: string;
  server_jar: string;
  source?: string | null;
  entrypoint_kind?: string | null;
  supports_mods: boolean;
  supports_plugins: boolean;
  supports_datapacks: boolean;
  supports_resource_packs: boolean;
}

export interface ModuleProcessDetails {
  executable: string;
  args_template?: string[] | null;
  environment_template?: Record<string, string>;
  working_directory_template?: string | null;
  window_policy?: "background" | "external" | string;
  host_surface?: "managed_terminal" | "managed_native_window" | "external_window" | string;
  host_notes?: string | null;
}

export interface ModuleWorkshopDetails {
  provider: string;
  consumer_app_id?: number | null;
  supports_collections: boolean;
}

export interface ModuleModSourceDetails {
  provider: string;
  label: string;
  url: string;
  loaders?: string[];
  game_versions?: string[];
  install_note?: string | null;
}

export interface ModuleManualModStagingDetails {
  target_template: string;
  target_label: string;
  accepts?: string[];
}

export interface ModuleModEnablementDetails {
  setting_key: string;
  setting_label: string;
  id_strategy?: string | null;
  reference_strategy?: string | null;
  reference_game_id?: number | null;
}

export interface ModuleModsDetails {
  source?: ModuleModSourceDetails | null;
  manual_staging?: ModuleManualModStagingDetails | null;
  enablement?: ModuleModEnablementDetails | null;
}

export interface ModulePlayerQueryDetails {
  protocol: string;
  port_names: string[];
}

export type ModulePlayerListScope = "online";

export type ModulePlayerListSource = "runtime_action" | "structured_log" | "http_api" | "server_query" | "console_log" | "tcp_console" | "native_console" | "file_ipc";

export type ModulePlayerListCodec =
  | "dst_client_table_v1"
  | "rust_player_list"
  | "ark_list_players"
  | "conan_list_players"
  | "humanitz_players"
  | "zomboid_players"
  | "seven_days_players"
  | "squad_list_players"
  | "palworld_players"
  | "minecraft_players"
  | "nightingale_players"
  | "necesse_players"
  | "romestead_players"
  | "terraria_players"
  | "astroneer_players"
  | "soulmask_players"
  | "satisfactory_frm_players"
  | "barotrauma_players"
  | "return_to_moria_players"
  | "windrose_players"
  | "dragonwilds_players"
  | "scum_players"
  | "a2s_players";

export type RuntimePlayerIdentityKind =
  | "klei_user_id"
  | "steam_id"
  | "ark_account_id"
  | "conan_user_id"
  | "eos_id"
  | "player_name"
  | "session_id"
  | "palworld_user_id"
  | "minecraft_uuid"
  | "astroneer_guid";

export interface ModulePlayerListSpec {
  scope: ModulePlayerListScope;
  source: ModulePlayerListSource;
  action_id: string | null;
  player_action_ids: string[];
  response_codec: ModulePlayerListCodec;
  identity_kind: RuntimePlayerIdentityKind;
  refresh_interval_ms: number;
}

export type RuntimeLivePlayerStatus =
  | "ready"
  | "refreshing"
  | "stopped"
  | "unsupported"
  | "misconfigured"
  | "failed";

export type RuntimeLivePlayerIssueCode =
  | "process_unavailable"
  | "process_untracked"
  | "log_unavailable"
  | "collection_timeout"
  | "protocol_incomplete"
  | "capture_limit"
  | "io_failed"
  | "runtime_action_unavailable"
  | "adapter_unavailable"
  | "names_unavailable"
  | "query_unavailable"
  | "authentication_failed"
  | "extension_unavailable";

export interface RuntimeLivePlayerIdentifier {
  kind: RuntimePlayerIdentityKind;
  value: string;
  stable: boolean;
}

export interface RuntimeLivePlayerAttribute {
  key: string;
  value: string;
}

export interface RuntimeLivePlayerEntry {
  player_key: string;
  display_name: string;
  identifiers: RuntimeLivePlayerIdentifier[];
  available_action_ids: string[];
  ping_ms: number | null;
  session_started_at_unix_ms: number | null;
  role: string | null;
  attributes: RuntimeLivePlayerAttribute[];
}

export interface RuntimeLivePlayerIssue {
  code: RuntimeLivePlayerIssueCode;
  setting_keys: string[];
  summary: string;
}

export interface RuntimeLivePlayerSnapshot {
  snapshot_id: string;
  instance_id: string;
  status: RuntimeLivePlayerStatus;
  source: ModulePlayerListSource | null;
  observed_at_unix_ms: number | null;
  expires_at_unix_ms: number | null;
  complete: boolean;
  truncated: boolean;
  stale: boolean;
  current_players: number | null;
  max_players: number | null;
  entries: RuntimeLivePlayerEntry[];
  issue: RuntimeLivePlayerIssue | null;
}

export interface ExecuteInstancePlayerActionInput {
  instance_id: string;
  snapshot_id: string;
  player_key: string;
  action_id: string;
}

export interface ExecuteInstanceManualPlayerActionInput {
  instance_id: string;
  action_id: string;
  target: string;
  role: string | null;
}

export type RuntimeLivePlayerActionStatus = "sent";

export interface ExecuteInstancePlayerActionResult {
  action_id: string;
  status: RuntimeLivePlayerActionStatus;
  executed_at_unix_ms: number;
  summary: string;
}

export interface ModulePlayerActionDetails {
  id: string;
  kind?: string | null;
  label: string;
  label_zh_cn?: string | null;
  transport?: "stdin" | "source_rcon" | "websocket_rcon" | "battleye_rcon" | "telnet" | string | null;
  command_template: string;
  target_label?: string | null;
  target_label_zh_cn?: string | null;
  target_placeholder?: string | null;
  target_placeholder_zh_cn?: string | null;
  target_required?: boolean;
  target_encoding?: "raw" | "quoted_string" | string | null;
  role_values?: string[];
  process_key?: string | null;
  port_name?: string | null;
  password_setting_key?: string | null;
  enabled_setting_key?: string | null;
  destructive?: boolean;
}

export interface ModulePlayerManagementDetails {
  status: string;
  planned_surface: string;
  reason: string;
  verification: string;
}

export interface ModuleShutdownCommandDetails {
  transport?: "stdin" | "source_rcon" | "websocket_rcon" | "battleye_rcon" | "telnet" | "console_ctrl_c" | "window_close" | string | null;
  fallback_transport?: "stdin" | "source_rcon" | "websocket_rcon" | "battleye_rcon" | "telnet" | "console_ctrl_c" | "window_close" | string | null;
  command: string;
  process_key?: string | null;
  port_name?: string | null;
  password_setting_key?: string | null;
  enabled_setting_key?: string | null;
  wait_after_ms?: number;
}

export interface ModuleShutdownDetails {
  commands: ModuleShutdownCommandDetails[];
  grace_period_ms: number;
}

export interface ModuleStorageDetails {
  saves_path_template: string;
}

export interface ModulePortGroupDetails {
  id: string;
  members: string[];
  member_offsets?: Record<string, number>;
}

export interface ModuleBindAddressDetails {
  mode: "unsupported" | "strict";
  port_names: string[];
  required_setting_key?: string | null;
  startup_timeout_ms: number;
}

export type ModulePortRole = "player" | "service";

export interface ModulePortRoleDetails {
  role: ModulePortRole;
  port_names: string[];
}

export interface ModuleRuntimeDetails {
  program_sharing?: InstanceProgramMode;
  bind_address?: ModuleBindAddressDetails;
  port_roles?: ModulePortRoleDetails[];
  port_groups?: ModulePortGroupDetails[];
  player_count_source?: "player_query" | "player_list";
  player_query?: ModulePlayerQueryDetails | null;
  player_actions?: ModulePlayerActionDetails[];
  player_list?: ModulePlayerListSpec | null;
  player_management?: ModulePlayerManagementDetails | null;
  shutdown?: ModuleShutdownDetails | null;
  performance?: RuntimePerformancePolicy | null;
}

export interface ModuleDetails {
  summary: ModuleSummary;
  schema_json?: string | null;
  default_ports: DefaultPortDescriptor[];
  install?: ModuleInstallDetails | null;
  process?: ModuleProcessDetails | null;
  workshop?: ModuleWorkshopDetails | null;
  mods?: ModuleModsDetails | null;
  storage?: ModuleStorageDetails | null;
  runtime: ModuleRuntimeDetails;
}

export interface InstanceProcessRecord {
  run_id?: number | null;
  session_id?: string | null;
  process_key?: string | null;
  display_name?: string | null;
  pid?: number | null;
  status?: string | null;
  started_at?: string | null;
  stopped_at?: string | null;
  exit_code?: number | null;
  crash_flag?: boolean | null;
  log_path?: string | null;
  command_line?: string | null;
  is_primary?: boolean | null;
}

export interface RunSummary {
  run_id: number;
  session_id?: string | null;
  status: string;
  pid?: number | null;
  started_at?: string | null;
  stopped_at?: string | null;
  exit_code?: number | null;
  crash_flag?: boolean | null;
  log_path?: string | null;
  process_count?: number | null;
  processes?: InstanceProcessRecord[];
}

export interface InstanceSummary {
  id: string;
  name: string;
  module_id: string;
  status: InstanceStatus;
  active_process_count: number;
  autostart: boolean;
  bind_ip: string;
  port_count?: number | null;
}

export interface InstanceActiveRun {
  run_id: number;
  session_id?: string | null;
  pid?: number | null;
  log_path?: string | null;
  process_count?: number | null;
  processes?: InstanceProcessRecord[];
}

export type InstanceProgramMode = "shared" | "independent";
export type InstanceIsolationMode = "private" | "shared" | "damaged";
export type InstanceIsolationConflictKind = "configuration" | "saves" | "runtime";

export interface InstanceIsolationConflict {
  instance_id: string;
  instance_name: string;
  kind: InstanceIsolationConflictKind;
  path: string;
  other_path: string;
}

export interface InstanceIsolationReport {
  instance_id: string;
  mode: InstanceIsolationMode;
  runtime_path: string;
  data_path: string;
  config_path: string;
  saves_path: string;
  conflicts: InstanceIsolationConflict[];
  issues: string[];
}

export interface InstanceConnectionInfo {
  instance_id: string;
  bind_ip: string;
  ports: PortBinding[];
  settings_json: string;
}

export interface InstanceDetails {
  summary: InstanceSummary;
  ports: PortBinding[];
  settings_json: string;
  config_file_path: string;
  saves_path: string;
  backup_uses_declared_saves_path: boolean;
  auto_backup_on_stop: boolean;
  backup_retention_count: number;
  active_run?: InstanceActiveRun | null;
}

export type InstanceBackupKind = "manual" | "auto_stop" | "pre_restore";

export interface InstanceBackupResult {
  backup_id: string;
  instance_id: string;
  backup_kind: InstanceBackupKind;
  display_name?: string | null;
  created_at_unix_ms: number;
  backup_path: string;
  saves_path: string;
  file_count: number;
  total_bytes: number;
}

export interface InstanceBackupRestoreResult {
  instance_id: string;
  backup_id: string;
  restored_at_unix_ms: number;
  saves_path: string;
  restored_file_count: number;
  restored_total_bytes: number;
  safeguard_backup_id: string;
  safeguard_backup_path: string;
}

export interface InstanceArchiveResult {
  archive_id: string;
  external_saves_backup_id: string | null;
  instance_id: string;
  instance_name: string;
  module_id: string;
  deleted_at_unix_ms: number;
  previous_instance_root: string;
  archived_instance_root?: string | null;
  effective_saves_path: string;
  saves_archived_with_instance_root: boolean;
  preserved_external_saves_path?: string | null;
}

export interface InstanceDeletionResult {
  instance_id: string;
  instance_name: string;
  module_id: string;
  deleted_at_unix_ms: number;
  deleted_instance_root: string;
  preserved_external_saves_path: string | null;
  program_cleanup: ProgramCleanupResult;
}

export type DstWorldState = "new" | "existing" | "unrecognized";

export interface DstWorldStartPreview {
  instance_id: string;
  settings_json: string;
  shards: { shard: "Master" | "Caves" | "Islands" | "Volcano"; state: DstWorldState; enabled: boolean }[];
}

export interface DstWorldImportResult {
  instance_id: string;
  source_cluster_path: string;
  target_cluster_path: string;
  safeguard_path: string;
  imported_master: boolean;
  imported_caves: boolean;
  imported_shards: string[];
  imported_workshop_mod_ids: string[];
  copied_file_count: number;
  copied_total_bytes: number;
}


export interface RuntimeHealthSummary {
  status?: string | null;
  summary: string;
  reason?: { code: string; params: Record<string, string> };
  matched_line?: string | null;
}

export interface RuntimeDiagnosticSignal {
  code: string;
  severity: string;
  summary: string;
  matched_line?: string | null;
  actionable: boolean;
}

export interface LogTailSnapshot {
  snapshot_revision?: number;
  source_path?: string | null;
  total_lines?: number;
  lines: string[];
  truncated?: boolean | null;
  read_error?: string | null;
}

export type RuntimeLogSource = "game" | "console";

export interface RuntimePlayerSnapshot {
  current_players?: number | null;
  max_players?: number | null;
  query: RuntimePlayerQueryState;
}

export interface RuntimePlayerQueryState {
  status: string;
  summary: string;
}

export type RuntimePriorityClass = "idle" | "below_normal" | "normal" | "above_normal" | "high";

export interface RuntimeResourceLimits {
  cpu_percent: number | null;
  memory_limit_mib: number | null;
  host_memory_reserve_mib: number;
}

export interface RuntimePerformancePolicy {
  resource_limits: RuntimeResourceLimits;
  priority_class: RuntimePriorityClass;
  cpu_affinity_mask?: number | null;
  apply_to_child_processes: boolean;
  startup_stagger_ms: number;
  child_process_stagger_ms: number;
}

export interface RuntimePerformancePolicyPreview {
  summary: string;
  priority_source: string;
  cpu_affinity_source: string;
  cpu_affinity_preset?: string | null;
  logical_cpu_count: number;
}

export interface RuntimePerformanceApplication {
  pid: number;
  priority_class: RuntimePriorityClass;
  cpu_affinity_mask?: number | null;
  apply_to_child_processes: boolean;
  targeted_process_count: number;
  priority_applied_count: number;
  affinity_applied_count: number;
  warnings: string[];
}

export interface RuntimeStartupSchedule {
  delay_ms: number;
  instance_stagger_ms: number;
  effective_stagger_ms: number;
  child_process_stagger_ms: number;
  running_instance_count: number;
  active_process_count: number;
  process_count: number;
  queued_start_count: number;
  reason: string;
}

export interface RuntimePendingRestartSnapshot {
  instance_id: string;
  instance_name: string;
  delay_ms: number;
  recent_crash_count: number;
  exit_code?: number | null;
}

export interface RuntimeProcessPerformanceSnapshot {
  process_key: string;
  display_name: string;
  pid: number;
  policy: RuntimePerformancePolicy;
  application?: RuntimePerformanceApplication | null;
}

export interface RuntimePerformanceSnapshot {
  applied_resource_limits: RuntimeResourceLimits | null;
  status: string;
  summary: string;
  policy: RuntimePerformancePolicy;
  preview: RuntimePerformancePolicyPreview;
  process_count: number;
  processes: RuntimeProcessPerformanceSnapshot[];
}

export interface RuntimeStartupQueueSnapshot {
  status: string;
  summary: string;
  active_run_count: number;
  active_process_count: number;
  tracked_instance_count: number;
  tracked_process_count: number;
  next_start_delay_ms: number;
  projected_effective_stagger_ms: number;
  projected_queued_start_count: number;
  projected_process_count: number;
  pending_restart_count: number;
  next_restart_delay_ms: number;
  next_restart?: RuntimePendingRestartSnapshot | null;
  last_startup_schedule?: RuntimeStartupSchedule | null;
}

export interface RuntimeStabilitySnapshot {
  status: string;
  summary: string;
  recent_crash_count: number;
  last_exit_code?: number | null;
  restart_policy_enabled: boolean;
  restart_limit: number;
  restart_backoff_ms: number;
  pending_restart?: RuntimePendingRestartSnapshot | null;
}

export interface RuntimeWindowSurface {
  process_key: string;
  display_name: string;
  relation: string;
  pid: number;
  process_name: string;
  window_handle: string;
  title: string;
  class_name: string;
}

export interface RuntimeWindowSnapshot {
  instance_id: string;
  observed_at_unix_ms: number;
  status: string;
  summary: string;
  inspected_process_count: number;
  last_suppression_attempt?: RuntimeWindowSuppressionAttempt | null;
  windows: RuntimeWindowSurface[];
}

export interface RuntimeWindowSuppressionResult {
  instance_id: string;
  source: string;
  attempted_at_unix_ms: number;
  inspected_process_count: number;
  visible_window_count_before: number;
  suppressed_window_count: number;
  remaining_visible_window_count: number;
  summary: string;
}

export interface RuntimeWindowSuppressionAttempt {
  instance_id: string;
  source: string;
  status: string;
  attempted_at_unix_ms: number;
  inspected_process_count: number;
  visible_window_count_before: number;
  suppressed_window_count: number;
  remaining_visible_window_count: number;
  summary: string;
}

export interface RuntimeCommandDispatchOptions {
  silent?: boolean;
  throwOnError?: boolean;
  transport?: string;
  portName?: string | null;
  passwordSettingKey?: string | null;
  enabledSettingKey?: string | null;
  runtimeActionId?: string | null;
  runtimeActionTarget?: string | null;
  runtimeActionRole?: string | null;
}

export interface SaveInstanceSettingsOptions {
  expectedSettingsJson?: string;
  collectionRemoval?: { collectionId: string; memberIds: string[]; retainCollection?: boolean };
  throwOnError?: boolean;
  silent?: boolean;
}

export interface InstanceRuntimeCommandResult {
  instance_id: string;
  process_key: string;
  display_name: string;
  pid: number;
  command: string;
  response_text?: string | null;
  write_confirmation_pending: boolean;
  submitted_at_unix_ms: number;
}

export type PlayerAccessMutationOperation = "add" | "remove";
export type PlayerAccessPersistentStatus = "updated" | "unchanged";
export type PlayerAccessLiveStatus =
  | "applied"
  | "sent_unverified"
  | "not_running"
  | "restart_required"
  | "failed";
export type PlayerAccessVerificationStatus = "verified" | "unavailable" | "failed";

export interface InstancePlayerAccessMutationInput {
  instanceId: string;
  fieldKey: string;
  operation: PlayerAccessMutationOperation;
  value: unknown;
  expectedValue?: unknown;
}

export interface InstancePlayerAccessMutationResult {
  instanceId: string;
  fieldKey: string;
  operation: PlayerAccessMutationOperation;
  persistentStatus: PlayerAccessPersistentStatus;
  liveStatus: PlayerAccessLiveStatus;
  liveTarget?: string;
  verificationStatus: PlayerAccessVerificationStatus;
  liveError?: string | null;
  verificationError?: string | null;
  verificationResponse?: string | null;
}

export type InstanceBroadcastSource = "manual" | "startup" | "shutdown" | "runtime_health" | "periodic" | string;
export type InstanceBroadcastStatus = "generated" | "sent" | "failed" | "blocked" | string;

export interface InstanceBroadcastRule {
  enabled: boolean;
  prompt?: string | null;
}

export interface InstanceBroadcastPeriodicRule {
  enabled: boolean;
  interval_minutes: number;
  prompt?: string | null;
}

export interface InstanceBroadcastRules {
  startup: InstanceBroadcastRule;
  shutdown: InstanceBroadcastRule;
  runtime_health: InstanceBroadcastRule;
  periodic: InstanceBroadcastPeriodicRule;
  tone: string;
  cooldown_minutes: number;
}

export interface InstanceBroadcastPolicy {
  instance_id: string;
  enabled: boolean;
  rules: InstanceBroadcastRules;
  updated_at_unix_ms: number;
}

export interface UpdateInstanceBroadcastPolicyInput {
  instance_id: string;
  enabled: boolean;
  rules: InstanceBroadcastRules;
}

export interface InstanceBroadcastEvent {
  event_id: string;
  instance_id: string;
  module_id: string;
  source: InstanceBroadcastSource;
  rule_id?: string | null;
  message: string;
  ai_provider?: string | null;
  ai_model?: string | null;
  action_id?: string | null;
  transport?: string | null;
  command_preview?: string | null;
  status: InstanceBroadcastStatus;
  response_text?: string | null;
  error_message?: string | null;
  initiator?: "manual" | "auto" | "lifecycle" | "system" | string | null;
  policy_snapshot_json?: string | null;
  created_at_unix_ms: number;
}

export interface GenerateInstanceBroadcastInput {
  instanceId: string;
  settings: AssistantProviderSettingsInput;
  intent: string;
  tone?: string | null;
  source?: InstanceBroadcastSource | null;
  ruleId?: string | null;
  initiator?: "manual" | "auto" | "lifecycle" | "system" | string | null;
  policySnapshotJson?: string | null;
}

export interface GenerateInstanceBroadcastOutput {
  message: string;
  provider: string;
  model: string;
  endpointUrl: string;
  event: InstanceBroadcastEvent;
}

export interface SendInstanceBroadcastInput {
  instanceId: string;
  message: string;
  source?: InstanceBroadcastSource | null;
  ruleId?: string | null;
  aiProvider?: string | null;
  aiModel?: string | null;
  initiator?: "manual" | "auto" | "lifecycle" | "system" | string | null;
  policySnapshotJson?: string | null;
}

export interface SendInstanceBroadcastOutput {
  event: InstanceBroadcastEvent;
  actionId: string;
  transport: string;
  commandPreview: string;
}

export interface InstanceRuntimeOverview {
  recent_runs: RunSummary[];
  health: RuntimeHealthSummary;
  diagnostics: RuntimeDiagnosticSignal[];
  log_tail: LogTailSnapshot;
  players: RuntimePlayerSnapshot;
  performance: RuntimePerformanceSnapshot;
  startup_queue: RuntimeStartupQueueSnapshot;
  stability: RuntimeStabilitySnapshot;
}

export interface LaunchPlan {
  install_state: InstallState;
  uses_private_runtime: boolean;
  instance_id: string;
  instance_name?: string;
  module_id?: string;
  executable_path: string;
  executable_exists: boolean;
  ready_to_launch: boolean;
  validation_issues: Array<{
    code: string;
    severity: string;
    message: string;
    path?: string | null;
    context?: Record<string, string>;
  }>;
  working_directory: string;
  install_root: string;
  args?: string[];
  environment: Record<string, string>;
  command_line: string;
  window_policy?: "background" | "external" | string;
  uses_script_entrypoint?: boolean;
  host_surface?: "managed_terminal" | "managed_native_window" | "external_window" | string;
  host_notes?: string | null;
  performance_policy?: RuntimePerformancePolicy;
  performance_preview?: RuntimePerformancePolicyPreview;
}

export interface ModuleInstallResult {
  module_id: string;
  steam_app_id?: number | null;
  operation: string;
  install_root: string;
  executable_path: string;
  executable_exists: boolean;
  install_state: string;
  current_version?: string | null;
  output_excerpt?: string | null;
}

export interface ProgramCleanupRetention {
  install_root: string;
  reason: string;
}

export interface ProgramCleanupResult {
  removed_install_roots: string[];
  preserved_data_paths: string[];
  retained_installs: ProgramCleanupRetention[];
}

export interface ModuleUninstallResult {
  module_id: string;
  install_state: InstallState;
  executable_exists: boolean;
  cleanup: ProgramCleanupResult;
}

export interface SteamWorkshopDownloadItemResult {
  item_id: string;
  expected_path: string;
  expected_path_exists: boolean;
}

export interface SteamWorkshopDownloadResult {
  consumer_app_id: number;
  install_root: string;
  workshop_root: string;
  items: SteamWorkshopDownloadItemResult[];
  output_excerpt?: string | null;
}

export interface SteamWorkshopInstallationItemStatus {
  item_id: string;
  path: string;
  installed: boolean;
}

export interface SteamWorkshopInstallationSnapshot {
  consumer_app_id: number;
  searched_roots: string[];
  items: SteamWorkshopInstallationItemStatus[];
}

export interface ManualModStageItem {
  source_path: string;
  target_path: string;
  status: string;
  message?: string | null;
  file_count: number;
  total_bytes: number;
}

export interface ManualModStageResult {
  instance_id: string;
  module_id: string;
  source_label: string;
  target_label: string;
  target_path: string;
  affected_root_names: string[];
  items: ManualModStageItem[];
  copied_file_count: number;
  copied_total_bytes: number;
}

export interface ManualModInventoryItem {
  name: string;
  path: string;
  item_type: "directory" | "file";
  inferred_id?: string | null;
  file_count: number;
  total_bytes: number;
  modified_unix_ms?: number | null;
}

export interface ManualModInventoryResult {
  instance_id: string;
  module_id: string;
  source_label: string;
  target_label: string;
  target_path: string;
  target_exists: boolean;
  items: ManualModInventoryItem[];
}

export interface ManualModReferenceItem {
  reference: string;
  status: "resolved" | "failed";
  resolved_id?: string | null;
  title?: string | null;
  message?: string | null;
}

export interface ManualModReferenceResolveResult {
  instance_id: string;
  module_id: string;
  source_label: string;
  setting_key: string;
  setting_label: string;
  items: ManualModReferenceItem[];
  resolved_ids: string[];
}

export interface InstanceProvisioning {
  summary: InstanceSummary;
  config_file_path: string;
  ports: PortBinding[];
}

export interface InstallProgress {
  phase: "queued" | "preparing" | "downloading" | "extracting" | "installing" | "verifying" | "ready";
  downloaded_bytes: number | null;
  total_bytes: number | null;
  percent: number | null;
  elapsed_seconds: number;
}

export interface BackgroundJob {
  cancellable: boolean;
  cancel_requested: boolean;
  id: string;
  label: string;
  kind: string;
  status: string;
  progress_percent?: number | null;
  target_id?: string | null;
  detail?: string | null;
  output_excerpt?: string | null;
  install_progress?: InstallProgress | null;
}

export interface OverlayFamily {
  name: string;
  adapter_name_patterns?: string[];
  executable_name_patterns?: string[];
}

export interface BindAddressCandidate {
  address: string;
  kind: string;
  adapter_name?: string | null;
  family_name?: string | null;
}

export type SteamCmdSource = "configured" | "discovered" | string;

export type SteamCmdOwnership = "managed" | "external" | "none" | "invalid";

export interface SteamCmdStatus {
  ready: boolean;
  root: string;
  executable_path: string;
  executable_exists: boolean;
  configured_root: string;
  configured_executable_path: string;
  source: SteamCmdSource;
  ownership: SteamCmdOwnership;
  can_uninstall: boolean;
}

export interface SteamCmdPrepareSnapshot {
  cancellable: boolean;
  cancel_requested: boolean;
  cancelled: boolean;
  operation_id: string;
  active: boolean;
  phase: "queued" | "inspecting" | "downloading" | "extracting" | "updating" | "verifying" | "ready";
  detail: string;
  downloaded_bytes: number | null;
  total_bytes: number | null;
  output_excerpt: string;
  elapsed_seconds: number;
  idle_seconds: number;
  error: string | null;
}

export type DstModPrimitiveValue =
  | { kind: "string"; value: string }
  | { kind: "number"; value: number }
  | { kind: "boolean"; value: boolean }
  | { kind: "default" };

export interface DstModConfigChoice {
  label: string;
  hover?: string | null;
  value: DstModPrimitiveValue;
}

export interface DstModConfigOptionSpec {
  name: string;
  label: string;
  hover?: string | null;
  default_value?: DstModPrimitiveValue | null;
  options: DstModConfigChoice[];
}

export interface DstModConfigurationSpec {
  client_only: boolean;
  mod_id: string;
  mod_dir: string;
  modinfo_path: string;
  mod_name?: string | null;
  description?: string | null;
  status: string;
  message?: string | null;
  options: DstModConfigOptionSpec[];
}

export interface ProjectZomboidLocalModSpec {
  directory_name: string;
  mod_id?: string | null;
  mod_name?: string | null;
  mod_path: string;
  mod_info_path?: string | null;
  map_ids: string[];
  status: string;
  message?: string | null;
}

export interface ProjectZomboidWorkshopItemSpec {
  workshop_item_id: string;
  item_path: string;
  mods_path?: string | null;
  status: string;
  message?: string | null;
  mods: ProjectZomboidLocalModSpec[];
}

export interface ProjectZomboidWorkshopModsSnapshot {
  workshop_root: string;
  workshop_root_exists: boolean;
  items: ProjectZomboidWorkshopItemSpec[];
}

export interface SystemSnapshot {
  telemetry?: SystemTelemetry | null;
  memory_commit_used_bytes?: number | null;
  memory_commit_limit_bytes?: number | null;
  disk_volumes?: SystemDiskVolume[] | null;
  cpu_percent?: number | null;
  cpu_name?: string | null;
  cpu_frequency_mhz?: number | null;
  cpu_max_frequency_mhz?: number | null;
  cpu_physical_cores?: number | null;
  cpu_logical_cores?: number | null;
  cpu_single_core_peak_percent?: number | null;
  cpu_performance_percent?: number | null;
  cpu_cores?: CpuCoreSnapshot[] | null;
  memory_percent?: number | null;
  memory_total_bytes?: number | null;
  memory_available_bytes?: number | null;
  memory_modules?: MemoryModuleSnapshot[] | null;
  disk_used_percent?: number | null;
  disk_used_bytes?: number | null;
  disk_total_bytes?: number | null;
  disk_label?: string | null;
  disk_volume_id?: string | null;
  disk_model?: string | null;
  disk_volume_name?: string | null;
  disk_file_system?: string | null;
  disk_read_bps?: number | null;
  disk_write_bps?: number | null;
  disk_read_latency_ms?: number | null;
  disk_write_latency_ms?: number | null;
  disk_queue_length?: number | null;
  network_receive_bps?: number | null;
  network_transmit_bps?: number | null;
  network_adapters?: NetworkAdapterSnapshot[] | null;
  instance_process_memory_bytes?: number | null;
  instance_process_memory_percent?: number | null;
  instance_process_count?: number | null;
  instance_process_threads?: number | null;
  instance_process_handles?: number | null;
  running_instances?: number | null;
  total_online_players?: number | null;
  total_player_capacity?: number | null;
  player_count_queried_instances?: number | null;
  player_count_queryable_instances?: number | null;
}

export interface CpuCoreSnapshot {
  name: string;
  utility_percent: number;
  performance_percent: number;
  frequency_mhz: number;
}

export interface MemoryModuleSnapshot {
  bank_label: string;
  device_locator: string;
  manufacturer: string;
  part_number: string;
  capacity_bytes: number;
  speed_mts: number;
  configured_clock_mts: number;
  configured_voltage_mv: number;
  memory_type: string;
  inferred_cas_latency?: number | null;
  timing_summary: string;
}

export interface NetworkAdapterSnapshot {
  rate_status?: TelemetrySampleStatus | null;
  name: string;
  description: string;
  status: string;
  family_name?: string | null;
  ipv4_addresses: string[];
  mac_address?: string | null;
  link_speed_bps: number;
  received_bytes: number;
  transmitted_bytes: number;
  receive_bps: number;
  transmit_bps: number;
}

export interface BootstrapResponse {
  booted_at_unix_ms: number;
  state: {
    settings: AppSettings;
    storage: StorageStatus;
    snapshot: SystemSnapshot;
    modules: ModuleSummary[];
    instances: InstanceSummary[];
    jobs: BackgroundJob[];
  };
}

export interface CreateInstanceInput {
  name: string;
  module_id: string;
  program_mode?: InstanceProgramMode;
}

export interface UpdateInstanceInput {
  id: string;
  bind_ip: string;
  auto_backup_on_stop: boolean;
  backup_retention_count: number;
  settings_json: string;
  ports: PortBinding[];
}
export interface SteamWorkshopLookupChild {
  id: string;
  item_kind: string;
  status: string;
  title?: string | null;
  preview_url?: string | null;
  consumer_app_id?: number | null;
  tags?: string[];
}

export interface SteamWorkshopLookupItem {
  id: string;
  title?: string | null;
  preview_url?: string | null;
  description?: string | null;
  description_excerpt?: string | null;
  detail_url: string;
  item_kind: string;
  status: string;
  message?: string | null;
  localization_warning?: string | null;
  consumer_app_id?: number | null;
  creator_app_id?: number | null;
  creator_id?: string | null;
  file_size?: number | null;
  created_at_unix?: number | null;
  updated_at_unix?: number | null;
  subscriptions?: number | null;
  favorites?: number | null;
  views?: number | null;
  tags?: string[];
  child_count: number;
  children: SteamWorkshopLookupChild[];
}

export type SteamWorkshopBrowseKind = "item" | "collection";

export interface SteamWorkshopSearchResult {
  browse_kind: SteamWorkshopBrowseKind;
  page_size: number;
  total_count: number | null;
  has_more: boolean;
  app_id: number;
  query: string;
  sort: string;
  page: number;
  source_url: string;
  items: SteamWorkshopLookupItem[];
}
export interface SteamNewsItem {
  gid: string;
  title: string;
  url: string;
  author?: string | null;
  feed_label?: string | null;
  excerpt: string;
  published_at_unix_ms: number;
}

export interface SteamReviewSummary {
  app_id: number;
  review_score?: number | null;
  review_score_desc: string;
  total_positive: number;
  total_negative: number;
  total_reviews: number;
  positive_percent: number;
  source_url: string;
}
export interface AssistantProviderSettingsInput {
  provider: string;
  model: string;
  baseUrl: string;
  apiKey: string;
}

export interface AssistantRunInput {
  settings: AssistantProviderSettingsInput;
  promptLabel: string;
  prompt: string;
  context: string;
}

export interface AssistantRunOutput {
  provider: string;
  model: string;
  endpointUrl: string;
  content: string;
}

export type AssistantOperationAction =
  | "start_server"
  | "stop_server"
  | "restart_server"
  | "create_backup"
  | "restore_backup"
  | "install_server"
  | "create_server"
  | "validate_server"
  | "apply_beginner_config"
  | "customize_config"
  | "patch_instance_text"
  | "patch_instance_files"
  | "repair_ports"
  | "run_gm_command"
  | "install_fun_mod"
  | "install_site_mod"
  | "broadcast"
  | "none";

export interface AssistantTaskRequirementView {
  id: string;
  kind: "setting" | "port" | "forbidden_action" | "unverified";
  description: string;
  sourceText: string;
  target: string | null;
  expectedDisplay: string | null;
}

export interface AssistantTaskReceipt {
  goal: "inspect" | "apply_change" | "restore_service" | "launch_service" | "prepare_service";
  preserveExistingMods: boolean;
  id: string;
  operationLimit: number;
  instanceId: string | null;
  moduleId: string | null;
  status: "proposed" | "completed" | "failed" | "inconclusive";
  requirements: AssistantTaskRequirementView[];
  checks: Array<{
    name: string;
    status: "satisfied" | "failed" | "unknown";
    summary: string;
    evidence: unknown;
  }>;
}

export interface AssistantConversationCreated {
  conversationId: string;
  revision: number;
}

export interface AssistantSavedConversation extends AssistantConversationCreated {
  title: string;
  updatedAtUnixMs: number;
}

export interface AssistantConversationControlResult {
  conversationId: string;
  stopping: boolean;
}

export interface AssistantConversationState {
  conversationId: string;
  status: "idle" | "running" | "paused" | "unavailable";
  revision: number | null;
  continuation: AssistantContinuation | null;
  progress?: AssistantProgressSnapshot | null;
  messages?: Array<{ role: "user" | "assistant"; content: string }>;
  messagesTruncated?: boolean;
}

export type AssistantProgressKind = "model_start" | "text_delta" | "phase" | "tool_started" | "tool_completed" | "tool_failed";
export interface AssistantProgressSnapshot {
  revision: number;
  cursor: number;
  reset: boolean;
  text: string;
  events: Array<{ cursor: number; kind: AssistantProgressKind; text: string; toolName: string | null }>;
}
export interface AssistantLiveProgress {
  text: string;
  phase: string;
  tools: Array<{ cursor: number; name: string; status: "running" | "completed" | "failed" }>;
  connectionIssue: boolean;
}

export interface AssistantExecuteOperationInput {
  conversationId: string;
  settings: AssistantProviderSettingsInput;
  prompt: string;
  context?: string | null;
  selectedInstanceId?: string | null;
  selectedModuleId?: string | null;
}

export interface AssistantConfirmOperationInput {
  conversationId: string;
  settings: AssistantProviderSettingsInput;
  confirmationToken: string;
  planSummary: string;
  continueTask?: boolean;
}

export interface AssistantOperationVerification {
  status: "verified" | "failed" | "inconclusive";
  summary: string;
  runId: number | null;
  evidence: unknown;
  canContinue: boolean;
}

export type AssistantRunPauseReason = "model_slice" | "read_slice" | "operation_slice" | "model_limit"
  | "read_limit" | "operation_limit" | "work_time" | "repeated_operation" | "invalid_fingerprint"
  | "work_in_progress" | "not_paused" | "state_unavailable" | "investigation_failed";

export interface AssistantContinuation {
  reason: AssistantRunPauseReason;
  summary: string;
  canResume: boolean;
}

export interface AssistantExecuteOperationOutput {
  completedOperations?: Array<{
    action: AssistantOperationAction;
    instanceId: string | null;
    message: string;
    verification: AssistantOperationVerification | null;
    task: AssistantTaskReceipt | null;
    fileChangeResult?: AssistantExecuteOperationOutput["fileChangeResult"];
    fileChangesResult?: AssistantFileChangesResult | null;
  }>;
  continuation: AssistantContinuation | null;
  conversationId: string | null;
  conversationRevision: number | null;
  task?: AssistantTaskReceipt | null;
  handled: boolean;
  action: AssistantOperationAction;
  message: string;
  requiresConfirmation: boolean;
  confirmationToken?: string | null;
  confirmationExpiresAtUnixMs?: number | null;
  planSummary?: string | null;
  instanceId?: string | null;
  moduleId?: string | null;
  appliedSettingsKeys: string[];
  rejectedSettingsKeys: string[];
  appliedPortNames: string[];
  rejectedPortNames: string[];
  workshopItemIds: string[];
  modReferences: string[];
  resolvedModIds: string[];
  sourcePaths: string[];
  runtimeCommands: string[];
  runtimeResponseTexts: string[];
  configDocumentCount: number;
  assistantReason?: string | null;
  fileChangePreview?: {
    file: string;
    sourceSha256: string;
    resultSha256: string;
    before: string;
    after: string;
  } | null;
  fileChangeResult?: {
    file: string;
    sourceSha256: string;
    resultSha256: string;
    backupId: string;
    readBackVerified: boolean;
  } | null;
  fileChangePreviews?: Array<{
    file: string;
    sourceSha256: string;
    resultSha256: string;
    edits: Array<{ before: string; after: string }>;
  }>;
  fileChangesResult?: AssistantFileChangesResult | null;
  verification?: AssistantOperationVerification | null;
  followUp?: AssistantExecuteOperationOutput | null;
}

export interface AssistantFileChangesResult {
  status: "applied" | "not_applied" | "rolled_back" | "partial";
  files: Array<{
    file: string;
    sourceSha256: string;
    resultSha256: string;
    backupId?: string | null;
    state: "not_applied" | "applied" | "rolled_back" | "recovery_required";
    readBackVerified: boolean;
    error?: string | null;
  }>;
  error?: string | null;
}

export interface AssistantSecretDescriptor {
  provider: string;
  baseUrl: string;
}

export interface AssistantSecretStatus {
  stored: boolean;
}

export type AssistantChatRole = "assistant" | "user";
export type AssistantChatMessageState = "ready" | "running" | "error";

export interface AssistantChatMessage {
  id: string;
  role: AssistantChatRole;
  content: string;
  label?: string | null;
  meta?: string | null;
  state?: AssistantChatMessageState;
}

export interface AssistantExecutionState {
  progress?: AssistantLiveProgress;
  stopping?: boolean;
  status: "idle" | "running" | "success" | "error" | "cancelled" | "inconclusive" | "paused" | "limit-reached";
  promptLabel: string | null;
  result: AssistantRunOutput | null;
  error: string | null;
}

export interface AppUpdateMetadata {
  version: string;
  currentVersion: string;
  date?: string | null;
  body?: string | null;
}

export type AppUpdateCheckResult =
  | { status: "current"; current_version?: string; currentVersion?: string }
  | { status: "available"; update: AppUpdateMetadata };

export type AppUpdateInstallEvent =
  | { event: "started"; data: { content_length?: number | null; contentLength?: number | null } }
  | { event: "progress"; data: { chunk_length?: number; chunkLength?: number } }
  | { event: "finished" }
  | { event: "installing" };

export type AppUpdateStateStatus =
  | "idle"
  | "checking"
  | "current"
  | "available"
  | "downloading"
  | "installing"
  | "failed";

export interface AppUpdateState {
  status: AppUpdateStateStatus;
  currentVersion: string;
  availableVersion?: string | null;
  releaseNotes?: string | null;
  publishedAt?: string | null;
  contentLength?: number | null;
  downloadedBytes: number;
  downloadPercent: number;
  error?: string | null;
}
