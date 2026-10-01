import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import { readPreferredLocale } from "./locale-preference";
import { invokeOrMock, shouldUseLanApi } from "./api-transport";
import { storageManagementRequests } from "./storage-management-requests";
import type { ArkClusterReport, ArkClusterOperationResult, OperateArkClusterInput } from "./ark-clusters";

import type { ArkClusterBackupSummary, ArkClusterRestoreResult, ArkClusterRecoveryResult, PendingArkClusterRestore, CreateArkClusterBackupInput, RestoreArkClusterBackupInput } from "./ark-cluster-backups";
import type {
  AssistantConfirmOperationInput,
  AssistantConversationCreated,
  AssistantConversationState,
  AssistantSavedConversation,
  AssistantConversationControlResult,
  AssistantProviderSettingsInput,
  AssistantExecuteOperationInput,
  AssistantExecuteOperationOutput,
  AssistantRunInput,
  AssistantRunOutput,
  AppPathSettingsInput,
  AppSettings,
  AppUpdateCheckResult,
  AppUpdateInstallEvent,
  AssistantSecretDescriptor,
  AssistantSecretStatus,
  BackgroundJob,
  BootstrapResponse,
  CreateInstanceInput,
  InstanceBackupRestoreResult,
  InstanceBackupResult,
  GenerateInstanceBroadcastInput,
  GenerateInstanceBroadcastOutput,
  InstanceDeletionResult,
  InstanceArchiveResult,
  InstanceBroadcastEvent,
  InstanceBroadcastPolicy,
  InstanceDetails,
  InstanceConnectionInfo,
  SaveInstanceSettingsOptions,
  InstanceIsolationReport,
  InstancePlayerAccessMutationInput,
  InstancePlayerAccessMutationResult,
  InstanceRuntimeCommandResult,
  InstanceProvisioning,
  InstanceRuntimeOverview,
  RuntimeCommandDispatchOptions,
  RuntimeWindowSnapshot,
  RuntimeWindowSuppressionResult,
  SendInstanceBroadcastInput,
  SendInstanceBroadcastOutput,
  InstanceSummary,
  LaunchPlan,
  LogTailSnapshot,
  RuntimeLogSource,
  ManualModInventoryResult,
  ManualModReferenceResolveResult,
  ModuleDetails,
  ModuleInstallResult,
  ModuleUninstallResult,
  ModuleSummary,
  ProjectZomboidWorkshopModsSnapshot,
  OverlayFamily,
  BindAddressCandidate,
  DstModConfigurationSpec,
  ManualModStageResult,
  SteamCmdStatus,
  SteamCmdPrepareSnapshot,
  SteamNewsItem,
  SteamReviewSummary,
  SteamWorkshopDownloadResult,
  SteamWorkshopInstallationSnapshot,
  SteamWorkshopLookupItem,
  SteamWorkshopBrowseKind,
  SteamWorkshopSearchResult,
  StorageStatus,
  DstWorldImportResult,
  DstWorldStartPreview,
  ExecuteInstanceManualPlayerActionInput,
  ExecuteInstancePlayerActionInput,
  ExecuteInstancePlayerActionResult,
  UpdateInstanceBroadcastPolicyInput,
  UpdateInstanceInput,
  RuntimeLivePlayerSnapshot
} from "./types";

export const readArkCluster = (instanceId: string) => invokeOrMock<ArkClusterReport>("read_ark_cluster", { input: { instance_id: instanceId } });
export const operateArkCluster = (input: OperateArkClusterInput) => invokeOrMock<ArkClusterOperationResult>("operate_ark_cluster", { input });
export const listArkClusterBackups = (instanceId: string) => invokeOrMock<ArkClusterBackupSummary[]>("list_ark_cluster_backups", { input: { instance_id: instanceId } });
export const createArkClusterBackup = (input: CreateArkClusterBackupInput) => invokeOrMock<ArkClusterBackupSummary>("create_ark_cluster_backup", { input });
export const restoreArkClusterBackup = (input: RestoreArkClusterBackupInput) => invokeOrMock<ArkClusterRestoreResult>("restore_ark_cluster_backup", { input });
export const readPendingArkClusterRestore = (instanceId: string) => invokeOrMock<PendingArkClusterRestore | null>("read_pending_ark_cluster_restore", { input: { instance_id: instanceId } });
export const recoverArkClusterRestore = (input: RestoreArkClusterBackupInput) => invokeOrMock<ArkClusterRecoveryResult>("recover_ark_cluster_restore", { input });

