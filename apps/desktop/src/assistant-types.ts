import type { LibraryPageMode, RuntimeRefreshIssue } from "./app-state";
import type { AiSettings } from "./ai-settings";
import type {
  BootstrapResponse,
  InstanceDetails,
  InstanceRuntimeOverview,
  LaunchPlan,
  LogTailSnapshot,
  ModuleDetails,
  ServerWorkspaceSection,
  SteamCmdStatus,
  ViewKey
} from "./types";

export type AssistantSeverity = "info" | "warning" | "critical";

export type AssistantActionId =
  | "ensure-storage"
  | "ensure-steamcmd"
  | "resume-runtime-refresh"
  | "view-system"
  | "open-ai-settings"
  | "view-library"
  | "view-servers"
  | "refresh-launch-preview";

export interface AssistantAction {
  id: AssistantActionId;
  label: string;
}

export interface AssistantIssue {
  id: string;
  severity: AssistantSeverity;
  title: string;
  detail: string;
  action?: AssistantAction | null;
}

export interface AssistantPromptCard {
  id: string;
  label: string;
  preview: string;
  prompt: string;
  payload: string;
}

export interface AssistantCapsuleModel {
  panelTitle: string;
  tone: AssistantSeverity;
}

export interface AssistantBuildInput {
  aiSettings: AiSettings;
  locale: string;
  activeJobsCount: number;
  activeView: ViewKey;
  bootstrap: BootstrapResponse;
  storageReady: boolean;
  libraryPage: LibraryPageMode;
  overlayNames: string[];
  runtimeAutoRefreshPaused: boolean;
  runtimeRefreshIssue: RuntimeRefreshIssue | null;
  selectedInstanceDetails: InstanceDetails | null;
  selectedInstanceId: string | null;
  selectedModuleId: string | null;
  selectedInstanceModuleDetails: ModuleDetails | null;
  selectedLaunchPlan: LaunchPlan | null;
  selectedLaunchPlanError: string | null;
  selectedLogDocument: LogTailSnapshot | null;
  selectedModuleDetails: ModuleDetails | null;
  selectedRuntime: InstanceRuntimeOverview | null;
  serverWorkspaceSection: ServerWorkspaceSection;
  steamCmdStatus: SteamCmdStatus | null;
}
