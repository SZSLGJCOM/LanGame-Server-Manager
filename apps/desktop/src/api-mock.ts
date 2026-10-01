import { version as desktopVersion } from "../package.json";
import type {
  AssistantConfirmOperationInput,
  AssistantProviderSettingsInput,
  AssistantExecuteOperationInput,
  AssistantRunInput,
  AssistantSecretDescriptor,
  AppPathSettingsInput,
  AppSettings,
  BackgroundJob,
  CreateInstanceInput,
  GenerateInstanceBroadcastInput,
  GenerateInstanceBroadcastOutput,
  InstanceBackupResult,
  InstanceBroadcastEvent,
  InstanceBroadcastPolicy,
  InstanceDeletionResult,
  InstanceArchiveResult,
  InstanceDetails,
  InstancePlayerAccessMutationInput,
  InstancePlayerAccessMutationResult,
  InstanceProvisioning,
  InstanceProcessRecord,
  InstanceRuntimeCommandResult,
  InstanceRuntimeOverview,
  RuntimeWindowSnapshot,
  RuntimeWindowSuppressionAttempt,
  RuntimeWindowSuppressionResult,
  SendInstanceBroadcastInput,
  SendInstanceBroadcastOutput,
  InstanceSummary,
  LaunchPlan,
  LogTailSnapshot,
  ManualModInventoryResult,
  ManualModReferenceResolveResult,
  ManualModStageResult,
  ModulePlayerActionDetails,
  ModuleUninstallResult,
  SteamCmdStatus,
  SteamCmdPrepareSnapshot,
  SteamWorkshopDownloadResult,
  StorageStatus,
  DstWorldImportResult,
  ExecuteInstanceManualPlayerActionInput,
  ExecuteInstancePlayerActionInput,
  ExecuteInstancePlayerActionResult,
  UpdateInstanceBroadcastPolicyInput,
  UpdateInstanceInput
} from "./types";
import { buildMockInstanceIsolation, mockInstancePrograms } from "./api-mock/instance-isolation";
import { mockModuleSchemasById } from "./api-mock/module-assets";
import { buildMockModuleDetails } from "./api-mock/module-details";
import { moduleRequiresSteamCmd } from "./module-installation-dependency";
import { buildWorkshopCollectionRemovalPlan } from "./views/servers/mod-workbench-collection-removal";
import { changedSettingKeys } from "./views/servers/mod-settings-patch";
import { readRecoveryPolicy } from "./views/servers/runtime-recovery-model";
import {
  assistantSecretKey,
  buildMockAssistantResponse,
  buildMockNecesseCommandLines,
  buildMockRuntimePerformancePolicyPreview,
  defaultMockRuntimePerformancePolicy,
  localizeMockSteamReviewSummary,
  lookupMockWorkshopItems,
  mockSteamNewsCatalog,
  mockSteamReviewSummaryCatalog,
  normalizeAssistantEndpoint,
  parseSettingsJson,
  readMockDstModConfigurationSpecs,
  readMockProjectZomboidWorkshopModsSnapshot,
  resolveMockRuntimePerformancePolicy,
  sampleSteamAboutHtml,
  searchMockWorkshopItems
} from "./api-mock/catalogs";
import {
  buildMockBackupUsesDeclaredSavesPath,
  buildMockPorts,
  buildMockSavesPath,
  buildMockSettingsForModule
} from "./api-mock/module-settings";
import { applyMockPlayerAccessMutation } from "./api-mock/player-access";
import {
  mockInferManualModId,
  mockInstanceRootFromDetails,
  mockPathIsWithinRoot,
  mockRuntimeDisplayName,
  mockRuntimeTransportIsRemote,
  mockRuntimeTransportLabel
} from "./api-mock/runtime-helpers";
import { buildMockAppUpdateCheckResult, emitMockAppUpdateInstallEvents } from "./api-mock/app-updates";
import { buildMockRuntimeDiagnostics, buildMockRuntimeHealth } from "./api-mock/runtime-health";
import { MockLivePlayerStore, mockSteamQueryVisibilityIssue } from "./api-mock/live-players";
import { buildMockSevenDaysBanSettings } from "./api-mock/seven-days-player-actions";
import { resolveMockDeclaredRuntimeAction } from "./api-mock/runtime-actions";
import { mockBindAddressCandidates, mockBootstrap, mockOverlays } from "./api-mock/bootstrap";
import { MockDstWorldStateStore } from "./api-mock/dst-world-state";
import { isArkModule } from "./ark-clusters";
import { buildMockArkProcesses, mockArkCommandTarget, MockArkMapLogs, updateMockArkPorts } from "./api-mock/ark-maps";
import { listMockAssistantConversations, getMockAssistantConversationState, confirmMockAssistantOperation, previewMockAssistantOperation, resumeMockAssistantConversation, createMockAssistantConversation, cancelMockAssistantTurn, deleteMockAssistantConversation } from "./api-mock/assistant-operations";

import { MockStorageManagement } from "./api-mock/storage-management";
const mockStorageManagement = new MockStorageManagement();

function mockSchemaConsumesAction(value: unknown, actionId: string): boolean {
  if (Array.isArray(value)) {
    return value.some((entry) => mockSchemaConsumesAction(entry, actionId));
  }
  if (!value || typeof value !== "object") {
    return false;
  }
  const object = value as Record<string, unknown>;
  if (Array.isArray(object.consume_action_ids)
    && object.consume_action_ids.some((candidate) => candidate === actionId)) {
    return true;
  }
  return Object.values(object).some((entry) => mockSchemaConsumesAction(entry, actionId));
}

function clone<T>(value: T): T {
  return typeof structuredClone === "function"
    ? structuredClone(value)
    : JSON.parse(JSON.stringify(value)) as T;
}

const mockInstanceDetailsStore = new Map<string, InstanceDetails>();
const mockInstanceBackupsStore = new Map<string, InstanceBackupResult[]>();
const mockLogDocumentStore = new Map<string, LogTailSnapshot>();
const mockSuppressedRuntimeWindowInstances = new Set<string>();
const mockRuntimeWindowSuppressionStore = new Map<string, RuntimeWindowSuppressionAttempt>();
const mockAssistantSecretStore = new Map<string, string>();
const mockManualModInventoryStore = new Map<string, ManualModInventoryResult>();
const mockWorkshopInstallationStore = new Map<string, Set<string>>();
const mockBroadcastPolicyStore = new Map<string, InstanceBroadcastPolicy>();
const mockBroadcastEventStore = new Map<string, InstanceBroadcastEvent[]>();
const mockLivePlayerStore = new MockLivePlayerStore();
const mockDstWorldStateStore = new MockDstWorldStateStore();
const mockArkMapLogs = new MockArkMapLogs();
let mockSteamCmdProgress: SteamCmdPrepareSnapshot | null = null;
let mockSteamCmdStatus: SteamCmdStatus = {
  ready: false,
  root: mockBootstrap.state.settings.steamcmd_root,
  executable_path: `${mockBootstrap.state.settings.steamcmd_root}/steamcmd.exe`,
  executable_exists: false,
  configured_root: mockBootstrap.state.settings.steamcmd_root,
  configured_executable_path: `${mockBootstrap.state.settings.steamcmd_root}/steamcmd.exe`,
  source: "configured",
  ownership: "none",
  can_uninstall: false
};

function pushMockJob(job: Omit<BackgroundJob, "cancellable" | "cancel_requested"> & Partial<Pick<BackgroundJob, "cancellable" | "cancel_requested">>) {
  mockBootstrap.state.jobs = [{ cancellable: false, cancel_requested: false, ...job }, ...mockBootstrap.state.jobs.filter((existing) => existing.id !== job.id)].slice(0, 24);
}

function upsertMockSummary(summary: InstanceSummary) {
  const existingIndex = mockBootstrap.state.instances.findIndex((item) => item.id === summary.id);
  if (existingIndex === -1) {
    mockBootstrap.state.instances = [...mockBootstrap.state.instances, summary];
  } else {
    const next = [...mockBootstrap.state.instances];
    next[existingIndex] = summary;
    mockBootstrap.state.instances = next;
  }
  mockBootstrap.state.snapshot.running_instances = mockBootstrap.state.instances.filter(
    (instance) => instance.status === "Running"
  ).length;
}

function buildMockActiveProcesses(summary: Pick<InstanceSummary, "id" | "module_id">, settings: Record<string, unknown> = {}): InstanceProcessRecord[] {
  if (isArkModule(summary.module_id)) return buildMockArkProcesses(summary, settings);
  if (summary.module_id === "dontstarve") {
    const cavesFailed = summary.id === "srv-dst-partial-error";
    return [
      {
        run_id: 1,
        session_id: "demo-session",
        process_key: "master",
        display_name: "Master",
        pid: 4321,
        status: "running",
        log_path: `D:/LanGame/instances/${summary.id}/logs/run-1-master.log`,
        is_primary: true
      },
      {
        run_id: 2,
        session_id: "demo-session",
        process_key: "caves",
        display_name: "Caves",
        pid: cavesFailed ? null : 4322,
        status: cavesFailed ? "error" : "running",
        exit_code: cavesFailed ? 1 : null,
        crash_flag: cavesFailed,
        log_path: `D:/LanGame/instances/${summary.id}/logs/run-1-caves.log`,
        is_primary: false
      }
    ];
  }

  return [
    {
      run_id: 1,
      session_id: "demo-session",
      process_key: "main",
      display_name: "Server",
      pid: 4321,
      status: "running",
      log_path: `D:/LanGame/instances/${summary.id}/logs/run-1.log`,
      is_primary: true
    }
  ];
}

function ensureMockInstanceDetails(instanceId: string): InstanceDetails {
  const cached = mockInstanceDetailsStore.get(instanceId);
  if (cached) {
    return clone(cached);
  }

  const summary = mockBootstrap.state.instances.find((item) => item.id === instanceId) ?? mockBootstrap.state.instances[0];
  const settings = buildMockSettingsForModule(summary.module_id, summary.name, summary.id);
  const activeProcesses = buildMockActiveProcesses(summary, settings);
  const details: InstanceDetails = {
    summary,
    config_file_path: `D:/LanGame/instances/${summary.id}/config/instance.json`,
    saves_path: buildMockSavesPath(summary, settings),
    backup_uses_declared_saves_path: buildMockBackupUsesDeclaredSavesPath(summary),
    auto_backup_on_stop: true,
    backup_retention_count: 5,
    settings_json: JSON.stringify(settings, null, 2),
    ports: buildMockPorts(summary.module_id),
    active_run: Number(summary.active_process_count) > 0
      ? {
          run_id: 1,
          session_id: "demo-session",
          pid: 4321,
          log_path: activeProcesses[0]?.log_path ?? `D:/LanGame/instances/${summary.id}/logs/run-1.log`,
          process_count: activeProcesses.length,
          processes: activeProcesses
        }
      : null
  };
  mockInstanceDetailsStore.set(instanceId, clone(details));
  return details;
}

function ensureMockInstanceBackups(instanceId: string): InstanceBackupResult[] {
  const cached = mockInstanceBackupsStore.get(instanceId);
  if (cached) {
    return clone(cached);
  }

  const details = ensureMockInstanceDetails(instanceId);
  const createdAt = Date.now() - 60 * 60 * 1000;
  const backups: InstanceBackupResult[] = [
    {
      backup_id: `saves-${createdAt}`,
      instance_id: instanceId,
      backup_kind: "manual",
      created_at_unix_ms: createdAt,
      backup_path: `D:/LanGame/instances/${instanceId}/backups/saves-${createdAt}`,
      display_name: null,
      saves_path: details.saves_path,
      file_count: 12,
      total_bytes: 1572864
    }
  ];

  mockInstanceBackupsStore.set(instanceId, clone(backups));
  return clone(backups);
}

function upsertMockBackup(instanceId: string, backup: InstanceBackupResult) {
  const existing = ensureMockInstanceBackups(instanceId).filter((item) => item.backup_id !== backup.backup_id);
  const next = [backup, ...existing].sort((left, right) => right.created_at_unix_ms - left.created_at_unix_ms);
  mockInstanceBackupsStore.set(instanceId, clone(next));
}

function createDefaultMockBroadcastPolicy(instanceId: string): InstanceBroadcastPolicy {
  return {
    instance_id: instanceId,
    enabled: false,
    rules: {
      startup: { enabled: false, prompt: null },
      shutdown: { enabled: false, prompt: null },
      runtime_health: { enabled: false, prompt: null },
      periodic: { enabled: false, interval_minutes: 30, prompt: null },
      tone: "short",
      cooldown_minutes: 10
    },
    updated_at_unix_ms: Date.now()
  };
}

function ensureMockBroadcastPolicy(instanceId: string): InstanceBroadcastPolicy {
  const cached = mockBroadcastPolicyStore.get(instanceId);
  if (cached) {
    return clone(cached);
  }

  const policy = createDefaultMockBroadcastPolicy(instanceId);
  mockBroadcastPolicyStore.set(instanceId, clone(policy));
  return policy;
}

function upsertMockBroadcastPolicy(input: UpdateInstanceBroadcastPolicyInput): InstanceBroadcastPolicy {
  const cooldownMinutes = Number(input.rules?.cooldown_minutes ?? 10);
  const policy: InstanceBroadcastPolicy = {
    instance_id: input.instance_id,
    enabled: Boolean(input.enabled),
    rules: {
      startup: { enabled: Boolean(input.rules?.startup?.enabled), prompt: input.rules?.startup?.prompt ?? null },
      shutdown: { enabled: Boolean(input.rules?.shutdown?.enabled), prompt: input.rules?.shutdown?.prompt ?? null },
      runtime_health: { enabled: Boolean(input.rules?.runtime_health?.enabled), prompt: input.rules?.runtime_health?.prompt ?? null },
      periodic: {
        enabled: Boolean(input.rules?.periodic?.enabled),
        interval_minutes: Math.max(1, Math.min(1440, Number(input.rules?.periodic?.interval_minutes ?? 30) || 30)),
        prompt: input.rules?.periodic?.prompt ?? null
      },
      tone: String(input.rules?.tone ?? "short").trim() || "short",
      cooldown_minutes: Math.max(0, Math.min(1440, Number.isFinite(cooldownMinutes) ? cooldownMinutes : 10))
    },
    updated_at_unix_ms: Date.now()
  };
  mockBroadcastPolicyStore.set(input.instance_id, clone(policy));
  return clone(policy);
}