export const bootstrapApp = async (options: { includeSystemSnapshot?: boolean } = {}) => {
  const includeSystemSnapshot = options.includeSystemSnapshot ?? false;
  const latest = await invokeOrMock<BootstrapResponse>("bootstrap", {
    includeSystemSnapshot,
    include_system_snapshot: includeSystemSnapshot
  });
  if (includeSystemSnapshot && latest.state.snapshot.telemetry == null) {
    // A new backend always includes metadata, even before its first observation.
    throw new Error(JSON.stringify({
      code: "system_telemetry_contract_missing",
      message: "System telemetry metadata is missing."
    }));
  }
  return latest;
};
export const fetchAppVersion = () => invokeOrMock<string>("app_version");
export const checkAppUpdate = () => invokeOrMock<AppUpdateCheckResult>("check_app_update");
export const installAppUpdate = (onEvent: (event: AppUpdateInstallEvent) => void) => {
  if (isTauri()) {
    const onEventChannel = new Channel<AppUpdateInstallEvent>();
    onEventChannel.onmessage = onEvent;
    return invoke<void>("install_app_update", { onEvent: onEventChannel });
  }

  return invokeOrMock<void>("install_app_update", { onEvent });
};
export const readBackgroundJobs = () => invokeOrMock<BackgroundJob[]>("read_background_jobs");
export const fetchOverlayFamilies = () => invokeOrMock<OverlayFamily[]>("overlay_families");
export const fetchBindAddressCandidates = () => invokeOrMock<BindAddressCandidate[]>("bind_address_candidates");
export const ensureStorageReady = () => invokeOrMock<StorageStatus>("ensure_storage_ready");
export const probeSteamCmdStatus = () => invokeOrMock<SteamCmdStatus>("probe_steamcmd_status");
export const ensureSteamCmdReady = (operationId: string) => invokeOrMock<SteamCmdStatus>("ensure_steamcmd_ready", { operationId });
export const readSteamCmdPrepareProgress = (operationId: string | null) => invokeOrMock<SteamCmdPrepareSnapshot | null>("read_steamcmd_prepare_progress", { operationId });
export const cancelSteamCmdPreparation = (operationId: string) => invokeOrMock<SteamCmdPrepareSnapshot>("cancel_steamcmd_preparation", { operationId });
export const cancelInstallationJob = (jobId: string) => invokeOrMock<BackgroundJob>("cancel_installation_job", { jobId });
export const uninstallSteamCmd = () => invokeOrMock<SteamCmdStatus>("uninstall_steamcmd");
export const updateAppSettings = (input: AppPathSettingsInput) => invokeOrMock<AppSettings>("update_app_settings", { input });
export const pickDirectoryPath = (currentPath?: string | null) => {
  if (!isTauri() && shouldUseLanApi()) {
    return Promise.resolve(currentPath ?? null);
  }

  return invokeOrMock<string | null>("pick_directory_path", { currentPath, current_path: currentPath });
};
export const refreshModules = async (options: { includePreservedProgramCounts?: boolean } = {}) => {
  const includePreservedProgramCounts = options.includePreservedProgramCounts ?? true;
  const read = () => invokeOrMock<ModuleSummary[]>("refresh_modules", {
    includePreservedProgramCounts, include_preserved_program_counts: includePreservedProgramCounts
  });
  const modules = await (includePreservedProgramCounts ? storageManagementRequests.inspect(read) : read());
  if (includePreservedProgramCounts) return modules;
  // A filesystem status scan does not measure archive inventory. Keep its
  // omitted counts distinct from actual zeroes so the UI preserves known counts.
  return modules.map(({ instance_program_count: _instances, archived_program_count: _archives, ...summary }) => summary);
};
// Full catalog snapshots retain the archive inventory queue.
export const syncModulesToStorage = () => storageManagementRequests.inspect(() => invokeOrMock<ModuleSummary[]>("sync_modules_to_storage"));
export const listInstancesFromStorage = () => invokeOrMock<InstanceSummary[]>("list_instances_from_storage");
export const readInstanceConnectionInfo = (instanceIds: string[]) =>
  invokeOrMock<InstanceConnectionInfo[]>("read_instance_connection_info_from_storage", { instanceIds, instance_ids: instanceIds });
