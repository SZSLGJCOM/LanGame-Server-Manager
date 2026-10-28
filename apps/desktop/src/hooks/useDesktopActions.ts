import { useRef, useState, type Dispatch, type SetStateAction } from "react";
import {
  applyInstancePlayerAccessMutation,
  archiveInstance,
  createInstance,
  createInstanceBackup,
  deleteInstance,
  deleteInstanceBackup,
  ensureStorageReady,
  executeInstancePlayerAction,
  importDontStarveWorldData,
  installModule,
  installAuthenticatedModule,
  listInstanceBackups,
  logFrontendEvent,
  renameInstanceBackup,
  openLocalPath,
  readInstanceDetails,
  restoreInstanceBackup,
  sendInstanceRuntimeCommand,
  startInstance,
  stopInstance,
  suppressInstanceRuntimeWindows,
  uninstallModule,
  updateInstance,
  updateInstanceAutostart
} from "../api";
import { describeError } from "../app-state";
import { message, programCleanupDetails, type UiMessage } from "../app-ui";
import { useSteamCmdActions, type SteamCmdActionOptions } from "./useSteamCmdActions";
import { isInstallationCancelled } from "../installation-cancellation";
import { runSteamInstallationWithAccount, type SteamInstallationAccountContext } from "../steam-installation-account";
import { useI18n } from "../i18n";
import { formatInstallState } from "../install-state-presentation";
import { instancePanelReader, type InstancePanelLoadState, type InstancePanelPatch } from "../instance-panel-loader";
import type { InstallationSnapshot } from "../library-installation-refresh";
import { readModuleNotReadyError, serverStartFailureMessage, serverStartSuccessMessage } from "../server-start-error";
import { useInstanceSettingsSaveCoordinator } from "../views/settings/InstanceSettingsSaveContext";
import type { useInstanceRetirement } from "./useInstanceRetirement";
import type {
  BindAddressCandidate,
  BootstrapResponse,
  CreateInstanceInput,
  DstWorldStartPreview,
  ExecuteInstancePlayerActionInput,
  ExecuteInstancePlayerActionResult,
  InstanceBackupRestoreResult,
  InstanceBackupResult,
  InstanceDetails,
  InstanceProvisioning,
  InstancePlayerAccessMutationInput,
  InstancePlayerAccessMutationResult,
  InstanceRuntimeCommandResult,
  ModuleSummary,
  RuntimeCommandDispatchOptions,
  SaveInstanceSettingsOptions,
  RuntimeWindowSuppressionResult,
  ServerWorkspaceSection,
  StartInstanceResult,
  UpdateInstanceInput,
} from "../types";

type Setter<T> = Dispatch<SetStateAction<T>>;
type SelectionTarget = { moduleId?: string | null; instanceId?: string | null };

interface SharedActionOptions {
  refreshInstallationState: () => Promise<InstallationSnapshot>;
  setActivity: Setter<UiMessage>;
  setBootstrap: Setter<BootstrapResponse>;
  setSelectedModuleId: Setter<string | null>;
  setSelectedInstanceId: Setter<string | null>;
  reloadBootstrap: (preferred?: SelectionTarget, options?: { refreshOverlays?: boolean; reconcileRetirement?: string }) => Promise<BootstrapResponse>;
}

interface LibraryActionOptions extends SharedActionOptions, SteamCmdActionOptions {
  setLibraryTaskPolling: Setter<boolean>;
  refreshSteamCmdStatus: () => Promise<void>;
}