function pushMockBroadcastEvent(input: Omit<InstanceBroadcastEvent, "event_id" | "created_at_unix_ms">): InstanceBroadcastEvent {
  const event: InstanceBroadcastEvent = {
    ...input,
    initiator: input.initiator ?? (input.source === "manual" ? "manual" : "auto"),
    policy_snapshot_json: input.policy_snapshot_json ?? null,
    event_id: `mock-broadcast-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    created_at_unix_ms: Date.now()
  };
  const existing = mockBroadcastEventStore.get(input.instance_id) ?? [];
  const next = [event, ...existing].slice(0, 100);
  mockBroadcastEventStore.set(input.instance_id, clone(next));
  return clone(event);
}

function listMockBroadcastEvents(instanceId: string, limit = 50): InstanceBroadcastEvent[] {
  return clone((mockBroadcastEventStore.get(instanceId) ?? []).slice(0, Math.max(0, limit)));
}

function buildMockBroadcastMessage(input: GenerateInstanceBroadcastInput, details: InstanceDetails): string {
  const intent = String(input.intent ?? "").trim();
  const source = String(input.source ?? "manual");
  if (source === "startup") {
    return `${details.summary.name} 已开服，大家可以进了。`;
  }
  if (source === "shutdown") {
    return `${details.summary.name} 即将关服，请先保存进度。`;
  }
  if (source === "runtime_health") {
    return `${details.summary.name} 状态有波动，管理员正在关注。`;
  }
  if (source === "periodic") {
    return `${details.summary.name} 正常运行中，注意保存进度。`;
  }
  return intent || `${details.summary.name} 有新的服务器通知。`;
}

function encodeMockBroadcastPlaceholder(action: ModulePlayerActionDetails, value: string): string {
  if (action.target_encoding === "quoted_string") {
    return `"${value.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
  }
  return value;
}

function renderMockBroadcastCommand(action: ModulePlayerActionDetails, message: string, moduleId: string): string {
  if (action.transport === "palworld_rest") {
    if ([";", "&&", "||", "`", "$(", "{{", "}}"].some((fragment) => message.includes(fragment))) {
      throw new Error("Broadcast message contains unsupported command syntax.");
    }
    const dispatch = resolveMockDeclaredRuntimeAction([action], moduleId, {
      runtimeActionId: action.id,
      runtimeActionTarget: message
    });
    if (!dispatch) throw new Error("The broadcast action could not be resolved.");
    return dispatch.command;
  }
  const encoded = encodeMockBroadcastPlaceholder(action, message);
  return action.command_template.replace("{{message}}", encoded).replace("{{target}}", encoded).trim();
}

function normalizeMockBackupDisplayName(value: unknown): string | null {
  const trimmed = String(value ?? "").trim();
  return trimmed ? trimmed : null;
}

function renameMockBackup(instanceId: string, backupId: string, displayName: string | null): InstanceBackupResult {
  const backups = ensureMockInstanceBackups(instanceId);
  const current = backups.find((item) => item.backup_id === backupId);
  if (!current) {
    throw new Error(`Backup not found: ${backupId}`);
  }

  const renamed: InstanceBackupResult = {
    ...current,
    display_name: normalizeMockBackupDisplayName(displayName)
  };
  upsertMockBackup(instanceId, renamed);
  return renamed;
}

function deleteMockBackup(instanceId: string, backupId: string): InstanceBackupResult {
  const backups = ensureMockInstanceBackups(instanceId);
  const current = backups.find((item) => item.backup_id === backupId);
  if (!current) {
    throw new Error(`Backup not found: ${backupId}`);
  }

  const next = backups.filter((item) => item.backup_id !== backupId);
  mockInstanceBackupsStore.set(instanceId, clone(next));
  return clone(current);
}

function ensureMockLogDocument(instanceId: string, maxLines = 200): LogTailSnapshot {
  const cached = mockLogDocumentStore.get(instanceId);
  if (cached) {
    return {
      ...clone(cached),
      lines: clone(cached.lines.slice(-maxLines)),
      total_lines: cached.lines.length,
      truncated: cached.truncated || cached.lines.length > maxLines
    };
  }

  const details = ensureMockInstanceDetails(instanceId);
  const primaryPort = details.ports[0]?.port ?? 10999;
  const lines = [
    "[info] booting dedicated server",
    `[info] loading config from ${details.config_file_path}`,
    "[info] binding network sockets",
    `[info] listening on ${details.summary.bind_ip}:${primaryPort}`,
    String(details.summary.module_id).toLowerCase() === "dontstarve"
      ? `[info] Lan Server Started on port: ${primaryPort}`
      : "[info] heartbeat ok"
  ];
  const snapshot: LogTailSnapshot = {
    source_path: details.active_run?.log_path ?? `D:/LanGame/instances/${details.summary.id}/logs/latest.log`,
    lines,
    total_lines: lines.length,
    truncated: false,
    read_error: null
  };
  mockLogDocumentStore.set(instanceId, clone(snapshot));
  return { ...clone(snapshot), lines: lines.slice(-maxLines), truncated: lines.length > maxLines };
}
function buildMockRuntimePlayers(details: InstanceDetails) {
  const status = String(details.summary.status).toLowerCase();
  const moduleDetails = buildMockModuleDetails(String(details.summary.module_id));
  const moduleId = String(moduleDetails.summary.id).toLowerCase();
  const settings = parseSettingsJson(details.settings_json) ?? {};
  const configuredCapacity = Number(settings.max_players ?? settings.max_slots ?? settings.max_server_players ?? 0);
  const maxPlayers = Number.isFinite(configuredCapacity) && configuredCapacity > 0 ? configuredCapacity : null;
  if (moduleDetails.runtime.player_count_source === "player_list") {
    if (status !== "running") {
      return { current_players: null, max_players: maxPlayers,
        query: { status: "stopped", summary: "Online player counts are available after the instance starts." } };
    }
    const playerList = moduleDetails.runtime.player_list;
    const action = moduleDetails.runtime.player_actions?.find((candidate) => candidate.id === playerList?.action_id);
    const unavailable = playerList?.scope !== "online" || !action
      ? "The declared online player-list action is unavailable."
      : action.enabled_setting_key && settings[action.enabled_setting_key] !== true
        ? "Enable the declared player-list transport to query online players."
        : action.password_setting_key && (typeof settings[action.password_setting_key] !== "string"
          || !String(settings[action.password_setting_key]).trim())
          ? "Configure the declared player-list password to query online players."
          : null;
    if (unavailable) {
      return { current_players: null, max_players: maxPlayers,
        query: { status: "unavailable", summary: unavailable } };
    }
    const context = {
      instance_id: details.summary.id, module_id: moduleId, running: true,
      player_list: playerList ?? null, settings, current_players: null, max_players: maxPlayers
    };
    let snapshot = mockLivePlayerStore.read(context);
    if (snapshot.status === "refreshing" || snapshot.stale) {
      snapshot = mockLivePlayerStore.refresh(context);
    }
    const current = snapshot.current_players;
    const complete = snapshot.status === "ready" && snapshot.complete && !snapshot.truncated && !snapshot.stale
      && typeof current === "number" && Number.isSafeInteger(current) && current >= 0
      && current === snapshot.entries.length;
    return { current_players: complete ? current : null, max_players: maxPlayers,
      query: { status: complete ? "ready" : "unavailable",
        summary: complete ? "Online player count matches the complete player list."
          : "A complete current online player list is unavailable." } };
  }
  const playerQuery = moduleDetails.runtime.player_query;
  const supportedLivePlayerQueryProtocols = new Set(["a2s_info", "minecraft_query"]);
  const playerQueryPortNames = playerQuery?.port_names ?? [];
  const liveQuerySupported = supportedLivePlayerQueryProtocols.has(playerQuery?.protocol ?? "") && Array.isArray(playerQueryPortNames) && playerQueryPortNames.length > 0;

  const visibilityIssue = mockSteamQueryVisibilityIssue(moduleId, settings);
  if (visibilityIssue) {
    return {
      current_players: null,
      max_players: null,
      query: { status: "unsupported", summary: visibilityIssue.summary }
    };
  }

  if (status !== "running") {
    return {
      current_players: null,
      max_players: maxPlayers,
      query: liveQuerySupported
        ? {
            status: "stopped",
            summary: "Live player query is available after the instance starts."
          }
        : {
            status: "unsupported",
            summary: "This module does not expose a live player query protocol."
          }
    };
  }

  if (!liveQuerySupported) {
    return {
      current_players: null,
      max_players: maxPlayers,
      query: {
        status: "unsupported",
        summary: "This module does not expose a live player query protocol."
      }
    };
  }

  const currentPlayers = moduleId === "dontstarve" ? 2 : 1;
  return {
    current_players: currentPlayers,
    max_players: maxPlayers,
    query: {
      status: "ready",
      summary: "Live player query succeeded and the current player count is up to date."
    }
  };
}

function buildMockLivePlayerContext(instanceId: string) {
  const details = ensureMockInstanceDetails(instanceId);
  const moduleDetails = buildMockModuleDetails(details.summary.module_id);
  const settings = parseSettingsJson(details.settings_json) ?? {};
  const configuredCapacity = Number(settings.max_players ?? settings.max_slots ?? settings.max_server_players ?? 0);
  // List-backed counts consume the collector; they cannot seed that same collector.
  const overviewPlayers = moduleDetails.runtime.player_count_source === "player_list"
    ? { current_players: null, max_players: Number.isFinite(configuredCapacity) && configuredCapacity > 0 ? configuredCapacity : null }
    : buildMockRuntimePlayers(details);
  return {
    instance_id: details.summary.id,
    module_id: details.summary.module_id,
    running: String(details.summary.status).toLowerCase() === "running",
    player_list: moduleDetails.runtime.player_list ?? null,
    settings,
    current_players: overviewPlayers.current_players,
    max_players: overviewPlayers.max_players
  };
}

function buildMockRuntimePerformance(details: InstanceDetails) {
  const running = String(details.summary.status).toLowerCase() === "running";
  const settings = parseSettingsJson(details.settings_json) ?? {};
  const policy = resolveMockRuntimePerformancePolicy(settings, details.summary.id);
  const preview = buildMockRuntimePerformancePolicyPreview(settings, details.summary.id, policy);
  const affectedProcessCount = Math.max(1, details.active_run?.process_count ?? 1);
  const processes = (details.active_run?.processes ?? [])
    .filter((process) => typeof process.pid === "number")
    .map((process) => ({
      process_key: String(process.process_key ?? "main"),
      display_name: String(process.display_name ?? process.process_key ?? "Server"),
      pid: Number(process.pid),
      policy,
      application: running
        ? {
            pid: Number(process.pid),
            priority_class: policy.priority_class,
            cpu_affinity_mask: policy.cpu_affinity_mask,
            apply_to_child_processes: policy.apply_to_child_processes,
            targeted_process_count: affectedProcessCount,
            priority_applied_count: affectedProcessCount,
            affinity_applied_count: policy.cpu_affinity_mask ? affectedProcessCount : 0,
            warnings: []
          }
        : null
    }));

  return {
    status: running ? "managed" : "stopped",
    applied_resource_limits: null,
    summary: running
      ? `Performance policy is active across ${Math.max(1, processes.length)} tracked process record(s).`
      : preview.summary,
    policy,
    preview,
    process_count: processes.length,
    processes
  };
}

function buildMockStartupQueue() {
  const activeRuns = Array.from(mockInstanceDetailsStore.values()).filter(
    (details) => String(details.summary.status).toLowerCase() === "running"
  );
  const activeRunCount = activeRuns.length;
  const trackedProcessCount = activeRuns
    .map((details) => details.active_run?.process_count ?? details.active_run?.processes?.length ?? 1)
    .reduce((total, value) => total + value, 0);

  return {
    status: "ready",
    summary: `Startup scheduler is ready with ${activeRunCount} active instance(s), ${trackedProcessCount} active process record(s), and ${trackedProcessCount} tracked process(es).`,
    active_run_count: activeRunCount,
    active_process_count: trackedProcessCount,
    tracked_instance_count: activeRunCount,
    tracked_process_count: trackedProcessCount,
    next_start_delay_ms: 0,
    projected_effective_stagger_ms: Math.min(10_000, 1500 + Math.min(activeRunCount, 8) * 250 + Math.min(trackedProcessCount, 16) * 125),
    projected_queued_start_count: 0,
    projected_process_count: 1,
    pending_restart_count: 0,
    next_restart_delay_ms: 0,
    next_restart: null,
    last_startup_schedule: null
  };
}

function buildMockStability(details: InstanceDetails) {
  const running = String(details.summary.status).toLowerCase() === "running";
  const settings = parseSettingsJson(details.settings_json) ?? {};
  const runtimeRestart = readRecoveryPolicy(settings);
  const restartPolicyEnabled = runtimeRestart.enabled;
  const restartLimit = runtimeRestart.max_restarts;
  const restartBackoffMs = runtimeRestart.backoff_ms;
  const baseSummary = running ? "The instance is being supervised in this app session." : "No recent crash signals were found.";
  return {
    status: running ? "running" : "stable",
    summary: restartPolicyEnabled
      ? `${baseSummary} Auto-restart is armed with a ${restartLimit} crash restart limit and ${restartBackoffMs}ms backoff.`
      : baseSummary,
    recent_crash_count: 0,
    last_exit_code: running ? null : 0,
    restart_policy_enabled: restartPolicyEnabled,
    restart_limit: restartLimit,
    restart_backoff_ms: restartBackoffMs,
    pending_restart: null
  };
}

function buildMockRuntime(instanceId: string): InstanceRuntimeOverview {
  const details = ensureMockInstanceDetails(instanceId);
  const logDocument = ensureMockLogDocument(instanceId, 200);
  const health = buildMockRuntimeHealth(details, logDocument);
  return {
    recent_runs: [
      {
        run_id: details.active_run?.run_id ?? 1,
        session_id: details.active_run?.session_id ?? "demo-session",
        status: String(details.summary.status).toLowerCase(),
        pid: details.active_run?.pid ?? null,
        started_at: new Date(Date.now() - 45 * 60 * 1000).toISOString(),
        stopped_at: String(details.summary.status).toLowerCase() === "running" ? null : new Date().toISOString(),
        exit_code: String(details.summary.status).toLowerCase() === "running" ? null : 0,
        crash_flag: false,
        log_path: logDocument.source_path ?? null,
        process_count: details.active_run?.process_count ?? 1,
        processes: details.active_run?.processes ?? []
      }
    ],
    log_tail: logDocument,
    health,
    diagnostics: buildMockRuntimeDiagnostics(details, logDocument, health),
    players: buildMockRuntimePlayers(details),
    performance: buildMockRuntimePerformance(details),
    startup_queue: buildMockStartupQueue(),
    stability: buildMockStability(details)
  };
}

