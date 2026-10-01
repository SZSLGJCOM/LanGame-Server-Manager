import React, { act, StrictMode, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC } from "@tauri-apps/api/mocks";
import { I18nProvider, useI18n } from "../../src/i18n";
import { ZH_CN_UI_MESSAGES } from "../../src/i18n-messages-zh-ui";
import { bootstrapApp, readInstanceDetails, readInstanceRuntime } from "../../src/api";
import { createDefaultAiSettings } from "../../src/ai-settings";
import { createInitialAppUpdateState } from "../../src/app-update-model";
import { resolveUiMessage, type UiMessage } from "../../src/app-ui";
import { AppShell } from "../../src/components/AppShell";
import { ActivityNotice } from "../../src/components/ActivityNotice";
import { ServersView } from "../../src/views/ServersView";
import { LibraryServerActions } from "../../src/views/library/LibraryServerActions";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { BackgroundJob, SteamCmdPrepareSnapshot } from "../../src/types";
import type { AssistantCapsuleModel } from "../../src/assistant-types";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
Object.assign(globalThis, { __reliabilityFixtureCleanup: async () => {
  await act(async () => { root.unmount(); });
  return { browser_errors: errors, native_dialogs: 0 };
} });
const noOperation = () => {};
let scenarios = 0;
let resumes = 0;
let startAttempts = 0;
const installationStops: string[] = [];
const steamCmdStops: string[] = [];
const refreshError = ["自动刷新服务器信息失败。", ...Array.from({ length: 20 }, (_, index) =>
  `诊断详情 ${index + 1}：服务器快照暂时不可用，已有服务器与终端内容应保持位置。`),
  `Synthetic path: ${"refresh-segment-".repeat(40)}`, "REFRESH_ERROR_END"].join("\n");
const startupError = ["准备启动服务器失败。", "本地夹具模拟运行环境准备失败。", "START_ERROR_END"].join("\n");
const completionText = "服务器准备完成。COMPLETION_END";
const warningMessage: UiMessage = { key: "fixture.activity.warning", fallback: "实例设置尚未保存。\n完整警告详情 WARNING_END", tone: "warning" };

