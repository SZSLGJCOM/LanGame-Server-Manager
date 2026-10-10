import { message, type UiMessage } from "../app-ui";
import { previewDontStarveWorldStart } from "../api";
import { describeError, type RuntimeRefreshIssue } from "../app-state";
import { Fragment, useEffect, useId, useMemo, useRef, useState } from "react";
import type { AiSettings } from "../ai-settings";
import { ServerCardFrame } from "./servers/ServerCardFrame";
import { ShellIcon } from "../components/ShellIcon";
import { ActivityNotice } from "../components/ActivityNotice";
import { InlineConfirmAction } from "../components/InlineConfirmAction";
import { inspectInstanceRemoval, isStorageManagementAvailable } from "../api-storage";
import { formatInstanceRemovalPlan } from "./servers/instance-removal-presentation";
import { useI18n } from "../i18n";
import type {
  BindAddressCandidate,
  DstWorldImportResult,
  DstWorldStartPreview,
  ExecuteInstancePlayerActionInput,
  ExecuteInstancePlayerActionResult,
  InstanceBackupResult,
  InstanceDetails,
  InstancePlayerAccessMutationInput,
  InstancePlayerAccessMutationResult,
  InstanceRuntimeOverview,
  InstanceRuntimeCommandResult,
  InstanceSummary,
  InstanceConnectionInfo,
  LaunchPlan,
  ModuleDetails,
  RuntimeWindowSnapshot,
  RuntimeCommandDispatchOptions,
  SaveInstanceSettingsOptions,
  ServerWorkspaceSection,
  UpdateInstanceInput,
  BackgroundJob
} from "../types";
import { formatInstanceStatus } from "../view-models";
import {
  instanceHasRunningProcess,
  pendingRuntimeActionForIntent,
  type RuntimeActionPending
} from "../runtime-action-state";
import {
  resolveServerPrimaryAction,
  ServerInstallationRequestGate,
  type ServerInstallationIntent,
  type ServerModuleInstallation
} from "../server-primary-action";
import { RuntimeSurfaceWorkbench } from "./servers/RuntimeSurfaceWorkbench";
import { collectRuntimeLogPaths, type RuntimeLogStartupBoundary } from "../runtime-log-stream";
import { ModWorkbench } from "./servers/ModWorkbench";
import { ServerMaintenanceWorkspace } from "./servers/ServerMaintenanceWorkspace";
import { InstanceUnavailablePanel } from "./servers/InstanceUnavailablePanel";
import { GMToolsWorkbench } from "./servers/GMToolsWorkbench";
import type { InstancePanelLoadState } from "../instance-panel-loader";
import { PlayerCenterWorkbench } from "./servers/PlayerCenterWorkbench";
import { ServerDetailTabs, type ServerDetailTab, type ServerDetailTabSpec } from "./servers/ServerDetailTabs";
import { buildServerDetailTabSpecs } from "./servers/server-detail-tab-specs";
import { ConfigurationWorkspace } from "./settings/ConfigurationWorkspace";
import { useConfigurationFieldHelp } from "./settings/ConfigurationFieldHelp";
import { useInstanceSettingsSaveCoordinator } from "./settings/InstanceSettingsSaveContext";
import { InstanceSettingsDraftInvalidError } from "./settings/instance-settings-save-queue";
import { DstWorldStartError, prepareDstWorldStart } from "./servers/dst-world-start";
import { ServerCardConnection } from "./servers/ServerCardConnection";
import { ArchivedServerCard, DeletionErrorCard } from "./servers/ArchivedServerCard";
import { ServerArchiveDetails } from "./servers/ServerArchiveDetails";
import { ArchivedInstanceWorkspace } from "./servers/ArchivedInstanceWorkspace";
import { useInstanceArchives } from "./servers/useInstanceArchives";
import { useInstanceConnections } from "../hooks/useInstanceConnections";
import type { InstanceRetirementKind } from "../hooks/useInstanceRetirement";
import "./servers/instance-themes.css";
import "./servers/workbench.css";
import "./servers/server-archives.css";