export const readModuleDetails = async (moduleId: string, options: { includePreservedProgramCounts?: boolean } = {}) => {
  const includePreservedProgramCounts = options.includePreservedProgramCounts ?? true;
  const read = () => invokeOrMock<ModuleDetails>("read_module_details", {
    moduleId, module_id: moduleId,
    includePreservedProgramCounts, include_preserved_program_counts: includePreservedProgramCounts
  });
  const details = await (includePreservedProgramCounts ? storageManagementRequests.inspect(read) : read());
  if (includePreservedProgramCounts) return details;
  // Configuration reads do not acquire the archive inventory lock. Omit counts
  // that were not requested rather than presenting their defaults as measured zeroes.
  const { instance_program_count: _instances, archived_program_count: _archives, ...summary } = details.summary;
  return { ...details, summary };
};
export const readModuleConfigurationIcons = (moduleId: string) =>
  invokeOrMock<Record<string, string>>("read_module_configuration_icons", { moduleId });
export const readInstanceDetails = (instanceId: string) => invokeOrMock<InstanceDetails>("read_instance_details_from_storage", { instanceId, instance_id: instanceId });
export const readInstanceIsolation = (instanceId: string) =>
  invokeOrMock<InstanceIsolationReport>("read_instance_isolation", { input: { instance_id: instanceId } });
export const readInstanceRuntime = (instanceId: string) => invokeOrMock<InstanceRuntimeOverview>("read_instance_runtime_overview_from_storage", { instanceId, instance_id: instanceId });
export const readInstanceRuntimeWindowSnapshot = (instanceId: string) =>
  invokeOrMock<RuntimeWindowSnapshot>("read_instance_runtime_window_snapshot", {
    instanceId,
    instance_id: instanceId
  });
export const suppressInstanceRuntimeWindows = (instanceId: string) =>
  invokeOrMock<RuntimeWindowSuppressionResult>("suppress_instance_runtime_windows", {
    instanceId,
    instance_id: instanceId
  });
export const readInstanceLogDocument = (instanceId: string, maxLines = 200, runId?: number, source?: RuntimeLogSource) => invokeOrMock<LogTailSnapshot>("read_instance_log_document_from_storage", { instanceId, maxLines, runId, source });
export const previewInstanceLaunch = (instanceId: string) => invokeOrMock<LaunchPlan>("preview_instance_launch", { instanceId, instance_id: instanceId });
export const installModule = (moduleId: string, validate = false) => invokeOrMock<ModuleInstallResult>(validate ? "validate_module_game" : "install_module_game", { moduleId, module_id: moduleId });
export const uninstallModule = (moduleId: string) => invokeOrMock<ModuleUninstallResult>("uninstall_module_game", { moduleId, module_id: moduleId });
export const updateInstanceProgram = (instanceId: string, validate: boolean) =>
  invokeOrMock<ModuleInstallResult>("update_instance_program", { instanceId, validate });
export const downloadSteamWorkshopItems = (instanceId: string, ids: string[], missingOnly = false) =>
  invokeOrMock<SteamWorkshopDownloadResult>("download_steam_workshop_items", {
    instanceId,
    instance_id: instanceId,
    ids,
    missingOnly,
    missing_only: missingOnly,
    locale: readPreferredLocale()
  });
