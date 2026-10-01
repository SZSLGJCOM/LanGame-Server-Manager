import type { RuntimeRefreshIssue } from "../app-state";
import type { UiMessage } from "../app-ui";
import type { InstancePanelLoadState } from "../instance-panel-loader";
import type { InstanceRetirementKind } from "../hooks/useInstanceRetirement";
import type { AiSettings } from "../ai-settings";
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
  LaunchPlan,
  ModuleDetails,
  RuntimeWindowSnapshot,
  RuntimeCommandDispatchOptions,
  SaveInstanceSettingsOptions,
  ServerWorkspaceSection,
  UpdateInstanceInput,
  BackgroundJob
} from "../types";
import { ServersView } from "./ServersView";
import type { ServerModuleInstallation } from "../server-primary-action";

interface ServerWorkspaceViewProps {
  aiSettings: AiSettings;
  assistantCanRun: boolean;
  bindAddressCandidates: BindAddressCandidate[];
  instances: InstanceSummary[];
  moduleInstallations: Partial<Record<string, ServerModuleInstallation>>;
  instanceLaunchPlans: Partial<Record<string, LaunchPlan>>;
  instanceLaunchFailures: Partial<Record<string, true>>;
  jobs?: BackgroundJob[];
  launchPlan: LaunchPlan | null;
  launchPlanError: string | null;
  refreshIssue: RuntimeRefreshIssue | null;
  onActivity: (value: UiMessage) => void;
  runtime: InstanceRuntimeOverview | null;
  runtimeWindows: RuntimeWindowSnapshot | null;
  section: ServerWorkspaceSection;
  onWorkspaceSectionChange: (section: ServerWorkspaceSection) => void;
  selectedDetails: InstanceDetails | null;
  panelLoadState?: InstancePanelLoadState | null;
  selectedBackups: InstanceBackupResult[];
  selectedInstanceId: string | null;
  instanceRetirements?: ReadonlyMap<string, InstanceRetirementKind>;
  selectedModuleDetails: ModuleDetails | null;
  selectedModuleDetailsError: string | null;
  onCreateInstance: () => void;
  onCreateBackup: (instanceId: string) => void;
  onDeleteInstance: (instanceId: string) => void | Promise<void>;
  onArchiveInstance: (instanceId: string) => void | Promise<void>;
  onArchivesChanged: () => Promise<void>;
  onOpenLocalPath: (path: string) => void;
  onPickDirectory: (currentPath?: string | null) => Promise<string | null>;
  onImportDontStarveWorldData: (instanceId: string, sourcePath: string) => Promise<DstWorldImportResult>;
  onResumeAutoRefresh: () => void;
  onRestoreBackup: (instanceId: string, backupId: string) => void;
  onRenameBackup: (instanceId: string, backup: InstanceBackupResult, displayName: string) => Promise<boolean>;
  onDeleteBackup: (instanceId: string, backup: InstanceBackupResult) => void | Promise<void>;
  onSaveSettings: (input: UpdateInstanceInput, options?: SaveInstanceSettingsOptions) => Promise<InstanceDetails | undefined>;
  onSaveAutostart: (instanceId: string, autostart: boolean) => Promise<void>;
  onApplyPlayerAccessMutation: (
    input: InstancePlayerAccessMutationInput
  ) => Promise<InstancePlayerAccessMutationResult>;
  onExecutePlayerAction?: (input: ExecuteInstancePlayerActionInput) => Promise<ExecuteInstancePlayerActionResult>;
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
  onInstallModule: (moduleId: string, validate: boolean) => Promise<void>;
  onOpenModuleLibrary: (moduleId: string) => void;
}

export function ServerWorkspaceView(props: ServerWorkspaceViewProps) {
  return (
    <ServersView
      aiSettings={props.aiSettings}
      assistantCanRun={props.assistantCanRun}
      bindAddressCandidates={props.bindAddressCandidates}
      instances={props.instances}
      moduleInstallations={props.moduleInstallations}
      instanceLaunchPlans={props.instanceLaunchPlans}
      instanceLaunchFailures={props.instanceLaunchFailures}
      jobs={props.jobs}
      section={props.section}
      onWorkspaceSectionChange={props.onWorkspaceSectionChange}
      selectedInstanceId={props.selectedInstanceId}
      instanceRetirements={props.instanceRetirements}
      selectedDetails={props.selectedDetails}
      panelLoadState={props.panelLoadState}
      selectedBackups={props.selectedBackups}
      selectedModuleDetails={props.selectedModuleDetails}
      selectedModuleDetailsError={props.selectedModuleDetailsError}
      runtime={props.runtime}
      runtimeWindows={props.runtimeWindows}
      launchPlan={props.launchPlan}
      launchPlanError={props.launchPlanError}
      refreshIssue={props.refreshIssue}
      onActivity={props.onActivity}
      onResumeAutoRefresh={props.onResumeAutoRefresh}
      onSelectInstance={props.onSelectInstance}
      onStart={props.onStartServer}
      onStop={props.onStopServer}
      onInstallModule={props.onInstallModule}
      onOpenModuleLibrary={props.onOpenModuleLibrary}
      onPickDirectory={props.onPickDirectory}
      onImportDontStarveWorldData={props.onImportDontStarveWorldData}
      onOpenLocalPath={props.onOpenLocalPath}
      onSendRuntimeCommand={props.onSendRuntimeCommand}
      onSuppressRuntimeWindows={props.onSuppressRuntimeWindows}
      onCreateInstance={props.onCreateInstance}
      onCreateBackup={props.onCreateBackup}
      onArchiveInstance={props.onArchiveInstance}
      onArchivesChanged={props.onArchivesChanged}
      onDeleteInstance={props.onDeleteInstance}
      onRestoreBackup={props.onRestoreBackup}
      onRenameBackup={props.onRenameBackup}
      onDeleteBackup={props.onDeleteBackup}
      onSaveSettings={props.onSaveSettings}
      onSaveAutostart={props.onSaveAutostart}
      onApplyPlayerAccessMutation={props.onApplyPlayerAccessMutation}
      onExecutePlayerAction={props.onExecutePlayerAction}
    />
  );
}
