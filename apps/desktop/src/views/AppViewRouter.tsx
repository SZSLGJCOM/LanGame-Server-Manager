import type { ReactNode } from "react";
import { createDeferredModule } from "../deferred-module";
import { useDeferredModule } from "../hooks/useDeferredModule";
import type { LibraryPageMode, RuntimeRefreshIssue } from "../app-state";
import type { UiMessage } from "../app-ui";
import type { AiSettings } from "../ai-settings";
import type { ServerModuleInstallation } from "../server-primary-action";
import type { SteamInstallationAccountContext } from "../steam-installation-account";
import type { InstancePanelLoadState } from "../instance-panel-loader";
import type { InstanceRetirementKind } from "../hooks/useInstanceRetirement";
import type {
  AppPathSettingsInput,
  BackgroundJob,
  BindAddressCandidate,
  BootstrapResponse,
  CreateInstanceInput,
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
  LaunchPlan,
  ModuleDetails,
  ModuleSummary,
  RuntimeWindowSnapshot,
  RuntimeCommandDispatchOptions,
  SaveInstanceSettingsOptions,
  ServerWorkspaceSection,
  SteamCmdStatus,
  SteamCmdPrepareSnapshot,
  UpdateInstanceInput,
  ViewKey
} from "../types";
import { ViewLoadingState } from "./ViewLoadingState";

const libraryViewModule = createDeferredModule(
  () => import("./LibraryView").then((module) => module.LibraryView),
  () => import("./LibraryView?module-retry").then((module) => module.LibraryView)
);
const serverWorkspaceModule = createDeferredModule(
  () => import("./ServerWorkspaceView").then((module) => module.ServerWorkspaceView),
  () => import("./ServerWorkspaceView?module-retry").then((module) => module.ServerWorkspaceView)
);
const systemViewModule = createDeferredModule(
  () => import("./SystemView").then((module) => module.SystemView),
  () => import("./SystemView?module-retry").then((module) => module.SystemView)
);