export const stageManualModFiles = (instanceId: string, sourcePaths: string[]) =>
  invokeOrMock<ManualModStageResult>("stage_manual_mod_files", {
    instanceId,
    instance_id: instanceId,
    sourcePaths,
    source_paths: sourcePaths
  });
export const installManualModReferences = (instanceId: string, references: string[]) =>
  invokeOrMock<ManualModStageResult>("install_manual_mod_references", {
    instanceId,
    instance_id: instanceId,
    references
  });
export const readManualModInventory = (instanceId: string) =>
  invokeOrMock<ManualModInventoryResult>("read_manual_mod_inventory", { instanceId, instance_id: instanceId });
export const resolveManualModReferences = (instanceId: string, references: string[]) =>
  invokeOrMock<ManualModReferenceResolveResult>("resolve_manual_mod_references", {
    instanceId,
    instance_id: instanceId,
    references
  });
export const createInstance = (input: CreateInstanceInput) =>
  invokeOrMock<InstanceProvisioning>("create_instance_record", {
    input: { name: input.name, module_id: input.module_id }, programMode: input.program_mode
  });
export const deleteInstance = (instanceId: string) =>
  storageManagementRequests.mutate(() => invokeOrMock<InstanceDeletionResult>("delete_instance_record", { instanceId, instance_id: instanceId }));
export const archiveInstance = (instanceId: string) => {
  const dispatch = () => !isTauri() && shouldUseLanApi()
    ? Promise.reject(new Error("Storage management is available only on the desktop host."))
    : invokeOrMock<InstanceArchiveResult>("archive_instance_record", { instanceId, instance_id: instanceId });
  return !isTauri() && shouldUseLanApi() ? dispatch() : storageManagementRequests.mutate(dispatch);
};
export const createInstanceBackup = (instanceId: string) => invokeOrMock<InstanceBackupResult>("create_instance_backup", { instanceId, instance_id: instanceId });
export const listInstanceBackups = (instanceId: string) => invokeOrMock<InstanceBackupResult[]>("list_instance_backups", { instanceId, instance_id: instanceId });
export const renameInstanceBackup = (instanceId: string, backupId: string, displayName: string | null) =>
  invokeOrMock<InstanceBackupResult>("rename_instance_backup", {
    instanceId,
    instance_id: instanceId,
    backupId,
    backup_id: backupId,
    displayName,
    display_name: displayName
  });
export const deleteInstanceBackup = (instanceId: string, backupId: string) =>
  invokeOrMock<InstanceBackupResult>("delete_instance_backup", {
    instanceId,
    instance_id: instanceId,
    backupId,
    backup_id: backupId
  });
export const restoreInstanceBackup = (instanceId: string, backupId: string, locale: string = readPreferredLocale()) =>
  invokeOrMock<InstanceBackupRestoreResult>("restore_instance_backup", {
    instanceId,
    instance_id: instanceId,
    backupId,
    backup_id: backupId,
    locale
  });
export const previewDontStarveWorldStart = (instanceId: string) =>
  invokeOrMock<DstWorldStartPreview>("preview_dontstarve_world_start", {
    instanceId,
    instance_id: instanceId
  });
export const importDontStarveWorldData = (instanceId: string, sourcePath: string, locale: string = readPreferredLocale()) =>
  invokeOrMock<DstWorldImportResult>("import_dontstarve_world_data", {
    instanceId,
    instance_id: instanceId,
    sourcePath,
    source_path: sourcePath,
    locale
  });
export const updateInstance = (input: UpdateInstanceInput, expectedSettingsJson: string, collectionRemoval?: SaveInstanceSettingsOptions["collectionRemoval"]) =>
  collectionRemoval
    ? invokeOrMock<InstanceDetails>("remove_instance_workshop_collection", { input, expectedSettingsJson, ...collectionRemoval })
    : invokeOrMock<InstanceDetails>("update_instance_record_if_current", { input, expectedSettingsJson });
