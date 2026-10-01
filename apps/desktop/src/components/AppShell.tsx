import { useState, type ReactNode } from "react";
import type { AiSettings, PersistAiSettings } from "../ai-settings";
import type { AssistantConversationSummary } from "../assistant-conversations";
import type {
  AssistantActionId,
  AssistantBuildInput,
  AssistantCapsuleModel,
  AssistantPromptCard
} from "../assistant-types";
import type {
  AppUpdateState,
  BackgroundJob,
  AssistantChatMessage,
  AssistantExecutionState,
  SteamCmdPrepareSnapshot,
  ThemeMode,
  ViewKey
} from "../types";
import { useI18n } from "../i18n";
import { AppHeader } from "./AppHeader";
import { SteamCmdActivity } from "./SteamCmdActivity";
import { InstallationActivity } from "./InstallationActivity";
import { getLocalizedModuleDisplayName } from "../store-media";
import { selectActiveInstallationJob } from "../installation-job";
import type { UiMessage } from "../app-ui";
import type { RuntimeRefreshIssue } from "../app-state";
import { AutoRefreshStatus } from "./AutoRefreshStatus";
import { ActivityNoticeTarget } from "./ActivityNotice";
import { AppUpdateActivity } from "./AppUpdateActivity";
import { AppUpdatePrompt } from "./AppUpdatePrompt";
import { appUpdateCopy } from "./app-update-copy";
import { appUpdateReleaseUrl } from "../app-update-model";
import { openExternalUrl } from "../api";
import { ActivityBar, type ActivityBarEntry } from "./ActivityBar";
import { createActivityNoticeStore } from "./activity-notice-store";

interface AppShellProps {
  activeView: ViewKey;
  activityText: string;
  activityTone?: UiMessage["tone"];
  runtimeRefreshIssue: RuntimeRefreshIssue | null;
  runtimeAutoRefreshPaused: boolean;
  runtimePollIntervalMs: number;
  runtimeRefreshFailureLimit: number;
  onResumeRuntimeAutoRefresh: () => void;
  steamCmdProgress: SteamCmdPrepareSnapshot | null;
  steamCmdMessage: string;
  steamCmdStopPending: boolean;
  steamCmdStopError: string | null;
  installationStopPendingIds: string[];
  installationStopErrors: Record<string, string>;
  onCancelSteamCmd: (operationId: string) => void;
  onCancelInstallation: (jobId: string) => void;
  jobs: BackgroundJob[];
  aiSettings: AiSettings;
  appUpdateState: AppUpdateState;
  appUpdatesEnabled: boolean;
  assistant: AssistantCapsuleModel;
  assistantDraft: string;
  assistantInput: AssistantBuildInput;
  assistantExecution: AssistantExecutionState;
  assistantConfirmationOpen?: boolean;
  assistantMessages: AssistantChatMessage[];
  assistantConversations: AssistantConversationSummary[];
  assistantActiveConversationId: string | null;
  children: ReactNode;
  serverCount: number | null;
  theme: ThemeMode;
  onAssistantAction: (actionId: AssistantActionId) => void | Promise<void>;
  onAssistantDeleteConversation: (conversationId: string) => void;
  onAssistantDraftChange: (value: string) => void;
  onAssistantNewConversation: () => void;
  onAssistantSelectConversation: (conversationId: string) => void;
  onAssistantRunPrompt: (prompt: AssistantPromptCard) => void | Promise<void>;
  assistantRecoveryPending?: boolean;
  assistantContinuation?: import("../types").AssistantContinuation | null;
  onAssistantResume?: () => void | Promise<void>;
  onAssistantStop?: () => void | Promise<void>;
  onAssistantSendMessage: (message: string) => void | Promise<void>;
  onCheckAppUpdate: () => void;
  onClearAiSecret: PersistAiSettings;
  onInstallAppUpdate: () => void;
  onSaveAiSettings: PersistAiSettings;
  onSelectView: (view: ViewKey) => void;
  onThemeChange: (theme: ThemeMode) => void;
}