interface AppViewRouterProps {
  activeView: ViewKey;
  aiSettings: AiSettings;
  assistantCanRun: boolean;
  bindAddressCandidates: BindAddressCandidate[];
  bootstrap: BootstrapResponse;
  instances: InstanceSummary[];
  moduleInstallations: Partial<Record<string, ServerModuleInstallation>>;
  instanceLaunchPlans: Partial<Record<string, LaunchPlan>>;
  instanceLaunchFailures: Partial<Record<string, true>>;
  jobs: BackgroundJob[];
  libraryCatalogFocusId: string | null;
  libraryCatalogScrollLeft: number;
  libraryPage: LibraryPageMode;
  librarySearch: string;
  modules: ModuleSummary[];
  runtimeRefreshIssue: RuntimeRefreshIssue | null;
  systemRefreshing?: boolean;
  onActivity: (value: UiMessage) => void;
  selectedInstanceDetails: InstanceDetails | null;
  selectedPanelLoadState?: InstancePanelLoadState | null;
  selectedBackups: InstanceBackupResult[];
  selectedInstanceId: string | null;
  instanceRetirements?: ReadonlyMap<string, InstanceRetirementKind>;
  selectedInstanceModuleDetails: ModuleDetails | null;
  selectedInstanceModuleError: string | null;
  selectedLaunchPlan: LaunchPlan | null;
  selectedLaunchPlanError: string | null;
  selectedModuleDetails: ModuleDetails | null;
  selectedModuleId: string | null;
  selectedRuntime: InstanceRuntimeOverview | null;
  selectedRuntimeWindows: RuntimeWindowSnapshot | null;
  serverWorkspaceSection: ServerWorkspaceSection;
  onServerWorkspaceSectionChange: (section: ServerWorkspaceSection) => void;
  steamCmdBusy: boolean;
  steamCmdProgress: SteamCmdPrepareSnapshot | null;
  steamCmdMessage: string;
  steamCmdStatus: SteamCmdStatus | null;
  onCreateBackup: (instanceId: string) => void;
  onDeleteInstance: (instanceId: string) => void | Promise<void>;
  onArchiveInstance: (instanceId: string) => void | Promise<void>;
  onCreateServer: (input: CreateInstanceInput) => Promise<void>;
  creatingModuleIds: ReadonlySet<string>;
  creationStartedAtByModule: ReadonlyMap<string, number>;
  onEnsureSteamCmd: () => void;
  onUninstallSteamCmd: () => void;
  onInstallModule: (moduleId: string, validate: boolean, account?: SteamInstallationAccountContext) => Promise<void>;
  onLibraryCatalogFocusChange: (moduleId: string | null) => void;
  onLibraryCatalogScrollLeftChange: (scrollLeft: number) => void;
  onUninstallModule: (moduleId: string) => void | Promise<void>;
  onOpenLibraryCatalog: () => void;
  onOpenLibraryDetail: (moduleId: string) => void;
  onOpenInstance: (instanceId: string) => void;
  onOpenLocalPath: (path: string) => void;
  onPickDirectory: (currentPath?: string | null) => Promise<string | null>;
  onImportDontStarveWorldData: (instanceId: string, sourcePath: string) => Promise<DstWorldImportResult>;
  onResumeRuntimeAutoRefresh: () => void;
  onRestoreBackup: (instanceId: string, backupId: string) => void;
  onRenameBackup: (instanceId: string, backup: InstanceBackupResult, displayName: string) => Promise<boolean>;
  onDeleteBackup: (instanceId: string, backup: InstanceBackupResult) => void | Promise<void>;
  onSaveServerSettings: (input: UpdateInstanceInput, options?: SaveInstanceSettingsOptions) => Promise<InstanceDetails | undefined>;
  onSaveServerAutostart: (instanceId: string, autostart: boolean) => Promise<void>;
  onApplyPlayerAccessMutation: (
    input: InstancePlayerAccessMutationInput
  ) => Promise<InstancePlayerAccessMutationResult>;
  onExecutePlayerAction?: (input: ExecuteInstancePlayerActionInput) => Promise<ExecuteInstancePlayerActionResult>;
  onStorageChanged: () => Promise<void>;
  onSaveAppSettings: (settings: AppPathSettingsInput) => void | Promise<void>;
  onSearchChange: (value: string) => void;
  onSendRuntimeCommand: (
    instanceId: string,
    command: string,
    processKey?: string | null,
    options?: RuntimeCommandDispatchOptions
  ) => Promise<InstanceRuntimeCommandResult | null>;
  onSuppressRuntimeWindows: (instanceId: string) => Promise<void>;
  onSelectInstance: (instanceId: string) => void;
  onStartServer: (instanceId: string, expectedWorldStart?: DstWorldStartPreview) => void | Promise<void>;
  onStopServer: (instanceId: string) => void | Promise<void>;
}