interface ServersViewProps {
  onArchivesChanged: () => Promise<void>;
  aiSettings: AiSettings;
  assistantCanRun: boolean;
  bindAddressCandidates: BindAddressCandidate[];
  instances: InstanceSummary[];
  moduleInstallations: Partial<Record<string, ServerModuleInstallation>>;
  instanceLaunchPlans: Partial<Record<string, LaunchPlan>>;
  instanceLaunchFailures: Partial<Record<string, true>>;
  jobs?: BackgroundJob[];
  section: ServerWorkspaceSection;
  onWorkspaceSectionChange: (section: ServerWorkspaceSection) => void;
  selectedInstanceId: string | null;
  instanceRetirements?: ReadonlyMap<string, InstanceRetirementKind>;
  selectedDetails: InstanceDetails | null;
  panelLoadState?: InstancePanelLoadState | null;
  selectedBackups: InstanceBackupResult[];
  selectedModuleDetails: ModuleDetails | null;
  selectedModuleDetailsError: string | null;
  runtime: InstanceRuntimeOverview | null;
  runtimeWindows: RuntimeWindowSnapshot | null;
  launchPlan: LaunchPlan | null;
  launchPlanError: string | null;
  refreshIssue: RuntimeRefreshIssue | null;
  onActivity: (value: UiMessage) => void;
  onResumeAutoRefresh: () => void;
  onSelectInstance: (instanceId: string) => void;
  onStart: (instanceId: string, expectedWorldStart?: DstWorldStartPreview) => void | Promise<void>;
  onStop: (instanceId: string) => void | Promise<void>;
  onInstallModule: (moduleId: string, validate: boolean) => Promise<void>;
  onOpenModuleLibrary: (moduleId: string) => void;
  onCreateInstance: () => void;
  onPickDirectory: (currentPath?: string | null) => Promise<string | null>;
  onImportDontStarveWorldData: (instanceId: string, sourcePath: string) => Promise<DstWorldImportResult>;
  onOpenLocalPath: (path: string) => void;
  onSendRuntimeCommand: (
    instanceId: string,
    command: string,
    processKey?: string | null,
    options?: RuntimeCommandDispatchOptions
  ) => Promise<InstanceRuntimeCommandResult | null>;
  onSuppressRuntimeWindows: (instanceId: string) => Promise<void>;
  onCreateBackup: (instanceId: string) => void;
  onDeleteInstance: (instanceId: string) => void | Promise<void>;
  onArchiveInstance: (instanceId: string) => void | Promise<void>;
  onRestoreBackup: (instanceId: string, backupId: string) => void;
  onRenameBackup: (instanceId: string, backup: InstanceBackupResult, displayName: string) => Promise<boolean>;
  onDeleteBackup: (instanceId: string, backup: InstanceBackupResult) => void | Promise<void>;
  onSaveSettings: (input: UpdateInstanceInput, options?: SaveInstanceSettingsOptions) => Promise<InstanceDetails | undefined>;
  onSaveAutostart: (instanceId: string, autostart: boolean) => Promise<void>;
  onApplyPlayerAccessMutation: (
    input: InstancePlayerAccessMutationInput
  ) => Promise<InstancePlayerAccessMutationResult>;
  onExecutePlayerAction?: (input: ExecuteInstancePlayerActionInput) => Promise<ExecuteInstancePlayerActionResult>;
}

function metric(label: string, value: string, meta: string, icon: "server" | "settings" | "monitor" | "shield") {
  return (
    <article className="workspace-metric">
      <div className="workspace-metric-icon">
        <ShellIcon name={icon} className="workspace-metric-icon-svg" />
      </div>
      <div className="workspace-metric-copy">
        <div className="workspace-metric-label">{label}</div>
        <div className="workspace-metric-value">{value}</div>
        <div className="workspace-metric-meta">{meta}</div>
      </div>
    </article>
  );
}

function matchesQuery(instance: InstanceSummary, query: string): boolean {
  if (!query) {
    return true;
  }

  return `${instance.name} ${instance.id} ${instance.module_id}`.toLowerCase().includes(query);
}

interface ServerListCardProps {
  instance: InstanceSummary;
  retirement?: InstanceRetirementKind;
  storageBusy?: boolean;
  connection?: InstanceConnectionInfo;
  connectionFailed: boolean;
  bindAddressCandidates: BindAddressCandidate[];
  onConnectionRead: (connection: InstanceConnectionInfo) => void;
  onActivity: (value: UiMessage) => void;
  active: boolean;
  t: ReturnType<typeof useI18n>["t"];
  onSelectInstance: (instanceId: string) => void;
  onDeleteInstance: (instanceId: string) => void | Promise<void>;
  onArchiveInstance: (instanceId: string) => void | Promise<void>;
  onStart: (instanceId: string) => void;
  onStop: (instanceId: string) => void;
  pendingAction?: RuntimeActionPending | null;
  installation?: ServerModuleInstallation;
  launchPlan?: LaunchPlan;
  launchFailed?: boolean;
  pendingInstallation?: ServerInstallationIntent;
  jobs?: BackgroundJob[];
  onInstall: (moduleId: string, intent: ServerInstallationIntent) => void;
  onOpenModuleLibrary: (moduleId: string) => void;
}