function buildMockRuntimeWindowSnapshot(instanceId: string): RuntimeWindowSnapshot {
  const details = ensureMockInstanceDetails(instanceId);
  const observedAt = Date.now();
  const status = String(details.summary.status).toLowerCase();
  const lastSuppressionAttempt = mockRuntimeWindowSuppressionStore.get(instanceId) ?? null;

  if (status !== "running") {
    return {
      instance_id: instanceId,
      observed_at_unix_ms: observedAt,
      status: "stopped",
      summary: "The instance is not running, so LanGame did not inspect any visible process windows.",
      inspected_process_count: 0,
      last_suppression_attempt: lastSuppressionAttempt,
      windows: []
    };
  }

  const moduleId = String(details.summary.module_id).toLowerCase();
  const hostSurface = buildMockModuleDetails(details.summary.module_id).process?.host_surface ?? "managed_terminal";
  if (
    !mockSuppressedRuntimeWindowInstances.has(instanceId)
    && hostSurface === "managed_native_window"
  ) {
    return {
      instance_id: instanceId,
      observed_at_unix_ms: observedAt,
      status: "detected",
      summary: "Detected 1 visible top-level Windows surface across 2 tracked runtime process(es).",
      inspected_process_count: 2,
      last_suppression_attempt: lastSuppressionAttempt,
      windows: [
        {
          process_key: "main",
          display_name: "Server",
          relation: "tracked_process",
          pid: 4824,
          process_name: moduleId === "conanexiles" ? "ConanSandboxServer-Win64-Shipping.exe" : "ArkAscendedServer.exe",
          window_handle: "0x12AC4",
          title: moduleId === "conanexiles" ? "ConanSandboxServer-Win64-Shipping" : "Server Console (ArkAscendedServer)",
          class_name: "ConsoleWindowClass"
        }
      ]
    };
  }

  return {
    instance_id: instanceId,
    observed_at_unix_ms: observedAt,
    status: "clear",
    summary: "No visible top-level Windows surfaces were detected across 1 tracked runtime process.",
    inspected_process_count: 1,
    last_suppression_attempt: lastSuppressionAttempt,
    windows: []
  };
}

function buildMockLaunchPlan(instanceId: string): LaunchPlan {
  const details = ensureMockInstanceDetails(instanceId);
  const module = buildMockModuleDetails(details.summary.module_id);
  const installRoot = `D:/LanGame/instances/${details.summary.id}/runtime`;
  const configRoot = `D:/LanGame/instances/${details.summary.id}/config`;
  const dataRoot = `D:/LanGame/instances/${details.summary.id}/data`;
  const logsRoot = `D:/LanGame/instances/${details.summary.id}/logs`;
  const instanceRoot = `D:/LanGame/instances/${details.summary.id}`;
  const isDontStarve = String(module.summary.id).toLowerCase() === "dontstarve";
  const isAbioticFactor = String(module.summary.id).toLowerCase() === "abioticfactor";
  const isArkSurvivalAscended = String(module.summary.id).toLowerCase() === "arksurvivalascended";
  const isArkSurvivalEvolved = String(module.summary.id).toLowerCase() === "arksurvivalevolved";
  const isConanExiles = String(module.summary.id).toLowerCase() === "conanexiles";
  const isCoreKeeper = String(module.summary.id).toLowerCase() === "corekeeper";
  const isEnshrouded = String(module.summary.id).toLowerCase() === "enshrouded";
  const isNecesse = String(module.summary.id).toLowerCase() === "necesse";
  const isPalworld = String(module.summary.id).toLowerCase() === "palworld";
  const isProjectZomboid = String(module.summary.id).toLowerCase() === "projectzomboid";
  const isSatisfactory = String(module.summary.id).toLowerCase() === "satisfactory";
  const isSevenDaysToDie = String(module.summary.id).toLowerCase() === "sevendaystodie";
  const isTerraria = String(module.summary.id).toLowerCase() === "terraria";
  const isValheim = String(module.summary.id).toLowerCase() === "valheim";
  const isVRising = String(module.summary.id).toLowerCase() === "vrising";
  const settings = parseSettingsJson(details.settings_json) ?? {};
  const performancePolicy = resolveMockRuntimePerformancePolicy(
    settings,
    details.summary.id,
    module.runtime.performance ?? defaultMockRuntimePerformancePolicy()
  );
  const performancePreview = buildMockRuntimePerformancePolicyPreview(
    settings,
    details.summary.id,
    performancePolicy,
    module.runtime.performance ?? defaultMockRuntimePerformancePolicy()
  );
  const readStringSetting = (key: string, fallback: string) => {
    const value = settings[key];
    return typeof value === "string" && value.trim().length > 0 ? value.trim() : fallback;
  };
  const readNumberSetting = (key: string, fallback: number) => {
    const value = settings[key];
    return typeof value === "number" && Number.isFinite(value) ? value : fallback;
  };
  const readBooleanSetting = (key: string, fallback = false) => {
    const value = settings[key];
    return typeof value === "boolean" ? value : fallback;
  };
  const portByName = (name: string, fallback: number) => details.ports.find((port) => port.name === name)?.port ?? fallback;
  const bindIp = details.summary.bind_ip;
  const bindIpEnabled = bindIp.trim().length > 0 && bindIp !== "0.0.0.0";
  const executablePath = isPalworld
    ? `${installRoot}/Pal/Binaries/Win64/PalServer-Win64-Shipping-Cmd.exe`
    : isAbioticFactor
      ? `${installRoot}/AbioticFactor/Binaries/Win64/AbioticFactorServer-Win64-Shipping.exe`
    : isArkSurvivalAscended
      ? `${installRoot}/ShooterGame/Binaries/Win64/ArkAscendedServer.exe`
    : isArkSurvivalEvolved
      ? `${installRoot}/ShooterGame/Binaries/Win64/ShooterGameServer.exe`
    : isConanExiles
      ? `${installRoot}/ConanSandbox/Binaries/Win64/ConanSandboxServer-Win64-Shipping.exe`
    : isCoreKeeper
      ? `${installRoot}/CoreKeeperServer.exe`
    : isEnshrouded
      ? `${installRoot}/enshrouded_server.exe`
    : isNecesse
      ? `${installRoot}/jre/bin/java.exe`
    : isVRising
      ? `${installRoot}/VRisingServer.exe`
    : isProjectZomboid
      ? `${installRoot}/jre64/bin/java.exe`
    : isValheim
      ? `${installRoot}/valheim_server.exe`
    : isSevenDaysToDie
      ? `${installRoot}/7DaysToDieServer.exe`
    : isTerraria
      ? `${installRoot}/TerrariaServer.exe`
    : `${installRoot}/${module.process?.executable ?? "server.exe"}`;
  const executableExists = String(module.summary.install_state).toLowerCase() === "installed";
  const primaryPort = details.ports[0]?.port ?? 10999;
  const validationIssues = executableExists
    ? []
    : [
        {
          code: "launch_executable_missing",
          severity: "error",
          message: `Launch executable is missing: ${executablePath}. Install or repair the game files first.`,
          path: executablePath
        }
      ];
  const args = isDontStarve
    ? [
        "-persistent_storage_root",
        configRoot,
        "-conf_dir",
        "clusters",
        "-cluster",
        "main",
        "-shard",
        "Master",
        "-console",
        "-bind_ip",
        details.summary.bind_ip,
        "-port",
        String(details.ports.find((port) => port.name === "master")?.port ?? primaryPort)
      ]
    : isAbioticFactor
      ? [
          "-log",
          "-newconsole",
          `-PORT=${portByName("game", 7777)}`,
          `-QueryPort=${portByName("query", 27015)}`,
          `-MaxServerPlayers=${String(readNumberSetting("max_server_players", 6))}`,
          `-WorldSaveName=${readStringSetting("world_save_name", details.summary.id)}`,
          `-SandboxIniPath=Config/WindowsServer/LanGame/${details.summary.id}-SandboxSettings.ini`,
          `-AdminIniPath=SaveGames/Server/LanGame/${details.summary.id}-Admin.ini`,
          `-SteamServerName=${readStringSetting("server_name", details.summary.name)}`,
          ...(readStringSetting("server_password", "") ? [`-ServerPassword=${readStringSetting("server_password", "")}`] : []),
          ...(readStringSetting("admin_password", "") ? [`-AdminPassword=${readStringSetting("admin_password", "")}`] : []),
          ...(readBooleanSetting("lan_only") ? ["-LANOnly"] : []),
          ...(readStringSetting("platform_limited", "").toLowerCase() !== "all" && readStringSetting("platform_limited", "")
            ? [`-PlatformLimited=${readStringSetting("platform_limited", "")}`]
            : []),
          ...(bindIpEnabled ? [`-MultiHome=${bindIp}`] : []),
          ...(readBooleanSetting("use_local_ips") ? ["-UseLocalIPs"] : []),
          ...(readBooleanSetting("use_perf_threads") ? ["-useperfthreads"] : []),
          ...(readBooleanSetting("disable_async_loading_thread") ? ["-DisableAsyncLoadingThread"] : [])
        ]
    : isArkSurvivalAscended
      ? [
          `${readStringSetting("map_name", "TheIsland_WP")}?AltSaveDirectoryName=${details.summary.id}?QueryPort=${portByName("query", 27015)}?MaxPlayers=${readNumberSetting("max_players", 30)}${bindIpEnabled ? `?MultiHome=${bindIp}` : ""}?SessionName=${readStringSetting("server_name", details.summary.name)}${readStringSetting("admin_password", "") ? `?ServerAdminPassword=${readStringSetting("admin_password", "")}` : ""}${readBooleanSetting("rcon_enabled", true) ? `?RCONEnabled=true?RCONPort=${portByName("rcon", 27020)}` : ""}`,
          `-port=${portByName("game", 7777)}`,
          `-WinLiveMaxPlayers=${String(readNumberSetting("max_players", 30))}`,
          "-server",
          "-log",
          ...(readBooleanSetting("enable_idle_player_kick") ? ["-EnableIdlePlayerKick"] : []),
          ...(readBooleanSetting("enable_auto_destroy_structures") ? ["-AutoDestroyStructures"] : []),
          ...(readBooleanSetting("allow_flyer_speed_leveling") ? ["-AllowFlyerSpeedLeveling"] : []),
          ...(readBooleanSetting("use_dynamic_config") ? ["-UseDynamicConfig"] : []),
          ...(readBooleanSetting("no_transfer_from_filtering") ? ["-NoTransferFromFiltering"] : []),
          ...(readStringSetting("cluster_id", "") ? [`-clusterid=${readStringSetting("cluster_id", "")}`] : []),
          ...(readStringSetting("cluster_id", "") ? [`-ClusterDirOverride=${instanceRoot.split("/").join("\\")}\\saves\\cluster`] : []),
          ...(readBooleanSetting("battleye_enabled", true) ? [] : ["-NoBattlEye"]),
          ...(readBooleanSetting("server_game_log", true) ? ["-servergamelog"] : []),
          ...(readBooleanSetting("server_game_log_include_tribe_logs") ? ["-servergamelogincludetribelogs", "-ServerRCONOutputTribeLogs"] : []),
          ...(readBooleanSetting("notify_admin_commands_in_chat") ? ["-NotifyAdminCommandsInChat"] : []),
          ...(readStringSetting("active_event", "") ? [`-ActiveEvent=${readStringSetting("active_event", "")}`] : []),
          ...(readStringSetting("mod_ids_csv", "") ? [`-mods=${readStringSetting("mod_ids_csv", "")}`] : []),
          ...(readStringSetting("custom_launch_flags", "") ? readStringSetting("custom_launch_flags", "").split(/\s+/).filter(Boolean) : [])
        ]
    : isArkSurvivalEvolved
      ? [
          `${readStringSetting("map_name", "TheIsland")}?listen?AltSaveDirectoryName=${details.summary.id}${bindIpEnabled ? `?MultiHome=${bindIp}` : ""}`,
          "-server",
          "-log",
          ...(bindIpEnabled ? ["-MULTIHOME"] : []),
          ...(readBooleanSetting("battleye_enabled", true) ? [] : ["-NoBattlEye"]),
          ...(readBooleanSetting("auto_managed_mods", true) ? ["-automanagedmods"] : [])
        ]
    : isConanExiles
      ? [
          "ConanSandbox?listen",
          `-Port=${portByName("game", 7777)}`,
          `-QueryPort=${portByName("query", 27015)}`,
          `-MaxPlayers=${String(readNumberSetting("max_players", 20))}`,
          `-ServerName=${readStringSetting("server_name", details.summary.name)}`,
          `-RconPort=${portByName("rcon", 25575)}`,
          "-server",
          "-log",
          "-useallavailablecores",
          ...(bindIpEnabled ? [`-MULTIHOME=${bindIp}`] : [])
        ]
    : isPalworld
      ? [
          `-port=${details.ports.find((port) => port.name === "game")?.port ?? 8211}`,
          `-players=${String((details.settings_json ? JSON.parse(details.settings_json).max_players : 32) ?? 32)}`,
          `-logformat=${String((details.settings_json ? JSON.parse(details.settings_json).log_format : "text") ?? "text")}`,
          "-useperfthreads",
          "-NoAsyncLoadingThread",
          "-UseMultithreadForDS"
        ]
    : isCoreKeeper
      ? [
          "-batchmode",
          "-logfile",
          `${logsRoot}/CoreKeeperServer.log`,
          "-datapath",
          dataRoot,
          ...(readBooleanSetting("direct_connection_enabled") && bindIpEnabled ? ["-ip", bindIp] : []),
          ...(readBooleanSetting("direct_connection_enabled") ? ["-port", String(portByName("game", 27015))] : []),
          ...(readBooleanSetting("direct_connection_enabled") && readStringSetting("join_password", "") ? ["-password", readStringSetting("join_password", "")] : []),
          ...(readBooleanSetting("direct_connection_enabled") ? ["-allowonlyplatform", String(readNumberSetting("allowed_platform_code", 0))] : [])
        ]
    : isEnshrouded
      ? []
    : isNecesse
      ? [
          "-Dfile.encoding=UTF-8",
          "-jar",
          "Server.jar",
          "-nogui",
          "-world",
          readStringSetting("world_name", details.summary.name),
          "-port",
          String(portByName("game", 14159)),
          "-slots",
          String(readNumberSetting("max_slots", 8)),
          "-owner",
          readStringSetting("owner_name", "ChangeMeOwner"),
          "-motd",
          readStringSetting("motd", "Welcome to the server!"),
          "-password",
          readStringSetting("password", ""),
          "-pausewhenempty",
          readBooleanSetting("pause_when_empty") ? "1" : "0",
          "-giveclientspower",
          readBooleanSetting("strict_server_authority") ? "1" : "0",
          "-logging",
          readBooleanSetting("logging_enabled", true) ? "1" : "0",
          "-logs",
          "..\\logs",
          "-zipsaves",
          readBooleanSetting("zip_saves", true) ? "1" : "0",
          "-language",
          readStringSetting("language", "en"),
          "-ip",
          bindIp,
          "-datadir",
          dataRoot,
          ...(readBooleanSetting("ignore_seasons") ? ["-ignoreseasons"] : [])
        ]
    : isProjectZomboid
      ? [
          "-Djava.awt.headless=true",
          "-Dzomboid.steam=1",
          "-Dzomboid.znetlog=1",
          "-XX:+UseZGC",
          "-XX:-CreateCoredumpOnCrash",
          "-XX:-OmitStackTraceInFastThrow",
          `-Xms${String(readNumberSetting("memory_gb", 4))}g`,
          `-Xmx${String(readNumberSetting("memory_gb", 4))}g`,
          `-Duser.home=${configRoot}/runtime-home`,
          "-Djava.library.path=natives/;natives/win64/;.",
          "-cp",
          "java/*;java/",
          "zombie.network.GameServer",
          "-statistic",
          "0",
          "-servername",
          details.summary.id,
          "-adminusername",
          readStringSetting("admin_username", "admin"),
          "-adminpassword",
          readStringSetting("admin_password", ""),
          "-port",
          String(portByName("game", 16261)),
          "-udpport",
          String(portByName("direct", 16262))
        ]
    : isValheim
      ? [
          "-nographics",
          "-batchmode",
          "-name",
          details.summary.name,
          "-port",
          String(details.ports.find((port) => port.name === "game")?.port ?? 2456),
          "-world",
          details.summary.name,
          "-password",
          readStringSetting("server_password", ""),
          "-savedir",
          buildMockSavesPath(details.summary),
          "-public",
          "1",
          "-saveinterval",
          "1800",
          "-backups",
          "4",
          "-backupshort",
          "7200",
          "-backuplong",
          "43200"
        ]
    : isSatisfactory
      ? [
          "-unattended",
          `-UserDir=${dataRoot}`,
          `-Port=${portByName("game", 7777)}`,
          `-ReliablePort=${portByName("reliable", 8888)}`,
          ...(readNumberSetting("external_reliable_port", 0) > 0 ? [`-ExternalReliablePort=${readNumberSetting("external_reliable_port", 0)}`] : []),
          ...(readBooleanSetting("disable_packet_routing") ? ["-DisablePacketRouting"] : []),
          ...(readBooleanSetting("disable_seasonal_events") ? ["-DisableSeasonalEvents"] : []),
          ...(readBooleanSetting("allow_insecure_local_api") ? ["-ini:Engine:[SystemSettings]:FG.DedicatedServer.AllowInsecureLocalAccess=1"] : []),
          ...readStringSetting("custom_launch_flags", "").split(/\s+/).filter(Boolean)
        ]
    : isSevenDaysToDie
      ? [
          "-quit",
          "-batchmode",
          "-nographics",
          `-configfile=${configRoot}/serverconfig.xml`,
          "-dedicated"
        ]
    : isTerraria
      ? ["-config", `${configRoot}/serverconfig.txt`, "-ip", details.summary.bind_ip]
    : isVRising
      ? [
          "-persistentDataPath",
          instanceRoot,
          ...(bindIpEnabled ? ["-bindAddress", bindIp] : [])
        ]
    : [`--instance=${details.summary.id}`, `--bind=${details.summary.bind_ip}`, `--port=${primaryPort}`];

  return {
    instance_id: details.summary.id,
    instance_name: details.summary.name,
    module_id: details.summary.module_id,
    install_root: installRoot,
    install_state: module.summary.install_state,
    uses_private_runtime: true,
    working_directory: isPalworld
      ? `${installRoot}/Pal/Binaries/Win64`
      : isAbioticFactor
        ? `${installRoot}/AbioticFactor/Binaries/Win64`
      : isArkSurvivalAscended || isArkSurvivalEvolved
        ? `${installRoot}/ShooterGame/Binaries/Win64`
      : isConanExiles
        ? installRoot
      : isCoreKeeper
        ? installRoot
      : isEnshrouded
        ? installRoot
      : isNecesse
        ? installRoot
      : isProjectZomboid
        ? installRoot
      : isValheim
        ? installRoot
      : isVRising
        ? installRoot
      : isTerraria
        ? installRoot
      : installRoot,
    executable_path: executablePath,
    executable_exists: executableExists,
    ready_to_launch: validationIssues.length === 0,
    validation_issues: validationIssues,
    args,
    environment: Object.fromEntries(Object.entries(module.process?.environment_template ?? {}).map(
      ([key, value]) => [key, value.split("{{paths.data_dir}}").join(dataRoot)]
    )),
    command_line: [`"${executablePath}"`, ...args].join(" "),
    window_policy: module.process?.window_policy ?? "background",
    uses_script_entrypoint: executablePath.toLowerCase().endsWith(".bat") || executablePath.toLowerCase().endsWith(".cmd"),
    host_surface: module.process?.host_surface ?? "managed_terminal",
    host_notes: module.process?.host_notes ?? null,
    performance_policy: performancePolicy,
    performance_preview: performancePreview
  };
}