export function AppShell(props: AppShellProps) {
  const { t, locale } = useI18n();
  const [noticeStore] = useState(createActivityNoticeStore);
  const [updateRetryRequest, setUpdateRetryRequest] = useState(0);
  async function downloadAppInstaller() {
    const url = appUpdateReleaseUrl(props.appUpdateState.availableVersion);
    if (!props.appUpdatesEnabled || !url) throw new Error(appUpdateCopy(locale).releaseUnavailable);
    await openExternalUrl(url);
  }
  function retryAppUpdate() {
    setUpdateRetryRequest((current) => current + 1);
    props.onCheckAppUpdate();
  }
  const showSteamCmdProgress = props.steamCmdProgress?.active
    || ((props.steamCmdProgress?.error || props.steamCmdProgress?.cancelled) && props.activityText === props.steamCmdMessage);
  const installationJob = selectActiveInstallationJob(props.jobs);
  const showInstallationProgress = installationJob && (!props.steamCmdProgress?.active
    || (props.steamCmdProgress.phase === "queued" && installationJob.install_progress?.phase !== "queued"));
  const showActivityFeedback = Boolean(props.activityTone) || (!showInstallationProgress && !showSteamCmdProgress);
  const showRefreshFeedback = Boolean(props.runtimeRefreshIssue || props.runtimeAutoRefreshPaused);
  const activityEntries: ActivityBarEntry[] = [];
  if (showActivityFeedback) activityEntries.push({
    id: `activity:${props.activityText}`,
    content: <div className="shell-activity-feedback">
      <span className={`shell-activity-text${props.activityTone ? ` is-${props.activityTone}` : ""}`}
        role={props.activityTone === "error" ? "alert" : undefined} title={props.activityText}>{props.activityText}</span>
    </div>
  });
  if (showRefreshFeedback) activityEntries.push({ id: "runtime-refresh", content:
    <AutoRefreshStatus intervalMs={props.runtimePollIntervalMs} issue={props.runtimeRefreshIssue}
      paused={props.runtimeAutoRefreshPaused} failureLimit={props.runtimeRefreshFailureLimit}
      onResume={props.onResumeRuntimeAutoRefresh} />
  });

  return (
    <ActivityNoticeTarget.Provider value={{ element: null, store: noticeStore, dismissLabel: t("servers.mods.closeToast", undefined, "Close") }}>
    <div className="shell-frame" data-theme={props.theme}>
      <div className="shell-main shell-main--console">
        <AppHeader
          activeView={props.activeView}
          aiSettings={props.aiSettings}
          assistant={props.assistant}
          assistantDraft={props.assistantDraft}
          assistantInput={props.assistantInput}
          assistantExecution={props.assistantExecution}
          assistantConfirmationOpen={props.assistantConfirmationOpen}
          assistantMessages={props.assistantMessages}
          assistantConversations={props.assistantConversations}
          assistantActiveConversationId={props.assistantActiveConversationId}
          serverCount={props.serverCount}
          theme={props.theme}
          onAssistantAction={props.onAssistantAction}
          onAssistantDeleteConversation={props.onAssistantDeleteConversation}
          onAssistantDraftChange={props.onAssistantDraftChange}
          onAssistantNewConversation={props.onAssistantNewConversation}
          onAssistantSelectConversation={props.onAssistantSelectConversation}
          onAssistantRunPrompt={props.onAssistantRunPrompt}
          assistantRecoveryPending={props.assistantRecoveryPending}
          assistantContinuation={props.assistantContinuation}
          onAssistantResume={props.onAssistantResume}
          onAssistantStop={props.onAssistantStop}
          onAssistantSendMessage={props.onAssistantSendMessage}
          onClearAiSecret={props.onClearAiSecret}
          onSaveAiSettings={props.onSaveAiSettings}
          onSelectView={props.onSelectView}
          onThemeChange={props.onThemeChange}
        />

        <main className="shell-content-scroll">
          <div className="shell-content-body">{props.children}</div>
        </main>

        <AppUpdatePrompt enabled={props.appUpdatesEnabled} state={props.appUpdateState} retryRequest={updateRetryRequest}
          onCheck={props.onCheckAppUpdate} onInstall={props.onInstallAppUpdate} onDownloadInstaller={downloadAppInstaller} />
        {props.appUpdatesEnabled ? <AppUpdateActivity state={props.appUpdateState} onCheck={retryAppUpdate} /> : null}

        <footer className="shell-activity-bar" role="status" aria-live="polite" aria-atomic="false">
          <span className="shell-activity-label">{t("shell.activity")}</span>
          <ActivityBar store={noticeStore} entries={activityEntries} progress={showInstallationProgress ? (
            <InstallationActivity job={installationJob} name={getLocalizedModuleDisplayName(installationJob.target_id, locale, installationJob.label)} locale={locale} t={t}
              onStop={() => props.onCancelInstallation(installationJob.id)} stopPending={props.installationStopPendingIds.includes(installationJob.id)} stopError={props.installationStopErrors[installationJob.id]} />
          ) : showSteamCmdProgress && props.steamCmdProgress ? (
            <SteamCmdActivity snapshot={props.steamCmdProgress} message={props.steamCmdMessage} locale={locale} t={t}
              onStop={() => { if (props.steamCmdProgress) props.onCancelSteamCmd(props.steamCmdProgress.operation_id); }} stopPending={props.steamCmdStopPending} stopError={props.steamCmdStopError} />
          ) : null} />
        </footer>
      </div>
    </div>
    </ActivityNoticeTarget.Provider>
  );
}