export const updateInstanceAutostart = (instanceId: string, autostart: boolean) =>
  invokeOrMock<InstanceDetails>("update_instance_autostart", { instanceId, autostart });
export const applyInstancePlayerAccessMutation = (input: InstancePlayerAccessMutationInput) =>
  invokeOrMock<InstancePlayerAccessMutationResult>("apply_instance_player_access_mutation", { input });
export const lookupSteamWorkshopItems = async (ids: string[], locale: string = readPreferredLocale()): Promise<SteamWorkshopLookupItem[]> => {
  const uniqueIds = Array.from(new Set(ids.map((id) => id.trim()).filter(Boolean)));
  const items: SteamWorkshopLookupItem[] = [];
  // The public lookup command accepts at most 64 IDs. Keep large local libraries bounded.
  for (let offset = 0; offset < uniqueIds.length; offset += 64) {
    items.push(...await invokeOrMock<SteamWorkshopLookupItem[]>("lookup_steam_workshop_items", { ids: uniqueIds.slice(offset, offset + 64), locale }));
  }
  return items;
};
export const readSteamWorkshopItemDetails = (id: string, locale: string = readPreferredLocale()) =>
  invokeOrMock<SteamWorkshopLookupItem>("read_steam_workshop_item_details", { id, locale });
export const searchSteamWorkshopItems = (appId: number, query = "", sort = "trend", page = 1, locale: string = readPreferredLocale(), browseKind: SteamWorkshopBrowseKind = "item") =>
  invokeOrMock<SteamWorkshopSearchResult>("search_steam_workshop_items", {
    appId,
    app_id: appId,
    query,
    sort,
    page,
    locale,
    browseKind,
    browse_kind: browseKind
  });
export const readSteamWorkshopInstallationStatus = (instanceId: string, ids: string[]) =>
  invokeOrMock<SteamWorkshopInstallationSnapshot>("read_steam_workshop_installation_status", {
    instanceId,
    instance_id: instanceId,
    ids
  });
export const readDontStarveModConfigurationSpecs = (instanceId: string, ids: string[], locale?: string | null) =>
  invokeOrMock<DstModConfigurationSpec[]>("read_dontstarve_mod_configuration_specs", {
    instanceId,
    instance_id: instanceId,
    ids,
    locale
  });
export const readProjectZomboidWorkshopModsSnapshot = (instanceId: string, ids: string[]) =>
  invokeOrMock<ProjectZomboidWorkshopModsSnapshot>("read_project_zomboid_workshop_mods_snapshot", {
    instanceId,
    instance_id: instanceId,
    ids
  });
export const fetchSteamNewsForApp = (appId: number, count = 3, locale: string = readPreferredLocale()) =>
  invokeOrMock<SteamNewsItem[]>("fetch_steam_news_for_app", { appId, app_id: appId, count, locale });
export const registerMediaCacheSource = (url: string, kind: "image" | "video" | "hls", locale: string): Promise<string | null> => {
  if (!isTauri() && !shouldUseLanApi()) return Promise.resolve(null);
  return invokeOrMock<string | null>("register_media_cache_source", { url, kind, locale });
};
export const fetchSteamStoreAbout = (appId: number, locale: string) =>
  invokeOrMock<string | null>("fetch_steam_store_about", { appId, app_id: appId, locale });
export const fetchSteamReviewSummary = (appId: number, locale: string) =>
  invokeOrMock<SteamReviewSummary | null>("fetch_steam_review_summary", { appId, app_id: appId, locale });