export function useLibraryActions(options: LibraryActionOptions) {
  const { t } = useI18n();
  const steamCmdActions = useSteamCmdActions(options);

  async function handleEnsureStorageOnly() {
    options.setActivity(message("activity.initializingStorage"));
    try {
      const storage = await ensureStorageReady();
      options.setBootstrap((current) => ({ ...current, state: { ...current.state, storage } }));
      options.setActivity(message("activity.storageInitialized", { version: storage.schema_version ?? 0 }));
    } catch (error) {
      options.setActivity(message("activity.initStorageFailed", { message: describeError(error) }));
    }
  }

  async function handleInstallModule(moduleId: string, validate: boolean, account?: SteamInstallationAccountContext) {
    options.setLibraryTaskPolling(true);
    options.setActivity(message(validate ? "activity.preparingValidation" : "activity.preparingInstall", { moduleId }));
    try {
      const result = await runSteamInstallationWithAccount(() => installModule(moduleId, validate),
        (accountName, target) => installAuthenticatedModule(moduleId, validate, accountName, target), account);
      options.setActivity(
        message(validate ? "activity.validationCompleted" : "activity.installCompleted", {
          moduleId,
          state: formatInstallState(result.install_state, t)
        })
      );
    } catch (error) {
      const messageText = describeError(error);
      options.setActivity(
        isInstallationCancelled(error) ? message("activity.installationStopped", { moduleId })
          : message(validate ? "activity.validationFailed" : "activity.installFailed", { moduleId, message: messageText })
      );
    } finally {
      await refreshInstallationAfterMutation();
    }
  }

  async function handleUninstallModule(moduleId: string) {
    options.setLibraryTaskPolling(true);
    options.setActivity(message("activity.preparingUninstall", { moduleId }));
    try {
      const result = await uninstallModule(moduleId);
      const details = programCleanupDetails(result.cleanup, t);
      options.setActivity(
        message(details ? "activity.uninstallCompletedWithRetained" : "activity.uninstallCompleted", {
          moduleId,
          count: result.cleanup.removed_install_roots.length,
          details
        }, result.cleanup.retained_installs.length ? { tone: "warning" } : undefined)
      );
    } catch (error) {
      const messageText = describeError(error);
      options.setActivity(message("activity.uninstallFailed", { moduleId, message: messageText }));
    } finally {
      await refreshInstallationAfterMutation();
    }
  }

  async function refreshInstallationAfterMutation() {
    const failures: string[] = [];
    try {
      await options.reloadBootstrap();
    } catch (error) {
      failures.push(describeError(error));
    }
    try {
      await options.refreshInstallationState();
    } catch (error) {
      failures.push(describeError(error));
    }
    try {
      await options.refreshSteamCmdStatus();
    } catch (error) {
      failures.push(describeError(error));
    } finally {
      options.setLibraryTaskPolling(false);
    }
    if (failures.length) {
      options.setActivity(message("activity.refreshLibraryFailed", { message: failures.join("; ") }));
    }
  }

  return {
    ...steamCmdActions,
    handleEnsureStorageOnly,
    handleInstallModule,
    handleUninstallModule
  };
}

interface InstanceActionOptions extends SharedActionOptions {
  retirement: ReturnType<typeof useInstanceRetirement>;
  bindAddressCandidates: BindAddressCandidate[];
  clearSelectedInstancePanel: () => void;
  getCurrentInstanceId: () => string | null;
  instanceDetailsById: Partial<Record<string, InstanceDetails>>;
  markRuntimeRefreshed: () => void;
  modules: ModuleSummary[];
  openInstanceView: (section: ServerWorkspaceSection, instanceId: string) => void;
  onPanelLoaded: (instanceId: string, payload: InstancePanelPatch) => void;
  onPanelProgress: (state: InstancePanelLoadState) => void;
  onPanelSettled: (state: InstancePanelLoadState) => void;
  selectedInstanceId: string | null;
  setInstanceDetailsById: Setter<Partial<Record<string, InstanceDetails>>>;
  setSelectedInstanceBackups: Setter<InstanceBackupResult[]>;
  setSelectedInstanceDetails: Setter<InstanceDetails | null>;
}