function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(description);
}
async function settleUntil(predicate: () => boolean, description: string, timeout = 5000) {
  const deadline = performance.now() + timeout;
  while (!predicate()) {
    check(performance.now() < deadline, description);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const selected = fixture.querySelector<T>(selector);
  check(selected, `Missing ${selector}`);
  return selected;
}
function bounds() {
  return [".servers-page", ".server-list-panel", ".server-runtime-console-frame", "pre.server-runtime-console",
    ".server-runtime-console-command-form", ".shell-activity-bar"].map((selector) => {
    const rect = element(selector).getBoundingClientRect();
    return { selector, x: rect.x, y: rect.y, width: rect.width, height: rect.height };
  });
}
function unchanged(before: ReturnType<typeof bounds>, label: string) {
  const after = bounds();
  for (let index = 0; index < before.length; index++) {
    for (const property of ["x", "y", "width", "height"] as const) {
      check(Math.abs(before[index][property] - after[index][property]) < 1,
        `${label} displaced ${before[index].selector}.${property}: ${JSON.stringify({ before, after })}`);
    }
  }
  check(!element(".servers-page").classList.contains("servers-page--with-status"), "Notice still creates a status row above the server workspace");
  check(!fixture.querySelector(".dst-start-error"), "Startup failure still renders above the workspace");
  const footer = element(".shell-activity-bar");
  check(footer.scrollWidth <= footer.clientWidth + 1, "Activity bar overflowed the available viewport");
  check(Math.abs(footer.getBoundingClientRect().height - 54) < 1, "Activity bar must remain a single 54px row");
  check(element("pre.server-runtime-console").textContent?.includes("Existing terminal output"), "Notice removed terminal content");
}
function insideFooter(selected: HTMLElement, label: string) {
  const footer = element(".shell-activity-bar");
  check(footer.contains(selected), `${label} must be inside the activity bar`);
  const box = selected.getBoundingClientRect();
  const outer = footer.getBoundingClientRect();
  check(box.width > 0 && box.height > 0, `${label} is not visible`);
  check(box.left >= outer.left - 1 && box.right <= outer.right + 1 && box.top >= outer.top - 1 && box.bottom <= outer.bottom + 1,
    `${label} is clipped: ${JSON.stringify({ box: box.toJSON(), outer: outer.toJSON() })}`);
}
async function closeMessages() {
  const close = fixture.querySelector<HTMLButtonElement>(".shell-activity-panel-close");
  if (!close) return;
  await pressEnter(close);
  check(!fixture.querySelector(".shell-activity-panel"), "Message details did not close");
  check(document.activeElement === element(".shell-activity-expand"), "Closing messages did not restore focus to its trigger");
}
async function selectActivity(selector: string, expected?: string): Promise<HTMLElement> {
  await closeMessages();
  for (let index = 0; index < 20; index++) {
    const selected = [...element(".shell-activity-viewport").querySelectorAll<HTMLElement>(selector)]
      .find((node) => expected === undefined || node.textContent?.includes(expected));
    if (selected) return selected;
    const next = fixture.querySelector<HTMLButtonElement>(".shell-activity-next");
    check(next && !next.disabled, `Activity is not reachable: ${selector} ${expected ?? ""}`);
    await act(async () => { next.click(); });
  }
  throw new Error(`Activity rotation did not expose ${selector} ${expected ?? ""}`);
}
async function openMessages() {
  if (!fixture.querySelector(".shell-activity-panel")) await pressEnter(element<HTMLButtonElement>(".shell-activity-expand"));
  const panel = element(".shell-activity-panel");
  check(panel.getAttribute("role") === "dialog", "Complete messages must be an accessible dialog");
  const box = panel.getBoundingClientRect();
  check(box.width > 0 && box.left >= 0 && box.right <= innerWidth + 1 && box.top >= 0 && box.bottom <= innerHeight + 1,
    `Message details extend beyond the viewport: ${JSON.stringify(box.toJSON())}`);
  check(panel.scrollWidth <= panel.clientWidth + 1, "Long diagnostics created horizontal overflow in complete messages");
  return panel;
}
async function refreshInsideFooter() {
  const refresh = await selectActivity(".auto-refresh-status");
  insideFooter(refresh, "Refresh warning");
  const message = refresh.querySelector<HTMLElement>(".auto-refresh-status-alert-message");
  check(message?.textContent === refreshError, "Refresh message lost the complete multiline error");
}
async function feedbackInsideFooter(expected: string) {
  const feedback = await selectActivity(".shell-activity-feedback", expected);
  insideFooter(feedback, "Operation feedback");
  check(feedback.textContent?.includes(expected), "Activity bar lost the operation feedback");
}
async function completionInsideFooter() {
  const completion = await selectActivity(".shell-activity-text", completionText);
  insideFooter(completion, "Normal completion message");
  check(!completion.closest('[role="alert"]'), "A normal completion must not be announced as an error");
  check(!completion.classList.contains("is-error") && !completion.classList.contains("is-warning"),
    "Normal completion inherited the previous error severity");
  check(!element(".shell-activity-bar").textContent?.includes(startupError), "Completed activity retained the obsolete startup failure");
}
function checkResumeCount(expected: number) {
  check(resumes === expected, `Expected ${expected} refresh resumptions, received ${resumes}`);
}
async function pressEnter(button: HTMLButtonElement) {
  await act(async () => {
    button.focus();
    check(document.activeElement === button, "Recovery button cannot receive keyboard focus");
    const response = await fetch(`/__reliability_key/${nonce}/Enter`, { method: "POST" });
    check(response.ok, "Native Enter dispatch failed");
  });
}

async function run() {
  const bootstrap = await bootstrapApp({ includeSystemSnapshot: false });
  const original = bootstrap.state.instances[0];
  check(original, "Development fixture must provide an instance");
  // A synthetic module keeps this local component fixture independent of remote artwork.
  const instance = { ...original, module_id: "fixture-activity", name: "本地活动栏测试", status: "Stopped" as const, active_process_count: 0 };
  const details = { ...await readInstanceDetails(instance.id), summary: instance, active_run: null };
  const runtime = { ...await readInstanceRuntime(instance.id), recent_runs: [], diagnostics: [],
    log_tail: { source_path: "fixture/runtime.log", lines: ["Existing terminal output", "Ready for local fixture commands"],
      total_lines: 2, truncated: false, read_error: null } };
  const settings = createDefaultAiSettings();
  let activity: UiMessage = { key: "fixture.activity.ready", fallback: "活动栏已就绪" };
  let localNotices: Array<{ id: string; text: string; tone: "error" | "warning" | "success" | "info" }> = [];
  let localRetries = 0;
  let localDismissals = 0;
  let showCreationForm = false;
  let creating = false;
  let createCalls = 0;
  let updateRetries = 0;
  const createCompletion: { resolve?: () => void } = {};
  const createProps: ComponentProps<typeof LibraryServerActions> = {
    steamCmdStatus: null, steamCmdBusy: false,
    selected: { ...bootstrap.state.modules[0], install_state: "Installed" }, selectedModuleDetails: null,
    installLabel: "已安装", installBusy: false, installStatusClass: "is-success",
    creating: false, instanceName: "本地创建测试", onInstall: noOperation, onUninstall: noOperation,
    onInstanceNameChange: noOperation, onCreateServer: async () => {
      createCalls++; creating = true; activity = { key: "fixture.creating", fallback: "正在创建服务器 CREATE_PENDING" }; render();
      await new Promise<void>((resolve) => { createCompletion.resolve = resolve; });
      creating = false; activity = { key: "fixture.created", fallback: "服务器已创建 CREATE_COMPLETED" }; render();
    }
  };
  const viewProps: ComponentProps<typeof ServersView> = {
    aiSettings: settings, assistantCanRun: false, bindAddressCandidates: [], instances: [instance],
    moduleInstallations: { [instance.module_id]: { installState: "Installed", hasManagedInstallSource: true } },
    instanceLaunchPlans: {}, instanceLaunchFailures: {}, section: "overview", onWorkspaceSectionChange: noOperation,
    selectedInstanceId: instance.id, selectedDetails: details, selectedBackups: [], selectedModuleDetails: null,
    runtime, runtimeWindows: null, launchPlan: null, launchPlanError: null, refreshIssue: null,
    onResumeAutoRefresh: noOperation, onActivity: (next) => { activity = next; render(); },
    onSelectInstance: noOperation, onStart: async (id) => {
      check(id === instance.id, "Start targeted another instance"); startAttempts++; throw new Error(startupError);
    }, onStop: noOperation, onInstallModule: async () => {}, onOpenModuleLibrary: noOperation,
    onCreateInstance: noOperation, onPickDirectory: async () => null,
    onImportDontStarveWorldData: async () => { throw new Error("Unexpected world import"); },
    onOpenLocalPath: noOperation, onSendRuntimeCommand: async () => null, onSuppressRuntimeWindows: async () => {},
    onCreateBackup: noOperation, onArchivesChanged: async () => {}, onArchiveInstance: async () => {}, onDeleteInstance: async () => {}, onRestoreBackup: noOperation,
    onRenameBackup: async () => true, onDeleteBackup: async () => {}, onSaveSettings: async () => undefined,
    onSaveAutostart: async () => {}, onApplyPlayerAccessMutation: async () => { throw new Error("Unexpected player mutation"); }
  };
  const assistant: AssistantCapsuleModel = { panelTitle: "助手", tone: "info" };
  const shellProps: Omit<ComponentProps<typeof AppShell>, "children" | "activityText"> = {
    activeView: "servers", theme: "dark", serverCount: 1, aiSettings: settings,
    appUpdatesEnabled: true, appUpdateState: createInitialAppUpdateState("0.1.0"), assistant, assistantDraft: "",
    assistantExecution: { status: "idle", promptLabel: null, result: null, error: null },
    assistantMessages: [], assistantConversations: [], assistantActiveConversationId: null,
    assistantInput: { aiSettings: settings, locale: "zh-CN", activeJobsCount: 0, activeView: "servers", bootstrap,
      storageReady: true, libraryPage: "catalog", overlayNames: [], runtimeAutoRefreshPaused: false, runtimeRefreshIssue: null,
      selectedInstanceDetails: details, selectedInstanceId: instance.id, selectedModuleId: null, selectedInstanceModuleDetails: null,
      selectedLaunchPlan: null, selectedLaunchPlanError: null, selectedLogDocument: runtime.log_tail,
      selectedModuleDetails: null, selectedRuntime: runtime, serverWorkspaceSection: "overview", steamCmdStatus: null },
    jobs: [], steamCmdProgress: null, steamCmdMessage: "", steamCmdStopPending: false, steamCmdStopError: null,
    installationStopPendingIds: [], installationStopErrors: {},
    onCancelInstallation: (id) => { installationStops.push(id); }, onCancelSteamCmd: (id) => { steamCmdStops.push(id); },
    runtimeRefreshIssue: null, runtimeAutoRefreshPaused: false, runtimePollIntervalMs: 5000, runtimeRefreshFailureLimit: 3,
    onResumeRuntimeAutoRefresh: () => {
      resumes++; shellProps.runtimeRefreshIssue = null; shellProps.runtimeAutoRefreshPaused = false;
      viewProps.refreshIssue = null; render();
    },
    onAssistantAction: noOperation, onAssistantDeleteConversation: noOperation, onAssistantDraftChange: noOperation,
    onAssistantNewConversation: noOperation, onAssistantSelectConversation: noOperation, onAssistantRunPrompt: noOperation,
    onAssistantSendMessage: noOperation, onCheckAppUpdate: () => {
      updateRetries++; shellProps.appUpdateState = createInitialAppUpdateState("0.1.0"); render();
    }, onClearAiSecret: async (next) => next,
    onInstallAppUpdate: noOperation, onSaveAiSettings: async (next) => next, onSelectView: noOperation, onThemeChange: noOperation
  };
  function Harness() {
    const { t } = useI18n();
    return <AppShell {...shellProps} activityText={resolveUiMessage(t, activity)} activityTone={activity.tone}>
      {showCreationForm ? <div style={{ width: 380 }}><LibraryServerActions {...createProps} creating={creating} /></div>
        : <ServersView {...viewProps} />}
      {localNotices.map((notice) => <ActivityNotice key={notice.id} tone={notice.tone}
        onDismiss={() => { localDismissals++; }} action={notice.id === "workshop" ?
          <button type="button" className="auto-refresh-status-resume" onClick={() => {
            localRetries++; localNotices = localNotices.filter((item) => item.id !== notice.id); render();
          }}>重试</button> : undefined}>{notice.text}</ActivityNotice>)}
    </AppShell>;
  }
  function render() {
    root.render(<StrictMode><I18nProvider><InstanceSettingsSaveProvider><Harness /></InstanceSettingsSaveProvider></I18nProvider></StrictMode>);
  }
  await act(async () => { render(); });
  await settleUntil(() => Boolean(fixture.querySelector("pre.server-runtime-console")), "Complete shell did not mount its server console");
  const baseline = bounds();
  check(baseline.every((box) => box.width > 50 && box.height > 20), `Unusable layout baseline: ${JSON.stringify(baseline)}`);
  check(element(".shell-activity-bar").getBoundingClientRect().bottom <= innerHeight + 1, "Activity bar is outside the viewport");

  const issue = { message: refreshError, failedAt: 1_800_000_000_000, consecutiveFailures: 1 };
  shellProps.runtimeRefreshIssue = issue; viewProps.refreshIssue = issue;
  await act(async () => { render(); });
  unchanged(baseline, "long multiline refresh failure"); await refreshInsideFooter(); scenarios++;

  shellProps.runtimeAutoRefreshPaused = true;
  await act(async () => { render(); });
  unchanged(baseline, "refresh paused after failures"); await refreshInsideFooter();
  const resume = element<HTMLButtonElement>(".auto-refresh-status-resume");
  insideFooter(resume, "Resume control");
  await pressEnter(resume);
  checkResumeCount(1);
  check(!fixture.querySelector(".auto-refresh-status"), "Enter did not resume and clear refresh failure");
  unchanged(baseline, "refresh resumed"); scenarios++;

  shellProps.runtimeAutoRefreshPaused = true;
  await act(async () => { render(); });
  unchanged(baseline, "paused refresh without an issue");
  insideFooter(await selectActivity(".auto-refresh-status"), "Paused refresh status");
  await pressEnter(element<HTMLButtonElement>(".auto-refresh-status-resume"));
  checkResumeCount(2);
  check(!fixture.querySelector(".auto-refresh-status"), "Paused state did not resume without error details");
  scenarios++;

  const start = element<HTMLButtonElement>(".server-list-card-footer > button");
  check(!start.disabled && start.textContent?.includes("启动"), "Stopped instance must offer the real start action");
  await act(async () => { start.click(); });
  await settleUntil(() => activity.tone === "error", "ServersView did not forward its start preparation failure to activity feedback");
  check(startAttempts === 1, "Start failure duplicated the start request");
  unchanged(baseline, "start preparation failure"); await feedbackInsideFooter(startupError); scenarios++;

  activity = warningMessage;
  await act(async () => { render(); });
  unchanged(baseline, "warning feedback"); await feedbackInsideFooter(warningMessage.fallback!); scenarios++;

  activity = { key: "fixture.activity.error", fallback: startupError, tone: "error" };
  shellProps.runtimeRefreshIssue = issue; shellProps.runtimeAutoRefreshPaused = true; viewProps.refreshIssue = issue;
  await act(async () => { render(); });
  unchanged(baseline, "operation failure and paused refresh together"); await refreshInsideFooter(); await feedbackInsideFooter(startupError); scenarios++;

  activity = { key: "fixture.activity.completed", fallback: completionText };
  await act(async () => { render(); });
  unchanged(baseline, "normal completion during paused refresh"); await completionInsideFooter(); await refreshInsideFooter();
  const completionResume = element<HTMLButtonElement>(".auto-refresh-status-resume");
  check(!completionResume.disabled, "Normal completion disabled refresh recovery");
  insideFooter(completionResume, "Resume control alongside normal completion");
  await pressEnter(completionResume);
  checkResumeCount(3);
  check(!fixture.querySelector(".auto-refresh-status"), "Refresh recovery retained the obsolete refresh failure");
  unchanged(baseline, "normal completion after refresh recovery"); await completionInsideFooter(); scenarios++;

  activity = { key: "fixture.activity.error", fallback: startupError, tone: "error" };
  shellProps.runtimeRefreshIssue = issue; shellProps.runtimeAutoRefreshPaused = true; viewProps.refreshIssue = issue;
  const job: BackgroundJob = { id: "fixture-installation", label: "本地下载任务", target_id: "fixture-activity", kind: "DownloadGame",
    status: "Running", cancellable: true, cancel_requested: false, progress_percent: 42, detail: "本地下载", output_excerpt: "",
    install_progress: { phase: "downloading", downloaded_bytes: 42_000_000, total_bytes: 100_000_000, percent: 42, elapsed_seconds: 12 } };
  shellProps.jobs = [job];
  await act(async () => { render(); });
  unchanged(baseline, "installation progress and both failures"); await refreshInsideFooter(); await feedbackInsideFooter(startupError);
  let meter = element(".shell-task-meter");
  check(meter.getAttribute("aria-valuenow") === "42", "Installation notice lost measured progress");
  insideFooter(meter, "Installation progress");
  let stop = element<HTMLButtonElement>(".shell-task-stop");
  check(!stop.disabled, "Installation notice disabled cancellation"); insideFooter(stop, "Installation stop control");
  await act(async () => { stop.click(); }); scenarios++;
  const longWorkshopError = `工坊条目类型核验失败，Steam 返回 HTTP 429。${"完整诊断及请求限流详情。".repeat(60)}\nWORKSHOP_ERROR_END`;
  localNotices = [
    { id: "workshop", text: longWorkshopError, tone: "error" },
    { id: "warning", text: "停服后修改 Mod。WARNING_LOCAL_END", tone: "warning" },
    { id: "success", text: "设置已保存。SUCCESS_LOCAL_END", tone: "success" },
    { id: "info", text: "正在读取配置。INFO_LOCAL_END", tone: "info" }
  ];
  await act(async () => { render(); });
  unchanged(baseline, "multiple local notices beside download cancellation");
  check(!element(".shell-content-scroll").querySelector(".shell-activity-notice"), "Local notice still occupies the workspace");
  await selectActivity(".shell-activity-notice.is-error", longWorkshopError);
  check(element(".shell-activity-viewport").querySelectorAll(".shell-activity-notice").length === 1,
    "Concurrent local notices must occupy one presentation slot");
  insideFooter(element(".shell-task-stop"), "Cancellation alongside local notices");
  for (const notice of localNotices) {
    const selected = await selectActivity(".shell-activity-notice", notice.text);
    insideFooter(selected.querySelector<HTMLButtonElement>(".shell-activity-notice-close")!, "Notification close control");
  }
  scenarios++;
  const beforeRotation = element(".shell-activity-viewport").textContent;
  await pressEnter(element<HTMLButtonElement>(".shell-activity-next"));
  check(element(".shell-activity-viewport").textContent !== beforeRotation, "Next did not rotate to another message");
  await pressEnter(element<HTMLButtonElement>(".shell-activity-previous"));
  check(element(".shell-activity-viewport").textContent === beforeRotation, "Previous did not return to the prior message");
  unchanged(baseline, "manual message rotation"); scenarios++;
  const panel = await openMessages();
  check(panel.querySelectorAll(".shell-activity-notice").length === 4, "Complete messages lost a concurrent local notice");
  const diagnostic = panel.querySelector<HTMLElement>(".shell-activity-notice.is-error .shell-activity-notice-text");
  check(diagnostic?.textContent === longWorkshopError, "Complete messages lost multiline workshop diagnostics");
  check(diagnostic.scrollHeight <= diagnostic.clientHeight + 1 && getComputedStyle(diagnostic).whiteSpace === "pre-wrap",
    "Expanded diagnostics are still truncated");
  check(panel.textContent?.includes(refreshError) && panel.textContent.includes(startupError), "Complete messages lost global failures");
  await closeMessages();
  unchanged(baseline, "expanded message details"); scenarios++;
  const pause = element<HTMLButtonElement>(".shell-activity-pause");
  await pressEnter(pause);
  check(pause.getAttribute("aria-pressed") === "true", "Pause did not expose its active state");
  const pausedMessage = element(".shell-activity-viewport").textContent;
  await act(async () => { pause.blur(); });
  if (innerWidth === 1560) {
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 8200)); });
    check(element(".shell-activity-viewport").textContent === pausedMessage, "Paused rotation changed the current message");
  }
  await pressEnter(pause);
  check(pause.getAttribute("aria-pressed") === "false", "Resume did not clear pause state");
  await act(async () => { pause.blur(); });
  if (innerWidth === 1560) {
    await settleUntil(() => element(".shell-activity-viewport").textContent !== pausedMessage,
      "Resumed messages did not automatically rotate", 10000);
  }
  unchanged(baseline, "paused and resumed rotation"); scenarios++;
  await selectActivity(".shell-activity-notice.is-error", longWorkshopError);
  const retry = element<HTMLButtonElement>(".shell-activity-notice-actions button");
  await pressEnter(retry);
  check(localRetries === 1 && !fixture.querySelector(".shell-activity-notice.is-error"), "Notification retry was lost or duplicated");
  check(document.activeElement === element(".shell-activity-expand"), "Retrying a removed notice lost keyboard focus");
  unchanged(baseline, "retrying a local operation"); scenarios++;
  await selectActivity(".shell-activity-notice.is-warning");
  await pressEnter(element<HTMLButtonElement>(".shell-activity-notice.is-warning .shell-activity-notice-close"));
  check(localDismissals === 1 && !fixture.querySelector(".shell-activity-notice.is-warning"), "Notification could not be closed from the keyboard");
  check(document.activeElement === element(".shell-activity-expand"), "Dismissing the selected notice lost keyboard focus");
  unchanged(baseline, "closing a local notification"); scenarios++;
  const warningIndex = localNotices.findIndex((notice) => notice.id === "warning");
  const previousWarning = localNotices[warningIndex].text;
  localNotices[warningIndex] = { ...localNotices[warningIndex], text: "正在重新检查 Mod。" };
  await act(async () => { render(); });
  localNotices[warningIndex] = { ...localNotices[warningIndex], text: previousWarning };
  await act(async () => { render(); });
  check((await selectActivity(".shell-activity-notice.is-warning .shell-activity-notice-text")).textContent === previousWarning,
    "A new operation's repeated warning remained hidden after closing the previous one");
  unchanged(baseline, "repeated feedback from a new operation"); scenarios++;
  localNotices = [];
  await act(async () => { render(); });
  check(!(await openMessages()).querySelector(".shell-activity-notice"), "Unmounted view left obsolete notifications in the activity bar");
  await closeMessages();
  unchanged(baseline, "local notification owner unmounted"); scenarios++;

  const steamCmd: SteamCmdPrepareSnapshot = { operation_id: "fixture-steamcmd", active: true, phase: "downloading", detail: "本地下载",
    cancellable: true, cancel_requested: false, cancelled: false, downloaded_bytes: 25_000_000, total_bytes: 100_000_000,
    output_excerpt: "", elapsed_seconds: 10, idle_seconds: 0, error: null };
  shellProps.jobs = []; shellProps.steamCmdProgress = steamCmd;
  await act(async () => { render(); });
  unchanged(baseline, "SteamCMD progress and both failures"); await refreshInsideFooter(); await feedbackInsideFooter(startupError);
  meter = element(".shell-task-meter");
  check(meter.getAttribute("aria-valuenow") === "25", "SteamCMD notice lost measured progress"); insideFooter(meter, "SteamCMD progress");
  stop = element<HTMLButtonElement>(".shell-task-stop");
  check(!stop.disabled, "SteamCMD notice disabled cancellation"); insideFooter(stop, "SteamCMD stop control");
  await act(async () => { stop.click(); });
  const stopFailure = `停止下载未完成。${"完整停止诊断。".repeat(50)}\nSTOP_FAILURE_END`;
  shellProps.steamCmdStopError = stopFailure;
  await act(async () => { render(); });
  const taskDiagnostics = (await openMessages()).querySelector<HTMLElement>(".shell-task-diagnostics");
  check(taskDiagnostics?.textContent?.includes("STOP_FAILURE_END"), "Task cancellation diagnostics are not available in complete messages");
  check(taskDiagnostics.scrollHeight <= taskDiagnostics.clientHeight + 1, "Task diagnostics are still truncated");
  await closeMessages();
  shellProps.steamCmdStopError = null;
  scenarios++;

  const taskFailure = "SteamCMD 准备失败。\n完整任务诊断 TASK_FAILURE_END";
  activity = { key: "fixture.steamCmdFailed", fallback: "STEAMCMD_FAILED" };
  shellProps.steamCmdMessage = "STEAMCMD_FAILED";
  shellProps.steamCmdProgress = { ...steamCmd, active: false, error: taskFailure };
  shellProps.runtimeRefreshIssue = null; shellProps.runtimeAutoRefreshPaused = false;
  await act(async () => { render(); });
  check(!fixture.querySelector(".shell-activity-viewport"), "Task-only failure unexpectedly created a rotating message");
  insideFooter(element(".shell-activity-expand"), "Task-only details control");
  check((await openMessages()).querySelector(".shell-task-diagnostics")?.textContent?.includes("TASK_FAILURE_END"),
    "A task-only failure must expose its complete diagnosis");
  await closeMessages();
  unchanged(baseline, "task-only failure details"); scenarios++;
  check(errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  const finalServerBounds = bounds();
  shellProps.jobs = []; shellProps.steamCmdProgress = null; shellProps.runtimeRefreshIssue = null; shellProps.runtimeAutoRefreshPaused = false;
  shellProps.appUpdateState = { ...shellProps.appUpdateState, status: "downloading", availableVersion: "0.2.0", contentLength: 1024, downloadPercent: 40 };
  await act(async () => { render(); });
  check(!fixture.querySelector(".assistant-panel, .assistant-update-notice"), "Updating requires or inserts an assistant overlay");
  await selectActivity(".shell-activity-notice", "正在下载更新");
  insideFooter(element('.shell-update-progress progress'), "Update download progress with assistant closed");
  check(element('.shell-update-progress progress').getAttribute("value") === "40", "Download progress lost its value");
  unchanged(baseline, "update download with assistant closed"); scenarios++;
  shellProps.appUpdateState = { ...shellProps.appUpdateState, status: "installing", downloadPercent: 100 };
  await act(async () => { render(); });
  insideFooter(await selectActivity(".shell-activity-notice", "正在安装更新"), "Update installation");
  check(element(".shell-activity-notice-text").textContent?.includes("正在安装更新"), "Installation feedback disappeared");
  unchanged(baseline, "update installation"); scenarios++;
  const updateError = "更新安装失败。".repeat(80) + "UPDATE_FAILURE_END";
  shellProps.appUpdateState = { ...shellProps.appUpdateState, status: "failed", error: updateError };
  await act(async () => { render(); });
  check((await openMessages()).querySelector(".shell-activity-notice-text")?.textContent?.includes(updateError), "Full update failure detail was lost");
  await selectActivity(".shell-activity-notice", updateError);
  insideFooter(element(".shell-activity-notice-actions button"), "Update retry with assistant closed");
  await pressEnter(element<HTMLButtonElement>(".shell-activity-notice-actions button"));
  check(updateRetries === 1 && !fixture.querySelector(".shell-activity-notice"), "Update retry did not clear previous failure");
  unchanged(baseline, "update retry"); scenarios++;
  Object.assign(globalThis, { isTauri: true });
  mockIPC((command, args) => {
    check(command === "inspect_module_programs", `Unexpected native command: ${command}`);
    const input = args?.input;
    check(input !== null && typeof input === "object" && "module_id" in input && "program_mode" in input && "program_source" in input,
      "Program inspection omitted its creation input");
    check(input.module_id === createProps.selected.id && input.program_mode === "independent" && input.program_source === "verified",
      "Program inspection targeted another module or creation mode");
    return {
      installations: [{ id: 1, install_root: "fixture/library", scope: "library", install_state: "Installed",
        current_version: "fixture-version", used_by: [], modification_state: "unverified", pending_removal: false, size_bytes: 100 * 1024 ** 3 }],
      creation: { can_create: true, action: "existing_install",
        program_path: "fixture/library", additional_bytes: 0, reason: null }
    };
  });
  async function waitForProgramInventory() {
    await settleUntil(() => Boolean(fixture.querySelector('.library-program-inventory[aria-busy="false"] .library-program-metric')),
      "Program inspection did not resolve the creation plan");
  }
  showCreationForm = true; activity = { key: "fixture.creationReady", fallback: "准备创建服务器" };
  await act(async () => { render(); });
  await waitForProgramInventory();
  const creationBaseline = element(".library-sidebar-server-actions").getBoundingClientRect();
  function unchangedCreation(description: string) {
    const current = element(".library-sidebar-server-actions").getBoundingClientRect();
    for (const property of ["x", "y", "width", "height"] as const) {
      check(Math.abs(creationBaseline[property] - current[property]) < 1, `${description} moved creation controls: ${property}`);
    }
    check(!fixture.querySelector(".library-create-status"), "Creation progress still appears underneath the button");
  }
  await act(async () => { element<HTMLButtonElement>('.library-create-submit button').click(); });
  check(createCalls === 1 && creating, "Creation did not start once asynchronously");
  check(element<HTMLButtonElement>('.library-create-submit button').disabled, "Creating button permits duplicate submission");
  check(element(".library-create-server-form").getAttribute("aria-busy") === "true", "Creation form lost its busy state");
  const creationHint = ZH_CN_UI_MESSAGES["library.detail.creatingServerHint"];
  insideFooter(await selectActivity(".shell-activity-notice", creationHint), "Server file creation progress");
  check(element(".shell-activity-notice-text").textContent?.includes(creationHint), "Creation progress lost its file preparation explanation");
  unchangedCreation("pending instance creation"); scenarios++;
  showCreationForm = false;
  await act(async () => { render(); });
  check(creating, "Navigating away cancelled the background create operation");
  insideFooter(await selectActivity(".shell-activity-feedback", "CREATE_PENDING"), "Background creation feedback");
  showCreationForm = true;
  await act(async () => { render(); });
  await waitForProgramInventory();
  check(element<HTMLButtonElement>('.library-create-submit button').disabled, "Returning to pending creation enabled duplicate work");
  unchangedCreation("returning to background instance creation"); scenarios++;
  await act(async () => { createCompletion.resolve?.(); });
  await waitForProgramInventory();
  check(!creating && !element<HTMLButtonElement>('.library-create-submit button').disabled, "Completion did not restore the create button");
  check(!fixture.querySelector(".shell-activity-notice") && element(".shell-activity-bar").textContent?.includes("CREATE_COMPLETED"),
    "Creation completion did not replace the pending activity");
  unchangedCreation("completed instance creation"); scenarios++;
  check(errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  // Leave the real shell in a dense state for the optional visual acceptance capture.
  showCreationForm = false;
  activity = { key: "fixture.activity.error", fallback: startupError, tone: "error" };
  localNotices = [
    { id: "workshop", text: longWorkshopError, tone: "error" },
    { id: "warning", text: "停服后修改 Mod。WARNING_LOCAL_END", tone: "warning" },
    { id: "success", text: "设置已保存。SUCCESS_LOCAL_END", tone: "success" },
    { id: "info", text: "正在读取配置。INFO_LOCAL_END", tone: "info" }
  ];
  shellProps.jobs = [job];
  shellProps.runtimeRefreshIssue = issue;
  shellProps.runtimeAutoRefreshPaused = true;
  await act(async () => { render(); });
  await selectActivity(".shell-activity-notice.is-error", longWorkshopError);
  insideFooter(element(".shell-task-stop"), "Cancellation in final dense state");
  unchanged(baseline, "final dense activity state");
  if (new URLSearchParams(location.search).has("details")) await openMessages();
  await act(async () => {
    await Promise.all(element(".shell-activity-bar").getAnimations({ subtree: true })
      .filter((animation) => animation.effect?.getComputedTiming().iterations !== Infinity)
      .map((animation) => animation.finished));
  });
  return { status: "passed", scenarios, resumes, start_attempts: startAttempts,
    local_retries: localRetries, local_dismissals: localDismissals,
    create_calls: createCalls, update_retries: updateRetries, installation_stops: installationStops, steamcmd_stops: steamCmdStops,
    baseline, final_bounds: finalServerBounds, browser_errors: errors };
}

let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error(`Interaction stalled after ${scenarios} scenarios`)), 40_000);
})]).finally(() => clearTimeout(watchdog))
  .catch((error) => ({ status: "failed", scenarios, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