export const openExternalUrl = async (url: string) => {
  let target: URL;
  try {
    const trimmed = url.trim();
    if (/[\u0000-\u001f\u007f-\u009f\\]/.test(url) || !/^https?:\/\/[^/]/i.test(trimmed) || trimmed.length > 8192) {
      throw new Error("Invalid external URL.");
    }
    target = new URL(trimmed);
  } catch {
    throw new Error("Only absolute http(s) URLs are allowed.");
  }
  if ((target.protocol !== "http:" && target.protocol !== "https:") || target.username || target.password || target.href.length > 8192) {
    throw new Error("Only absolute http(s) URLs are allowed.");
  }

  if (!isTauri()) {
    window.open(target.href, "_blank", "noopener,noreferrer");
    return;
  }

  await invokeOrMock<void>("open_external_url", { url: target.href });
};
export const openLocalPath = (path: string) => invokeOrMock<void>("open_local_path", { path });
export const startInstance = async (instanceId: string, expectedWorldStart: DstWorldStartPreview | null = null) => {
  await invokeOrMock("start_instance_process", { instanceId, instance_id: instanceId, expectedWorldStart });
};
export const stopInstance = async (instanceId: string) => { await invokeOrMock("stop_instance_process", { instanceId, instance_id: instanceId }); };
export const readInstanceLivePlayers = (instanceId: string) =>
  invokeOrMock<RuntimeLivePlayerSnapshot>("read_instance_live_players", {
    instanceId,
    instance_id: instanceId
  });
export const refreshInstanceLivePlayers = (instanceId: string) =>
  invokeOrMock<RuntimeLivePlayerSnapshot>("refresh_instance_live_players", {
    instanceId,
    instance_id: instanceId
  });
export const executeInstancePlayerAction = (input: ExecuteInstancePlayerActionInput) =>
  invokeOrMock<ExecuteInstancePlayerActionResult>("execute_instance_player_action", { input });
export const executeDeclaredRuntimePlayerAction = (
  instanceId: string,
  actionId: string,
  target: string,
  role: string
) =>
  invokeOrMock<ExecuteInstancePlayerActionResult>("execute_instance_manual_player_action", {
    input: {
      instance_id: instanceId,
      action_id: actionId,
      target,
      role: role || null
    } satisfies ExecuteInstanceManualPlayerActionInput
  });
export const sendInstanceRuntimeCommand = (
  instanceId: string,
  command: string,
  processKey?: string | null,
  options: RuntimeCommandDispatchOptions = {}
) =>
  invokeOrMock<InstanceRuntimeCommandResult>("send_instance_runtime_command", {
    input: {
      instanceId,
      command,
      processKey,
      transport: options.transport,
      portName: options.portName,
      passwordSettingKey: options.passwordSettingKey,
      enabledSettingKey: options.enabledSettingKey,
      runtimeActionId: options.runtimeActionId,
      runtimeActionTarget: options.runtimeActionTarget,
      runtimeActionRole: options.runtimeActionRole
    }
  });
export const sendInstanceGmCommand = (
  instanceId: string,
  command: string,
  processKey?: string | null,
  options: RuntimeCommandDispatchOptions = {}
) =>
  invokeOrMock<InstanceRuntimeCommandResult>("send_instance_gm_command", {
    input: {
      instanceId,
      command,
      processKey,
      transport: options.transport,
      portName: options.portName,
      passwordSettingKey: options.passwordSettingKey,
      enabledSettingKey: options.enabledSettingKey,
      runtimeActionId: options.runtimeActionId,
      runtimeActionTarget: options.runtimeActionTarget,
      runtimeActionRole: options.runtimeActionRole
    }
  });
export const generateInstanceBroadcast = (input: GenerateInstanceBroadcastInput) =>
  invokeOrMock<GenerateInstanceBroadcastOutput>("generate_instance_broadcast", {
    input: {
      instanceId: input.instanceId,
      instance_id: input.instanceId,
      settings: input.settings,
      intent: input.intent,
      tone: input.tone,
      source: input.source,
      ruleId: input.ruleId,
      rule_id: input.ruleId,
      initiator: input.initiator,
      policySnapshotJson: input.policySnapshotJson,
      policy_snapshot_json: input.policySnapshotJson
    }
  });