function ServerListCard(props: ServerListCardProps) {
  const removalPending = useRef(false);
  const [removing, setRemoving] = useState(false);

  const actionModel = resolveServerPrimaryAction({
    instanceId: props.instance.id,
    moduleId: props.instance.module_id,
    status: props.instance.status,
    hasRunningProcess: instanceHasRunningProcess(props.instance),
    pendingRuntime: props.pendingAction,
    installation: props.installation,
    launchPlan: props.launchPlan,
    launchFailed: props.launchFailed,
    pendingInstallation: props.pendingInstallation,
    jobs: props.jobs
  });
  const deleteBlocked = actionModel.deleteBlocked || removing || props.storageBusy || Boolean(props.retirement);
  const archiveAvailable = isStorageManagementAvailable();
  async function removeInstance(kind: "archive" | "delete") {
    if (deleteBlocked || removalPending.current || (kind === "archive" && !archiveAvailable)) return;
    removalPending.current = true;
    setRemoving(true);
    try {
      await (kind === "archive" ? props.onArchiveInstance : props.onDeleteInstance)(props.instance.id);
    } finally {
      removalPending.current = false;
      setRemoving(false);
    }
  }
  const statusLabel = props.retirement
    ? props.t(props.retirement === "delete" ? "activity.deletingInstance" : "activity.archivingInstance", { id: props.instance.name })
    : formatInstanceStatus(actionModel.displayedStatus, props.t);

  return <ServerCardFrame id={props.instance.id} name={props.instance.name} moduleId={props.instance.module_id}
    active={props.active} status={actionModel.displayedStatus} statusLabel={statusLabel}
    onSelect={() => props.onSelectInstance(props.instance.id)}
    actions={<div className="row-card-actions row-card-actions--server server-list-card-actions">
              <InlineConfirmAction
                wrapperClassName="server-list-card-confirm server-list-card-delete"
                type="button"
                className="secondary-button danger server-list-card-icon-action"
                disabled={deleteBlocked}
                scopeKey={`${props.instance.id}:${props.active}`}
                confirmation={props.t("servers.details.deleteInstanceConfirm")}
                prepareConfirmation={async () => formatInstanceRemovalPlan(await inspectInstanceRemoval(props.instance.id), props.t)}
                confirmLabel={props.t("common.delete")}
                onConfirm={() => removeInstance("delete")}
                aria-label={`${props.t("servers.details.deleteInstance")}: ${props.instance.name}`}
                title={
                  actionModel.deleteBlocked
                    ? props.t("servers.details.deleteInstanceRequiresStop")
                    : props.t("servers.details.deleteInstance")
                }
              >
                <ShellIcon name="trash" className="server-list-card-action-icon" />
              </InlineConfirmAction>
              <InlineConfirmAction wrapperClassName="server-list-card-confirm server-list-card-archive"
                type="button" className="secondary-button server-list-card-icon-action"
                disabled={deleteBlocked || !archiveAvailable} scopeKey={`${props.instance.id}:${props.active}`}
                confirmation={props.t("servers.details.archiveInstanceConfirm")}
                confirmLabel={props.t("servers.details.archiveInstance")}
                onConfirm={() => removeInstance("archive")}
                aria-label={`${props.t("servers.details.archiveInstance")}: ${props.instance.name}`}
                title={props.t(!archiveAvailable ? "servers.details.archiveHostOnly" : actionModel.deleteBlocked
                  ? "servers.details.archiveInstanceRequiresStop" : "servers.details.archiveInstance")}>
                <ShellIcon name="inbox" className="server-list-card-action-icon" />
              </InlineConfirmAction>
            </div>}
    metadata={props.retirement ? null : <ServerCardConnection instance={props.instance} connection={props.connection}
              failed={props.connectionFailed} candidates={props.bindAddressCandidates}
              onRead={props.onConnectionRead} onActivity={props.onActivity} />}
    primary={<button
              type="button"
              className={actionModel.className}
              disabled={actionModel.disabled || Boolean(props.retirement)}
              aria-busy={actionModel.ariaBusy || Boolean(props.retirement)}
              aria-label={`${props.t(actionModel.labelKey)}: ${props.instance.name}`}
              title={props.t(actionModel.labelKey)}
              onClick={() => {
                if (actionModel.disabled || props.retirement) return;
                if (actionModel.action === "stop") props.onStop(props.instance.id);
                else if (actionModel.action === "start") props.onStart(props.instance.id);
                else if (actionModel.action === "install" || actionModel.action === "repair") props.onInstall(props.instance.module_id, actionModel.action);
                else if (actionModel.action === "library") props.onOpenModuleLibrary(props.instance.module_id);
              }}
            >
              <ShellIcon name={actionModel.iconName} className="server-list-card-action-icon" />
              <span>{props.t(actionModel.labelKey)}</span>
            </button>} />;
}

function ServerListRunningFilter(props: { checked: boolean; onChange: (checked: boolean) => void }) {
  const { t } = useI18n();
  const label = t("servers.list.onlyRunning");
  const help = useConfigurationFieldHelp(useId(), label, undefined, undefined, "instructions");
  return <label className="server-list-filter" ref={help.anchorRef} {...help.interactionProps}>
    <input
      type="checkbox"
      aria-label={label}
      aria-describedby={help.descriptionId}
      checked={props.checked}
      onChange={(event) => props.onChange(event.target.checked)}
    />
    {help.helpNode}
  </label>;
}

