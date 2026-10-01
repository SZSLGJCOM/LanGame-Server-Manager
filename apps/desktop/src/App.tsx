import { version as desktopVersion } from "../package.json";
import { useCallback, useEffect, useEffectEvent, useMemo, useRef, useState, type Dispatch, type SetStateAction } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { buildAssistantCapsuleModel } from "./assistant-summary";
import { ASSISTANT_REQUEST_SAFETY_LIMIT, assistantFileChangesReceipt, assistantWorkflowExecutionState, assistantWorkflowResultMessage, runAssistantWorkflow } from "./assistant-workflow";
import { assistantConversationProviderIdentity } from "./assistant-conversations";
import { AssistantTurnControl, validateAssistantConversationState } from "./assistant-turn-control";
import { AssistantProgressPoller } from "./assistant-progress";
import { formatDesktopError } from "./desktop-error-message";
import type { AssistantActionId, AssistantBuildInput, AssistantPromptCard } from "./assistant-types";
import { AiSettingsWriteQueue, formatAiProviderLabel, getAiProviderPreset, getAiSettingsStatus, loadAiSettings, normalizeAiSettings, persistAiSettings, type AiSettings } from "./ai-settings";
import { bootstrapApp, checkAppUpdate, clearAssistantSecret, getAssistantConversationState, resumeAssistantConversation, createAssistantConversation, cancelAssistantTurn, deleteAssistantConversation, confirmAssistantOperation, executeAssistantOperation, fetchBindAddressCandidates, fetchOverlayFamilies, generateInstanceBroadcast, installAppUpdate, listInstanceBroadcastEvents, listOllamaModels, logFrontendEvent, pickDirectoryPath, probeSteamCmdStatus, readAssistantSecretStatus, readInstanceBroadcastPolicy, sendInstanceBroadcast, storeAssistantSecret, updateAppSettings } from "./api";
import { applyAppUpdateCheckResult, createInitialAppUpdateState, failAppUpdateState, reduceAppUpdateInstallEvent } from "./app-update-model";
import {
  describeError,
  fallbackBootstrap,
  isActiveJobStatus,
  resolveSelectedId,
  BIND_ADDRESS_INITIAL_POLL_DELAY_MS,
  BIND_ADDRESS_POLL_MS,
  RUNTIME_REFRESH_FAILURE_LIMIT,
  RUNTIME_VIEW_BACKGROUND_POLL_MS,
  RUNTIME_VIEW_POLL_MS,
  SYSTEM_VIEW_BACKGROUND_POLL_MS,
  SYSTEM_VIEW_POLL_MS,
  type RuntimeRefreshIssue,
  type SelectedInstancePanelData
} from "./app-state";
import { message, resolveUiMessage, type UiMessage } from "./app-ui";
import { mergeBootstrapSnapshot } from "./bootstrap-snapshot";
import { steamCmdDetailMessage, steamCmdSummaryMessage } from "./steamcmd-ui";
import { useInstallationCancellation } from "./hooks/useInstallationCancellation";
import { useInstanceRetirement } from "./hooks/useInstanceRetirement";
import { useInstanceInventoryRefresh } from "./hooks/useInstanceInventoryRefresh";
import { AppShell } from "./components/AppShell";
import { DesktopExitBoundary } from "./components/DesktopExitBoundary";
import { StorageInitializationView } from "./components/StorageInitializationView";
import {
  useLibraryActions,
  useInstanceActions
} from "./hooks/useDesktopActions";
import { useAssistantConversations } from "./hooks/useAssistantConversations";
import { useAssistantConversationRecovery } from "./hooks/useAssistantConversationRecovery";
import { useAssistantOperationConfirmation } from "./hooks/useAssistantOperationConfirmation";
import { AssistantOperationDialog } from "./components/AssistantOperationDialog";
import { useLibraryInstallationRefresh } from "./hooks/useLibraryInstallationRefresh";
import { useServerModuleInstallations } from "./hooks/useServerModuleInstallations";
import { mergeModuleProgramCounts, mergeModuleSummaries, updateModuleDetailsSummary } from "./library-installation-refresh";
import { useDesktopUiState } from "./hooks/useDesktopUiState";
import {
  useBootstrapInitialization,
  useLibraryJobPolling,
  useRuntimeRefreshModeSync,
  useRuntimeViewPolling,
  useSelectedInstanceModuleDetailsSync,
  useSelectedInstancePanelSync,
  useSystemViewPolling,
  useSelectedModuleDetailsSync
} from "./hooks/useDesktopEffects";
import { useI18n } from "./i18n";
import type {
  AppPathSettingsInput,
  AppUpdateState,
  AssistantChatMessage,
  AssistantExecutionState,
  BackgroundJob,
  BootstrapResponse,
  DstWorldStartPreview,
  InstanceBackupResult,
  InstanceBroadcastPolicy,
  InstanceDetails,
  InstanceRuntimeOverview,
  LaunchPlan,
  LogTailSnapshot,
  ModuleDetails,
  RuntimeWindowSnapshot,
  BindAddressCandidate,
  OverlayFamily,
  SteamCmdStatus,
  SteamCmdPrepareSnapshot,
  ThemeMode
} from "./types";
import { INITIAL_THEME, applyThemeToDocument, persistTheme } from "./theme";
import { DEFAULT_BIND_ADDRESS_CANDIDATE, normalizeBindAddressCandidates } from "./bind-addresses";
import {
  createStorageInitializationState,
  reduceStorageInitializationState,
  resolveStorageInitializationSurface
} from "./storage-initialization";
import { AppViewRouter } from "./views/AppViewRouter";
import { InstanceSelectionCursor } from "./instance-panel-refresh";
import { instancePanelReader, formatInstancePanelError, type InstancePanelLoadState, type InstancePanelPatch } from "./instance-panel-loader";
import { completedInstallationJob } from "./installation-job";
import { getLocalizedModuleDisplayName } from "./store-media";

interface DesktopAppProps {
  theme: ThemeMode;
  onThemeChange: (theme: ThemeMode) => void;
}

const APP_UPDATE_CHECK_INTERVAL_MS = 4 * 60 * 60 * 1000;
const DESKTOP_UPDATES_ENABLED = import.meta.env.VITE_LANGAME_DESKTOP_UPDATES_ENABLED === "true";

export function App() {
  const [theme, setTheme] = useState<ThemeMode>(INITIAL_THEME);

  useEffect(() => {
    applyThemeToDocument(theme);
    persistTheme(theme);
  }, [theme]);

  return <DesktopExitBoundary><DesktopApp theme={theme} onThemeChange={setTheme} /></DesktopExitBoundary>;
}