export const sendInstanceBroadcast = (input: SendInstanceBroadcastInput) =>
  invokeOrMock<SendInstanceBroadcastOutput>("send_instance_broadcast", {
    input: {
      instanceId: input.instanceId,
      instance_id: input.instanceId,
      message: input.message,
      source: input.source,
      ruleId: input.ruleId,
      rule_id: input.ruleId,
      aiProvider: input.aiProvider,
      ai_provider: input.aiProvider,
      aiModel: input.aiModel,
      ai_model: input.aiModel,
      initiator: input.initiator,
      policySnapshotJson: input.policySnapshotJson,
      policy_snapshot_json: input.policySnapshotJson
    }
  });
export const readInstanceBroadcastPolicy = (instanceId: string) =>
  invokeOrMock<InstanceBroadcastPolicy>("read_instance_broadcast_policy", { instanceId, instance_id: instanceId });
export const updateInstanceBroadcastPolicy = (input: UpdateInstanceBroadcastPolicyInput) =>
  invokeOrMock<InstanceBroadcastPolicy>("update_instance_broadcast_policy", { input });
export const listInstanceBroadcastEvents = (instanceId: string, limit = 50) =>
  invokeOrMock<InstanceBroadcastEvent[]>("list_instance_broadcast_events", {
    instanceId,
    instance_id: instanceId,
    limit
  });
export const readAssistantSecretStatus = (descriptor: AssistantSecretDescriptor) => invokeOrMock<AssistantSecretStatus>("assistant_secret_status", { descriptor });
export const storeAssistantSecret = (descriptor: AssistantSecretDescriptor, apiKey: string) => invokeOrMock<AssistantSecretStatus>("assistant_store_secret", { descriptor, apiKey });
export const clearAssistantSecret = (descriptor: AssistantSecretDescriptor) => invokeOrMock<AssistantSecretStatus>("assistant_clear_secret", { descriptor });
export const listOllamaModels = (baseUrl?: string | null) => invokeOrMock<string[]>("assistant_list_ollama_models", { baseUrl, base_url: baseUrl });
export const runAssistant = (input: AssistantRunInput) => invokeOrMock<AssistantRunOutput>("assistant_run", { input });
export const getAssistantConversationState = (conversationId: string, settings: AssistantProviderSettingsInput, afterCursor?: number) =>
  invokeOrMock<AssistantConversationState>("assistant_get_conversation_state", { input: { conversationId, settings, afterCursor } });
export const listAssistantConversations = (settings: AssistantProviderSettingsInput) =>
  invokeOrMock<AssistantSavedConversation[]>("assistant_list_conversations", { input: { settings } });
export const resumeAssistantConversation = (conversationId: string, settings: AssistantProviderSettingsInput) =>
  invokeOrMock<AssistantExecuteOperationOutput>("assistant_resume_conversation", { input: { conversationId, settings } });
export const createAssistantConversation = (settings: AssistantProviderSettingsInput) =>
  invokeOrMock<AssistantConversationCreated>("assistant_create_conversation", { input: { settings } });
export const cancelAssistantTurn = (conversationId: string) =>
  invokeOrMock<AssistantConversationControlResult>("assistant_cancel_turn", { input: { conversationId } });
export const deleteAssistantConversation = (conversationId: string) =>
  invokeOrMock<AssistantConversationControlResult>("assistant_delete_conversation", { input: { conversationId } });
export const executeAssistantOperation = (input: AssistantExecuteOperationInput) =>
  invokeOrMock<AssistantExecuteOperationOutput>("assistant_execute_operation", {
    input: {
      conversationId: input.conversationId,
      settings: input.settings,
      prompt: input.prompt,
      context: input.context,
      selectedInstanceId: input.selectedInstanceId,
      selectedModuleId: input.selectedModuleId
    }
  });
export const confirmAssistantOperation = (input: AssistantConfirmOperationInput) =>
  invokeOrMock<AssistantExecuteOperationOutput>("assistant_confirm_operation", { input });
export const logFrontendEvent = (
  level: string,
  action: string,
  message: string,
  context?: Record<string, unknown>
) => invokeOrMock<void>("log_frontend_event", { level, action, message, context });