function requireMockSteamCmd() {
  if (!mockSteamCmdStatus.ready) {
    throw new Error(JSON.stringify({ code: "steamcmd_not_ready", path: mockSteamCmdStatus.executable_path,
      message: "SteamCMD is not ready. Check or install SteamCMD on the System page first.", output_excerpt: null }));
  }
}

export async function invokeMock<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const includePreservedProgramCounts = (args?.includePreservedProgramCounts ?? args?.include_preserved_program_counts) !== false;
  if (["bootstrap", "refresh_modules", "sync_modules_to_storage"].includes(command)
    || (command === "read_module_details" && includePreservedProgramCounts)) {
    mockBootstrap.state.modules = mockStorageManagement.withProgramCounts(mockBootstrap.state.modules,
      mockBootstrap.state.instances, mockInstancePrograms, mockBootstrap.state.settings.archives_root);
  }
  switch (command) {
    case "bootstrap": {
      const booted = clone(mockBootstrap);
      if (booted.state.snapshot.telemetry) {
        booted.state.snapshot.telemetry.observed_at_unix_ms = Date.now();
      }
      return booted as T;
    }
    case "read_background_jobs":
      return clone(mockBootstrap.state.jobs) as T;
    case "app_version":
      return desktopVersion as T;
    case "check_app_update":
      return buildMockAppUpdateCheckResult() as T;
    case "install_app_update": {
      emitMockAppUpdateInstallEvents(args?.onEvent);
      return undefined as T;
    }
    case "overlay_families":
      return clone(mockOverlays) as T;
    case "bind_address_candidates":
      return clone(mockBindAddressCandidates) as T;
    case "pick_directory_path": {
      const currentPath = String(args?.currentPath ?? args?.current_path ?? "").trim();
      if (typeof window !== "undefined" && typeof window.prompt === "function") {
        const selected = window.prompt("Select folder", currentPath);
        return (selected ? selected.trim() : null) as T;
      }
      return (currentPath || null) as T;
    }
    case "update_app_settings": {
      const input = (args?.input ?? {}) as AppPathSettingsInput;
      const nextSettings: AppSettings = {
        ...mockBootstrap.state.settings,
        servers_root: String(input.servers_root ?? mockBootstrap.state.settings.servers_root),
        games_root: String(input.games_root ?? mockBootstrap.state.settings.games_root),
        archives_root: String(input.archives_root ?? mockBootstrap.state.settings.archives_root),
        steamcmd_root: String(input.steamcmd_root ?? mockBootstrap.state.settings.steamcmd_root)
      };
      mockStorageManagement.validatePathChange(mockBootstrap.state.settings, nextSettings);
      mockBootstrap.state.settings = nextSettings;
      const steamCmdRootChanged = nextSettings.steamcmd_root !== mockSteamCmdStatus.configured_root;
      mockSteamCmdStatus = {
        ...mockSteamCmdStatus,
        root: nextSettings.steamcmd_root,
        executable_path: `${nextSettings.steamcmd_root}/steamcmd.exe`,
        executable_exists: steamCmdRootChanged ? false : mockSteamCmdStatus.executable_exists,
        ready: steamCmdRootChanged ? false : mockSteamCmdStatus.ready,
        configured_root: nextSettings.steamcmd_root,
        configured_executable_path: `${nextSettings.steamcmd_root}/steamcmd.exe`,
        source: "configured",
        ownership: steamCmdRootChanged ? "none" : mockSteamCmdStatus.ownership,
        can_uninstall: steamCmdRootChanged ? false : mockSteamCmdStatus.can_uninstall
      };
      return clone(nextSettings) as T;
    }
    case "ensure_storage_ready": {
      const storage: StorageStatus = {
        ...mockBootstrap.state.storage,
        database_exists: true,
        migrations_applied: true,
        schema_version: 1
      };
      mockBootstrap.state.storage = storage;
      return storage as T;
    }
    case "refresh_modules":
    case "sync_modules_to_storage":
      return clone(mockBootstrap.state.modules) as T;
    case "list_instances_from_storage":
      return clone(mockBootstrap.state.instances) as T;
    case "read_instance_connection_info_from_storage": {
      const ids = args?.instanceIds ?? args?.instance_ids;
      if (!Array.isArray(ids) || ids.length > 128 || ids.some((id) => typeof id !== "string")) {
        throw new Error("Invalid instance connection request");
      }
      return clone([...new Set(ids as string[])].map((instanceId) => {
        if (!mockBootstrap.state.instances.some((instance) => instance.id === instanceId)) {
          throw new Error(`Unknown instance: ${instanceId}`);
        }
        const details = ensureMockInstanceDetails(instanceId);
        const settings = parseSettingsJson(details.settings_json) ?? {};
        return { instance_id: instanceId, bind_ip: details.summary.bind_ip, ports: details.ports,
          settings_json: details.summary.module_id === "corekeeper"
            ? JSON.stringify({ game_id: settings.game_id, direct_connection_enabled: settings.direct_connection_enabled })
            : "{}" };
      })) as T;
    }
    case "read_module_details": {
      const details = buildMockModuleDetails(String(args?.moduleId ?? args?.module_id ?? "dontstarve"));
      if (includePreservedProgramCounts) return details as T;
      const { instance_program_count: _instances, archived_program_count: _archives, ...summary } = details.summary;
      return { ...details, summary } as T;
    }
    case "read_module_configuration_icons":
      return {} as T;
    case "read_instance_isolation": {
      const input = args?.input as { instance_id: string };
      const details = ensureMockInstanceDetails(input.instance_id);
      const instances = mockBootstrap.state.instances.map((instance) => ensureMockInstanceDetails(instance.id));
      return buildMockInstanceIsolation(details, instances) as T;
    }
    case "read_instance_details_from_storage":
      return ensureMockInstanceDetails(String(args?.instanceId ?? args?.instance_id ?? "srv-dst-1")) as T;
    case "read_dontstarve_mod_configuration_specs":
      return readMockDstModConfigurationSpecs(Array.isArray(args?.ids) ? args.ids as string[] : []) as T;
    case "read_project_zomboid_workshop_mods_snapshot":
      return readMockProjectZomboidWorkshopModsSnapshot(Array.isArray(args?.ids) ? args.ids as string[] : []) as T;
    case "read_instance_runtime_overview_from_storage":
      return buildMockRuntime(String(args?.instanceId ?? args?.instance_id ?? "srv-dst-1")) as T;
    case "read_instance_live_players": {
      const instanceId = String(args?.instanceId ?? args?.instance_id ?? "srv-dst-1");
      return mockLivePlayerStore.read(buildMockLivePlayerContext(instanceId)) as T;
    }
    case "refresh_instance_live_players": {
      const instanceId = String(args?.instanceId ?? args?.instance_id ?? "srv-dst-1");
      return mockLivePlayerStore.refresh(buildMockLivePlayerContext(instanceId)) as T;
    }
    case "execute_instance_player_action": {
      const input = (args?.input ?? {}) as ExecuteInstancePlayerActionInput & Record<string, unknown>;
      const context = buildMockLivePlayerContext(String(input.instance_id ?? ""));
      const player = mockLivePlayerStore.read(context).entries.find((entry) => entry.player_key === input.player_key);
      const result = mockLivePlayerStore.execute(context, input);
      if (context.module_id === "sevendaystodie" && input.action_id === "ban_player") {
        const current = ensureMockInstanceDetails(context.instance_id);
        const settings = buildMockSevenDaysBanSettings(
          parseSettingsJson(current.settings_json) ?? {}, player, result.executed_at_unix_ms
        );
        mockInstanceDetailsStore.set(context.instance_id, { ...current, settings_json: JSON.stringify(settings, null, 2) });
      }
      return result as T;
    }
    case "execute_instance_manual_player_action": {
      const input = (args?.input ?? {}) as ExecuteInstanceManualPlayerActionInput & Record<string, unknown>;
      const allowedKeys = new Set(["action_id", "instance_id", "role", "target"]);
      if (Object.keys(input).some((key) => !allowedKeys.has(key))) {
        throw new Error("Manual player actions accept no client dispatch metadata.");
      }
      const rawInstanceId = String(input.instance_id ?? "");
      const rawActionId = String(input.action_id ?? "");
      const target = String(input.target ?? "");
      const role = input.role === null || input.role === undefined ? "" : String(input.role);
      const invalidBoundedField = (value: string, maxChars: number) => (
        !value.trim()
        || [...value.trim()].length > maxChars
        || [...value].some((character) => /[\u0000-\u001f\u007f]/.test(character))
      );
      if (invalidBoundedField(rawInstanceId, 128)) {
        throw new Error("instance_id is invalid");
      }
      if (invalidBoundedField(rawActionId, 128)) {
        throw new Error("action_id is invalid");
      }
      if (invalidBoundedField(target, 256)) {
        throw new Error("target is invalid");
      }
      if (role && invalidBoundedField(role, 64)) {
        throw new Error("role is invalid");
      }
      const instanceId = rawInstanceId.trim();
      const actionId = rawActionId.trim();
      if (!instanceId || !actionId || !target.trim()) {
        throw new Error("Manual player actions require an instance, action, and target.");
      }
      const details = ensureMockInstanceDetails(instanceId);
      if (!details.active_run) {
        throw new Error("The server is not running.");
      }
      const moduleDetails = buildMockModuleDetails(details.summary.module_id);
      const playerList = moduleDetails.runtime.player_list;
      if (playerList?.action_id === actionId || playerList?.player_action_ids.includes(actionId)) {
        throw new Error("Structured player actions require an authoritative player snapshot.");
      }
      if (moduleDetails.runtime.player_management?.status === "pending_adapter") {
        throw new Error("This module does not expose verified manual player actions.");
      }
      const action = (moduleDetails.runtime.player_actions ?? [])
        .find((candidate) => candidate.id === actionId);
      if (!action || action.kind === "broadcast" || !action.command_template.includes("{{target}}")) {
        throw new Error("The requested runtime action is not an identity-bound player action.");
      }
      if (mockSchemaConsumesAction(mockModuleSchemasById[details.summary.module_id], actionId)) {
        throw new Error("This action is owned by the persistent access-control workflow.");
      }
      resolveMockDeclaredRuntimeAction(
        moduleDetails.runtime.player_actions ?? [],
        details.summary.module_id,
        {
          runtimeActionId: actionId,
          runtimeActionTarget: target,
          runtimeActionRole: role
        }
      );
      const result: ExecuteInstancePlayerActionResult = {
        action_id: actionId,
        status: "sent",
        executed_at_unix_ms: Date.now(),
        summary: "The declared manual player action was sent."
      };
      return result as T;
    }
    case "read_instance_runtime_window_snapshot":
      return buildMockRuntimeWindowSnapshot(String(args?.instanceId ?? args?.instance_id ?? "srv-dst-1")) as T;
    case "suppress_instance_runtime_windows": {
      const instanceId = String(args?.instanceId ?? args?.instance_id ?? "srv-dst-1");
      const details = ensureMockInstanceDetails(instanceId);
      const windowPolicy = buildMockModuleDetails(details.summary.module_id).process?.window_policy ?? "background";

      if (String(windowPolicy).toLowerCase() === "external") {
        throw new Error("This module is configured for external windows and cannot be suppressed from the managed surface.");
      }

      const before = buildMockRuntimeWindowSnapshot(instanceId);
      if (before.windows.length > 0) {
        mockSuppressedRuntimeWindowInstances.add(instanceId);
      }
      const after = buildMockRuntimeWindowSnapshot(instanceId);
      const result: RuntimeWindowSuppressionResult = {
        instance_id: instanceId,
        source: "manual",
        attempted_at_unix_ms: Date.now(),
        inspected_process_count: Math.max(before.inspected_process_count, after.inspected_process_count),
        visible_window_count_before: before.windows.length,
        suppressed_window_count: Math.max(0, before.windows.length - after.windows.length),
        remaining_visible_window_count: after.windows.length,
        summary:
          after.windows.length === 0
            ? "LanGame pulled the visible native windows back into the managed runtime surface."
            : "LanGame attempted suppression, but visible native windows are still exposed."
      };
      mockRuntimeWindowSuppressionStore.set(instanceId, {
        ...result,
        status:
          result.visible_window_count_before <= 0
            ? "idle"
            : result.remaining_visible_window_count <= 0
              ? "clear"
              : "partial"
      });
      return result as T;
    }
    case "read_instance_log_document_from_storage": {
      const instanceId = String(args?.instanceId ?? args?.instance_id ?? "srv-dst-1");
      const runId = Number(args?.runId ?? args?.run_id ?? 0) || null;
      const snapshot = ensureMockLogDocument(instanceId, Number(args?.maxLines ?? args?.max_lines ?? 200));
      const details = ensureMockInstanceDetails(instanceId);

      const source = args?.source;
      if (source != null && source !== "game" && source !== "console") throw new Error("Unknown runtime log source");
      if (source != null && !runId) throw new Error("An explicit runtime log source requires a run ID");
      if (source === "game" && !isArkModule(details.summary.module_id)) throw new Error("Game source selection requires an ARK map");
      if (isArkModule(details.summary.module_id) && runId && source != null) {
        const process = buildMockArkProcesses(details.summary, parseSettingsJson(details.settings_json) ?? {})
          .find((process) => process.run_id === runId);
        if (!process) throw new Error(`ARK map run ${runId} is unavailable.`);
        if (source === "console") return { source_path: process.log_path, lines: [`[LanGameCMD] ${process.display_name} process attached`], total_lines: 1, truncated: false, read_error: null } as T;
        const edition = details.summary.module_id === "arksurvivalascended" ? "ascended" : "evolved";
        return { ...mockArkMapLogs.read(details, process, Number(args?.maxLines ?? args?.max_lines ?? 200)),
          source_path: `D:/LanGame/instances/${instanceId}/logs/ark-${edition}-${process.is_primary ? "server" : process.process_key}.log` } as T;
      }

      if (isArkModule(details.summary.module_id) && runId && runId !== 1) {
        const process = buildMockArkProcesses(details.summary, parseSettingsJson(details.settings_json) ?? {})
          .find((process) => process.run_id === runId);
        if (!process) throw new Error(`ARK map run ${runId} is unavailable.`);
        return mockArkMapLogs.read(details, process, Number(args?.maxLines ?? args?.max_lines ?? 200)) as T;
      }

      if (details.summary.module_id === "dontstarve" && runId === 2) {
        return {
          ...snapshot,
          source_path: `D:/LanGame/instances/${instanceId}/logs/run-1-caves.log`,
          lines: [...snapshot.lines, "[caves] dedicated shard heartbeat ok"],
          total_lines: (snapshot.total_lines ?? snapshot.lines.length) + 1
        } as T;
      }

      if (details.summary.module_id === "dontstarve" && runId === 1) {
        return {
          ...snapshot,
          source_path: `D:/LanGame/instances/${instanceId}/logs/run-1-master.log`,
          lines: [...snapshot.lines, "[master] dedicated shard heartbeat ok"],
          total_lines: (snapshot.total_lines ?? snapshot.lines.length) + 1
        } as T;
      }

      return snapshot as T;
    }
    case "probe_steamcmd_status":
      return clone(mockSteamCmdStatus) as T;
    case "read_steamcmd_prepare_progress":
      return (mockSteamCmdProgress && (args?.operationId == null || mockSteamCmdProgress.operation_id === args.operationId) ? clone(mockSteamCmdProgress) : null) as T;
    case "cancel_steamcmd_preparation": {
      if (!mockSteamCmdProgress || mockSteamCmdProgress.operation_id !== args?.operationId) throw new Error("SteamCMD preparation progress is no longer available.");
      if (mockSteamCmdProgress.active && mockSteamCmdProgress.cancellable) {
        mockSteamCmdProgress = { ...mockSteamCmdProgress, active: false, cancellable: false, cancel_requested: true, cancelled: true, error: null };
        mockSteamCmdStatus = { ...mockSteamCmdStatus, ready: false };
      }
      return clone(mockSteamCmdProgress) as T;
    }
    case "cancel_installation_job": {
      const job = mockBootstrap.state.jobs.find((item) => item.id === args?.jobId);
      if (!job) throw new Error("Installation task is no longer available.");
      if (job.cancellable && ["Pending", "Running"].includes(job.status)) {
        const stopped = { ...job, status: "Cancelled", cancellable: false, cancel_requested: true };
        pushMockJob(stopped);
        return clone(stopped) as T;
      }
      return clone(job) as T;
    }
    case "ensure_steamcmd_ready": {
      mockSteamCmdProgress = {
        operation_id: String(args?.operationId ?? ""), active: false, phase: "ready", detail: "",
        cancellable: false, cancel_requested: false, cancelled: false,
        downloaded_bytes: null, total_bytes: null, output_excerpt: "",
        elapsed_seconds: 0, idle_seconds: 0, error: null
      };
      mockSteamCmdStatus = {
        ...mockSteamCmdStatus,
        ready: true,
        executable_exists: true,
        ownership: "managed",
        can_uninstall: true
      };
      pushMockJob({
        id: `steamcmd-${Date.now()}`,
        kind: "InstallSteamCmd",
        label: "Prepare SteamCMD",
        status: "Completed",
        progress_percent: 100,
        detail: `SteamCMD ready at ${mockSteamCmdStatus.executable_path}`,
        output_excerpt: "Preview mode only. SteamCMD was marked ready in mock state."
      });
      return clone(mockSteamCmdStatus) as T;
    }
    case "uninstall_steamcmd": {
      mockSteamCmdStatus = {
        ...mockSteamCmdStatus,
        ready: false,
        executable_exists: false,
        ownership: "none",
        can_uninstall: false
      };
      return clone(mockSteamCmdStatus) as T;
    }
    case "preview_instance_launch":
      return buildMockLaunchPlan(String(args?.instanceId ?? args?.instance_id ?? "srv-dst-1")) as T;
        case "create_instance_backup": {
      const instanceId = String(args?.instanceId ?? args?.instance_id ?? "srv-dst-1");
      const details = ensureMockInstanceDetails(instanceId);
      const createdAt = Date.now();
      const backup: InstanceBackupResult = {
        backup_id: `saves-${createdAt}`,
        instance_id: instanceId,
        backup_kind: "manual",
        created_at_unix_ms: createdAt,
        backup_path: `D:/LanGame/instances/${instanceId}/backups/saves-${createdAt}`,
        display_name: null,
        saves_path: details.saves_path,
        file_count: 3,
        total_bytes: 524288
      };
      upsertMockBackup(instanceId, backup);
      return backup as T;
    }
    case "list_instance_backups": {
      const instanceId = String(args?.instanceId ?? args?.instance_id ?? "srv-dst-1");
      return ensureMockInstanceBackups(instanceId) as T;
    }
    case "rename_instance_backup": {
      const instanceId = String(args?.instanceId ?? args?.instance_id ?? "srv-dst-1");
      const backupId = String(args?.backupId ?? args?.backup_id ?? "");
      const displayName = normalizeMockBackupDisplayName(args?.displayName ?? args?.display_name ?? null);
      return renameMockBackup(instanceId, backupId, displayName) as T;
    }
    case "delete_instance_backup": {
      const instanceId = String(args?.instanceId ?? args?.instance_id ?? "srv-dst-1");
      const backupId = String(args?.backupId ?? args?.backup_id ?? "");
      return deleteMockBackup(instanceId, backupId) as T;
    }
    case "restore_instance_backup": {
      const instanceId = String(args?.instanceId ?? args?.instance_id ?? "srv-dst-1");
      const backupId = String(args?.backupId ?? args?.backup_id ?? "");
      const details = ensureMockInstanceDetails(instanceId);
      if (String(details.summary.status).toLowerCase() === "running") {
        throw new Error("Stop the server before restoring a backup.");
      }

      const selectedBackup = ensureMockInstanceBackups(instanceId).find((item) => item.backup_id === backupId);
      if (!selectedBackup) {
        throw new Error(`Backup not found: ${backupId}`);
      }

      const safeguardCreatedAt = Date.now();
      const safeguardBackup: InstanceBackupResult = {
        backup_id: `pre-restore-${safeguardCreatedAt}`,
        instance_id: instanceId,
        backup_kind: "pre_restore",
        created_at_unix_ms: safeguardCreatedAt,
        backup_path: `D:/LanGame/instances/${instanceId}/backups/pre-restore-${safeguardCreatedAt}`,
        display_name: null,
        saves_path: details.saves_path,
        file_count: selectedBackup.file_count,
        total_bytes: selectedBackup.total_bytes
      };
      upsertMockBackup(instanceId, safeguardBackup);

      return {
        instance_id: instanceId,
        backup_id: backupId,
        restored_at_unix_ms: Date.now(),
        saves_path: details.saves_path,
        restored_file_count: selectedBackup.file_count,
        restored_total_bytes: selectedBackup.total_bytes,
        safeguard_backup_id: safeguardBackup.backup_id,
        safeguard_backup_path: safeguardBackup.backup_path
      } as T;
    }
    case "preview_dontstarve_world_start": {
      const instanceId = String(args?.instanceId ?? args?.instance_id ?? "srv-dst-1");
      return mockDstWorldStateStore.preview(ensureMockInstanceDetails(instanceId)) as T;
    }
    case "import_dontstarve_world_data": {
      const instanceId = String(args?.instanceId ?? args?.instance_id ?? "srv-dst-1");
      const sourcePath = String(args?.sourcePath ?? args?.source_path ?? "").trim();
      const details = ensureMockInstanceDetails(instanceId);
      if (String(details.summary.status).toLowerCase() === "running") {
        throw new Error("Stop the server before importing world data.");
      }
      if (!sourcePath) {
        throw new Error("Select a DST cluster folder first.");
      }
      mockDstWorldStateStore.start(details);

      const stamp = Date.now();
      const dstSettings = parseSettingsJson(details.settings_json);
      const iaLayout = dstSettings?.shard_layout === "island_adventures";
      const importedCaves = iaLayout || dstSettings?.enable_caves === true;
      const result: DstWorldImportResult = {
        instance_id: instanceId,
        source_cluster_path: sourcePath,
        target_cluster_path: `D:/LanGame/instances/${instanceId}/config/clusters/main`,
        safeguard_path: `D:/LanGame/instances/${instanceId}/backups/dst-import-${stamp}`,
        imported_master: true,
        imported_caves: importedCaves,
        imported_shards: iaLayout ? ["Master", "Caves", "Islands", "Volcano"] : importedCaves ? ["Master", "Caves"] : ["Master"],
        imported_workshop_mod_ids: iaLayout ? ["3435352667", "1467214795"] : [],
        copied_file_count: 42,
        copied_total_bytes: 6 * 1024 * 1024
      };
      const log = ensureMockLogDocument(instanceId, 200);
      log.lines = [...log.lines, `[info] imported DST world data from ${sourcePath}`];
      log.total_lines = log.lines.length;
      mockLogDocumentStore.set(instanceId, clone(log));
      return result as T;
    }
    case "update_instance_program": {
      const instanceId = String(args?.instanceId ?? args?.instance_id ?? "");
      const details = ensureMockInstanceDetails(instanceId);
      if (details.summary.status !== "Stopped" || details.active_run) throw new Error("Stop the instance before updating program files.");
      const program = mockInstancePrograms.get(instanceId);
      if (program?.mode === "shared") {
        for (const [otherId, otherProgram] of mockInstancePrograms) {
          if (otherProgram.mode !== "shared" || otherProgram.root !== program.root) continue;
          const other = ensureMockInstanceDetails(otherId);
          if (other.summary.status !== "Stopped" || other.active_run) {
            throw new Error("Stop every instance using this shared program before updating it.");
          }
        }
      }
      const module = buildMockModuleDetails(details.summary.module_id);
      if (moduleRequiresSteamCmd(module.summary, module.install)) requireMockSteamCmd();
      const root = mockInstancePrograms.get(instanceId)?.root ?? `${mockInstanceRootFromDetails(details)}/runtime`;
      return { module_id: module.summary.id, steam_app_id: module.summary.steam_app_id ?? 0,
        operation: args?.validate ? "validate" : "update", install_root: root,
        executable_path: `${root}/${module.process?.executable ?? "server.exe"}`,
        executable_exists: true, install_state: "Installed", current_version: null,
        output_excerpt: "Preview mode only. No program files were changed." } as T;
    }
    case "install_module_game":
    case "validate_module_game": {
      const moduleId = String(args?.moduleId ?? args?.module_id ?? "dontstarve");
      const operation = command === "validate_module_game" ? "validate" : "install";
      const module = buildMockModuleDetails(moduleId);
      const installRoot = `D:/LanGame/server-files/${module.install?.shared_game_dir ?? module.summary.id}`;
      if (moduleRequiresSteamCmd(module.summary, module.install)) requireMockSteamCmd();
      mockBootstrap.state.modules = mockBootstrap.state.modules.map((module) =>
        module.id === moduleId ? { ...module, install_state: "Installed" } : module
      );
      pushMockJob({
        id: `${operation}-${moduleId}-${Date.now()}`,
        kind: operation === "validate" ? "ValidateGame" : "DownloadGame",
        label: `${operation === "validate" ? "Validate" : "Install"} ${moduleId}`,
        status: "Completed",
        progress_percent: 100,
        target_id: moduleId,
        detail: `${moduleId} ${operation} complete.`,
        output_excerpt: "Preview mode only. No Tauri bridge detected."
      });
      return {
        module_id: moduleId,
        steam_app_id: module.summary.steam_app_id ?? null,
        operation,
        install_root: installRoot,
        executable_path: `${installRoot}/${module.process?.executable ?? "server.exe"}`,
        executable_exists: true,
        install_state: "Installed",
        output_excerpt: "Preview mode only. No Tauri bridge detected."
      } as T;
    }
    case "uninstall_module_game": {
      const moduleId = String(args?.moduleId ?? args?.module_id ?? "dontstarve");
      const module = buildMockModuleDetails(moduleId);
      const installRoot = `D:/LanGame/server-files/${module.install?.shared_game_dir ?? module.summary.id}`;
      const used = [...mockInstancePrograms.values()].some((program) => program.root === installRoot);
      const installState = used ? "Installed" : "NotInstalled";
      mockBootstrap.state.modules = mockBootstrap.state.modules.map((module) =>
        module.id === moduleId ? { ...module, install_state: installState } : module
      );
      pushMockJob({
        id: `uninstall-${moduleId}-${Date.now()}`,
        kind: "UninstallGame",
        label: `Uninstall ${moduleId}`,
        status: "Completed",
        progress_percent: 100,
        target_id: moduleId,
        detail: `${moduleId} uninstall complete.`,
        output_excerpt: "Preview mode only. No Tauri bridge detected."
      });
      const result: ModuleUninstallResult = {
        module_id: moduleId,
        install_state: installState,
        executable_exists: used,
        cleanup: {
          removed_install_roots: used ? [] : [installRoot],
          preserved_data_paths: [],
          retained_installs: used ? [{ install_root: installRoot, reason: "in_use" }] : []
        }
      };
      return result as T;
    }
    case "download_steam_workshop_items": {
      const instanceId = String(args?.instanceId ?? args?.instance_id ?? "srv-dst-1");
      const details = ensureMockInstanceDetails(instanceId);
      const module = buildMockModuleDetails(details.summary.module_id);
      const consumerAppId = Number(module.workshop?.consumer_app_id ?? 0);
      if (!consumerAppId) {
        throw new Error(`Module ${module.summary.id} does not declare Steam Workshop support.`);
      }
      const itemIds = Array.isArray(args?.ids)
        ? Array.from(new Set((args.ids as string[]).flatMap((entry) => String(entry).match(/\d{6,}/g) ?? [])))
        : [];
      if (itemIds.length === 0) {
        throw new Error("No valid Steam Workshop item IDs were provided.");
      }
      requireMockSteamCmd();
      const installedIds = mockWorkshopInstallationStore.get(instanceId) ?? new Set<string>();
      itemIds.forEach((itemId) => installedIds.add(itemId));
      mockWorkshopInstallationStore.set(instanceId, installedIds);
      const installRoot = `D:/LanGame/server-files/${module.install?.shared_game_dir ?? module.summary.id}`;
      const workshopRoot = `${installRoot}/steamapps/workshop/content/${consumerAppId}`;
      const result: SteamWorkshopDownloadResult = {
        consumer_app_id: consumerAppId,
        install_root: installRoot,
        workshop_root: workshopRoot,
        items: itemIds.map((itemId) => ({
          item_id: itemId,
          expected_path: `${workshopRoot}/${itemId}`,
          expected_path_exists: true
        })),
        output_excerpt: "Preview mode only. Mock SteamCMD marked the Workshop cache as downloaded."
      };
      pushMockJob({
        id: `workshop-${instanceId}-${Date.now()}`,
        kind: "DownloadWorkshop",
        label: `Download Workshop items for ${details.summary.name}`,
        status: "Completed",
        progress_percent: 100,
        target_id: instanceId,
        detail: `Downloaded ${itemIds.length} Workshop item(s) in preview mode.`,
        output_excerpt: result.output_excerpt
      });
      return result as T;
    }
    case "read_steam_workshop_installation_status": {
      const instanceId = String(args?.instanceId ?? args?.instance_id ?? "srv-dst-1");
      const details = ensureMockInstanceDetails(instanceId);
      const module = buildMockModuleDetails(details.summary.module_id);
      const consumerAppId = Number(module.workshop?.consumer_app_id ?? 0);
      if (!consumerAppId) {
        throw new Error(`Module ${module.summary.id} does not declare Steam Workshop support.`);
      }
      const requestedItemIds = Array.isArray(args?.ids)
        ? Array.from(new Set((args.ids as string[]).flatMap((entry) => String(entry).match(/\d{6,}/g) ?? [])))
        : [];
      const installedIds = mockWorkshopInstallationStore.get(instanceId) ?? new Set<string>();
      const itemIds = Array.from(new Set([...requestedItemIds, ...installedIds]));
      const installRoot = `D:/LanGame/server-files/${module.install?.shared_game_dir ?? module.summary.id}`;
      const workshopRoot = `${installRoot}/steamapps/workshop/content/${consumerAppId}`;
      const items = itemIds.map((itemId) => {
        const dstSpec = module.summary.id === "dontstarve"
          ? readMockDstModConfigurationSpecs([itemId])[0]
          : null;
        const pzItem = module.summary.id === "projectzomboid"
          ? readMockProjectZomboidWorkshopModsSnapshot([itemId]).items[0]
          : null;
        const installed = installedIds.has(itemId) ||
          Boolean(dstSpec && ["loaded", "no_options", "loaded_with_warnings"].includes(dstSpec.status)) ||
          Boolean(pzItem && ["installed", "installed_with_warnings"].includes(pzItem.status));
        if (installed) {
          installedIds.add(itemId);
        }
        return {
          item_id: itemId,
          path: `${workshopRoot}/${itemId}`,
          installed
        };
      });
      mockWorkshopInstallationStore.set(instanceId, installedIds);
      return {
        consumer_app_id: consumerAppId,
        searched_roots: [workshopRoot],
        items
      } as T;
    }
    case "read_manual_mod_inventory": {
      const instanceId = String(args?.instanceId ?? args?.instance_id ?? "srv-dst-1");
      const details = ensureMockInstanceDetails(instanceId);
      const module = buildMockModuleDetails(details.summary.module_id);
      const staging = module.mods?.manual_staging;
      if (!staging) {
        throw new Error(`Module ${module.summary.id} does not declare a local mod inventory target.`);
      }
      const installRoot = `D:/LanGame/server-files/${module.install?.shared_game_dir ?? module.summary.id}`;
      const instanceRoot = mockInstanceRootFromDetails(details);
      const targetPath = staging.target_template
        .replace("{{paths.install_root}}", installRoot)
        .replace("{{paths.instance_root}}", instanceRoot);
      const cached = mockManualModInventoryStore.get(instanceId);
      if (cached) {
        return clone(cached) as T;
      }
      const result: ManualModInventoryResult = {
        instance_id: instanceId,
        module_id: module.summary.id,
        source_label: module.mods?.source?.label ?? module.summary.name,
        target_label: staging.target_label,
        target_path: targetPath,
        target_exists: true,
        items: module.summary.id === "arksurvivalascended"
          ? [
              {
                name: "927084_5864784",
                path: `${targetPath}/927084_5864784`,
                item_type: "directory",
                inferred_id: "927084",
                file_count: 12,
                total_bytes: 18 * 1024 * 1024,
                modified_unix_ms: Date.now() - 3600_000
              }
            ]
          : []
      };
      return result as T;
    }
    case "resolve_manual_mod_references": {
      const instanceId = String(args?.instanceId ?? args?.instance_id ?? "srv-dst-1");
      const details = ensureMockInstanceDetails(instanceId);
      const module = buildMockModuleDetails(details.summary.module_id);
      const enablement = module.mods?.enablement;
      if (!enablement) {
        throw new Error(`Module ${module.summary.id} does not declare a mod enablement target.`);
      }
      const references = Array.isArray(args?.references) ? args.references as string[] : [];
      const items = references.map((reference) => {
        const direct = reference.match(/\b(\d{5,})\b/)?.[1] ?? null;
        const slugMatch = reference.match(/curseforge\.com\/[^/]+\/mods\/([^/?#]+)/i);
        const resolvedId = direct ?? (slugMatch ? "1346144" : null);
        return {
          reference,
          status: resolvedId ? "resolved" : "failed",
          resolved_id: resolvedId,
          title: slugMatch ? "DevKitLiveModTesting" : null,
          message: resolvedId ? null : "Mock resolver could not find a mod ID."
        };
      });
      const resolvedIds = Array.from(new Set(items.map((item) => item.resolved_id).filter((id): id is string => Boolean(id))));
      if (resolvedIds.length === 0) {
        throw new Error("No mod ID could be resolved from the dropped link.");
      }
      const result: ManualModReferenceResolveResult = {
        instance_id: instanceId,
        module_id: module.summary.id,
        source_label: module.mods?.source?.label ?? module.summary.name,
        setting_key: enablement.setting_key,
        setting_label: enablement.setting_label,
        items: items as ManualModReferenceResolveResult["items"],
        resolved_ids: resolvedIds
      };
      return result as T;
    }
    case "stage_manual_mod_files": {
      const instanceId = String(args?.instanceId ?? args?.instance_id ?? "srv-dst-1");
      const details = ensureMockInstanceDetails(instanceId);
      const module = buildMockModuleDetails(details.summary.module_id);
      const staging = module.mods?.manual_staging;
      if (!staging) {
        throw new Error(`Module ${module.summary.id} does not declare a local mod drop target.`);
      }
      const sourcePaths = Array.isArray(args?.sourcePaths)
        ? args.sourcePaths as string[]
        : Array.isArray(args?.source_paths)
          ? args.source_paths as string[]
          : [];
      if (sourcePaths.length === 0) {
        throw new Error("Drop a downloaded mod file or folder first.");
      }
      const installRoot = `D:/LanGame/server-files/${module.install?.shared_game_dir ?? module.summary.id}`;
      const instanceRoot = mockInstanceRootFromDetails(details);
      const targetPath = staging.target_template
        .replace("{{paths.install_root}}", installRoot)
        .replace("{{paths.instance_root}}", instanceRoot);
      const stageItems = sourcePaths.map((sourcePath) => {
        const sourceName = sourcePath.split(/[\\/]/).filter(Boolean).slice(-1)[0] ?? "mod";
        return {
          source_path: sourcePath,
          target_path: `${targetPath}/${sourceName}`,
          status: "installed",
          message: null,
          file_count: 3,
          total_bytes: 1024 * 1024
        };
      });
      const result: ManualModStageResult = {
        instance_id: instanceId,
        module_id: module.summary.id,
        source_label: module.mods?.source?.label ?? module.summary.name,
        target_label: staging.target_label,
        target_path: targetPath,
        affected_root_names: [...new Set(stageItems.map((item) =>
          item.target_path.split(/[\\/]/).filter(Boolean).slice(-1)[0]!))].sort(),
        items: stageItems,
        copied_file_count: sourcePaths.length * 3,
        copied_total_bytes: sourcePaths.length * 1024 * 1024
      };
      mockManualModInventoryStore.set(instanceId, {
        instance_id: instanceId,
        module_id: module.summary.id,
        source_label: module.mods?.source?.label ?? module.summary.name,
        target_label: staging.target_label,
        target_path: targetPath,
        target_exists: true,
        items: stageItems.map((item) => {
          const name = item.target_path.split(/[\\/]/).filter(Boolean).slice(-1)[0] ?? "mod";
          return {
            name,
            path: item.target_path,
            item_type: "directory",
            inferred_id: mockInferManualModId(module.mods?.enablement?.id_strategy, name),
            file_count: item.file_count,
            total_bytes: item.total_bytes,
            modified_unix_ms: Date.now()
          };
        })
      });
      return result as T;
    }
    case "install_manual_mod_references": {
      const instanceId = String(args?.instanceId ?? args?.instance_id ?? "srv-dst-1");
      const details = ensureMockInstanceDetails(instanceId);
      const module = buildMockModuleDetails(details.summary.module_id);
      const staging = module.mods?.manual_staging;
      const provider = module.mods?.source?.provider?.toLowerCase() ?? "";
      if (!staging) {
        throw new Error(`Module ${module.summary.id} does not declare a local mod drop target.`);
      }
      if (module.mods?.enablement) {
        throw new Error(`Module ${module.summary.id} resolves site references into ${module.mods.enablement.setting_label}.`);
      }
      if (!["modrinth", "thunderstore"].includes(provider)) {
        throw new Error(`${module.mods?.source?.label ?? module.summary.name} does not support automatic package downloads in the mock API.`);
      }
      const references = Array.isArray(args?.references) ? args.references as string[] : [];
      if (references.length === 0) {
        throw new Error("Drop a mod link or paste a mod id first.");
      }
      const extension = provider === "modrinth" ? "jar" : "zip";
      const sourcePaths = references.map((reference, index) => {
        const slug = reference.split(/[/:?#]+/).filter(Boolean).slice(-1)[0] ?? `mod-${index + 1}`;
        return `D:/LanGame/mock-downloads/${slug}.${extension}`;
      });
      return await invokeMock<T>("stage_manual_mod_files", {
        instanceId,
        instance_id: instanceId,
        sourcePaths,
        source_paths: sourcePaths
      });
    }
    case "create_instance_record": {
      const input = args?.input as CreateInstanceInput;
      const module = buildMockModuleDetails(input.module_id);
      const mode = args?.programMode ?? args?.program_mode ?? module.runtime.program_sharing ?? "independent";
      if (mode !== "shared" && mode !== "independent") throw new Error("Invalid program mode.");
      if (mode === "shared" && module.runtime.program_sharing !== "shared") throw new Error("This game requires an independent installation.");
      const instanceId = `srv-${input.module_id}-${mockBootstrap.state.instances.length + 1}`;
      const settings = {
        ...buildMockSettingsForModule(input.module_id, input.name, instanceId),
        bind_ip: "0.0.0.0"
      };
      const summary: InstanceSummary = {
        id: instanceId,
        name: input.name,
        module_id: input.module_id,
        status: "Stopped",
        active_process_count: 0,
        bind_ip: "0.0.0.0",
        port_count: buildMockPorts(input.module_id).length,
        autostart: false
      };
      upsertMockSummary(summary);
      const provisioningPorts = buildMockPorts(input.module_id);
      const provisioning: InstanceProvisioning = {
        summary,
        config_file_path: `D:/LanGame/instances/${summary.id}/config/instance.json`,
        ports: provisioningPorts
      };
      mockInstancePrograms.set(instanceId, { mode, root: mode === "shared"
        ? `${mockBootstrap.state.settings.games_root}/${module.install?.shared_game_dir ?? module.summary.id}`
        : `${mockBootstrap.state.settings.servers_root}/${instanceId}/runtime` });
      mockInstanceDetailsStore.set(summary.id, {
        summary,
        config_file_path: provisioning.config_file_path,
        saves_path: buildMockSavesPath(summary, settings),
        backup_uses_declared_saves_path: buildMockBackupUsesDeclaredSavesPath(summary),
        auto_backup_on_stop: false,
        backup_retention_count: 10,
        settings_json: JSON.stringify(settings, null, 2),
        ports: clone(provisioning.ports),
        active_run: null
      });
      mockInstanceBackupsStore.set(summary.id, []);
      if (summary.module_id === "dontstarve") mockDstWorldStateStore.create(summary.id);
      mockLogDocumentStore.delete(summary.id);
      mockArkMapLogs.clear(summary.id);
      mockSuppressedRuntimeWindowInstances.delete(summary.id);
      mockRuntimeWindowSuppressionStore.delete(summary.id);
      return provisioning as T;
    }
    case "inspect_module_programs":
    case "inspect_instance_removal":
      throw new Error("Installation ownership inspection requires a connected desktop host; the development preview does not inspect real files.");
    case "list_instance_archives":
      return mockStorageManagement.list(mockBootstrap.state.settings.archives_root) as T;
    case "read_instance_archive_details":
      return mockStorageManagement.details((args?.input as { archive_id: string }).archive_id,
        mockBootstrap.state.settings.archives_root) as T;
    case "scan_storage_usage": {
      const input = args?.input as { scan_id: string };
      return await mockStorageManagement.scan(input.scan_id, mockBootstrap.state.settings,
        mockBootstrap.state.instances.map((instance) => ensureMockInstanceDetails(instance.id))) as T;
    }
    case "cancel_storage_usage_scan":
      return mockStorageManagement.cancel((args?.input as { scan_id: string }).scan_id) as T;
    case "restore_instance_archive": {
      const archiveId = (args?.input as { archive_id: string }).archive_id;
      const archived = mockStorageManagement.restore(archiveId, mockBootstrap.state.settings.archives_root,
        mockBootstrap.state.instances.map((instance) => instance.id));
      const details = { ...archived.details, summary: { ...archived.details.summary, status: "Stopped", active_process_count: 0, autostart: false }, active_run: null };
      mockInstanceDetailsStore.set(details.summary.id, details);
      mockInstanceBackupsStore.set(details.summary.id, archived.backups);
      if (archived.program) mockInstancePrograms.set(details.summary.id, archived.program);
      mockBootstrap.state.instances.push(details.summary);
      return { archive_id: archiveId, instance_id: details.summary.id, instance_name: details.summary.name,
        restored_instance_root: archived.summary.previous_instance_root,
        external_saves_backup_id: archived.summary.external_saves_backup_id,
        preserved_external_saves_path: archived.summary.preserved_external_saves_path,
        external_saves_restore_required: Boolean(archived.summary.external_saves_backup_id) } as T;
    }
    case "purge_instance_archive":
      return mockStorageManagement.purge((args?.input as { archive_id: string }).archive_id,
        mockBootstrap.state.settings.archives_root) as T;
    case "archive_instance_record":
    case "delete_instance_record": {
      const instanceId = String(args?.instanceId ?? args?.instance_id ?? "");
      const details = ensureMockInstanceDetails(instanceId);
      const previousInstanceRoot = mockInstanceRootFromDetails(details);
      const deletedAtUnixMs = Date.now();
      const archivedInstanceRoot = `${mockBootstrap.state.settings.archives_root}/${instanceId}-${deletedAtUnixMs}`;
      const savesArchivedWithInstanceRoot = mockPathIsWithinRoot(details.saves_path, previousInstanceRoot);
      const archiveResult: InstanceArchiveResult = {
        archive_id: crypto.randomUUID(),
        external_saves_backup_id: null,
        instance_id: instanceId,
        instance_name: details.summary.name,
        module_id: details.summary.module_id,
        deleted_at_unix_ms: deletedAtUnixMs,
        previous_instance_root: previousInstanceRoot,
        archived_instance_root: archivedInstanceRoot,
        effective_saves_path: details.saves_path,
        saves_archived_with_instance_root: savesArchivedWithInstanceRoot,
        preserved_external_saves_path: savesArchivedWithInstanceRoot ? null : details.saves_path
      };
      const result: InstanceArchiveResult | InstanceDeletionResult = command === "archive_instance_record" ? archiveResult : {
        instance_id: instanceId, instance_name: details.summary.name, module_id: details.summary.module_id,
        deleted_at_unix_ms: deletedAtUnixMs, deleted_instance_root: previousInstanceRoot,
        preserved_external_saves_path: savesArchivedWithInstanceRoot ? null : details.saves_path,
        program_cleanup: { removed_install_roots: [], preserved_data_paths: [], retained_installs: [] }
      };
      if (command === "archive_instance_record") {
        mockStorageManagement.archive(archiveResult, details, mockInstanceBackupsStore.get(instanceId) ?? [], mockInstancePrograms.get(instanceId));
      }
      mockBootstrap.state.instances = mockBootstrap.state.instances.filter((instance) => instance.id !== instanceId);
      mockInstanceDetailsStore.delete(instanceId);
      mockInstancePrograms.delete(instanceId);
      mockDstWorldStateStore.delete(instanceId);
      mockInstanceBackupsStore.delete(instanceId);
      mockLogDocumentStore.delete(instanceId);
      mockArkMapLogs.clear(instanceId);
      mockSuppressedRuntimeWindowInstances.delete(instanceId);
      mockRuntimeWindowSuppressionStore.delete(instanceId);
      mockBroadcastPolicyStore.delete(instanceId);
      mockBroadcastEventStore.delete(instanceId);
      mockLivePlayerStore.delete(instanceId);
      return result as T;
    }
    case "update_instance_autostart": {
      const instanceId = String(args?.instanceId ?? args?.instance_id ?? "");
      if (typeof args?.autostart !== "boolean") throw new Error("Autostart must be a boolean.");
      const current = ensureMockInstanceDetails(instanceId);
      const updated = { ...current, summary: { ...current.summary, autostart: args.autostart } };
      upsertMockSummary(updated.summary);
      mockInstanceDetailsStore.set(instanceId, clone(updated));
      return clone(updated) as T;
    }
    case "remove_instance_workshop_collection": {
      const input = args?.input as UpdateInstanceInput;
      const current = ensureMockInstanceDetails(input.id);
      const memberIds = args?.memberIds;
      if (current.summary.module_id !== "squad" || current.active_run ||
          current.summary.status !== "Stopped" || !Array.isArray(memberIds) || !memberIds.length ||
          !memberIds.every((id): id is string => typeof id === "string")) {
        throw new Error("Collection file removal requires a stopped Squad instance and selected members.");
      }
      if (input.bind_ip !== current.summary.bind_ip || input.auto_backup_on_stop !== current.auto_backup_on_stop ||
          input.backup_retention_count !== current.backup_retention_count || JSON.stringify(input.ports) !== JSON.stringify(current.ports)) {
        throw new Error("Collection removal cannot change other instance settings.");
      }
      const settings = parseSettingsJson(current.settings_json);
      const proposed = parseSettingsJson(input.settings_json);
      if (!settings || !proposed) throw new Error("Invalid collection settings.");
      const plan = buildWorkshopCollectionRemovalPlan({ settings, moduleId: "squad",
        collectionId: String(args?.collectionId ?? ""), selectedMemberIds: memberIds });
      if (changedSettingKeys(plan.nextSettings, proposed, [...Object.keys(plan.nextSettings), ...Object.keys(proposed)]).length) {
        throw new Error("Collection removal cannot change other instance settings.");
      }
      const saved = await invokeMock<InstanceDetails>("update_instance_record_if_current", { input, expectedSettingsJson: args?.expectedSettingsJson });
      const inventory = mockManualModInventoryStore.get(input.id);
      if (inventory) mockManualModInventoryStore.set(input.id, { ...inventory,
        items: inventory.items.filter((item) => !memberIds.includes(item.name)) });
      return saved as T;
    }
    case "update_instance_record_if_current": {
      const input = args?.input as UpdateInstanceInput;
      const current = ensureMockInstanceDetails(input.id);
      const expectedSettingsJson = String(args?.expectedSettingsJson ?? args?.expected_settings_json ?? "");
      const currentSettings = parseSettingsJson(current.settings_json);
      const expectedSettings = parseSettingsJson(expectedSettingsJson);
      const settingsMatch = currentSettings != null && expectedSettings != null
        ? JSON.stringify(currentSettings) === JSON.stringify(expectedSettings)
        : current.settings_json === expectedSettingsJson;
      if (!settingsMatch) {
        throw new Error("Instance settings changed while this edit was pending. Reload the server settings and retry.");
      }
      const nextSettings = parseSettingsJson(input.settings_json);
      const nextPorts = isArkModule(current.summary.module_id)
        ? updateMockArkPorts(current, nextSettings ?? {}, buildMockPorts(current.summary.module_id), input.ports)
        : clone(input.ports);
      const updatedSummary: InstanceSummary = {
        ...current.summary,
        bind_ip: input.bind_ip,
        port_count: nextPorts.length
      };
      upsertMockSummary(updatedSummary);
      const updatedDetails: InstanceDetails = {
        ...current,
        summary: updatedSummary,
        saves_path: buildMockSavesPath(
          updatedSummary,
          parseSettingsJson(input.settings_json) ?? parseSettingsJson(current.settings_json) ?? undefined
        ),
        auto_backup_on_stop: input.auto_backup_on_stop,
        backup_retention_count: Math.max(1, input.backup_retention_count),
        settings_json: input.settings_json,
        ports: nextPorts
      };
      mockInstanceDetailsStore.set(input.id, clone(updatedDetails));
      mockLivePlayerStore.delete(input.id);
      return updatedDetails as T;
    }
    case "apply_instance_player_access_mutation": {
      const input = args?.input as InstancePlayerAccessMutationInput;
      const current = ensureMockInstanceDetails(input.instanceId);
      const settings = parseSettingsJson(current.settings_json) ?? {};
      const schema = mockModuleSchemasById[current.summary.module_id] as {
        properties?: Record<string, Record<string, unknown>>;
      } | undefined;
      const property = schema?.properties?.[input.fieldKey];
      if (!property) {
        throw new Error("Player-access field is not declared by the module schema.");
      }
      const currentValue = settings[input.fieldKey]
        ?? property.default
        ?? (property.type === "array" ? [] : "");
      const running = String(current.summary.status).toLowerCase() === "running";
      const outcome = applyMockPlayerAccessMutation(property, currentValue, {
        expectedValue: input.expectedValue,
        operation: input.operation,
        value: clone(input.value)
      }, running, {
        fieldKey: input.fieldKey,
        properties: schema?.properties ?? {},
        settings
      });
      if (outcome.changed) {
        settings[input.fieldKey] = outcome.value;
        mockInstanceDetailsStore.set(input.instanceId, {
          ...current,
          settings_json: JSON.stringify(settings, null, 2)
        });
      }
      const result: InstancePlayerAccessMutationResult = {
        instanceId: input.instanceId,
        fieldKey: input.fieldKey,
        operation: input.operation,
        persistentStatus: outcome.changed ? "updated" : "unchanged",
        liveStatus: outcome.liveStatus,
        liveTarget: outcome.liveTarget,
        verificationStatus: outcome.verificationStatus,
        liveError: null,
        verificationResponse: null
      };
      return result as T;
    }
    case "start_instance_process": {
      const instanceId = String(args?.instanceId ?? args?.instance_id ?? "srv-dst-1");
      const current = ensureMockInstanceDetails(instanceId);
      const expectedWorldStart = args?.expectedWorldStart ?? args?.expected_world_start;
      if (expectedWorldStart !== undefined && expectedWorldStart !== null) {
        mockDstWorldStateStore.validateConfirmation(current, expectedWorldStart);
      }
      const module = buildMockModuleDetails(current.summary.module_id);
      if (module.summary.install_state !== "Installed") {
        throw new Error(JSON.stringify({
          code: "module_not_ready",
          module_id: module.summary.id,
          module_name: module.summary.name,
          install_state: module.summary.install_state,
          message: `Module ${module.summary.id} is not ready to start: ${module.summary.install_state}.`
        }));
      }
      const activeProcesses = buildMockActiveProcesses(current.summary, parseSettingsJson(current.settings_json) ?? {});
      if (isArkModule(current.summary.module_id)) mockArkMapLogs.clear(instanceId);
      mockDstWorldStateStore.start(current);
      const summary: InstanceSummary = {
        ...current.summary,
        status: activeProcesses.some((process) => process.status === "error") ? "Error" : "Running",
        active_process_count: activeProcesses.filter((process) => process.status === "running").length
      };
      mockSuppressedRuntimeWindowInstances.delete(instanceId);
      upsertMockSummary(summary);
      mockInstanceDetailsStore.set(instanceId, {
        ...current,
        summary,
        active_run: {
          run_id: 1,
          session_id: "demo-session",
          pid: 4321,
          log_path: activeProcesses[0]?.log_path ?? `D:/LanGame/instances/${summary.id}/logs/run-1.log`,
          process_count: activeProcesses.length,
          processes: activeProcesses
        }
      });
      mockLivePlayerStore.delete(instanceId);
      const log = ensureMockLogDocument(instanceId, 200);
      log.lines = [...log.lines, `[info] ${summary.name} start command accepted`];
      log.total_lines = log.lines.length;
      mockLogDocumentStore.set(instanceId, clone(log));
      return undefined as T;
    }
    case "stop_instance_process": {
      const instanceId = String(args?.instanceId ?? args?.instance_id ?? "srv-dst-1");
      const current = ensureMockInstanceDetails(instanceId);
      const summary: InstanceSummary = { ...current.summary, status: "Stopped", active_process_count: 0 };
      mockSuppressedRuntimeWindowInstances.delete(instanceId);
      upsertMockSummary(summary);
      mockInstanceDetailsStore.set(instanceId, {
        ...current,
        summary,
        active_run: null
      });
      mockLivePlayerStore.delete(instanceId);
      if (current.auto_backup_on_stop) {
        const createdAt = Date.now();
        const backup: InstanceBackupResult = {
          backup_id: `auto-stop-${createdAt}`,
          instance_id: instanceId,
          backup_kind: "auto_stop",
          created_at_unix_ms: createdAt,
          backup_path: `D:/LanGame/instances/${instanceId}/backups/auto-stop-${createdAt}`,
          display_name: null,
          saves_path: current.saves_path,
          file_count: 4,
          total_bytes: 786432
        };
        upsertMockBackup(instanceId, backup);
        const retentionCount = Math.max(1, current.backup_retention_count);
        const retained = ensureMockInstanceBackups(instanceId).slice(0, retentionCount);
        mockInstanceBackupsStore.set(instanceId, clone(retained));
      }
      const log = ensureMockLogDocument(instanceId, 200);
      log.lines = [...log.lines, `[info] ${summary.name} stop command accepted`];
      log.total_lines = log.lines.length;
      mockLogDocumentStore.set(instanceId, clone(log));
      return undefined as T;
    }
    case "send_instance_runtime_command":
    case "send_instance_gm_command": {
      const input = (args?.input ?? {}) as Record<string, unknown>;
      const instanceId = String(input.instanceId ?? "srv-dst-1");
      const details = ensureMockInstanceDetails(instanceId);
      const moduleDetails = buildMockModuleDetails(details.summary.module_id);
      const runtimeActionId = String(input.runtimeActionId ?? "").trim();
      const structuredActionIds = new Set([
        moduleDetails.runtime.player_list?.action_id,
        ...(moduleDetails.runtime.player_list?.player_action_ids ?? [])
      ].filter((actionId): actionId is string => Boolean(actionId)));
      if (runtimeActionId && structuredActionIds.has(runtimeActionId)) {
        throw new Error("Structured live-player actions must use the live-player snapshot service.");
      }
      const declaredAction = resolveMockDeclaredRuntimeAction(
        moduleDetails.runtime.player_actions ?? [],
        details.summary.module_id,
        input
      );
      const processKey = declaredAction?.processKey
        ?? (String(input.processKey ?? (isArkModule(details.summary.module_id) ? "main" : "master")).trim() || (isArkModule(details.summary.module_id) ? "main" : "master"));
      const transport = declaredAction?.transport
        ?? (String(input.transport ?? "stdin").trim().toLowerCase() || "stdin");
      if (transport === "palworld_rest" && !declaredAction) {
        throw new Error("Palworld REST requests must use a declared runtime action.");
      }
      if (details.summary.module_id === "palworld" && ["source_rcon", "humanitz_rcon"].includes(transport)) {
        throw new Error("Palworld management uses the authenticated REST API.");
      }
      const remoteTransport = mockRuntimeTransportIsRemote(transport);
      const commandText = declaredAction?.command ?? String(input.command ?? "").trim();
      const arkTarget = mockArkCommandTarget(details, processKey, transport);
      const log = ensureMockLogDocument(instanceId, 200);
      const nextLines = [`[${mockRuntimeTransportLabel(transport)}:${processKey}] ${commandText}`];
      if (String(details.summary.module_id).toLowerCase() === "necesse") {
        nextLines.push(...buildMockNecesseCommandLines(details, commandText));
      }
      if (arkTarget && processKey !== "main") {
        for (const line of nextLines) mockArkMapLogs.append(details, arkTarget, line);
      } else {
        log.lines = [...log.lines, ...nextLines];
        log.total_lines = log.lines.length;
        mockLogDocumentStore.set(instanceId, clone(log));
      }
      const result: InstanceRuntimeCommandResult = {
        write_confirmation_pending: false,
        instance_id: instanceId,
        process_key: arkTarget ? processKey : remoteTransport ? transport : processKey,
        display_name: arkTarget?.display_name ?? mockRuntimeDisplayName(transport, processKey),
        pid: remoteTransport ? 0 : processKey === "caves" ? 4322 : 4321,
        command: commandText,
        response_text: remoteTransport
          ? `Mock ${mockRuntimeTransportLabel(transport)} response for: ${commandText}`
          : null,
        submitted_at_unix_ms: Date.now()
      };
      return result as T;
    }
    case "read_instance_broadcast_policy": {
      const instanceId = String(args?.instanceId ?? args?.instance_id ?? "srv-dst-1");
      return ensureMockBroadcastPolicy(instanceId) as T;
    }
    case "update_instance_broadcast_policy": {
      const input = args?.input as UpdateInstanceBroadcastPolicyInput;
      return upsertMockBroadcastPolicy(input) as T;
    }
    case "list_instance_broadcast_events": {
      const instanceId = String(args?.instanceId ?? args?.instance_id ?? "srv-dst-1");
      const limit = Math.max(1, Number(args?.limit ?? 50) || 50);
      return listMockBroadcastEvents(instanceId, limit) as T;
    }
    case "generate_instance_broadcast": {
      const rawInput = (args?.input ?? {}) as GenerateInstanceBroadcastInput & Record<string, unknown>;
      const input: GenerateInstanceBroadcastInput = {
        ...rawInput,
        instanceId: String(rawInput.instanceId ?? rawInput.instance_id ?? "srv-dst-1")
      };
      const details = ensureMockInstanceDetails(input.instanceId);
      const source = String(input.source ?? "manual");
      const message = buildMockBroadcastMessage(input, details).slice(0, 240);
      const event = pushMockBroadcastEvent({
        instance_id: details.summary.id,
        module_id: details.summary.module_id,
        source,
        rule_id: typeof input.ruleId === "string" ? input.ruleId : typeof rawInput.rule_id === "string" ? rawInput.rule_id : null,
        message,
        ai_provider: input.settings.provider,
        ai_model: input.settings.model,
        action_id: null,
        transport: null,
        command_preview: null,
        status: "generated",
        response_text: null,
        error_message: null,
        initiator: typeof input.initiator === "string" ? input.initiator : source === "manual" ? "manual" : "auto",
        policy_snapshot_json: typeof input.policySnapshotJson === "string"
          ? input.policySnapshotJson
          : typeof rawInput.policy_snapshot_json === "string"
            ? rawInput.policy_snapshot_json
            : null
      });
      return {
        message,
        provider: input.settings.provider,
        model: input.settings.model,
        endpointUrl: normalizeAssistantEndpoint(input.settings.baseUrl),
        event
      } as GenerateInstanceBroadcastOutput as T;
    }
    case "send_instance_broadcast": {
      const rawInput = (args?.input ?? {}) as SendInstanceBroadcastInput & Record<string, unknown>;
      const input: SendInstanceBroadcastInput = {
        ...rawInput,
        instanceId: String(rawInput.instanceId ?? rawInput.instance_id ?? "srv-dst-1")
      };
      const details = ensureMockInstanceDetails(input.instanceId);
      if (String(details.summary.status).toLowerCase() !== "running") {
        throw new Error("Start the instance before sending a broadcast.");
      }
      const module = buildMockModuleDetails(details.summary.module_id);
      const action = (module.runtime.player_actions ?? []).find((item) => item.kind === "broadcast");
      if (!action) {
        throw new Error(`Module ${module.summary.id} does not declare a broadcast action.`);
      }
      const messageText = String(input.message ?? "").trim();
      const commandText = renderMockBroadcastCommand(action, messageText, module.summary.id);
      const log = ensureMockLogDocument(input.instanceId, 200);
      log.lines = [...log.lines, `[${mockRuntimeTransportLabel(String(action.transport ?? "stdin"))}:broadcast] ${commandText}`];
      log.total_lines = log.lines.length;
      mockLogDocumentStore.set(input.instanceId, clone(log));
      const event = pushMockBroadcastEvent({
        instance_id: details.summary.id,
        module_id: details.summary.module_id,
        source: String(input.source ?? "manual"),
        rule_id: typeof input.ruleId === "string" ? input.ruleId : typeof rawInput.rule_id === "string" ? rawInput.rule_id : null,
        message: messageText,
        ai_provider: input.aiProvider ?? (typeof rawInput.ai_provider === "string" ? rawInput.ai_provider : null),
        ai_model: input.aiModel ?? (typeof rawInput.ai_model === "string" ? rawInput.ai_model : null),
        action_id: action.id,
        transport: action.transport ?? "stdin",
        command_preview: commandText,
        status: "sent",
        response_text: `Mock ${mockRuntimeTransportLabel(String(action.transport ?? "stdin"))} accepted broadcast.`,
        error_message: null,
        initiator: typeof input.initiator === "string" ? input.initiator : input.source === "manual" ? "manual" : "auto",
        policy_snapshot_json: typeof input.policySnapshotJson === "string"
          ? input.policySnapshotJson
          : typeof rawInput.policy_snapshot_json === "string"
            ? rawInput.policy_snapshot_json
            : null
      });
      return {
        event,
        actionId: action.id,
        transport: action.transport ?? "stdin",
        commandPreview: commandText
      } as SendInstanceBroadcastOutput as T;
    }
    case "lookup_steam_workshop_items":
      return lookupMockWorkshopItems(Array.isArray(args?.ids) ? args.ids as string[] : []) as T;
    case "read_steam_workshop_item_details":
      return lookupMockWorkshopItems([String(args?.id ?? "")])[0] as T;
    case "search_steam_workshop_items": {
      const appId = Number(args?.appId ?? args?.app_id ?? 0);
      const query = String(args?.query ?? "");
      const sort = String(args?.sort ?? "trend");
      const page = Number(args?.page ?? 1) || 1;
      const browseKind = args?.browseKind ?? args?.browse_kind ?? "item";
      if (browseKind !== "item" && browseKind !== "collection") {
        throw new Error("Steam Workshop browse kind must be item or collection.");
      }
      return searchMockWorkshopItems(appId, query, sort, page, browseKind) as T;
    }
    case "fetch_steam_news_for_app": {
      const appId = Number(args?.appId ?? args?.app_id ?? 0);
      const count = Math.max(1, Number(args?.count ?? 3) || 3);
      return clone((mockSteamNewsCatalog[appId] ?? []).slice(0, count)) as T;
    }
    case "fetch_steam_store_about": {
      const appId = Number(args?.appId ?? args?.app_id ?? 0);
      return (appId === 251570 ? sampleSteamAboutHtml : null) as T;
    }
    case "fetch_steam_review_summary": {
      const appId = Number(args?.appId ?? args?.app_id ?? 0);
      return localizeMockSteamReviewSummary(mockSteamReviewSummaryCatalog[appId], args?.locale) as T;
    }
    case "open_external_url": {
      const url = String(args?.url ?? "").trim();
      if (url && typeof window !== "undefined") {
        window.open(url, "_blank", "noopener,noreferrer");
      }
      return undefined as T;
    }
    case "open_local_path":
      return undefined as T;
    case "assistant_secret_status": {
      const descriptor = args?.descriptor as AssistantSecretDescriptor;
      return { stored: mockAssistantSecretStore.has(assistantSecretKey(descriptor)) } as T;
    }
    case "assistant_store_secret": {
      const descriptor = args?.descriptor as AssistantSecretDescriptor;
      const apiKey = String(args?.apiKey ?? args?.api_key ?? "").trim();
      const key = assistantSecretKey(descriptor);
      if (apiKey) {
        mockAssistantSecretStore.set(key, apiKey);
      }
      return { stored: mockAssistantSecretStore.has(key) } as T;
    }
    case "assistant_clear_secret": {
      const descriptor = args?.descriptor as AssistantSecretDescriptor;
      mockAssistantSecretStore.delete(assistantSecretKey(descriptor));
      return { stored: false } as T;
    }
    case "assistant_list_ollama_models":
      return ["qwen3:4b", "qwen3.5:8b", "llama3.1:8b"] as T;
    case "assistant_get_conversation_state": case "assistant_list_conversations": {
      const input = args?.input as { conversationId: string; settings: AssistantProviderSettingsInput };
      return (command === "assistant_list_conversations" ? listMockAssistantConversations(input.settings) : getMockAssistantConversationState(input.conversationId, input.settings)) as T;
    }
    case "assistant_resume_conversation": {
      const input = args?.input as { conversationId: string; settings: AssistantProviderSettingsInput };
      return resumeMockAssistantConversation(input.conversationId, input.settings) as T;
    }
    case "assistant_create_conversation":
      return createMockAssistantConversation((args?.input as { settings: AssistantProviderSettingsInput }).settings) as T;
    case "assistant_cancel_turn":
      return cancelMockAssistantTurn((args?.input as { conversationId: string }).conversationId) as T;
    case "assistant_delete_conversation":
      return deleteMockAssistantConversation((args?.input as { conversationId: string }).conversationId) as T;
    case "assistant_execute_operation":
      return previewMockAssistantOperation(args?.input as AssistantExecuteOperationInput) as T;
    case "assistant_confirm_operation":
      return await confirmMockAssistantOperation(args?.input as AssistantConfirmOperationInput, invokeMock) as T;
    case "assistant_run": {
      const input = args?.input as AssistantRunInput;
      return {
        provider: input.settings.provider,
        model: input.settings.model,
        endpointUrl: normalizeAssistantEndpoint(input.settings.baseUrl),
        content: buildMockAssistantResponse(input)
      } as T;
    }
    case "log_frontend_event":
      return undefined as T;
    default:
      throw new Error(`Mock command ${command} is unsupported.`);
  }
}