export function AppViewRouter(props: AppViewRouterProps) {
  const library = useDeferredModule(libraryViewModule, props.activeView === "library");
  const servers = useDeferredModule(serverWorkspaceModule, props.activeView === "servers");
  const system = useDeferredModule(systemViewModule, props.activeView === "system");
  const selected = props.activeView === "library" ? library : props.activeView === "servers" ? servers : system;
  if (selected.status !== "ready") {
    return <ViewLoadingState error={selected.error} canRetry={selected.canRetry} onRetry={selected.retry} />;
  }
  const LibraryView = library.value;
  const ServerWorkspaceView = servers.value;
  const SystemView = system.value;
  let content: ReactNode = null;

  switch (props.activeView) {
    case "system":
      if (!SystemView) break;
      content = (
        <SystemView
          snapshot={props.bootstrap.state.snapshot}
          systemRefreshing={props.systemRefreshing}
          instances={props.instances}
          bindAddressCandidates={props.bindAddressCandidates}
          appSettings={props.bootstrap.state.settings}
          steamCmdStatus={props.steamCmdStatus}
          steamCmdBusy={props.steamCmdBusy}
          steamCmdProgress={props.steamCmdProgress}
          steamCmdMessage={props.steamCmdMessage}
          onOpenInstance={props.onOpenInstance}
          onPickDirectory={props.onPickDirectory}
          onSaveAppSettings={props.onSaveAppSettings}
          onEnsureSteamCmd={props.onEnsureSteamCmd}
          onUninstallSteamCmd={props.onUninstallSteamCmd}
        />
      );
      break;
    case "library":
      if (!LibraryView) break;
      content = (
        <LibraryView
          mode={props.libraryPage}
          steamCmdStatus={props.steamCmdStatus}
          steamCmdBusy={props.steamCmdBusy}
          modules={props.modules}
          instances={props.instances}
          selectedModuleId={props.selectedModuleId}
          selectedModuleDetails={props.selectedModuleDetails}
          search={props.librarySearch}
          catalogFocusId={props.libraryCatalogFocusId}
          catalogScrollLeft={props.libraryCatalogScrollLeft}
          jobs={props.jobs}
          onSearchChange={props.onSearchChange}
          onCatalogFocusChange={props.onLibraryCatalogFocusChange}
          onCatalogScrollLeftChange={props.onLibraryCatalogScrollLeftChange}
          onOpenModule={props.onOpenLibraryDetail}
          onBackToCatalog={props.onOpenLibraryCatalog}
          onInstall={props.onInstallModule}
          onUninstall={props.onUninstallModule}
          onCreateServer={props.onCreateServer}
          creatingModuleIds={props.creatingModuleIds}
          creationStartedAtByModule={props.creationStartedAtByModule}
        />
      );
      break;
    case "servers":
      if (!ServerWorkspaceView) break;
      content = (
        <ServerWorkspaceView
          aiSettings={props.aiSettings}
          moduleInstallations={props.moduleInstallations}
          instanceLaunchPlans={props.instanceLaunchPlans}
          instanceLaunchFailures={props.instanceLaunchFailures}
          onInstallModule={props.onInstallModule}
          onOpenModuleLibrary={props.onOpenLibraryDetail}
          onCreateInstance={props.onOpenLibraryCatalog}
          assistantCanRun={props.assistantCanRun}
          bindAddressCandidates={props.bindAddressCandidates}
          instances={props.instances}
          jobs={props.jobs}
          selectedInstanceId={props.selectedInstanceId}
          instanceRetirements={props.instanceRetirements}
          selectedDetails={props.selectedInstanceDetails}
          panelLoadState={props.selectedPanelLoadState}
          selectedBackups={props.selectedBackups}
          selectedModuleDetails={props.selectedInstanceModuleDetails}
          selectedModuleDetailsError={props.selectedInstanceModuleError}
          runtime={props.selectedRuntime}
          runtimeWindows={props.selectedRuntimeWindows}
          launchPlan={props.selectedLaunchPlan}
          launchPlanError={props.selectedLaunchPlanError}
          section={props.serverWorkspaceSection}
          onWorkspaceSectionChange={props.onServerWorkspaceSectionChange}
          refreshIssue={props.runtimeRefreshIssue}
          onActivity={props.onActivity}
          onResumeAutoRefresh={props.onResumeRuntimeAutoRefresh}
          onSelectInstance={props.onSelectInstance}
          onStartServer={props.onStartServer}
          onStopServer={props.onStopServer}
          onPickDirectory={props.onPickDirectory}
          onImportDontStarveWorldData={props.onImportDontStarveWorldData}
          onOpenLocalPath={props.onOpenLocalPath}
          onSendRuntimeCommand={props.onSendRuntimeCommand}
          onSuppressRuntimeWindows={props.onSuppressRuntimeWindows}
          onCreateBackup={props.onCreateBackup}
          onArchiveInstance={props.onArchiveInstance}
          onArchivesChanged={props.onStorageChanged}
          onDeleteInstance={props.onDeleteInstance}
          onRestoreBackup={props.onRestoreBackup}
          onRenameBackup={props.onRenameBackup}
          onDeleteBackup={props.onDeleteBackup}
          onSaveSettings={props.onSaveServerSettings}
          onSaveAutostart={props.onSaveServerAutostart}
          onApplyPlayerAccessMutation={props.onApplyPlayerAccessMutation}
          onExecutePlayerAction={props.onExecutePlayerAction}
        />
      );
      break;
    default:
      content = null;
      break;
  }

  return content;
}