function DesktopApp({ theme, onThemeChange }: DesktopAppProps) {
  const { locale, t } = useI18n();
  const [bootstrap, setBootstrap] = useState<BootstrapResponse>(fallbackBootstrap);
  const getCurrentBootstrap = useEffectEvent(() => bootstrap);
  const [appUpdateState, setAppUpdateState] = useState<AppUpdateState>(() => createInitialAppUpdateState(desktopVersion));
  const [appUpdateChecksReady, setAppUpdateChecksReady] = useState(false);
  const [overlays, setOverlays] = useState<OverlayFamily[]>([]);
  const [bindAddressCandidates, setBindAddressCandidates] = useState<BindAddressCandidate[]>([DEFAULT_BIND_ADDRESS_CANDIDATE]);
  const [selectedModuleId, setSelectedModuleId] = useState<string | null>(null);
  const [selectedInstanceId, setSelectedInstanceIdState] = useState<string | null>(null);
  const retirement = useInstanceRetirement((instanceId) => instancePanelReader.invalidate(instanceId));
  const selectedInstanceCursorRef = useRef<InstanceSelectionCursor | null>(null);
  if (!selectedInstanceCursorRef.current) {
    selectedInstanceCursorRef.current = new InstanceSelectionCursor(null);
  }
  const selectedInstanceCursor = selectedInstanceCursorRef.current;
  const setSelectedInstanceId = useCallback<Dispatch<SetStateAction<string | null>>>((next) => {
    const prepared = selectedInstanceCursor.prepare(next);
    setSelectedInstanceIdState((current) => prepared.commit(current));
  }, [selectedInstanceCursor]);
  const [selectedModuleDetails, setSelectedModuleDetails] = useState<ModuleDetails | null>(null);
  const [selectedInstanceModuleDetails, setSelectedInstanceModuleDetails] = useState<ModuleDetails | null>(null);
  const [selectedInstanceModuleError, setSelectedInstanceModuleError] = useState<{ moduleId: string; message: string } | null>(null);
  const [selectedInstanceDetails, setSelectedInstanceDetails] = useState<InstanceDetails | null>(null);
  const [selectedInstanceBackups, setSelectedInstanceBackups] = useState<InstanceBackupResult[]>([]);
  const [selectedPanelLoadState, setSelectedPanelLoadState] = useState<InstancePanelLoadState | null>(null);
  const [panelRetryGeneration, setPanelRetryGeneration] = useState(0);
  const [selectedRuntime, setSelectedRuntime] = useState<InstanceRuntimeOverview | null>(null);
  const [selectedRuntimeWindows, setSelectedRuntimeWindows] = useState<RuntimeWindowSnapshot | null>(null);
  const [selectedLogDocument, setSelectedLogDocument] = useState<LogTailSnapshot | null>(null);
  const [selectedLaunchPlan, setSelectedLaunchPlan] = useState<LaunchPlan | null>(null);
  const [selectedLaunchPlanError, setSelectedLaunchPlanError] = useState<string | null>(null);
  const [steamCmdStatus, setSteamCmdStatus] = useState<SteamCmdStatus | null>(null);
  const [steamCmdMessage, setSteamCmdMessage] = useState<UiMessage>(message("activity.checkingSteamCmd"));
  const [steamCmdBusy, setSteamCmdBusy] = useState(false);
  const [steamCmdProgress, setSteamCmdProgress] = useState<SteamCmdPrepareSnapshot | null>(null);
  const [storageInitialization, setStorageInitialization] = useState(createStorageInitializationState);
  const [instanceDetailsById, setInstanceDetailsById] = useState<Partial<Record<string, InstanceDetails>>>({});
  const [instanceBackupsById, setInstanceBackupsById] = useState<Partial<Record<string, InstanceBackupResult[]>>>({});
  const [instanceRuntimesById, setInstanceRuntimesById] = useState<Partial<Record<string, InstanceRuntimeOverview>>>({});
  const [activity, setActivity] = useState<UiMessage>(message("activity.loadingDesktop"));
  const [libraryTaskPolling, setLibraryTaskPolling] = useState(false);
  const [runtimeRefreshMode, setRuntimeRefreshMode] = useState<"live" | "throttled">("live");
  const [, setRuntimeLastUpdatedAt] = useState<number | null>(null);
  const [runtimeRefreshIssue, setRuntimeRefreshIssue] = useState<RuntimeRefreshIssue | null>(null);
  const [runtimeAutoRefreshPaused, setRuntimeAutoRefreshPaused] = useState(false);
  const [systemRefreshIssue, setSystemRefreshIssue] = useState<RuntimeRefreshIssue | null>(null);
  const [aiSettings, setAiSettings] = useState<AiSettings>(() => loadAiSettings());
  const aiSettingsWriteQueueRef = useRef(new AiSettingsWriteQueue());
  const [assistantDraft, setAssistantDraft] = useState("");
  const assistantRequestRef = useRef(false);
  const assistantTurnRef = useRef<AssistantTurnControl | null>(null);
  const assistantProgressRef = useRef<AssistantProgressPoller | null>(null);
  const [assistantExecution, setAssistantExecution] = useState<AssistantExecutionState>({
    status: "idle",
    promptLabel: null,
    result: null,
    error: null
  });
  const {
    activeView,
    libraryCatalogFocusId,
    libraryCatalogScrollLeft,
    libraryPage,
    librarySearch,

    openInstanceView,
    openLibraryCatalog,
    openLibraryDetail,
    openServerWorkspace,
    openView,
    serverWorkspaceSection,
    handleNavSelect,
    handleSearchChange,
    setLibraryCatalogFocusId,
    setLibraryCatalogScrollLeft
  } = useDesktopUiState({
    onSelectInstance: setSelectedInstanceId,
    onSelectModule: setSelectedModuleId
  });
  const previousNavigationRef = useRef({ activeView, libraryPage });

  useEffect(() => {
    clearSelectedInstancePanel();
    setSelectedPanelLoadState(null);
    setRuntimeRefreshIssue(null);
  }, [selectedInstanceId]);
  const steamCmdProbeTokenRef = useRef(0);
  const steamCmdOperationBusyRef = useRef(false);
  const hasAutoSteamCmdProbeRef = useRef(false);
  const appUpdateCheckPendingRef = useRef(false);
  const appUpdateInstallPendingRef = useRef(false);

  useEffect(() => {
    if (!isTauri()) {
      return;
    }

    let active = true;
    let unlisten: (() => void) | undefined;
    void listen<{ message: string }>("app-shutdown-failed", (event) => {
      setActivity(message("activity.appShutdownFailed", { message: event.payload.message }));
    }).then((dispose) => {
      if (active) {
        unlisten = dispose;
      } else {
        dispose();
      }
    });

    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  const runAutomaticAppUpdateCheck = useEffectEvent(() => {
    if (["available", "downloading", "installing"].includes(appUpdateState.status)) {
      return;
    }

    void handleCheckAppUpdate({ silent: true });
  });

  const modules = bootstrap.state.modules;
  const instances = bootstrap.state.instances;
  const jobs = bootstrap.state.jobs;
  const hasActiveJobs = jobs.some((job) => isActiveJobStatus(job.status));
  const storageInitializationReady = storageInitialization.status === "ready";
  const storageInitializationSurface = resolveStorageInitializationSurface(storageInitialization);
  const storageReady =
    storageInitializationReady &&
    bootstrap.state.storage.database_exists &&
    bootstrap.state.storage.migrations_applied;
  const shouldLoadSelectedPanel = activeView === "servers" && storageReady;
  const shouldPollRuntimeView = activeView === "servers" && storageReady;
  const shouldPollSystemView = activeView === "system" && storageReady;
  const runtimePollingEnabled = shouldPollRuntimeView && !runtimeAutoRefreshPaused;
  const selectedPanelSyncEnabled = shouldLoadSelectedPanel && !runtimePollingEnabled && !retirement.operations.has(selectedInstanceId ?? "");
  const systemPollingEnabled = shouldPollSystemView;
  const runtimePollIntervalMs =
    runtimeRefreshMode === "throttled" ? RUNTIME_VIEW_BACKGROUND_POLL_MS : RUNTIME_VIEW_POLL_MS;
  const systemPollIntervalMs =
    runtimeRefreshMode === "throttled" ? SYSTEM_VIEW_BACKGROUND_POLL_MS : SYSTEM_VIEW_POLL_MS;
  const shouldLoadLibraryDetails = storageReady && activeView === "library" && libraryPage === "detail";
  const shouldLoadSelectedInstanceModuleDetails = storageReady && activeView === "servers";
  const selectedInstanceSummary = instances.find((instance) => instance.id === selectedInstanceId) ?? null;
  const selectedInstanceModuleId =
    selectedInstanceSummary?.module_id ?? selectedInstanceDetails?.summary.module_id ?? null;
  const assistantConversationScopeKey = [
    activeView,
    libraryPage,
    serverWorkspaceSection,
    selectedModuleId ?? "-",
    selectedInstanceId ?? "-"
  ].join("|");
  const assistantScopeRef = useRef(assistantConversationScopeKey);
  const assistantConversations = useAssistantConversations(assistantConversationScopeKey);
  const assistantConfirmation = useAssistantOperationConfirmation(assistantConversationScopeKey);
  const assistantMessages = assistantConversations.messages;
  useAssistantConversationRecovery(assistantConversationScopeKey, aiSettings, storageReady && getAiSettingsStatus(aiSettings).ready,
    assistantConversations.restore, (error) => setActivity(message("assistant.session.checkFailed", { message: describeError(error) })));

  useEffect(() => () => { assistantProgressRef.current?.stop(); }, []);

  function createAssistantMessageId() {
    if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
      return crypto.randomUUID();
    }
    return `assistant-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
  }

  function createAssistantMessage(message: Omit<AssistantChatMessage, "id">): AssistantChatMessage {
    return {
      id: createAssistantMessageId(),
      ...message
    };
  }

  function resetAssistantConversationUi() {
    setAssistantDraft("");
    setAssistantExecution({
      status: "idle",
      promptLabel: null,
      result: null,
      error: null
    });
  }

  function handleNewAssistantConversation() {
    if (assistantExecution.status === "running") {
      return;
    }
    assistantConversations.startNew();
    resetAssistantConversationUi();
  }

  function handleSelectAssistantConversation(conversationId: string) {
    if (assistantExecution.status === "running") {
      return;
    }
    assistantConversations.select(conversationId);
    resetAssistantConversationUi();
  }

  async function handleDeleteAssistantConversation(conversationId: string) {
    if (assistantExecution.status === "running") {
      return;
    }
    const originScope = assistantConversationScopeKey;
    let deletingActiveConversation = false;
    try {
      const backendId = assistantConversations.backendId(conversationId);
      if (backendId) {
        const result = await deleteAssistantConversation(backendId);
        if (result.conversationId !== backendId) throw new Error("Deleted conversation binding did not match.");
      }
      deletingActiveConversation = assistantConversations.remove(conversationId);
    } catch (error) {
      setActivity(message("assistant.history.deleteFailed", { message: describeError(error) }));
      return;
    }
    if (deletingActiveConversation && !assistantRequestRef.current && assistantScopeRef.current === originScope) {
      resetAssistantConversationUi();
    }
  }

  function summarizeAssistantPromptLabel(value: string) {
    const trimmed = value.trim();
    if (!trimmed) {
      return t("assistant.run.customPrompt");
    }

    return trimmed.length > 36 ? `${trimmed.slice(0, 36)}...` : trimmed;
  }

  useEffect(() => {
    let cancelled = false;
    let refreshing = false;

    function refreshBindAddressCandidates() {
      if (refreshing) {
        return;
      }
      refreshing = true;
      fetchBindAddressCandidates()
        .then((candidates) => {
          if (!cancelled) {
            setBindAddressCandidates(normalizeBindAddressCandidates(candidates));
          }
        })
        .catch(() => {
          if (!cancelled) {
            setBindAddressCandidates((current) => current.length ? current : [DEFAULT_BIND_ADDRESS_CANDIDATE]);
          }
        })
        .finally(() => {
          refreshing = false;
        });
    }

    function refreshWhenVisible() {
      if (document.visibilityState === "visible") {
        refreshBindAddressCandidates();
      }
    }

    const initialTimer = window.setTimeout(refreshBindAddressCandidates, BIND_ADDRESS_INITIAL_POLL_DELAY_MS);
    window.addEventListener("focus", refreshBindAddressCandidates);
    document.addEventListener("visibilitychange", refreshWhenVisible);
    const intervalId = window.setInterval(refreshBindAddressCandidates, BIND_ADDRESS_POLL_MS);

    return () => {
      cancelled = true;
      window.removeEventListener("focus", refreshBindAddressCandidates);
      document.removeEventListener("visibilitychange", refreshWhenVisible);
      window.clearTimeout(initialTimer);
      window.clearInterval(intervalId);
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    const descriptor = {
      provider: aiSettings.provider,
      baseUrl: aiSettings.baseUrl
    };
    const settingsRevision = aiSettingsWriteQueueRef.current.revision;

    readAssistantSecretStatus(descriptor)
      .then((status) => {
        if (cancelled || settingsRevision !== aiSettingsWriteQueueRef.current.revision) {
          return;
        }

        setAiSettings((current) => {
          if (current.provider !== descriptor.provider || current.baseUrl !== descriptor.baseUrl) {
            return current;
          }
          if (current.apiKeyStored === status.stored) {
            return current;
          }

          const nextSettings = {
            ...current,
            apiKeyStored: status.stored,
            apiKey: ""
          };
          persistAiSettings(nextSettings);
          return nextSettings;
        });
      })
      .catch(() => undefined);

    return () => {
      cancelled = true;
    };
  }, [aiSettings.provider, aiSettings.baseUrl]);

  useEffect(() => {
    if (aiSettings.provider !== "ollama") {
      return;
    }

    let cancelled = false;
    const ollamaPreset = getAiProviderPreset("ollama");
    const baseUrl = aiSettings.baseUrl.trim() || ollamaPreset.defaultBaseUrl;

    listOllamaModels(baseUrl)
      .then((models) => {
        if (cancelled) {
          return;
        }

        const normalizedModels = models
          .map((model) => model.trim())
          .filter((model, index, all) => model.length > 0 && all.indexOf(model) === index);

        setAiSettings((current) => {
          if (current.provider !== "ollama") {
            return current;
          }

          const currentBaseUrl = current.baseUrl.trim() || ollamaPreset.defaultBaseUrl;
          if (currentBaseUrl !== baseUrl) {
            return current;
          }

          const currentModel = current.model.trim();
          const nextModel = normalizedModels.length === 0
            ? ""
            : normalizedModels.includes(currentModel)
              ? currentModel
              : normalizedModels[0];

          if (nextModel === currentModel) {
            return current;
          }

          const nextSettings = {
            ...current,
            model: nextModel
          };
          persistAiSettings(nextSettings);
          return nextSettings;
        });
      })
      .catch(() => undefined);

    return () => {
      cancelled = true;
    };
  }, [aiSettings.provider, aiSettings.baseUrl]);
  async function runSteamCmdProbe(options: { silent?: boolean } = {}) {
    if (options.silent && steamCmdOperationBusyRef.current) return;
    const probeId = ++steamCmdProbeTokenRef.current;
    if (!options.silent) {
      setSteamCmdProgress(null);
      steamCmdOperationBusyRef.current = true;
      setSteamCmdBusy(true);
      setSteamCmdMessage(message("activity.checkingSteamCmd"));
    }

    try {
      const status = await probeSteamCmdStatus();
      if (probeId !== steamCmdProbeTokenRef.current) {
        return;
      }

      setSteamCmdStatus(status);
      setSteamCmdMessage(steamCmdDetailMessage(status));
      if (!options.silent) setActivity(steamCmdSummaryMessage(status));
    } catch (error) {
      if (probeId === steamCmdProbeTokenRef.current) {
        setSteamCmdStatus(null);
        const messageText = describeError(error);
        setSteamCmdMessage(message("activity.steamCmdFailed", { message: messageText }));
        if (!options.silent) setActivity(message("activity.steamCmdFailed", { message: messageText }));
      }
    } finally {
      if (!options.silent && probeId === steamCmdProbeTokenRef.current) {
        steamCmdOperationBusyRef.current = false;
        setSteamCmdBusy(false);
      }
    }
  }

  useEffect(() => {
    if (bootstrap === fallbackBootstrap || !storageReady || hasAutoSteamCmdProbeRef.current) {
      return;
    }

    hasAutoSteamCmdProbeRef.current = true;
    void runSteamCmdProbe();
  }, [bootstrap, storageReady]);

  useEffect(() => {
    if (!DESKTOP_UPDATES_ENABLED || !appUpdateChecksReady || !isTauri()) {
      return;
    }

    runAutomaticAppUpdateCheck();
    const intervalId = window.setInterval(runAutomaticAppUpdateCheck, APP_UPDATE_CHECK_INTERVAL_MS);
    return () => window.clearInterval(intervalId);
  }, [appUpdateChecksReady]);

  useEffect(() => {
    const previous = previousNavigationRef.current;
    if (previous.activeView === activeView && previous.libraryPage === libraryPage) {
      return;
    }

    previousNavigationRef.current = { activeView, libraryPage };
    void logFrontendEvent("info", "frontend.navigation.changed", `Navigated to ${activeView}`, {
      from_view: previous.activeView,
      to_view: activeView,
      from_library_page: previous.libraryPage,
      to_library_page: libraryPage
    }).catch(() => undefined);
  }, [activeView, libraryPage]);

  useEffect(() => {
    assistantScopeRef.current = assistantConversationScopeKey;
    setAssistantDraft("");
    if (assistantRequestRef.current) {
      assistantProgressRef.current?.stop();
      setAssistantExecution((current) => ({ ...current, progress: undefined }));
      return;
    }
    setAssistantExecution({
      status: "idle",
      promptLabel: null,
      result: null,
      error: null
    });
  }, [assistantConversationScopeKey]);

  async function reloadBootstrap(
    preferred?: { moduleId?: string | null; instanceId?: string | null },
    options?: { refreshOverlays?: boolean; reconcileRetirement?: string }
  ) {
    const requested = getCurrentBootstrap();
    const [latest, latestOverlays] = await Promise.all([
      bootstrapApp(),
      options?.refreshOverlays ? fetchOverlayFamilies() : Promise.resolve<OverlayFamily[] | null>(null)
    ]);

    const merge = (current: BootstrapResponse) => {
      const next = mergeBootstrapSnapshot(current, latest, requested);
      return { ...next, state: { ...next.state,
        instances: retirement.mergeInstances(current.state.instances, next.state.instances, options?.reconcileRetirement)
      } };
    };
    const resolved = merge(getCurrentBootstrap());
    setBootstrap(merge);
    if (latestOverlays) {
      setOverlays(latestOverlays);
    }
    setSelectedModuleId((current) => resolveSelectedId(preferred?.moduleId ?? current, resolved.state.modules));
    setSelectedInstanceId((current) => resolveSelectedId(preferred?.instanceId ?? current, resolved.state.instances));
    return latest;
  }

  function clearSelectedInstancePanel() {
    setSelectedInstanceDetails(null);
    setSelectedInstanceBackups([]);
    setSelectedRuntime(null);
    setSelectedRuntimeWindows(null);
    setSelectedLogDocument(null);
    setSelectedLaunchPlan(null);
    setSelectedLaunchPlanError(null);
  }

  function markRuntimeRefreshed() {
    setRuntimeLastUpdatedAt(Date.now());
    setRuntimeRefreshIssue(null);
    setRuntimeAutoRefreshPaused(false);
  }

  function markRuntimeRefreshFailed(error: unknown) {
    const messageText = describeError(error);
    setRuntimeRefreshIssue((current) => {
      const consecutiveFailures = (current?.consecutiveFailures ?? 0) + 1;
      if (consecutiveFailures >= RUNTIME_REFRESH_FAILURE_LIMIT) {
        setRuntimeAutoRefreshPaused(true);
      }

      return {
        message: messageText,
        failedAt: Date.now(),
        consecutiveFailures
      };
    });
  }

  function cacheInstancePanel(instanceId: string, next: InstancePanelPatch) {
    if (next.details) setInstanceDetailsById((current) => ({ ...current, [instanceId]: next.details }));
    if (next.backups) setInstanceBackupsById((current) => ({ ...current, [instanceId]: next.backups }));
    if (next.runtime) setInstanceRuntimesById((current) => ({ ...current, [instanceId]: next.runtime }));
  }

  function applySelectedInstancePanel(next: InstancePanelPatch) {
    if (next.details) setSelectedInstanceDetails(next.details);
    if (next.backups) setSelectedInstanceBackups(next.backups);
    if (next.runtime) setSelectedRuntime(next.runtime);
    if (next.runtimeWindows) setSelectedRuntimeWindows(next.runtimeWindows);
    if (next.logDocument) setSelectedLogDocument(next.logDocument);
    if (next.launchPlan !== undefined) setSelectedLaunchPlan(next.launchPlan);
    if (next.launchPlanError !== undefined) setSelectedLaunchPlanError(next.launchPlanError);
  }

  function replaceSelectedInstancePanel(next: SelectedInstancePanelData) {
    applySelectedInstancePanel(next);
    markRuntimeRefreshed();
  }

  const handleRuntimeRefreshModeChange = useEffectEvent((mode: "live" | "throttled") => {
    setRuntimeRefreshMode(mode);
  });

  const handleLibraryJobsSynced = useEffectEvent((jobs: BackgroundJob[]) => {
    const finished = completedInstallationJob(bootstrap.state.jobs, jobs);
    if (finished) {
      const name = getLocalizedModuleDisplayName(finished.target_id, locale, finished.label);
      const status = finished.status.toLowerCase();
      const operation = finished.kind.toLowerCase() === "validategame" ? "validated"
        : finished.kind.toLowerCase() === "uninstallgame" ? "uninstalled" : "completed";
      setActivity(message(`installation.${status === "completed" ? operation : status}`, {
        name, message: finished.detail ?? finished.output_excerpt ?? ""
      }));
    }
    setBootstrap((current) => ({
      ...current,
      state: {
        ...current.state,
        jobs
      }
    }));
  });

  const handleSelectedModuleDetailsLoaded = useEffectEvent((details: ModuleDetails | null) => {
    setSelectedModuleDetails(details);
  });

  const handleSelectedInstanceModuleDetailsLoaded = useEffectEvent((details: ModuleDetails | null, error?: { moduleId: string; message: string }) => {
    setSelectedInstanceModuleDetails(details);
    setSelectedInstanceModuleError(error ?? null);
  });

  const handleSelectedPanelClear = useEffectEvent(() => {
    clearSelectedInstancePanel();
  });

  const handleSelectedPanelLoaded = useEffectEvent((instanceId: string, payload: InstancePanelPatch) => {
    if (retirement.isPending(instanceId) || selectedInstanceCursor.current() !== instanceId) return;
    cacheInstancePanel(instanceId, payload);
    applySelectedInstancePanel(payload);
  });

  const handleSelectedPanelProgress = useEffectEvent((state: InstancePanelLoadState) => {
    if (retirement.isPending(state.instanceId)) return;
    if (selectedInstanceCursor.current() === state.instanceId) setSelectedPanelLoadState(state);
  });

  const handleSelectedPanelSettled = useEffectEvent((state: InstancePanelLoadState) => {
    if (retirement.isPending(state.instanceId) || selectedInstanceCursor.current() !== state.instanceId) return;
    // Auxiliary panels keep their own errors without suspending live server status.
    const errors = [state.errors.details, state.errors.runtime]
      .filter((error): error is string => Boolean(error))
      .map(error => formatInstancePanelError(error, t));
    if (errors.length) markRuntimeRefreshFailed(new Error(errors.join("\n")));
    else markRuntimeRefreshed();
  });

  const handleBootstrapReady = useEffectEvent((payload: {
    attempt: number;
    booted: BootstrapResponse;
  }) => {
    if (storageInitialization.status !== "pending" || storageInitialization.attempt !== payload.attempt) {
      return;
    }

    setBootstrap(payload.booted);
    setSelectedModuleId(resolveSelectedId(null, payload.booted.state.modules));
    setSelectedInstanceId(resolveSelectedId(null, payload.booted.state.instances));
    setStorageInitialization((current) =>
      reduceStorageInitializationState(current, { type: "ready", attempt: payload.attempt })
    );
    setActivity(message("activity.dataSynced"));
  });

  const handleBootstrapMetadata = useEffectEvent((payload: {
    attempt: number;
    appVersion: string | null;
    overlays: OverlayFamily[] | null;
  }) => {
    if (storageInitialization.attempt !== payload.attempt) {
      return;
    }

    if (payload.appVersion !== null) {
      setAppUpdateState((current) => ({ ...current, currentVersion: payload.appVersion ?? current.currentVersion }));
      setAppUpdateChecksReady(true);
    }
    if (payload.overlays !== null) {
      setOverlays(payload.overlays);
    }
  });

  const handleBootstrapInitFailed = useEffectEvent((payload: {
    attempt: number;
    error: unknown;
    storage: BootstrapResponse["state"]["storage"] | null;
  }) => {
    if (storageInitialization.status !== "pending" || storageInitialization.attempt !== payload.attempt) {
      return;
    }

    const errorMessage = describeError(payload.error);
    setStorageInitialization((current) =>
      reduceStorageInitializationState(current, {
        type: "failed",
        attempt: payload.attempt,
        error: errorMessage,
        storage: payload.storage
      })
    );
    setActivity(message("activity.initFailed"));
  });

  function handleRetryStorageInitialization() {
    if (storageInitialization.status !== "failed") {
      return;
    }

    setStorageInitialization((current) => reduceStorageInitializationState(current, { type: "retry" }));
    setActivity(message("storage.initialization.retrying"));
  }

  const handleSystemViewSynced = useEffectEvent((latest: BootstrapResponse, _requestRevision: number, requested?: BootstrapResponse) => {
    setBootstrap((current) => {
      const next = mergeBootstrapSnapshot(current, latest, requested);
      return { ...next, state: { ...next.state,
        instances: retirement.mergeInstances(current.state.instances, next.state.instances)
      } };
    });
    setSystemRefreshIssue(null);
  });

  const handleSystemPollingError = useEffectEvent((error: unknown, _requestRevision: number) => {
    setSystemRefreshIssue((current) => ({
      message: formatDesktopError(t, error),
      failedAt: Date.now(),
      consecutiveFailures: (current?.consecutiveFailures ?? 0) + 1
    }));
  });

  const handleRuntimeInstancesSynced = useEffectEvent((payload: {
    instances: BootstrapResponse["state"]["instances"];
    selectedInstanceId: string | null;
    requestedInstances?: BootstrapResponse["state"]["instances"];
  }) => {
    setBootstrap((current) => ({
      ...current,
      state: { ...current.state, instances: payload.requestedInstances && current.state.instances !== payload.requestedInstances
        ? current.state.instances : retirement.mergeInstances(current.state.instances, payload.instances) }
    }));
    if (!retirement.isPending(selectedInstanceCursor.current())
      && (!payload.requestedInstances || getCurrentBootstrap().state.instances === payload.requestedInstances)) {
      setSelectedInstanceId(payload.selectedInstanceId);
    }
  });

  const handleRuntimeSelectionCleared = useEffectEvent(() => {
    if (retirement.isPending(selectedInstanceCursor.current())) return;
    clearSelectedInstancePanel();
    markRuntimeRefreshed();
  });

  const handleRuntimePollingError = useEffectEvent((error: unknown) => {
    markRuntimeRefreshFailed(new Error(formatInstancePanelError(describeError(error), t)));
  });

  useBootstrapInitialization({
    attempt: storageInitialization.attempt,
    onReady: handleBootstrapReady,
    onMetadata: handleBootstrapMetadata,
    onError: handleBootstrapInitFailed
  });
  useRuntimeRefreshModeSync(handleRuntimeRefreshModeChange);
  const refreshInstallationState = useLibraryInstallationRefresh({
    enabled: storageReady && (activeView === "library" || activeView === "servers"),
    getCurrentInstanceId: () => retirement.isPending(selectedInstanceCursor.current()) ? null : selectedInstanceCursor.current(),
    onModules: (nextModules) => {
      setBootstrap((current) => {
        const next = mergeModuleSummaries(current.state.modules, nextModules);
        return next === current.state.modules ? current : { ...current, state: { ...current.state, modules: next } };
      });
      setSelectedModuleId((current) => resolveSelectedId(current, nextModules));
      setSelectedModuleDetails((current) => updateModuleDetailsSummary(current, nextModules));
      setSelectedInstanceModuleDetails((current) => updateModuleDetailsSummary(current, nextModules));
    },
    onProgramCounts: (nextModules) => {
      setBootstrap((current) => {
        const next = mergeModuleProgramCounts(current.state.modules, nextModules);
        return next === current.state.modules ? current : { ...current, state: { ...current.state, modules: next } };
      });
      const updateDetails = (current: ModuleDetails | null) => current
        ? updateModuleDetailsSummary(current, mergeModuleProgramCounts([current.summary], nextModules)) : current;
      setSelectedModuleDetails(updateDetails);
      setSelectedInstanceModuleDetails(updateDetails);
    },
    onPreview: (preview) => {
      setSelectedLaunchPlan(preview.launchPlan);
      setSelectedLaunchPlanError(preview.launchPlanError);
    },
    onError: (error) => {
      if (!retirement.isPending()) setActivity(message("activity.refreshLibraryFailed", { message: describeError(error) }));
    }
  });
  const inspectableInstances = useMemo(() => instances.filter((instance) => !retirement.operations.has(instance.id)),
    [instances, retirement.operations]);
  const serverInstallations = useServerModuleInstallations({
    enabled: storageReady && activeView === "servers",
    modules,
    instances: inspectableInstances,
    selectedInstanceModuleDetails,
    selectedLaunchPlan,
    onError: (error) => {
      void logFrontendEvent("warn", "instance.installation_capabilities_failed", describeError(error)).catch(() => undefined);
    }
  });
  useSelectedModuleDetailsSync({ enabled: shouldLoadLibraryDetails, selectedModuleId, modules, onLoaded: handleSelectedModuleDetailsLoaded });
  useSelectedInstanceModuleDetailsSync({
    enabled: shouldLoadSelectedInstanceModuleDetails,
    selectedModuleId: selectedInstanceModuleId,
    retryGeneration: panelRetryGeneration,
    onLoaded: handleSelectedInstanceModuleDetailsLoaded
  });
  useSelectedInstancePanelSync({
    selectedInstanceId,
    enabled: selectedPanelSyncEnabled,
    retryGeneration: panelRetryGeneration,
    onClear: handleSelectedPanelClear,
    onLoaded: handleSelectedPanelLoaded,
    onProgress: handleSelectedPanelProgress,
    onSettled: handleSelectedPanelSettled
  });
  useInstanceInventoryRefresh({
    enabled: shouldPollRuntimeView,
    getCurrentInstances: () => getCurrentBootstrap().state.instances,
    getCurrentInstanceId: () => selectedInstanceCursor.current(),
    onInstancesSynced: handleRuntimeInstancesSynced,
    onSelectionCleared: handleRuntimeSelectionCleared,
    onError: handleRuntimePollingError
  });
  useRuntimeViewPolling({
    enabled: runtimePollingEnabled,
    selectedInstanceId,
    intervalMs: runtimePollIntervalMs,
    retryGeneration: panelRetryGeneration,
    getCurrentInstances: () => getCurrentBootstrap().state.instances,
    isInstancePending: retirement.isPending,
    onInstancesSynced: handleRuntimeInstancesSynced,
    onSelectionCleared: handleRuntimeSelectionCleared,
    onSelectionLoaded: handleSelectedPanelLoaded,
    onProgress: handleSelectedPanelProgress,
    onSettled: handleSelectedPanelSettled,
    onError: handleRuntimePollingError
  });
  const systemRefreshing = useSystemViewPolling({
    enabled: systemPollingEnabled,
    intervalMs: systemPollIntervalMs,
    requestRevision: retirement.currentRevision(),
    getCurrentSnapshot: getCurrentBootstrap,
    onSynced: handleSystemViewSynced,
    onError: handleSystemPollingError
  });
  function handleResumeRuntimeAutoRefresh() {
    setPanelRetryGeneration((current) => current + 1);
    setRuntimeAutoRefreshPaused(false);
    setRuntimeRefreshIssue(null);
    setActivity(message("activity.autoRefreshResumed"));
  }

  async function handleCheckAppUpdate(options: { silent?: boolean } = {}) {
    if (!DESKTOP_UPDATES_ENABLED || appUpdateCheckPendingRef.current || appUpdateInstallPendingRef.current) {
      return;
    }

    appUpdateCheckPendingRef.current = true;
    setAppUpdateState((current) => ({ ...current, status: "checking", error: null }));
    try {
      const result = await checkAppUpdate();
      setAppUpdateState((current) => applyAppUpdateCheckResult(current, result));
    } catch (error) {
      if (options.silent) {
        void logFrontendEvent("warn", "app.update.auto_check_failed", describeError(error)).catch(() => undefined);
      }

      setAppUpdateState((current) => failAppUpdateState(current, error));
    } finally {
      appUpdateCheckPendingRef.current = false;
    }
  }

  async function handleInstallAppUpdate() {
    if (appUpdateInstallPendingRef.current || appUpdateState.status !== "available") {
      return;
    }

    appUpdateInstallPendingRef.current = true;
    setAppUpdateState((current) => ({
      ...current,
      status: "downloading",
      downloadedBytes: 0,
      downloadPercent: 0,
      error: null
    }));

    try {
      await installAppUpdate((event) => {
        setAppUpdateState((current) => reduceAppUpdateInstallEvent(current, event));
      });
    } catch (error) {
      setAppUpdateState((current) => failAppUpdateState(current, error));
    } finally {
      appUpdateInstallPendingRef.current = false;
    }
  }

  async function handleSaveAiSettings(settings: AiSettings) {
    const nextSettings = normalizeAiSettings(settings);
    const providerLabel = formatAiProviderLabel(nextSettings.provider, locale);
    const typedApiKey = nextSettings.apiKey.trim();

    try {
      const persistedSettings = await aiSettingsWriteQueueRef.current.run(async () => {
        let apiKeyStored = nextSettings.apiKeyStored;
        if (typedApiKey) {
          if (!nextSettings.baseUrl.trim()) {
            throw new Error(t("assistant.settings.baseUrlRequired"));
          }

          const secretStatus = await storeAssistantSecret(
            {
              provider: nextSettings.provider,
              baseUrl: nextSettings.baseUrl
            },
            typedApiKey
          );
          apiKeyStored = secretStatus.stored;
        }

        return { ...nextSettings, apiKeyStored, apiKey: "" };
      });
      if (!persistedSettings) {
        return null;
      }

      persistAiSettings(persistedSettings);
      setAiSettings(persistedSettings);
      setActivity(
        message("assistant.settings.saved", {
          provider: providerLabel,
          keyNote: typedApiKey ? t("assistant.settings.keyStoredSuffix") : ""
        })
      );
      return persistedSettings;
    } catch (error) {
      setActivity(
        message("assistant.settings.failed", { message: describeError(error) })
      );
      throw error;
    }
  }
  async function handlePickDirectory(currentPath?: string | null) {
    try {
      return await pickDirectoryPath(currentPath ?? null);
    } catch (error) {
      setActivity(message("settings.paths.pickFailed", { message: describeError(error) }));
      return null;
    }
  }

  async function handleSaveAppSettings(settings: AppPathSettingsInput) {
    const nextSettings = {
      games_root: settings.games_root.trim(),
      servers_root: settings.servers_root.trim(),
      archives_root: settings.archives_root.trim(),
      steamcmd_root: settings.steamcmd_root.trim()
    };

    try {
      const savedSettings = await updateAppSettings(nextSettings);
      await reloadBootstrap();
      await runSteamCmdProbe();

      setActivity(message("settings.paths.saved", { path: savedSettings.games_root }));
    } catch (error) {
      setActivity(message("settings.paths.failed", { message: describeError(error) }));
      throw error;
    }
  }

  async function handleClearAiSecret(settings: AiSettings) {
    const nextSettings = normalizeAiSettings(settings);
    const providerLabel = formatAiProviderLabel(nextSettings.provider, locale);

    try {
      const persistedSettings = await aiSettingsWriteQueueRef.current.run(async () => {
        if (nextSettings.baseUrl.trim()) {
          await clearAssistantSecret({
            provider: nextSettings.provider,
            baseUrl: nextSettings.baseUrl
          });
        }
        return { ...nextSettings, apiKeyStored: false, apiKey: "" };
      });
      if (!persistedSettings) {
        return null;
      }

      persistAiSettings(persistedSettings);
      setAiSettings(persistedSettings);
      setActivity(
        message("assistant.settings.cleared", { provider: providerLabel })
      );
      return persistedSettings;
    } catch (error) {
      setActivity(
        message("assistant.settings.clearFailed", { message: describeError(error) })
      );
      throw error;
    }
  }
  async function handleAssistantStop() {
    const active = assistantTurnRef.current;
    if (!active || active.stopRequested) return;
    assistantProgressRef.current?.stop();
    setAssistantExecution((current) => ({ ...current, stopping: true }));
    assistantConfirmation.respond(false);
    try {
      await active.stop();
    } catch (error) {
      if (assistantTurnRef.current === active) {
        assistantProgressRef.current?.start();
        setAssistantExecution((current) => ({ ...current, stopping: false }));
        setActivity(message("assistant.run.stopFailed", { message: describeError(error) }));
      }
    }
  }

  async function executeAssistantOperationRequest(input: {
    promptLabel: string;
    prompt: string;
    userMessage: AssistantChatMessage;
    resume?: { conversationId: string; backendConversationId: string };
  }) {
    if (assistantRequestRef.current) return;
    const originScope = assistantConversationScopeKey;
    const notReadyMessage = t("assistant.run.notReady");
    let turn = input.resume ?? assistantConversations.beginTurn(input.userMessage, assistantConversationProviderIdentity(aiSettings));

    if (!assistantCanRun) {
      setAssistantExecution({
        status: "error",
        promptLabel: input.promptLabel,
        result: null,
        error: notReadyMessage
      });
      assistantConversations.appendMessage(
        turn.conversationId,
        createAssistantMessage({
          role: "assistant",
          label: input.promptLabel,
          content: notReadyMessage,
          state: "error"
        })
      );
      setActivity(message("assistant.run.notReady"));
      return;
    }

    const providerLabel = formatAiProviderLabel(aiSettings.provider, locale);
    const selectedHints = { selectedInstanceId, selectedModuleId };
    assistantRequestRef.current = true;
    const active = new AssistantTurnControl(cancelAssistantTurn);
    let progress: AssistantProgressPoller | null = null;
    let baselineRevision = 0;
    assistantTurnRef.current = active;
    setAssistantExecution({
      status: "running",
      promptLabel: input.promptLabel,
      result: null,
      error: null
    });
    setActivity(message("assistant.run.started", { provider: providerLabel, prompt: input.promptLabel }));

    try {
      const settings = {
          provider: aiSettings.provider,
          model: aiSettings.model,
          baseUrl: aiSettings.baseUrl,
          apiKey: aiSettings.apiKey
      };
      let backendId = turn.backendConversationId;
      if (backendId && !input.resume) {
        const state = await getAssistantConversationState(backendId, settings);
        validateAssistantConversationState(state, backendId);
        baselineRevision = state.revision ?? 0;
        if (state.status === "unavailable") {
          assistantConversations.markUnavailable(turn.conversationId, input.userMessage.id);
          const notice = createAssistantMessage({ role: "assistant", content: t("assistant.session.expired"), state: "ready" });
          assistantConversations.appendMessage(turn.conversationId, notice);
          turn = assistantConversations.beginTurn(input.userMessage, assistantConversationProviderIdentity(aiSettings));
          assistantConversations.appendMessage(turn.conversationId, createAssistantMessage({ role: "assistant", content: t("assistant.session.expired"), state: "ready" }));
          backendId = null;
        } else if (state.status === "running") {
          await active.bind(backendId);
          assistantConversations.setRecoveryPending(turn.conversationId, true);
          assistantConversations.appendMessage(turn.conversationId, createAssistantMessage({ role: "assistant", content: t("assistant.session.running"), state: "ready" }));
          setAssistantExecution({ status: "inconclusive", promptLabel: input.promptLabel, result: null, error: null });
          return;
        }
      }
      if (!backendId) {
        const created = await createAssistantConversation(settings);
        assistantConversations.bindBackend(turn.conversationId, created);
        backendId = created.conversationId;
        baselineRevision = created.revision;
      }
      assistantConversations.setContinuation(turn.conversationId, null);
      assistantConversations.setRecoveryPending(turn.conversationId, false);
      await active.bind(backendId);
      if (active.stopRequested) {
        assistantConversations.appendMessage(turn.conversationId, createAssistantMessage({
          role: "assistant", content: t("assistant.run.stopped"), state: "ready"
        }));
        setAssistantExecution({ status: "cancelled", promptLabel: input.promptLabel, result: null, error: null });
        return;
      }
      const operationInput = {
        conversationId: backendId,
        settings,
        prompt: input.prompt,
        context: JSON.stringify({ interfaceLanguage: locale }),
        ...selectedHints
      };
      progress = new AssistantProgressPoller(backendId, baselineRevision,
        (cursor) => getAssistantConversationState(backendId, settings, cursor ?? 0),
        (next) => setAssistantExecution((current) => current.status === "running" ? { ...current, progress: next } : current),
        () => assistantTurnRef.current === active && assistantScopeRef.current === originScope && !active.stopRequested);
      assistantProgressRef.current = progress;
      progress.start();
      const operation = input.resume
        ? await resumeAssistantConversation(backendId, settings)
        : await executeAssistantOperation(operationInput);
      active.validate(operation);

      const outcome = await runAssistantWorkflow(operation, {
        confirmPreview: (preview) => !active.stopRequested && assistantScopeRef.current === originScope
          ? assistantConfirmation.confirmPreview(preview) : false,
        cancelPending: () => active.stop().then(() => undefined),
        shouldStop: () => active.stopRequested,
        executeConfirmed: async (binding) => {
          const completed = await confirmAssistantOperation({ settings: operationInput.settings, ...binding });
          active.validate(completed);
          return completed;
        },
        onResult: async (completed) => {
          for (const step of completed.completedOperations ?? []) {
            const parts = [step.message, step.verification?.summary].filter((part): part is string => Boolean(part));
            if (step.fileChangesResult) parts.push(assistantFileChangesReceipt(step.fileChangesResult, (key, fallback) => t(key, {}, fallback)));
            if (step.fileChangeResult) parts.push(`${t("assistant.operation.file.path")}: ${step.fileChangeResult.file}\n${t("assistant.operation.file.backup")}: ${step.fileChangeResult.backupId}\n${t(step.fileChangeResult.readBackVerified ? "assistant.operation.file.verified" : "assistant.operation.file.failed")}`);
            assistantConversations.appendMessage(turn.conversationId, createAssistantMessage({
              role: "assistant", label: input.promptLabel, content: [...new Set(parts)].join("\n\n"), meta: step.action,
              state: step.verification?.status === "failed" || step.task?.status === "failed" || step.fileChangeResult?.readBackVerified === false
                || (step.fileChangesResult && step.fileChangesResult.status !== "applied") ? "error" : "ready"
            }));
          }
          const metaParts = [
            completed.assistantReason,
            completed.appliedSettingsKeys.length ? `settings: ${completed.appliedSettingsKeys.join(", ")}` : "",
            completed.appliedPortNames.length ? `ports: ${completed.appliedPortNames.join(", ")}` : "",
            completed.workshopItemIds.length ? `mods: ${completed.workshopItemIds.join(", ")}` : "",
            completed.resolvedModIds.length ? `site mods: ${completed.resolvedModIds.join(", ")}` : "",
            completed.sourcePaths.length ? `local files: ${completed.sourcePaths.length}` : "",
            completed.runtimeCommands.length ? `gm: ${completed.runtimeCommands.join(", ")}` : "",
            completed.configDocumentCount ? `config files: ${completed.configDocumentCount}` : ""
          ].filter(Boolean);
          assistantConversations.appendMessage(
            turn.conversationId,
            createAssistantMessage({
              role: "assistant",
              label: input.promptLabel,
              ...assistantWorkflowResultMessage(completed, (key, fallback) => t(key, {}, fallback)),
              meta: completed.action === "none" ? undefined : metaParts.join(" | ") || completed.action
            })
          );
          if (!completed.handled) return;
          // Keep execution evidence in the conversation that initiated the workflow.
          await reloadBootstrap(undefined, { refreshOverlays: true });
        }
      });
      await active.waitForCancellation();
      assistantConversations.setContinuation(turn.conversationId, !active.stopRequested && outcome.status === "paused"
        ? outcome.lastOperation?.continuation ?? null : null);
      if (outcome.status === "cancelled" || outcome.status === "limit-reached") {
        const summary = outcome.pendingOperation?.planSummary ?? "";
        const content = outcome.status === "limit-reached"
          ? t("assistant.operation.stepLimit", { limit: ASSISTANT_REQUEST_SAFETY_LIMIT }, "Stopped after {limit} confirmed operations. Review the completed results before starting another request.")
          : !summary ? t("assistant.run.stopped")
            : outcome.completedSteps > 0
            ? t("assistant.operation.followUpCancelled", { summary }, "Next operation cancelled: {summary}. Earlier operations remain completed; their results are shown above.")
            : t("assistant.operation.cancelled", { summary }, "Cancelled without executing: {summary}");
        assistantConversations.appendMessage(
          turn.conversationId,
          createAssistantMessage({ role: "assistant", label: input.promptLabel, content, state: "ready" })
        );
      }
      const verification = outcome.lastOperation?.verification;
      const status = assistantWorkflowExecutionState(outcome);
      const errorText = status === "error" ? verification?.summary ?? outcome.lastOperation?.message ?? "" : null;
      if (assistantScopeRef.current === originScope) {
        setAssistantExecution({
          status,
          promptLabel: input.promptLabel,
          result: null,
          error: errorText
        });
        setActivity(status === "error"
          ? message("assistant.run.failed", { prompt: input.promptLabel, message: errorText ?? "" })
          : message(status === "success" ? "assistant.run.success" : `assistant.run.${status}`, { prompt: input.promptLabel }));
      }
    } catch (error) {
      await active.waitForCancellation();
      if (active.conversationId || turn.backendConversationId) {
        assistantConversations.setRecoveryPending(turn.conversationId, true);
      }
      const errorText = active.stopRequested ? t("assistant.session.stopUnconfirmed") : formatDesktopError(t, error);
      if (assistantScopeRef.current === originScope) setAssistantExecution({
        status: active.stopRequested ? "inconclusive" : "error",
        promptLabel: input.promptLabel,
        result: null,
        error: errorText
      });
      assistantConversations.appendMessage(
        turn.conversationId,
        createAssistantMessage({
          role: "assistant",
          label: input.promptLabel,
          content: errorText,
          state: active.stopRequested ? "ready" : "error"
        })
      );
      if (assistantScopeRef.current === originScope) setActivity(message(active.stopRequested ? "assistant.run.inconclusive" : "assistant.run.failed", { prompt: input.promptLabel, message: errorText }));
    } finally {
      progress?.stop();
      if (assistantProgressRef.current === progress) assistantProgressRef.current = null;
      await active.waitForCancellation();
      if (assistantTurnRef.current === active) assistantTurnRef.current = null;
      assistantRequestRef.current = false;
      if (assistantScopeRef.current !== originScope) {
        setAssistantExecution({ status: "idle", promptLabel: null, result: null, error: null });
      }
    }
  }

  async function handleAssistantResume() {
    const conversation = assistantConversations.activeConversation;
    if (assistantRequestRef.current || !conversation?.backendConversationId
      || conversation.providerIdentity !== assistantConversationProviderIdentity(aiSettings)) return;
    if (conversation.recoveryPending) {
      assistantRequestRef.current = true;
      try {
        const state = await getAssistantConversationState(conversation.backendConversationId, {
          provider: aiSettings.provider, model: aiSettings.model, baseUrl: aiSettings.baseUrl, apiKey: aiSettings.apiKey
        });
        validateAssistantConversationState(state, conversation.backendConversationId);
        if (state.status === "unavailable") assistantConversations.markUnavailable(conversation.id);
        else {
          assistantConversations.setContinuation(conversation.id, state.status === "paused" ? state.continuation : null);
          assistantConversations.setRecoveryPending(conversation.id, state.status === "running");
        }
        if (state.status !== "paused") assistantConversations.appendMessage(conversation.id, createAssistantMessage({
          role: "assistant", content: t(state.status === "unavailable" ? "assistant.session.expired"
            : state.status === "running" ? "assistant.session.running" : "assistant.session.noCheckpoint"), state: "ready"
        }));
        setAssistantExecution({ status: state.status === "paused" ? "paused" : "inconclusive", promptLabel: null, result: null, error: null });
      } catch (error) {
        setActivity(message("assistant.session.checkFailed", { message: describeError(error) }));
      } finally {
        assistantRequestRef.current = false;
      }
      return;
    }
    if (!conversation.continuation?.canResume) return;
    await executeAssistantOperationRequest({
      promptLabel: t("assistant.run.resume"), prompt: "",
      userMessage: createAssistantMessage({ role: "user", content: t("assistant.run.resume"), state: "ready" }),
      resume: { conversationId: conversation.id, backendConversationId: conversation.backendConversationId }
    });
  }

  async function handleAssistantSendMessage(messageText: string) {
    const trimmed = messageText.trim();
    if (!trimmed || assistantExecution.status === "running") {
      return;
    }

    await executeAssistantOperationRequest({
      promptLabel: summarizeAssistantPromptLabel(trimmed),
      prompt: trimmed,
      userMessage: createAssistantMessage({
        role: "user",
        content: trimmed,
        state: "ready"
      })
    });
  }

  async function handleRunAssistantPrompt(prompt: AssistantPromptCard) {
    if (assistantExecution.status === "running") {
      return;
    }

    await executeAssistantOperationRequest({
      promptLabel: prompt.label,
      prompt: prompt.prompt,
      userMessage: createAssistantMessage({
        role: "user",
        label: prompt.label,
        content: prompt.preview,
        state: "ready"
      })
    });
  }

  async function handleAssistantAction(actionId: AssistantActionId) {
    switch (actionId) {
      case "ensure-storage":
        openView("system");
        if (storageInitialization.status === "failed") {
          handleRetryStorageInitialization();
        } else if (storageReady) {
          await handleEnsureStorageOnly();
        }
        return;
      case "ensure-steamcmd":
        if (!storageReady) {
          return;
        }
        openView("system");
        await handleEnsureSteamCmd();
        return;
      case "resume-runtime-refresh":
        if (!storageReady) {
          return;
        }
        handleResumeRuntimeAutoRefresh();
        return;
      case "view-system":
        handleNavSelect("system");
        return;
      case "view-library":
        openLibraryCatalog();
        return;
      case "view-servers":
        handleNavSelect("servers");
        return;
      case "refresh-launch-preview":
        if (!storageReady) {
          return;
        } else if (selectedInstanceId) {
          openInstanceView("overview", selectedInstanceId);
          await handleRefreshLaunchPreview();
        } else {
          handleNavSelect("servers");
        }
        return;
      default:
        return;
    }
  }

  const {
    handleEnsureSteamCmd,
    handleCancelSteamCmd,
    steamCmdStopPending,
    steamCmdStopError,
    handleUninstallSteamCmd,
    handleEnsureStorageOnly,
    handleSyncStorageOnly,
    handleInstallModule,
    handleUninstallModule
  } = useLibraryActions({
    refreshInstallationState,
    refreshSteamCmdStatus: () => runSteamCmdProbe({ silent: true }),
    setActivity,
    setBootstrap,
    setSelectedModuleId,
    setSelectedInstanceId,
    setLibraryTaskPolling,
    setSteamCmdBusy: (busy) => {
      steamCmdOperationBusyRef.current = busy;
      setSteamCmdBusy(busy);
    },
    setSteamCmdMessage,
    setSteamCmdStatus,
    setSteamCmdProgress,
    onSteamCmdOperationStart: () => {
      hasAutoSteamCmdProbeRef.current = true;
      steamCmdProbeTokenRef.current += 1;
    },
    reloadBootstrap
  });
  const { handleCancelInstallation, installationStopPendingIds, installationStopErrors } = useInstallationCancellation(jobs);

  const {
    creatingModuleIds,
    creationStartedAtByModule,
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
  } = useInstanceActions({
    retirement,
    refreshInstallationState,
    bindAddressCandidates,
    cacheInstancePanel,
    clearSelectedInstancePanel,
    getCurrentInstanceId: () => selectedInstanceCursor.current(),
    instanceBackupsById,
    instanceDetailsById,
    markRuntimeRefreshed,
    modules,
    openInstanceView,
    replaceSelectedInstancePanel,
    reloadBootstrap,
    selectedInstanceId,
    setActivity,
    setBootstrap,
    setInstanceDetailsById,
    setSelectedInstanceDetails,
    setInstanceBackupsById,
    setInstanceRuntimesById,
    setSelectedInstanceBackups,
    setSelectedInstanceId,
    setSelectedModuleId
  });
  useLibraryJobPolling({
    enabled: storageReady && (activeView === "library" || activeView === "servers" || libraryTaskPolling
      || hasActiveJobs || creatingModuleIds.size > 0),
    onJobsSynced: handleLibraryJobsSynced
  });
  const activeJobsCount = jobs.filter((job) => isActiveJobStatus(job.status)).length;
  const assistantCanRun = storageReady && getAiSettingsStatus(aiSettings).ready;

  function normalizeBroadcastPrompt(value: string | null | undefined): string | null {
    const trimmed = String(value ?? "").trim();
    return trimmed ? trimmed.slice(0, 320) : null;
  }

  function broadcastPolicyAuditSnapshot(policy: InstanceBroadcastPolicy): string {
    return JSON.stringify({
      enabled: policy.enabled,
      rules: policy.rules,
      updated_at_unix_ms: policy.updated_at_unix_ms
    });
  }

  function automaticBroadcastIntent(instanceId: string, source: "startup" | "shutdown", policy: InstanceBroadcastPolicy): string {
    const customPrompt = normalizeBroadcastPrompt(source === "startup" ? policy.rules.startup.prompt : policy.rules.shutdown.prompt);
    if (customPrompt) {
      return customPrompt;
    }
    const instanceName = instances.find((instance) => instance.id === instanceId)?.name ?? instanceId;
    return source === "startup"
      ? t("assistant.broadcast.defaultStartupIntent", { name: instanceName })
      : t("assistant.broadcast.defaultShutdownIntent", { name: instanceName });
  }

  async function maybeRunLifecycleBroadcast(instanceId: string, source: "startup" | "shutdown") {
    if (!assistantCanRun) {
      return;
    }

    try {
      const policy = await readInstanceBroadcastPolicy(instanceId);
      const rule = source === "startup" ? policy.rules.startup : policy.rules.shutdown;
      if (!policy.enabled || !rule.enabled) {
        return;
      }

      const events = await listInstanceBroadcastEvents(instanceId, 20);
      const lastSent = events.find((event) => (event.rule_id === source || event.source === source) && event.status === "sent");
      const cooldownMs = Math.max(0, policy.rules.cooldown_minutes) * 60_000;
      if (lastSent && cooldownMs > 0 && Date.now() - lastSent.created_at_unix_ms < cooldownMs) {
        return;
      }

      const policySnapshotJson = broadcastPolicyAuditSnapshot(policy);
      const generated = await generateInstanceBroadcast({
        instanceId,
        settings: {
          provider: aiSettings.provider,
          model: aiSettings.model,
          baseUrl: aiSettings.baseUrl,
          apiKey: aiSettings.apiKey
        },
        intent: automaticBroadcastIntent(instanceId, source, policy),
        tone: policy.rules.tone,
        source,
        ruleId: source,
        initiator: "lifecycle",
        policySnapshotJson
      });
      await sendInstanceBroadcast({
        instanceId,
        message: generated.message,
        source,
        ruleId: source,
        aiProvider: generated.provider,
        aiModel: generated.model,
        initiator: "lifecycle",
        policySnapshotJson
      });
    } catch (error) {
      void logFrontendEvent("warn", "instance.broadcast.lifecycle_failed", describeError(error), {
        instanceId,
        source
      });
    }
  }

  async function handleStartServerWithBroadcast(instanceId: string, expectedWorldStart?: DstWorldStartPreview) {
    const started = await handleStartServer(instanceId, expectedWorldStart);
    if (started) {
      void maybeRunLifecycleBroadcast(instanceId, "startup");
    }
  }

  async function handleStopServerWithBroadcast(instanceId: string) {
    await maybeRunLifecycleBroadcast(instanceId, "shutdown");
    await handleStopServer(instanceId);
  }

  const assistantInput: AssistantBuildInput = {
    selectedModuleId,
    aiSettings,
    locale,
    activeJobsCount,
    activeView,
    bootstrap,
    storageReady,
    libraryPage,
    overlayNames: overlays.map((overlay) => overlay.name),
    runtimeAutoRefreshPaused,
    runtimeRefreshIssue,
    selectedInstanceDetails,
    selectedInstanceId,
    selectedInstanceModuleDetails,
    selectedLaunchPlan,
    selectedLaunchPlanError,
    selectedLogDocument,
    selectedModuleDetails,
    selectedRuntime,
    serverWorkspaceSection,
    steamCmdStatus
  };
  const assistant = buildAssistantCapsuleModel(assistantInput);
  return (
    <>
      <AppShell
        activeView={activeView}
        activityText={resolveUiMessage(t, activity)}
        activityTone={activity.tone}
        runtimeRefreshIssue={activeView === "system" ? systemRefreshIssue : runtimeRefreshIssue}
        runtimeAutoRefreshPaused={activeView === "system" ? false : runtimeAutoRefreshPaused}
        runtimePollIntervalMs={activeView === "system" ? systemPollIntervalMs : runtimePollIntervalMs}
        runtimeRefreshFailureLimit={RUNTIME_REFRESH_FAILURE_LIMIT}
        onResumeRuntimeAutoRefresh={handleResumeRuntimeAutoRefresh}
        steamCmdStopPending={steamCmdStopPending}
        steamCmdStopError={steamCmdStopError}
        installationStopPendingIds={installationStopPendingIds}
        installationStopErrors={installationStopErrors}
        onCancelSteamCmd={(operationId) => void handleCancelSteamCmd(operationId)}
        onCancelInstallation={(jobId) => void handleCancelInstallation(jobId)}
        steamCmdProgress={steamCmdProgress}
        steamCmdMessage={resolveUiMessage(t, steamCmdMessage)}
        jobs={jobs}
        aiSettings={aiSettings}
        appUpdateState={appUpdateState}
        appUpdatesEnabled={DESKTOP_UPDATES_ENABLED}
        serverCount={storageReady ? instances.length : null}
        assistant={assistant}
        assistantDraft={assistantDraft}
        assistantInput={assistantInput}
        assistantExecution={assistantExecution}
        assistantConfirmationOpen={assistantConfirmation.preview !== null}
        assistantMessages={assistantMessages}
        assistantConversations={assistantConversations.conversations}
        assistantActiveConversationId={assistantConversations.activeConversationId}
        theme={theme}
        onSelectView={handleNavSelect}
        onAssistantAction={(actionId) => void handleAssistantAction(actionId)}
        onAssistantDraftChange={setAssistantDraft}
        onAssistantNewConversation={handleNewAssistantConversation}
        onAssistantSelectConversation={handleSelectAssistantConversation}
        onAssistantDeleteConversation={handleDeleteAssistantConversation}
        onAssistantRunPrompt={(prompt) => void handleRunAssistantPrompt(prompt)}
        assistantContinuation={assistantConversations.activeConversation?.providerIdentity === assistantConversationProviderIdentity(aiSettings)
          ? assistantConversations.activeConversation.continuation : null}
        assistantRecoveryPending={assistantConversations.activeConversation?.providerIdentity === assistantConversationProviderIdentity(aiSettings)
          && assistantConversations.activeConversation.recoveryPending}
        onAssistantResume={handleAssistantResume}
        onAssistantStop={handleAssistantStop}
        onAssistantSendMessage={(message) => void handleAssistantSendMessage(message)}
        onCheckAppUpdate={() => void handleCheckAppUpdate()}
        onClearAiSecret={handleClearAiSecret}
        onInstallAppUpdate={() => void handleInstallAppUpdate()}
        onThemeChange={onThemeChange}
        onSaveAiSettings={handleSaveAiSettings}
      >
      {storageInitializationSurface === "content" ? (
        <AppViewRouter
        activeView={activeView}
        aiSettings={aiSettings}
        assistantCanRun={assistantCanRun}
        bootstrap={bootstrap}
        overlays={overlays}
        bindAddressCandidates={bindAddressCandidates}
        modules={modules}
        instances={instances}
        jobs={jobs}
        libraryPage={libraryPage}
        libraryCatalogFocusId={libraryCatalogFocusId}
        libraryCatalogScrollLeft={libraryCatalogScrollLeft}
        serverWorkspaceSection={serverWorkspaceSection}
        onServerWorkspaceSectionChange={openServerWorkspace}
        selectedModuleId={selectedModuleId}
        selectedModuleDetails={selectedModuleDetails}

        selectedInstanceId={selectedInstanceId}
        instanceRetirements={retirement.operations}
        selectedInstanceDetails={selectedInstanceDetails?.summary.id === selectedInstanceId ? selectedInstanceDetails : null}
        selectedPanelLoadState={selectedPanelLoadState?.instanceId === selectedInstanceId ? selectedPanelLoadState : null}
        selectedInstanceModuleDetails={selectedInstanceModuleDetails}
        selectedInstanceModuleError={selectedInstanceModuleError?.moduleId === selectedInstanceModuleId ? selectedInstanceModuleError.message : null}
        selectedBackups={selectedInstanceBackups}
        selectedRuntime={selectedRuntime}
        selectedRuntimeWindows={selectedRuntimeWindows}
        selectedLaunchPlan={selectedLaunchPlan}
        selectedLaunchPlanError={selectedLaunchPlanError}
        steamCmdStatus={steamCmdStatus}
        steamCmdBusy={steamCmdBusy}
        steamCmdProgress={steamCmdProgress}
        steamCmdMessage={resolveUiMessage(t, steamCmdMessage)}
        runtimeRefreshIssue={runtimeRefreshIssue}
        systemRefreshing={systemRefreshing}
        onActivity={setActivity}
        librarySearch={librarySearch}
        search=""
        instanceDetailsById={instanceDetailsById}
        instanceRuntimesById={instanceRuntimesById}
        moduleInstallations={serverInstallations.moduleInstallations}
        instanceLaunchPlans={serverInstallations.instanceLaunchPlans}
        instanceLaunchFailures={serverInstallations.instanceLaunchFailures}
        onOpenLibraryCatalog={openLibraryCatalog}
        onOpenLibraryDetail={openLibraryDetail}
        onOpenInstance={(instanceId) => openInstanceView("overview", instanceId)}
        onLibraryCatalogFocusChange={setLibraryCatalogFocusId}
        onLibraryCatalogScrollLeftChange={setLibraryCatalogScrollLeft}
        onPickDirectory={handlePickDirectory}
        onSaveAppSettings={handleSaveAppSettings}
        onStorageChanged={async () => { await reloadBootstrap(); }}
        onImportDontStarveWorldData={(instanceId, sourcePath) => handleImportDontStarveWorldData(instanceId, sourcePath)}
        onSelectInstance={setSelectedInstanceId}
        onSearchChange={handleSearchChange}

        onResumeRuntimeAutoRefresh={handleResumeRuntimeAutoRefresh}
        onEnsureSteamCmd={() => void handleEnsureSteamCmd()}
        onUninstallSteamCmd={() => void handleUninstallSteamCmd()}
        onInstallModule={handleInstallModule}
        onUninstallModule={handleUninstallModule}
        onCreateServer={handleCreateServer}
        creatingModuleIds={creatingModuleIds}
        creationStartedAtByModule={creationStartedAtByModule}
        onCreateBackup={(instanceId) => void handleCreateBackup(instanceId)}
        onRestoreBackup={(instanceId, backupId) => void handleRestoreBackup(instanceId, backupId)}
        onRenameBackup={handleRenameBackup}
        onArchiveInstance={handleArchiveInstance}
        onDeleteInstance={handleDeleteInstance}
        onDeleteBackup={handleDeleteBackup}
        onSaveServerSettings={(input, options) => handleSaveSettings(input, options)}
        onSaveServerAutostart={handleSaveAutostart}
        onApplyPlayerAccessMutation={(input) => handleApplyPlayerAccessMutation(input)}
        onExecutePlayerAction={handleExecutePlayerAction}
        onStartServer={handleStartServerWithBroadcast}
        onStopServer={handleStopServerWithBroadcast}

        onEnsureStorageOnly={() => void handleEnsureStorageOnly()}
        onSyncStorageOnly={() => void handleSyncStorageOnly()}

        onOpenLocalPath={(path) => void handleOpenLocalPath(path)}
        onSendRuntimeCommand={(instanceId, command, processKey, options) =>
          handleSendRuntimeCommand(instanceId, command, processKey, options)
        }
        onSuppressRuntimeWindows={(instanceId) => handleSuppressRuntimeWindows(instanceId)}

        />
      ) : (
        <StorageInitializationView
          state={storageInitialization}
          onRetry={handleRetryStorageInitialization}
        />
      )}
    </AppShell>
    {assistantConfirmation.preview ? <AssistantOperationDialog
      key={assistantConfirmation.preview.confirmationToken}
      preview={assistantConfirmation.preview}
      onRespond={assistantConfirmation.respond}
    /> : null}
  </>
  );
}