export function useInstanceActions(options: InstanceActionOptions) {
  const settingsSaveCoordinator = useInstanceSettingsSaveCoordinator();
  const { t } = useI18n();
  const creationRequests = useRef(new Map<string, { promise: Promise<void>; startedAt: number }>());
  const [creatingModuleIds, setCreatingModuleIds] = useState<ReadonlySet<string>>(() => new Set());

  function reportOperationRefreshFailure(completed: UiMessage, error: unknown) {
    options.setActivity(message("activity.operationRefreshFailed", {
      message: t(completed.key, completed.params, completed.fallback),
      refreshMessage: describeError(error)
    }, { tone: "warning", runtimeStartNotice: completed.runtimeStartNotice }));
  }

  async function refreshInstancePanelAfterMutation(instanceId: string, refreshBootstrap = true) {
    const controller = new AbortController();
    let panel: InstancePanelPatch = {};
    // A poll started before persistence cannot acknowledge the mutation. Reuse
    // its native slot, then read fresh state through the same bounded reader.
    instancePanelReader.invalidate(instanceId);
    try {
      const state = await instancePanelReader.load(instanceId, {
        signal: controller.signal,
        onUpdate: (patch) => {
          panel = { ...panel, ...patch };
          options.onPanelLoaded(instanceId, patch);
        },
        onProgress: options.onPanelProgress
      });
      options.onPanelSettled(state);
      const errors = Object.values(state.errors).filter((error): error is string => Boolean(error));
      if (refreshBootstrap) {
        try {
          await options.reloadBootstrap();
        } catch (error) {
          errors.push(describeError(error));
        }
      }
      if (errors.length) throw new Error(errors.join("\n"));
      return panel;
    } finally {
      controller.abort();
    }
  }

  function summarizeWindowSuppressionResult(result: RuntimeWindowSuppressionResult) {
    if (result.visible_window_count_before <= 0) {
      return t("activity.runtimeWindowsSuppressNoWindows");
    }

    if (result.remaining_visible_window_count <= 0) {
      return t("activity.runtimeWindowsSuppressSuppressed", { count: result.suppressed_window_count });
    }

    return t("activity.runtimeWindowsSuppressPartial", {
      before: result.visible_window_count_before,
      remaining: result.remaining_visible_window_count
    });
  }

  function updateSelectedBackups(instanceId: string, backups: InstanceBackupResult[]) {
    if (options.getCurrentInstanceId() === instanceId) {
      options.setSelectedInstanceBackups(backups);
    }
  }

  function backupLabel(backup: InstanceBackupResult) {
    const customLabel = String(backup.display_name ?? "").trim();
    if (customLabel) {
      return customLabel;
    }

    const normalizedPath = String(backup.backup_path ?? "").replace(/\\/g, "/").replace(/\/+$/g, "");
    const lastSlashIndex = normalizedPath.lastIndexOf("/");
    if (lastSlashIndex >= 0 && lastSlashIndex < normalizedPath.length - 1) {
      return normalizedPath.slice(lastSlashIndex + 1);
    }

    return backup.backup_id;
  }

  function removeCachedInstance(instanceId: string) {
    options.setInstanceDetailsById((current) => {
      const { [instanceId]: _removed, ...rest } = current;
      return rest;
    });
  }

  async function handleRefreshWorkspace(scopeKey: string) {
    options.setActivity(message("activity.refreshingScope", {}, { scopeKey }));
    try {
      const latest = await options.reloadBootstrap(undefined, { refreshOverlays: true });
      if (scopeKey === "monitor") {
        options.markRuntimeRefreshed();
      }
      options.setActivity(
        message(
          "activity.scopeRefreshed",
          { servers: latest.state.instances.length, modules: latest.state.modules.length },
          { scopeKey }
        )
      );
    } catch (error) {
      options.setActivity(message("activity.refreshScopeFailed", { message: describeError(error) }, { scopeKey }));
    }
  }

  async function handleCreateServer(input: CreateInstanceInput) {
    const existing = creationRequests.current.get(input.module_id);
    if (existing) return existing.promise;
    // Register before starting IPC so repeated submits in the same render and
    // a remounted library panel share the original creation request.
    const creation = Promise.resolve().then(async () => {
      options.setActivity(message("activity.creatingServer", { moduleId: input.module_id, name: input.name }));
      let provisioning: InstanceProvisioning;
      try {
        provisioning = await createInstance(input);
      } catch (error) {
        options.setActivity(isInstallationCancelled(error)
          ? message("activity.serverCreationCancelled")
          : message("activity.createServerFailed", { message: describeError(error) }));
        return;
      }
      const completed = message("activity.serverCreated", { name: provisioning.summary.name });
      try {
        await options.reloadBootstrap({ moduleId: input.module_id, instanceId: provisioning.summary.id });
        options.openInstanceView("settings", provisioning.summary.id);
        options.setActivity(completed);
      } catch (error) {
        reportOperationRefreshFailure(completed, error);
      }
    });
    creationRequests.current.set(input.module_id, { promise: creation, startedAt: Date.now() });
    setCreatingModuleIds(new Set(creationRequests.current.keys()));
    try {
      await creation;
    } finally {
      creationRequests.current.delete(input.module_id);
      setCreatingModuleIds(new Set(creationRequests.current.keys()));
    }
  }

  async function handleStartServer(instanceId: string, expectedWorldStart?: DstWorldStartPreview) {
    options.setActivity(message("activity.startingServer", { id: instanceId }));
    let result: StartInstanceResult;
    try {
      await settingsSaveCoordinator.flush(instanceId);
      result = expectedWorldStart
        ? await startInstance(instanceId, expectedWorldStart) : await startInstance(instanceId);
    } catch (error) {
      const errorMessage = describeError(error);
      options.setActivity({ ...serverStartFailureMessage(error), tone: "error" });
      void logFrontendEvent("error", "instance.start.frontend_failed", errorMessage, { instanceId }).catch(
        () => undefined
      );
      if (readModuleNotReadyError(error)) {
        try {
          await options.refreshInstallationState();
        } catch (refreshError) {
          void logFrontendEvent("warn", "instance.start.installation_refresh_failed", describeError(refreshError), { instanceId }).catch(
            () => undefined
          );
        }
      }
      return false;
    }
    const completed = serverStartSuccessMessage(result, instanceId);
    try {
      await options.reloadBootstrap({ instanceId });
      options.setSelectedInstanceId(instanceId);
      options.markRuntimeRefreshed();
      options.setActivity(completed);
    } catch (error) {
      reportOperationRefreshFailure(completed, error);
    }
    return true;
  }

  async function handleStopServer(instanceId: string) {
    options.setActivity(message("activity.stoppingServer", { id: instanceId }));
    try {
      await stopInstance(instanceId);
    } catch (error) {
      const errorMessage = describeError(error);
      options.setActivity(message("activity.stopServerFailed", { message: errorMessage }, { tone: "error" }));
      void logFrontendEvent("error", "instance.stop.frontend_failed", errorMessage, { instanceId }).catch(
        () => undefined
      );
      return;
    }
    const completed = message("activity.serverStopSent", { id: instanceId });
    try {
      await options.reloadBootstrap({ instanceId });
      options.setSelectedInstanceId(instanceId);
      options.markRuntimeRefreshed();
      options.setActivity(completed);
    } catch (error) {
      reportOperationRefreshFailure(completed, error);
    }
  }

  async function handleRefreshLaunchPreview() {
    const instanceId = options.getCurrentInstanceId();
    if (!instanceId) {
      options.setActivity(message("activity.noInstanceSelected"));
      return;
    }

    options.setActivity(message("activity.generatingLaunchPreview", { id: instanceId }));
    try {
      const snapshot = await options.refreshInstallationState();
      if (snapshot.instanceId !== instanceId || options.getCurrentInstanceId() !== instanceId) return;
      if (snapshot.preview.launchPlan) {
        options.setActivity(message("activity.launchPreviewRefreshed", { name: snapshot.preview.launchPlan.instance_name }));
      } else if (snapshot.preview.launchPlanError) {
        options.setActivity(message("activity.launchPreviewFailed", { message: snapshot.preview.launchPlanError }));
      }
    } catch (error) {
      if (options.getCurrentInstanceId() === instanceId) {
        options.setActivity(message("activity.launchPreviewFailed", { message: describeError(error) }));
      }
    }
  }


  async function handleSaveAutostart(instanceId: string, autostart: boolean) {
    const savedDetails = await updateInstanceAutostart(instanceId, autostart);
    // Merge only the owned field so an overlapping configuration refresh cannot be rolled back.
    options.setSelectedInstanceDetails((current) => current?.summary.id === instanceId
      ? { ...current, summary: { ...current.summary, autostart: savedDetails.summary.autostart } }
      : current);
    options.setInstanceDetailsById((current) => {
      const details = current[instanceId];
      return { ...current, [instanceId]: details
        ? { ...details, summary: { ...details.summary, autostart: savedDetails.summary.autostart } }
        : savedDetails };
    });
    options.setBootstrap((current) => ({ ...current, state: { ...current.state,
      instances: current.state.instances.map((instance) => instance.id === instanceId
        ? { ...instance, autostart: savedDetails.summary.autostart } : instance)
    } }));
  }

  async function handleSaveSettings(input: UpdateInstanceInput, saveOptions: SaveInstanceSettingsOptions = {}) {
    const baselineDetails = options.instanceDetailsById[input.id];

    function settingsSignature(details: InstanceDetails) {
      return JSON.stringify({
        bind_ip: details.summary.bind_ip,
        port_count: details.summary.port_count,
        ports: details.ports,
        settings_json: details.settings_json,
        config_file_path: details.config_file_path,
        saves_path: details.saves_path,
        backup_uses_declared_saves_path: details.backup_uses_declared_saves_path,
        auto_backup_on_stop: details.auto_backup_on_stop,
        backup_retention_count: details.backup_retention_count
      });
    }

    const baselineSignature = baselineDetails ? settingsSignature(baselineDetails) : null;
    if (!saveOptions.silent) {
      options.setActivity(message("activity.savingSettings", { id: input.id }));
    }
    let savedDetails: InstanceDetails;
    try {
      const expectedSettingsJson = saveOptions.expectedSettingsJson;
      if (expectedSettingsJson == null) {
        throw new Error(t("activity.settingsBaselineMissing"));
      }
      savedDetails = await updateInstance(input, expectedSettingsJson, saveOptions.collectionRemoval);
    } catch (error) {
      const messageText = describeError(error);
      options.setActivity(message("activity.saveSettingsFailed", { message: messageText }));
      if (saveOptions.throwOnError) {
        throw error;
      }
      return;
    }

    try {
      const panel = await refreshInstancePanelAfterMutation(input.id);
      if (!saveOptions.silent) {
        options.setActivity(message("activity.settingsSaved", { name: panel.details?.summary.name ?? savedDetails.summary.name }));
      }
    } catch (error) {
      const savedSignature = settingsSignature(savedDetails);
      function mergeSavedSettings(current: InstanceDetails) {
        const currentSignature = settingsSignature(current);
        // A refresh may already have cached a newer configuration before failing later.
        if (currentSignature !== baselineSignature && currentSignature !== savedSignature) return current;
        return {
          ...current,
          summary: { ...current.summary, bind_ip: savedDetails.summary.bind_ip, port_count: savedDetails.summary.port_count },
          ports: savedDetails.ports,
          settings_json: savedDetails.settings_json,
          config_file_path: savedDetails.config_file_path,
          saves_path: savedDetails.saves_path,
          backup_uses_declared_saves_path: savedDetails.backup_uses_declared_saves_path,
          auto_backup_on_stop: savedDetails.auto_backup_on_stop,
          backup_retention_count: savedDetails.backup_retention_count
        };
      }

      options.setInstanceDetailsById((current) => {
        const details = current[input.id];
        if (!details) return current;
        const merged = mergeSavedSettings(details);
        return merged === details ? current : { ...current, [input.id]: merged };
      });
      if (options.getCurrentInstanceId() === input.id) {
        options.setSelectedInstanceDetails((current) => current?.summary.id === input.id
          ? mergeSavedSettings(current)
          : current);
      }
      options.setActivity(message("activity.settingsRefreshFailed", { message: describeError(error) }));
    }
    return savedDetails;
  }

  async function handleApplyPlayerAccessMutation(
    input: InstancePlayerAccessMutationInput
  ): Promise<InstancePlayerAccessMutationResult> {
    options.setActivity(message("activity.savingSettings", { id: input.instanceId }));
    let result: InstancePlayerAccessMutationResult;
    try {
      result = await applyInstancePlayerAccessMutation(input);
    } catch (error) {
      options.setActivity(message("activity.saveSettingsFailed", { message: describeError(error) }));
      throw error;
    }

    try {
      const panel = await refreshInstancePanelAfterMutation(input.instanceId);
      options.setActivity(message("activity.settingsSaved", { name: panel.details?.summary.name ?? input.instanceId }));
    } catch (error) {
      options.setActivity(message("activity.playerAccessRefreshFailed", { message: describeError(error) }));
    }
    return result;
  }

  async function handleExecutePlayerAction(
    input: ExecuteInstancePlayerActionInput
  ): Promise<ExecuteInstancePlayerActionResult> {
    try {
      return await executeInstancePlayerAction(input);
    } finally {
      // A command can be sent before native roster readback fails. Always reload
      // authoritative details, but preserve that original action failure.
      try {
        await refreshInstancePanelAfterMutation(input.instance_id);
      } catch (error) {
        options.setActivity(message("activity.playerAccessRefreshFailed", { message: describeError(error) }));
      }
    }
  }

  async function handleSendRuntimeCommand(
    instanceId: string,
    command: string,
    processKey?: string | null,
    dispatchOptions: RuntimeCommandDispatchOptions = {}
  ): Promise<InstanceRuntimeCommandResult | null> {
    const trimmedCommand = command.trim();
    if (!trimmedCommand) {
      if (!dispatchOptions.silent) {
        options.setActivity(message("activity.runtimeCommandEmpty"));
      }
      return null;
    }

    if (!dispatchOptions.silent) {
      const target = processKey?.trim() || t("activity.runtimeCommandPrimaryTarget");
      options.setActivity(
        message("activity.runtimeCommandStarted", { target })
      );
    }

    let result: InstanceRuntimeCommandResult;
    try {
      result = await sendInstanceRuntimeCommand(instanceId, trimmedCommand, processKey ?? null, dispatchOptions);
    } catch (error) {
      const messageText = describeError(error);
      if (!dispatchOptions.silent) {
        options.setActivity(
          message("activity.runtimeCommandFailed", { message: messageText })
        );
      }
      if (dispatchOptions.throwOnError) {
        throw error;
      }
      return null;
    }

    try {
      await refreshInstancePanelAfterMutation(instanceId, !dispatchOptions.silent);
    } catch (error) {
      if (!dispatchOptions.silent) {
        options.setActivity(message("activity.refreshServersFailed", { message: describeError(error) }, { tone: "error" }));
      }
      // The dispatch receipt remains authoritative even when subsequent view
      // reads fail. Reporting a send failure could prompt a duplicate command.
      return result;
    }
    if (!dispatchOptions.silent) {
      options.setActivity(message("activity.runtimeCommandSent", { target: result.display_name }));
    }
    return result;
  }

  async function handleSuppressRuntimeWindows(instanceId: string) {
    options.setActivity(message("activity.runtimeWindowsSuppressStarted"));

    let result: RuntimeWindowSuppressionResult;
    try {
      result = await suppressInstanceRuntimeWindows(instanceId);
    } catch (error) {
      options.setActivity(
        message("activity.runtimeWindowsSuppressFailed", { message: describeError(error) })
      );
      return;
    }
    const completed = message("activity.runtimeWindowsSuppressSucceeded", { result: summarizeWindowSuppressionResult(result) });
    try {
      await refreshInstancePanelAfterMutation(instanceId);
      options.setActivity(completed);
    } catch (error) {
      reportOperationRefreshFailure(completed, error);
    }
  }

  async function handleCreateBackup(instanceId: string) {
    options.setActivity(message("activity.backingUpSaves", { id: instanceId }));
    let result: InstanceBackupResult;
    try {
      result = await createInstanceBackup(instanceId);
    } catch (error) {
      options.setActivity(message("activity.instanceBackupFailed", { message: describeError(error) }));
      return;
    }
    const completed = (name = options.instanceDetailsById[instanceId]?.summary.name ?? instanceId) =>
      message("activity.instanceBackupCreated", { name, path: result.backup_path, count: result.file_count });
    try {
      const details = options.instanceDetailsById[instanceId] ?? await readInstanceDetails(instanceId);
      options.setInstanceDetailsById((current) => ({ ...current, [instanceId]: details }));
      const backups = await listInstanceBackups(instanceId);
      updateSelectedBackups(instanceId, backups);
      options.setActivity(completed(details.summary.name));
    } catch (error) {
      reportOperationRefreshFailure(completed(), error);
    }
  }

  async function handleRestoreBackup(instanceId: string, backupId: string) {
    options.setActivity(message("activity.restoringBackup", { id: instanceId }));
    let result: InstanceBackupRestoreResult;
    try {
      result = await restoreInstanceBackup(instanceId, backupId);
    } catch (error) {
      options.setActivity(message("activity.instanceBackupRestoreFailed", { message: describeError(error) }));
      return;
    }
    const completed = (name = options.instanceDetailsById[instanceId]?.summary.name ?? instanceId) =>
      message("activity.instanceBackupRestored", { name, backupId: result.backup_id, safeguardId: result.safeguard_backup_id });
    try {
      const details = options.instanceDetailsById[instanceId] ?? await readInstanceDetails(instanceId);
      options.setInstanceDetailsById((current) => ({ ...current, [instanceId]: details }));
      const backups = await listInstanceBackups(instanceId);
      updateSelectedBackups(instanceId, backups);
      options.setActivity(completed(details.summary.name));
    } catch (error) {
      reportOperationRefreshFailure(completed(), error);
    }
  }

  async function handleRenameBackup(instanceId: string, backup: InstanceBackupResult, displayName: string): Promise<boolean> {
    const currentCustomName = String(backup.display_name ?? "").trim();
    const fallbackName = backupLabel(backup);
    const nextDisplayName = displayName.trim();
    if (currentCustomName) {
      if (nextDisplayName === currentCustomName) {
        return true;
      }
    } else if (!nextDisplayName || nextDisplayName === fallbackName) {
      return true;
    }

    options.setActivity(message("activity.renamingBackup", { id: instanceId }));
    let result: InstanceBackupResult;
    try {
      result = await renameInstanceBackup(instanceId, backup.backup_id, nextDisplayName || null);
    } catch (error) {
      options.setActivity(message("activity.instanceBackupRenameFailed", { message: describeError(error) }));
      return false;
    }
    const completed = (name = options.instanceDetailsById[instanceId]?.summary.name ?? instanceId) =>
      message("activity.instanceBackupRenamed", { name, backupName: backupLabel(result) });
    try {
      const details = options.instanceDetailsById[instanceId] ?? await readInstanceDetails(instanceId);
      options.setInstanceDetailsById((current) => ({ ...current, [instanceId]: details }));
      const backups = await listInstanceBackups(instanceId);
      updateSelectedBackups(instanceId, backups);
      options.setActivity(completed(details.summary.name));
    } catch (error) {
      reportOperationRefreshFailure(completed(), error);
    }
    return true;
  }

  async function handleDeleteBackup(instanceId: string, backup: InstanceBackupResult) {
    options.setActivity(message("activity.deletingBackup", { id: instanceId }));
    let result: InstanceBackupResult;
    try {
      result = await deleteInstanceBackup(instanceId, backup.backup_id);
    } catch (error) {
      options.setActivity(message("activity.instanceBackupDeleteFailed", { message: describeError(error) }));
      return;
    }
    const completed = (name = options.instanceDetailsById[instanceId]?.summary.name ?? instanceId) =>
      message("activity.instanceBackupDeleted", { name, backupName: backupLabel(result) });
    try {
      const details = options.instanceDetailsById[instanceId] ?? await readInstanceDetails(instanceId);
      options.setInstanceDetailsById((current) => ({ ...current, [instanceId]: details }));
      const backups = await listInstanceBackups(instanceId);
      updateSelectedBackups(instanceId, backups);
      options.setActivity(completed(details.summary.name));
    } catch (error) {
      reportOperationRefreshFailure(completed(), error);
    }
  }

  async function handleArchiveInstance(instanceId: string) {
    if (!options.retirement.begin(instanceId, "archive")) return;
    options.setActivity(message("activity.archivingInstance", { id: instanceId }));

    try {
      const result = await archiveInstance(instanceId);
      const archivedPath = String(result.archived_instance_root ?? "").trim() || result.previous_instance_root;
      await finishInstanceRemoval(instanceId, result.saves_archived_with_instance_root ? "activity.instanceArchived"
        : result.external_saves_backup_id ? "activity.instanceArchivedWithExternalSnapshot" : "activity.instanceArchivedWithExternalSaves", {
        name: result.instance_name, path: archivedPath,
        savesPath: result.preserved_external_saves_path ?? result.effective_saves_path
      });
    } catch (error) {
      await refreshAfterFailedRemoval(instanceId, "activity.instanceArchiveFailed", error);
    } finally {
      options.retirement.finish(instanceId);
    }
  }

  async function finishInstanceRemoval(instanceId: string, key: string, params: Record<string, string>, tone?: "warning") {
    removeCachedInstance(instanceId);
    options.setBootstrap((current) => ({ ...current, state: {
      ...current.state, instances: current.state.instances.filter((instance) => instance.id !== instanceId)
    } }));
    if (options.getCurrentInstanceId() === instanceId) {
      options.clearSelectedInstancePanel();
      options.setSelectedInstanceId(null);
    }
    try { await options.reloadBootstrap(undefined, { reconcileRetirement: instanceId }); }
    catch (error) {
      options.setActivity(message("activity.instanceRemovalRefreshFailed", { message: t(key, params), refreshMessage: describeError(error) }));
      return;
    }
    options.setActivity(message(key, params, tone ? { tone } : undefined));
  }

  async function refreshAfterFailedRemoval(instanceId: string, key: string, error: unknown) {
    let detail = describeError(error);
    try { await options.reloadBootstrap(undefined, { reconcileRetirement: instanceId }); }
    catch (refreshError) {
      detail = t("activity.instanceRemovalRefreshFailed", { message: detail, refreshMessage: describeError(refreshError) });
    }
    options.setActivity(message(key, { message: detail }));
    throw error;
  }

  async function handleDeleteInstance(instanceId: string) {
    if (!options.retirement.begin(instanceId, "delete")) return;
    options.setActivity(message("activity.deletingInstance", { id: instanceId }));
    try {
      const result = await deleteInstance(instanceId);
      const key = result.preserved_external_saves_path ? "activity.instanceDeletedWithExternalSaves" : "activity.instanceDeleted";
      const params = {
        name: result.instance_name, savesPath: result.preserved_external_saves_path ?? ""
      };
      const details = programCleanupDetails(result.program_cleanup, t);
      await finishInstanceRemoval(instanceId, details ? "activity.completedWithProgramCleanup" : key,
        details ? { message: t(key, params), details } : params,
        result.program_cleanup.retained_installs.length ? "warning" : undefined);
    } catch (error) {
      await refreshAfterFailedRemoval(instanceId, "activity.instanceDeleteFailed", error);
    } finally {
      options.retirement.finish(instanceId);
    }
  }

  async function handleImportDontStarveWorldData(instanceId: string, sourcePath: string) {
    const trimmedSourcePath = sourcePath.trim();
    if (!trimmedSourcePath) {
      const emptyPathError = new Error(t("activity.dstWorldImportMissingSource"));
      options.setActivity(
        message("activity.dstWorldImportFailed", { message: emptyPathError.message })
      );
      throw emptyPathError;
    }

    options.setActivity(
      message("activity.dstWorldImportStarted", { instanceId })
    );

    try {
      const result = await importDontStarveWorldData(instanceId, trimmedSourcePath);
      const importedShards = [
        result.imported_master ? t("activity.dstWorldImportShardMaster") : null,
        result.imported_caves ? t("activity.dstWorldImportShardCaves") : null
      ]
        .filter(Boolean)
        .join(" + ");
      options.setActivity(
        message("activity.dstWorldImportSucceeded", {
          shards: importedShards || t("activity.dstWorldImportShardNone"),
          count: result.copied_file_count
        })
      );
      return result;
    } catch (error) {
      const messageText = describeError(error);
      options.setActivity(
        message("activity.dstWorldImportFailed", { message: messageText })
      );
      throw error;
    }
  }
  async function handleOpenLocalPath(path: string) {
    const trimmedPath = path.trim();
    if (!trimmedPath) {
      options.setActivity(message("activity.openPathUnavailable"));
      return;
    }

    try {
      await openLocalPath(trimmedPath);
    } catch (error) {
      options.setActivity(message("activity.openPathFailed", { message: describeError(error) }));
    }
  }

  return {
    creatingModuleIds,
    creationStartedAtByModule: new Map([...creationRequests.current].map(([id, request]) => [id, request.startedAt])),
    handleRefreshWorkspace,
    handleCreateServer,
    handleStartServer,
    handleStopServer,
    handleRefreshLaunchPreview,
    handleSaveSettings,
    handleSaveAutostart,
    handleApplyPlayerAccessMutation,
    handleExecutePlayerAction,
    handleCreateBackup,
    handleRestoreBackup,
    handleRenameBackup,
    handleArchiveInstance,
    handleDeleteInstance,
    handleDeleteBackup,
    handleImportDontStarveWorldData,
    handleOpenLocalPath,
    handleSendRuntimeCommand,
    handleSuppressRuntimeWindows
  };
}