export function ServersView(props: ServersViewProps) {
  const { locale, t } = useI18n();
  const archives = useInstanceArchives(props.onArchivesChanged);
  const [listMode, setListMode] = useState<"normal" | "archived">("normal");
  const [selectedArchiveId, setSelectedArchiveId] = useState<string | null>(null);
  const [selectedDeletionId, setSelectedDeletionId] = useState<string | null>(null);
  const [pendingRemovals, setPendingRemovals] = useState(0);
  const storageBusy = archives.loading || pendingRemovals > 0 || archives.operation !== null || Boolean(props.instanceRetirements?.size);
  const selectedRetirement = props.instanceRetirements?.get(props.selectedInstanceId ?? "");
  const settingsSaveCoordinator = useInstanceSettingsSaveCoordinator();
  const readableInstances = useMemo(() => props.instances.filter((instance) => !props.instanceRetirements?.has(instance.id)),
    [props.instances, props.instanceRetirements]);
  const listConnections = useInstanceConnections(readableInstances, selectedRetirement ? null : props.selectedDetails);
  const runtimeActionGate = useRef(new Set<string>());
  const runtimeStartupBoundaries = useRef(new Map<string, RuntimeLogStartupBoundary>());
  const detailPanelId = useId();
  const [requestedDetailTab, setActiveDetailTab] = useState<ServerDetailTab>("runtime");
  const [requestedArchiveDetailTab, setArchiveDetailTab] = useState<ServerDetailTab>("runtime");
  const [retainedGmInstanceId, setRetainedGmInstanceId] = useState<string | null>(null);
  const [runtimeActionsByInstanceId, setRuntimeActionsByInstanceId] = useState<Partial<Record<string, RuntimeActionPending>>>({});
  const [installationsByModuleId, setInstallationsByModuleId] = useState<Partial<Record<string, ServerInstallationIntent>>>({});
  const installationGateRef = useRef<ServerInstallationRequestGate | null>(null);
  if (!installationGateRef.current) installationGateRef.current = new ServerInstallationRequestGate();
  const installationGate = installationGateRef.current;
  const [searchText, setSearchText] = useState("");
  const [onlyRunning, setOnlyRunning] = useState(false);
  const normalizedSearchText = useMemo(() => searchText.trim().toLowerCase(), [searchText]);
  const pendingDeletions = archives.list?.pending_deletions ?? [];
  const pendingIds = new Set(pendingDeletions.map((entry) => entry.instance_id));
  const visibleDeletions = onlyRunning ? [] : pendingDeletions.filter((entry) =>
    `${entry.instance_name} ${entry.instance_id} ${entry.module_id}`.toLowerCase().includes(normalizedSearchText));
  const visibleArchives = archives.available ? (archives.list?.archives ?? []).filter((entry) =>
    `${entry.instance_name ?? ""} ${entry.instance_id ?? ""} ${entry.archive_id} ${entry.module_id ?? ""}`.toLowerCase().includes(normalizedSearchText)) : [];
  const selectedDeletion = pendingDeletions.find((entry) => entry.operation_id === selectedDeletionId);
  const selectedArchive = archives.list?.archives.find((entry) => entry.archive_id === selectedArchiveId);
  const visibleInstances = useMemo(
    () =>
      props.instances.filter((instance) =>
        (!onlyRunning || instanceHasRunningProcess(instance)) && matchesQuery(instance, normalizedSearchText)
      ),
    [onlyRunning, normalizedSearchText, props.instances]
  );
  const runningCount = props.instances.filter((instance) => instanceHasRunningProcess(instance)).length;
  const autostartCount = props.instances.filter((instance) => instance.autostart).length;
  const selectedPorts = props.selectedDetails?.ports.length ?? 0;
  const selectedInstance = props.instances.find((instance) => instance.id === props.selectedInstanceId);
  const selectedDetailsError = props.panelLoadState ? props.panelLoadState.errors.details : props.refreshIssue?.message;
  const selectedDetailsLoading = props.panelLoadState?.pending.includes("details") ?? !selectedDetailsError;
  const selectedRemovalBlocked = !selectedInstance || resolveServerPrimaryAction({
    instanceId: selectedInstance.id, moduleId: selectedInstance.module_id, status: selectedInstance.status,
    hasRunningProcess: instanceHasRunningProcess(selectedInstance),
    pendingRuntime: runtimeActionsByInstanceId[selectedInstance.id]
  }).deleteBlocked;
  const launchWindowPolicy = props.launchPlan?.window_policy ?? props.selectedModuleDetails?.process?.window_policy ?? "background";
  const launchHostSurface = props.launchPlan?.host_surface
    ?? props.selectedModuleDetails?.process?.host_surface
    ?? (launchWindowPolicy === "external" ? "external_window" : "managed_terminal");
  const selectedModuleId = props.selectedDetails?.summary.module_id ?? "";
  const detailTabs = buildServerDetailTabSpecs({ moduleId: selectedModuleId,
    moduleDetails: props.selectedModuleDetails, t });
  // Capability fallback must preserve navigation intent across card changes and loading.
  const activeDetailTab = detailTabs.find((tab) => tab.id === requestedDetailTab)?.disabled
    ? "runtime" : requestedDetailTab;
  const selectedDetailsId = props.selectedDetails?.summary.id ?? null;

  useEffect(() => {
    setRetainedGmInstanceId((previous) => activeDetailTab === "gm"
      ? selectedDetailsId : previous === selectedDetailsId ? previous : null);
  }, [activeDetailTab, selectedDetailsId]);

  useEffect(() => {
    if (props.section === "settings") {
      setActiveDetailTab("settings");
    }
  }, [props.section]);

  async function runRuntimeAction(instanceId: string, intent: "start" | "stop") {
    if (runtimeActionGate.current.has(instanceId)) return;
    runtimeActionGate.current.add(instanceId);
    if (intent === "start") {
      const selected = props.selectedDetails?.summary.id === instanceId;
      // Capture before settings/preflight awaits: native output may arrive before
      // the runtime tab mounts, and the previous producer may still be draining.
      runtimeStartupBoundaries.current.set(instanceId, {
        instanceId, startedAtUnixMs: Date.now(),
        previousLogPaths: collectRuntimeLogPaths(selected ? props.selectedDetails : null, selected ? props.runtime : null)
      });
    }
    props.onActivity(message(intent === "start" ? "activity.startingServer" : "activity.stoppingServer", { id: instanceId }));
    const pendingAction = pendingRuntimeActionForIntent(intent);
    setRuntimeActionsByInstanceId((current) => ({ ...current, [instanceId]: pendingAction }));

    try {
      if (intent === "start") {
        let expectedWorldStart: DstWorldStartPreview | undefined;
        const instance = props.instances.find((item) => item.id === instanceId);
        if (instance?.module_id === "dontstarve") {
          expectedWorldStart = await prepareDstWorldStart(instanceId, {
            flush: (id) => settingsSaveCoordinator.flush(id),
            preview: previewDontStarveWorldStart
          });
        } else {
          await settingsSaveCoordinator.flush(instanceId);
        }
        props.onSelectInstance(instanceId);
        setActiveDetailTab("runtime");
        props.onWorkspaceSectionChange("overview");
        await props.onStart(instanceId, expectedWorldStart);
      } else {
        let saveFailure: string | null = null;
        try {
          await settingsSaveCoordinator.flushBeforeStop(instanceId);
        } catch (error) {
          // A failed live API must be reported, but must not prevent stopping a broken server.
          saveFailure = describeError(error);
        }
        await props.onStop(instanceId);
        if (saveFailure) props.onActivity(message("settings.configuration.save.failed", { message: saveFailure }, { tone: "error" }));
      }
    } catch (error) {
      props.onActivity(error instanceof DstWorldStartError
        ? message(`dst.start.error.${error.code}`, undefined, { fallback: error.message, tone: "error" })
        : error instanceof InstanceSettingsDraftInvalidError
          ? message("dst.start.error.invalid", undefined, { tone: "error" })
        : message("dst.start.error.prepare", { message: describeError(error) }, { tone: "error" }));
    } finally {
      runtimeActionGate.current.delete(instanceId);
      runtimeStartupBoundaries.current.delete(instanceId);
      setRuntimeActionsByInstanceId((current) => {
        if (current[instanceId] !== pendingAction) {
          return current;
        }
        const { [instanceId]: _completed, ...rest } = current;
        return rest;
      });
    }
  }

  async function runInstallationAction(moduleId: string, intent: ServerInstallationIntent) {
    await installationGate.run(moduleId, intent, async (targetId, validate) => {
      setInstallationsByModuleId((current) => ({ ...current, [targetId]: intent }));
      try {
        await props.onInstallModule(targetId, validate);
      } finally {
        setInstallationsByModuleId((current) => {
          const { [targetId]: _completed, ...rest } = current;
          return rest;
        });
      }
    });
  }

  function activateDetailTab(tab: ServerDetailTabSpec) {
    setActiveDetailTab(tab.id);
    props.onWorkspaceSectionChange(tab.id === "settings" ? "settings" : "overview");
  }

  function selectDetailTab(tab: ServerDetailTabSpec) {
    if (tab.disabled) {
      return;
    }
    activateDetailTab(tab);
  }

  async function retireInstance(instanceId: string, action: ServersViewProps["onArchiveInstance"]) {
    setPendingRemovals((count) => count + 1);
    try {
      await action(instanceId);
    } finally {
      try { await archives.refresh(); }
      finally { setPendingRemovals((count) => count - 1); }
    }
  }

  const hasInstances = props.instances.length > 0;
  const hasArchives = Boolean(archives.list?.archives.length);
  // An empty normal list alone does not mean this is a new workspace.
  const showFirstServer = !hasInstances && !props.selectedInstanceId && !archives.operation && pendingRemovals === 0
    && !archives.error && !archives.notices.some((notice) => notice.tone === "error")
    && (!archives.available || (archives.list !== null && !hasArchives
      && pendingDeletions.length === 0 && archives.list.issues.length === 0));
  const showMetrics = hasInstances || hasArchives || pendingDeletions.length > 0
    || Boolean(props.selectedInstanceId) || pendingRemovals > 0 || archives.operation !== null;
  const detailScrollClassName = [
    "server-detail-scroll",
    `server-detail-scroll--${activeDetailTab}`
  ].join(" ");

  return (
    <div className={`page-grid workspace-page servers-page${!showMetrics ? " servers-page--no-metrics" : ""}`}>
      {archives.error && <ActivityNotice tone="error" action={
        <button type="button" className="secondary-button" onClick={() => void archives.refresh()} disabled={archives.loading}>{t("common.refresh")}</button>
      }>{archives.error}</ActivityNotice>}
      {archives.notices.map((notice, index) => <ActivityNotice key={`archive-notice-${index}`} tone={notice.tone}>{notice.text}</ActivityNotice>)}
      {archives.list?.issues.map((issue, index) => <ActivityNotice key={`archive-issue-${index}`} tone="warning">{issue}</ActivityNotice>)}

      {showMetrics ? <section className="workspace-metrics">
        {metric(t("servers.metrics.total"), String(props.instances.length), t("servers.metrics.running", { count: runningCount }), "server")}
        {metric(t("servers.metrics.autostart"), String(autostartCount), t("servers.metrics.autostartMeta"), "settings")}
        {metric(
          t("servers.metrics.currentStatus"),
          props.selectedDetails ? formatInstanceStatus(props.selectedDetails.summary.status, t)
            : props.selectedInstanceId ? t(selectedDetailsError || !selectedDetailsLoading
              ? "servers.unavailable.status" : "servers.loading.details") : t("common.notSelected"),
          props.selectedDetails?.summary.name ?? props.instances.find(instance => instance.id === props.selectedInstanceId)?.name
            ?? t("servers.metrics.currentStatusMeta"),
          "monitor"
        )}
        {metric(
          t("servers.metrics.registeredPorts"),
          String(selectedPorts),
          props.selectedDetails ? t("servers.metrics.registeredPortsReady") : t("servers.metrics.registeredPortsPending"),
          "shield"
        )}
      </section> : null}

      <section className="workspace-section-grid server-layout-grid">
        <section className="server-list-panel" aria-label={t("servers.list.title")}>
          <div className="panel-head server-list-controls">
            <div className="server-list-search-row">
            <div className="server-list-search-field">
              <ShellIcon name="search" className="server-list-search-icon" />
              <input
                type="search"
                className="text-input server-list-search"
                value={searchText}
                onChange={(event) => setSearchText(event.target.value)}
                placeholder={t("servers.list.searchPlaceholder", undefined, "Search instances")}
                aria-label={t("servers.list.searchPlaceholder", undefined, "Search instances")}
              />
            </div>
            <div className="server-list-mode-toggle" role="group" aria-label={t("servers.list.mode")}>
              {(["normal", "archived"] as const).map((mode) => <button key={mode} type="button" data-server-list-mode={mode}
                aria-pressed={listMode === mode} disabled={mode === "archived" && !archives.available}
                title={!archives.available && mode === "archived" ? t("storage.hostOnly") : undefined}
                onClick={() => { setListMode(mode); setSelectedDeletionId(null); if (archives.available) void archives.refresh(); }}>
                {t(`servers.list.${mode}`)}
              </button>)}
            </div>
            </div>
            <div className="server-list-filters">
              <span className="server-list-result-count" role="status">
                {t("servers.list.results", {
                  count: listMode === "archived" ? visibleArchives.length : visibleInstances.filter((instance) => !pendingIds.has(instance.id)).length + visibleDeletions.length,
                  total: listMode === "archived" ? archives.list?.archives.length ?? 0 : props.instances.filter((instance) => !pendingIds.has(instance.id)).length + pendingDeletions.length
                }, "{count} of {total} instances")}
              </span>
              {listMode === "normal" ? <ServerListRunningFilter checked={onlyRunning} onChange={setOnlyRunning} />
                : <button type="button" className="secondary-button server-archives-refresh" disabled={storageBusy}
                  onClick={() => void archives.refresh()} aria-label={t("common.refresh")} title={t("common.refresh")}><ShellIcon name="refresh" /></button>}
            </div>
          </div>

          <div className="table-list">
            {listMode === "archived" ? <>
              {archives.loading && <p className="server-list-notice" role="status">{t("storage.loadingArchives")}</p>}
              {!archives.loading && !archives.error && visibleArchives.length === 0 && <p className="server-list-notice">{t(normalizedSearchText ? "servers.archives.noMatches" : "storage.emptyArchives")}</p>}
              {visibleArchives.map((archive) => <ArchivedServerCard key={archive.archive_id} archive={archive}
                active={archive.archive_id === selectedArchiveId} disabled={storageBusy}
                restoring={archives.operation?.id === archive.archive_id && archives.operation.kind === "restore"}
                onSelect={() => setSelectedArchiveId((current) => current === archive.archive_id ? null : archive.archive_id)}
                onRestore={() => archives.restore(archive)} onPurge={() => archives.purge(archive)}
                onOpenLocalPath={props.onOpenLocalPath} />)}
            </> : <>
            {visibleDeletions.map((entry) => <Fragment key={entry.operation_id}><DeletionErrorCard entry={entry}
              active={entry.operation_id === selectedDeletionId} disabled={storageBusy}
              onSelect={() => setSelectedDeletionId((current) => current === entry.operation_id ? null : entry.operation_id)}
              onRetry={() => archives.retry(entry)} />
              {entry.operation_id === selectedDeletionId && <ServerArchiveDetails deletion={entry} />}
            </Fragment>)}
            {visibleInstances.filter((instance) => !pendingIds.has(instance.id)).map((instance) => (
              <ServerListCard
                key={instance.id}
                instance={instance}
                retirement={props.instanceRetirements?.get(instance.id)}
                connection={listConnections.connections[instance.id]}
                connectionFailed={listConnections.failed}
                bindAddressCandidates={props.bindAddressCandidates}
                onConnectionRead={listConnections.acceptConnection}
                onActivity={props.onActivity}
                active={!selectedDeletion && instance.id === props.selectedInstanceId}
                t={t}
                onSelectInstance={(id) => { setSelectedDeletionId(null); props.onSelectInstance(id); }}
                onArchiveInstance={(id) => retireInstance(id, props.onArchiveInstance)}
                onDeleteInstance={(id) => retireInstance(id, props.onDeleteInstance)}
                storageBusy={storageBusy}
                onStart={(instanceId) => void runRuntimeAction(instanceId, "start")}
                onStop={(instanceId) => void runRuntimeAction(instanceId, "stop")}
                pendingAction={runtimeActionsByInstanceId[instance.id] ?? null}
                installation={props.moduleInstallations[instance.module_id]}
                launchPlan={props.instanceLaunchPlans[instance.id]}
                launchFailed={props.instanceLaunchFailures[instance.id]}
                pendingInstallation={installationsByModuleId[instance.module_id]}
                jobs={props.jobs}
                onInstall={(moduleId, intent) => void runInstallationAction(moduleId, intent)}
                onOpenModuleLibrary={props.onOpenModuleLibrary}
              />
            ))}
            {!hasInstances && pendingDeletions.length === 0 && <p className="server-list-notice">{t("servers.list.emptyNormal")}</p>}
            </>}
          </div>
        </section>

        <section className="server-detail-panel">
          {listMode === "archived" ? selectedArchive ? (
            <ArchivedInstanceWorkspace key={selectedArchive.archive_id} archive={selectedArchive}
              requestedTab={requestedArchiveDetailTab} onTabChange={setArchiveDetailTab} />
          ) : (
            <div className="detail-stack detail-stack--server">
              <div className="server-workspace-empty">
                <div className="server-workspace-empty-symbol"><ShellIcon name="inbox" className="server-workspace-empty-icon" /></div>
                <h3>{t("servers.archives.selectTitle")}</h3>
                <p>{t("servers.archives.selectDetails")}</p>
              </div>
            </div>
          ) : selectedRetirement ? (
            <div className="detail-stack detail-stack--server" data-instance-retirement={props.selectedInstanceId}>
              <div className="server-workspace-empty" role="status" aria-busy="true">
                <div className="server-workspace-empty-symbol"><ShellIcon name="loader" className="server-workspace-empty-icon" /></div>
                <p>{t(selectedRetirement === "delete" ? "activity.deletingInstance" : "activity.archivingInstance", {
                  id: props.instances.find((instance) => instance.id === props.selectedInstanceId)?.name ?? props.selectedInstanceId ?? ""
                })}</p>
              </div>
            </div>
          ) : props.selectedDetails ? (
            <div className="detail-stack detail-stack--server">
              <div className="server-detail-subheader">
                <ServerDetailTabs tabs={detailTabs} activeTab={activeDetailTab}
                  panelId={detailPanelId}
                  label={t("servers.tabs.detailSectionsAria", undefined, "Instance detail sections")}
                  onSelect={selectDetailTab} />
              </div>

              <div key={props.selectedDetails.summary.id} className={detailScrollClassName}
                id={detailPanelId} role="tabpanel" aria-labelledby={`${detailPanelId}-${activeDetailTab}`}>
              {activeDetailTab === "settings" && props.selectedDetails ? (
                <ConfigurationWorkspace
                  details={props.selectedDetails}
                  moduleDetails={props.selectedModuleDetails}
                  moduleDetailsError={props.selectedModuleDetailsError}
                  onRetryModuleDetails={props.onResumeAutoRefresh}
                  bindAddressCandidates={props.bindAddressCandidates}
                  runtime={props.runtime}
                  launchPlan={props.launchPlan}
                  launchPlanError={props.launchPlanError}
                  onSave={props.onSaveSettings}
                />
              ) : null}

              <ServerMaintenanceWorkspace active={activeDetailTab === "maintenance"}
                details={props.selectedDetails} moduleDetails={props.selectedModuleDetails}
                locale={locale} t={t} selectedBackups={props.selectedBackups}
                panelLoadState={props.panelLoadState} jobs={props.jobs} runtime={props.runtime}
                aiSettings={props.aiSettings} assistantCanRun={props.assistantCanRun}
                onResumeAutoRefresh={props.onResumeAutoRefresh}
                onCreateBackup={props.onCreateBackup} onRestoreBackup={props.onRestoreBackup}
                onRenameBackup={props.onRenameBackup} onDeleteBackup={props.onDeleteBackup}
                onSaveSettings={props.onSaveSettings} onSaveAutostart={props.onSaveAutostart}
                onPickDirectory={props.onPickDirectory} onImportDontStarveWorldData={props.onImportDontStarveWorldData}
                onOpenLocalPath={props.onOpenLocalPath} />

              {activeDetailTab === "mods" && props.selectedDetails ? (
                <ModWorkbench
                  key={props.selectedDetails.summary.id}
                  details={props.selectedDetails}
                  moduleDetails={props.selectedModuleDetails}
                  jobs={props.jobs}
                  launchPlan={props.launchPlan}
                  onSaveSettings={props.onSaveSettings}
                />
              ) : null}

              {activeDetailTab === "runtime" ? (
                <RuntimeSurfaceWorkbench
                  details={props.selectedDetails}
                  moduleDetails={props.selectedModuleDetails}
                  runtime={props.runtime}
                  runtimeWindows={props.runtimeWindows}
                  panelLoadState={props.panelLoadState}
                  onRetryReads={props.onResumeAutoRefresh}
                  startupPending={runtimeActionsByInstanceId[props.selectedDetails.summary.id] === "starting"}
                  startupBoundary={runtimeStartupBoundaries.current.get(props.selectedDetails.summary.id)}
                  launchHostSurface={launchHostSurface}
                  onSendRuntimeCommand={props.onSendRuntimeCommand}
                  onSuppressRuntimeWindows={props.onSuppressRuntimeWindows}
                />
              ) : null}

              {activeDetailTab === "players" && props.selectedDetails ? (
                <PlayerCenterWorkbench
                  details={props.selectedDetails}
                  moduleDetails={props.selectedModuleDetails}
                  runtime={props.runtime}
                  onSaveSettings={props.onSaveSettings}
                  onApplyPlayerAccessMutation={props.onApplyPlayerAccessMutation}
                  onExecutePlayerAction={props.onExecutePlayerAction}
                  onOpenSettings={() => {
                    setActiveDetailTab("settings");
                    props.onWorkspaceSectionChange("settings");
                  }}
                />
              ) : null}

              {props.selectedDetails && (activeDetailTab === "gm" || retainedGmInstanceId === props.selectedDetails.summary.id) ? (
                <div hidden={activeDetailTab !== "gm"}>
                  <GMToolsWorkbench details={props.selectedDetails} />
                </div>
              ) : null}
              </div>
            </div>
          ) : props.selectedInstanceId ? (
            <InstanceUnavailablePanel key={props.selectedInstanceId} instanceId={props.selectedInstanceId}
              error={selectedDetailsError} loading={selectedDetailsLoading}
              deleteDisabled={storageBusy || selectedRemovalBlocked}
              onRetry={props.onResumeAutoRefresh} onDelete={(id) => retireInstance(id, props.onDeleteInstance)}
              onOpenLocalPath={props.onOpenLocalPath} />
          ) : (
            <div className="detail-stack detail-stack--server">
            <div className="server-workspace-empty">
              <div className="server-workspace-empty-symbol">
                <ShellIcon name="server" className="server-workspace-empty-icon" />
              </div>
              <h3>{hasInstances
                ? t("servers.details.selectTitle", undefined, "Select an instance")
                : t(showFirstServer ? "servers.list.noInstances" : "servers.details.noActiveTitle")}</h3>
              <p>{hasInstances
                ? t("servers.details.empty")
                : t(showFirstServer ? "servers.list.noInstancesBody"
                  : hasArchives ? "servers.details.archivedBody" : "servers.details.noActiveBody")}</p>
              {showFirstServer ? (
                <button type="button" className="primary-button" onClick={props.onCreateInstance}>
                  {t("library.detail.createServer")}
                </button>
              ) : null}
            </div>
            </div>
          )}
        </section>
      </section>
    </div>
  );
}
