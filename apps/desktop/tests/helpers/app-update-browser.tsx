import { unavailableStorageMigration } from "./storage-migration-fixture";
import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, StrictMode, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { bootstrapApp } from "../../src/api";
import { I18nProvider, useI18n } from "../../src/i18n";
import { createDefaultAiSettings } from "../../src/ai-settings";
import { applyAppUpdateCheckResult, createInitialAppUpdateState } from "../../src/app-update-model";
import { fallbackBootstrap } from "../../src/app-state";
import { AppShell } from "../../src/components/AppShell";
import { AppUpdatePrompt } from "../../src/components/AppUpdatePrompt";
import { SystemView } from "../../src/views/SystemView";
import type { AppUpdateState } from "../../src/types";
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
const nativeOpen = window.open;
const installerUrls: string[] = [];
let failOpening = false;
window.open = (url) => {
  installerUrls.push(String(url));
  if (failOpening) throw new Error("系统阻止了下载页面。");
  return null;
};
Object.assign(globalThis, { __reliabilityFixtureCleanup: async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  await act(async () => { root.unmount(); });
  window.open = nativeOpen;
  return { browser_errors: errors, native_dialogs: 0 };
} });
const noop = () => {};
const cases: string[] = [];
let checks = 0;
let installs = 0;
let localeChange: (locale: "zh-CN" | "en-US") => void = noop;
let isolatedPrompt = false;
let showIsolatedPrompt = true;
let deferredDownload: { resolve?: () => void; reject?: (error: Error) => void } = {};
const settings = createDefaultAiSettings();
let bootstrap = fallbackBootstrap;
const props: Omit<ComponentProps<typeof AppShell>, "children"> = {
  activeView: "system", activityText: "服务器运行状态", theme: "dark", serverCount: 0, aiSettings: settings,
  appUpdatesEnabled: false, appUpdateState: createInitialAppUpdateState("0.1.0"),
  assistant: { panelTitle: "LAN", tone: "info" }, assistantDraft: "",
  assistantExecution: { status: "idle", promptLabel: null, result: null, error: null },
  assistantMessages: [], assistantConversations: [], assistantActiveConversationId: null,
  assistantInput: { aiSettings: settings, locale: "zh-CN", activeJobsCount: 0, activeView: "system",
    bootstrap: fallbackBootstrap, storageReady: true, libraryPage: "catalog", overlayNames: [],
    runtimeAutoRefreshPaused: false, runtimeRefreshIssue: null, selectedInstanceDetails: null,
    selectedInstanceId: null, selectedModuleId: null, selectedInstanceModuleDetails: null,
    selectedLaunchPlan: null, selectedLaunchPlanError: null, selectedLogDocument: null,
    selectedModuleDetails: null, selectedRuntime: null, serverWorkspaceSection: "overview", steamCmdStatus: null },
  jobs: [], steamCmdProgress: null, steamCmdMessage: "", steamCmdStopPending: false, steamCmdStopError: null,
  installationStopPendingIds: [], installationStopErrors: {}, onCancelInstallation: noop, onCancelSteamCmd: noop,
  runtimeRefreshIssue: null, runtimeAutoRefreshPaused: false, runtimePollIntervalMs: 5000,
  runtimeRefreshFailureLimit: 3, onResumeRuntimeAutoRefresh: noop,
  onAssistantAction: noop, onAssistantDeleteConversation: noop, onAssistantDraftChange: noop,
  onAssistantNewConversation: noop, onAssistantSelectConversation: noop, onAssistantRunPrompt: noop,
  onAssistantSendMessage: noop, onClearAiSecret: async (next) => next, onSaveAiSettings: async (next) => next,
  onSelectView: noop, onThemeChange: noop,
  onCheckAppUpdate: () => { checks++; props.appUpdateState = { ...props.appUpdateState, status: "checking" }; render(); },
  onInstallAppUpdate: () => { installs++; props.appUpdateState = { ...props.appUpdateState, status: "downloading", downloadPercent: 0 }; render(); }
};
function Harness() {
  localeChange = useI18n().setLocale;
  if (isolatedPrompt) return showIsolatedPrompt ? <AppUpdatePrompt enabled state={props.appUpdateState} retryRequest={0}
    onCheck={noop} onInstall={noop} onDownloadInstaller={() => new Promise<void>((resolve, reject) => {
      deferredDownload = { resolve, reject };
    })} /> : null;
  return <AppShell {...props}><SystemView snapshot={bootstrap.state.snapshot} instances={bootstrap.state.instances}
    bindAddressCandidates={[]} appSettings={bootstrap.state.settings} steamCmdStatus={null} steamCmdBusy={false}
    steamCmdProgress={null} steamCmdMessage="" onOpenInstance={noop} onPickDirectory={async () => null}
    storageMigration={unavailableStorageMigration} onEnsureSteamCmd={noop} onUninstallSteamCmd={noop} /></AppShell>;
}
function render() { root.render(<StrictMode><I18nProvider><Harness /></I18nProvider></StrictMode>); }
function check(value: unknown, message: string): asserts value { if (!value) throw new Error(message); }
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const found = document.querySelector<T>(selector);
  check(found, `Missing ${selector}`); return found;
}
function button(text: string): HTMLButtonElement {
  const found = [...element(".app-update-panel").querySelectorAll("button")].find((item) => item.textContent === text);
  check(found, `Missing button ${text}`); return found;
}
async function click(node: HTMLElement) { await act(async () => { node.click(); }); }
async function pointerClick(node: HTMLElement) {
  const bounds = node.getBoundingClientRect();
  await act(async () => {
    const result = await fetch(`/__reliability_pointer/${nonce}`, { method: "POST", body: JSON.stringify({
      type: "click", x: bounds.left + bounds.width / 2, y: bounds.top + bounds.height / 2
    }) });
    check(result.ok, "Native pointer click failed");
  });
}
async function settleUntil(predicate: () => boolean, message: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    check(performance.now() < deadline, message);
    await act(async () => { await new Promise<void>((resolve) => requestAnimationFrame(() => resolve())); });
  }
}
async function key(value: string) {
  await act(async () => { const result = await fetch(`/__reliability_key/${nonce}/${value}`, { method: "POST" }); check(result.ok, `Key ${value} failed`); });
}
async function update(next: Partial<AppUpdateState>) {
  await act(async () => { props.appUpdateState = { ...props.appUpdateState, ...next }; render(); });
}
function panelBounds() {
  const panel = element<HTMLDialogElement>(".app-update-panel");
  const bounds = panel.getBoundingClientRect();
  check(panel.open && bounds.width > 200 && bounds.height > 100, "Update dialog is not visibly open");
  check(bounds.left >= 0 && bounds.top >= 0 && bounds.right <= innerWidth && bounds.bottom <= innerHeight,
    `Update dialog clipped: ${JSON.stringify(bounds.toJSON())}`);
  check(panel.scrollWidth <= panel.clientWidth + 1, "Update content overflowed horizontally");
}
async function close() {
  await key("Escape");
  check(!document.querySelector(".app-update-panel"), "Escape did not close updates");
  check(document.activeElement === element(".app-update-trigger"), "Dismissal did not restore focus to the update entry");
}
async function openUpdates() {
  await act(async () => { element(".app-update-trigger").focus(); });
  await click(element(".app-update-trigger"));
  panelBounds();
}
async function current() {
  await act(async () => {
    props.appUpdateState = applyAppUpdateCheckResult(props.appUpdateState, { status: "current", currentVersion: "0.1.0" }); render();
  });
}

async function run() {
  // Use the production System page with the same synthetic bootstrap as the
  // desktop layout fixtures, without starting a native runtime or updater.
  bootstrap = await bootstrapApp({ includeSystemSnapshot: true });
  props.serverCount = bootstrap.state.instances.length;
  props.assistantInput.bootstrap = bootstrap;
  await act(async () => { await prepareBrowserLocaleCatalogs(); render(); });
  await settleUntil(() => Boolean(document.querySelector(".shell-theme-icon-button")), "Header did not mount");
  const quietTools = element(".shell-header-tools").getBoundingClientRect();
  const quietRow = element(".shell-header-row").getBoundingClientRect();
  const quietCenter = element(".shell-header-center").getBoundingClientRect();
  function headerLayout(hasUpdate: boolean) {
    check(!document.querySelector(".assistant-update-badge"), "Update was placed inside the assistant");
    const tools = element(".shell-header-tools");
    const bounds = tools.getBoundingClientRect();
    check(Math.abs(bounds.right - quietTools.right) < 1, "Update moved the window controls");
    check(Math.abs(element(".shell-header-row").getBoundingClientRect().height - quietRow.height) < 1, "Update changed header height");
    check(Math.abs(element(".shell-header-center").getBoundingClientRect().left - quietCenter.left) < 1, "Update moved the assistant");
    if (hasUpdate) {
      const trigger = element<HTMLButtonElement>(".shell-header-tools > .app-update-trigger");
      const triggerBounds = trigger.getBoundingClientRect();
      check(tools.firstElementChild === trigger && !trigger.disabled, "Update entry is not reachable before the theme control");
      check(triggerBounds.width >= 32 && triggerBounds.height >= 32, "Update entry is too small to click");
      check(triggerBounds.left >= quietCenter.right && triggerBounds.right <= element(".shell-theme-icon-button").getBoundingClientRect().left,
        "Update entry overlaps another header control");
      check(trigger.getAttribute("aria-label")?.includes(props.appUpdateState.availableVersion!), "Update entry omits the available version");
      check(trigger.getAttribute("aria-haspopup") === "dialog", "Update entry does not describe its dialog");
    } else {
      check(!document.querySelector(".app-update-trigger"), "A build without an available update shows an entry");
      check(tools.firstElementChild === element(".shell-theme-icon-button"), "Hidden update reserves a toolbar slot");
      check(Math.abs(bounds.width - quietTools.width) < 1, "Hidden update changes toolbar width");
    }
  }
  function noPrompt(reason: string, hasUpdate = true) {
    check(!document.querySelector(".app-update-panel"), `${reason}: unexpected update prompt`); headerLayout(hasUpdate);
  }
  noPrompt("disabled initial build", false);
  await update({ status: "available", availableVersion: "0.2.0" }); noPrompt("disabled build with known version", false);
  check(checks === 0 && installs === 0, "Disabled build invoked an updater"); cases.push("disabled-build");
  props.appUpdatesEnabled = true;
  await update({ status: "idle", availableVersion: null }); noPrompt("idle", false);
  await update({ status: "checking" }); noPrompt("first background check", false);
  await update({ status: "failed", error: "检查更新失败" }); noPrompt("failure before discovery", false);
  await current(); noPrompt("current version", false);
  cases.push("quiet-header");

  const notes = "修复安装与更新流程。\n<img src=x onerror=alert(1)>\n" + "更新说明 ".repeat(150);
  await act(async () => { element(".shell-theme-icon-button").focus(); });
  await update({ status: "available", availableVersion: "0.2.0", releaseNotes: notes, error: null });
  noPrompt("new release discovery");
  check(document.activeElement === element(".shell-theme-icon-button"), "Background discovery stole focus");
  check(element(".app-update-trigger").title === "发现新版本 · 0.2.0", "Available update hint is missing");
  check(checks === 0 && installs === 0 && installerUrls.length === 0, "Discovery performed an update action");
  cases.push("persistent-header-entry");
  await openUpdates(); headerLayout(true);
  check(element(".app-update-heading h2").textContent === "发现新版本 0.2.0", "Header entry did not open the discovered release");
  check(element(".app-update-notes").textContent === notes.trim(), "Release notes changed");
  check(!element(".app-update-notes").querySelector("img"), "Release notes became executable markup");
  check(!document.querySelector(".app-update-versions, .app-update-confirmation"), "Update prompt contains obsolete version rows or confirmation");
  check(element(".app-update-detail").textContent?.includes("保存并停止运行中的服务器"), "Online update omits stop-server impact");
  check(document.activeElement === element(".app-update-heading h2") && installs === 0, "Prompt did not focus its title or installed automatically");
  cases.push("manual-prompt");
  await close();
  noPrompt("dismissed release");
  await openUpdates(); await click(element(".app-update-close")); noPrompt("close button");
  await update({ status: "checking" }); noPrompt("checking a known release");
  await update({ status: "available" }); noPrompt("same-version polling");
  await current(); noPrompt("update no longer available", false);
  await update({ status: "available", availableVersion: "0.2.0" }); noPrompt("same-version rediscovery");
  await openUpdates(); await close();
  cases.push("same-version-reopen");
  await update({ availableVersion: "0.3.0" }); noPrompt("newer release discovery"); await openUpdates();
  await click(button("稍后")); noPrompt("Later"); check(installs === 0, "Later started installation");
  cases.push("new-version-and-later");

  await update({ availableVersion: "0.4.0", releaseNotes: null }); noPrompt("release without notes"); await openUpdates();
  check(!document.querySelector(".app-update-notes"), "Missing release notes created placeholder content");
  failOpening = true;
  await click(button("前往下载"));
  check(element(".app-update-error").textContent?.includes("系统阻止了下载页面"), "Download-page error was not shown inline");
  check(installs === 0, "Opening an installer page installed the update");
  failOpening = false;
  const download = button("前往下载");
  await act(async () => { download.click(); download.click(); });
  check(installerUrls.length === 2, "Repeated download click opened multiple pages");
  noPrompt("successful installer-page open");
  check(installerUrls.every((url) => url === "https://github.com/SZSLGJCOM/LanGame-Server-Manager/releases/tag/v0.4.0"), "Installer page does not target the discovered official release");
  cases.push("installer-page");

  await update({ availableVersion: "0.5.0" }); noPrompt("new release before keyboard activation");
  await act(async () => { element(".app-update-trigger").focus(); }); await key("Enter"); panelBounds();
  await key("Tab");
  check(document.activeElement === element(".app-update-close"), "Title focus did not lead into the update actions");
  await act(async () => { button("在线更新").focus(); }); await key("Tab");
  check(document.activeElement === element(".app-update-close"), "Tab escaped the update dialog");
  cases.push("keyboard-focus");
  const install = button("在线更新");
  await act(async () => { install.click(); install.click(); });
  check(installs === 1 && props.appUpdateState.status === "downloading", "Online update did not directly install exactly once");
  check(!document.querySelector(".app-update-confirmation"), "Online update added a second confirmation");
  cases.push("online-update-once");
  check(!element<HTMLProgressElement>(".app-update-panel progress").hasAttribute("value"), "Unknown download size is not indeterminate");
  await update({ contentLength: 1000, downloadedBytes: 420, downloadPercent: 42 });
  check(element<HTMLProgressElement>(".app-update-panel progress").value === 42, "Download progress incorrect");
  check(element(".app-update-trigger").title === "正在下载更新 · 0.5.0 · 42%", "Header download progress is stale");
  cases.push("download-progress");
  await update({ status: "installing", downloadPercent: 100 });
  check(!element<HTMLProgressElement>(".app-update-panel progress").hasAttribute("value"), "Installation pretends to have download progress");
  await update({ status: "failed", error: "安装失败。\n" + "diagnostic-".repeat(180) });
  check(element(".app-update-error").textContent === props.appUpdateState.error, "Failure details lost"); panelBounds();
  check(button("重新检查") && installs === 1, "Failed install must recheck before installing again");
  await close(); noPrompt("dismissed failure");
  await openUpdates();
  check(element(".app-update-error").textContent === props.appUpdateState.error && checks === 0, "Header entry retried or lost the failure");
  await close();
  cases.push("installing-and-failure");
  await click(element(".shell-activity-notice-actions .app-update-action"));
  check(checks === 1 && installs === 1 && props.appUpdateState.status === "checking", "Activity retry bypassed checking");
  panelBounds();
  await update({ status: "available", error: null });
  check(button("在线更新"), "Explicit retry failed to reopen the dismissed version");
  await click(button("在线更新")); check(installs === 2, "Rechecked update did not install");
  cases.push("activity-retry");
  await update({ contentLength: 1000, downloadedBytes: 420, downloadPercent: 42 });
  await close();
  check(element<HTMLProgressElement>(".shell-update-progress progress").value === 42, "Closing the prompt lost activity progress");
  await openUpdates();
  check(element<HTMLProgressElement>(".app-update-panel progress").value === 42, "Header entry lost an active download");
  await close();
  await update({ status: "installing", downloadPercent: 100 }); noPrompt("installation after dismissal");
  check(element(".shell-activity-bar").textContent?.includes("正在安装更新"), "Closed prompt lost installation feedback");
  await openUpdates(); check(!element<HTMLProgressElement>(".app-update-panel progress").hasAttribute("value"), "Reopened installation shows download progress");
  await close();
  await current(); noPrompt("current after progress", false);
  await update({ status: "available", availableVersion: "0.5.0" }); noPrompt("same-version check after progress");
  cases.push("closed-progress");

  // The asynchronous external-boundary fixture uses the real prompt to exercise
  // late completion independently of the browser's synchronous window.open API.
  isolatedPrompt = true;
  await update({ availableVersion: "1.0.0" }); await openUpdates();
  await click(button("前往下载")); const oldSuccess = deferredDownload.resolve;
  check(oldSuccess, "Deferred download did not start");
  await update({ availableVersion: "1.1.0" });
  check(!document.querySelector(".app-update-panel"), "A newer release automatically replaced an open prompt");
  await openUpdates();
  await act(async () => { oldSuccess(); });
  check(element(".app-update-heading h2").textContent?.includes("1.1.0"), "Old download completion closed the new version prompt");
  await click(button("前往下载")); const oldFailure = deferredDownload.reject;
  check(oldFailure, "Deferred failing download did not start");
  await update({ availableVersion: "1.2.0" }); await openUpdates();
  await act(async () => { oldFailure(new Error("OLD_DOWNLOAD_ERROR")); });
  check(!document.querySelector(".app-update-error"), "Old download error contaminated a newer version");
  await click(button("前往下载")); const unmountedFailure = deferredDownload.reject;
  check(unmountedFailure, "Unmount fixture download did not start");
  showIsolatedPrompt = false; await act(async () => { render(); });
  await act(async () => { unmountedFailure(new Error("UNMOUNTED_DOWNLOAD_ERROR")); });
  check(!document.querySelector(".app-update-panel"), "Unmounted download recreated the prompt");
  cases.push("stale-download-isolation");

  isolatedPrompt = false;
  await update({ availableVersion: "0.6.0", releaseNotes: "改进桌面安装与更新体验。\n优化服务器运行管理和错误提示。" });
  noPrompt("remounted shell with known release"); await openUpdates();
  for (const theme of ["light", "dark"] as const) {
    props.theme = theme; document.documentElement.dataset.theme = theme; await act(async () => { render(); }); panelBounds(); headerLayout(true);
  }
  await act(async () => { localeChange("en-US"); });
  await settleUntil(() => document.querySelector(".app-update-panel")?.textContent?.includes("Update available") === true, "English catalog did not load");
  check(button("Download installer") && button("Update now"), "Update actions are not localized"); panelBounds();
  check(element(".app-update-trigger").title === "Update available · 0.6.0", "Header hint is not localized");
  await act(async () => { localeChange("zh-CN"); });
  await settleUntil(() => document.querySelector(".app-update-panel")?.textContent?.includes("发现新版本") === true, "Chinese catalog did not load");
  if (innerWidth < 1000) { props.theme = "light"; document.documentElement.dataset.theme = "light"; await act(async () => { render(); }); }
  await settleUntil(() => [...element(".app-update-panel").querySelectorAll("button")]
    .every((control) => control.getAnimations().length === 0), "Update button theme transitions did not settle");
  cases.push("theme-and-viewport");
  await close(); noPrompt("ready for later download");
  check(element(".app-update-trigger").matches(":focus-visible"), "Keyboard dismissal lost its focus indicator");
  await pointerClick(element(".app-update-trigger")); panelBounds();
  await pointerClick(element(".app-update-close")); noPrompt("pointer dismissal");
  const trigger = element(".app-update-trigger");
  check(document.activeElement === trigger, "Pointer dismissal lost the update entry's focus");
  check(!trigger.matches(":focus-visible") && getComputedStyle(trigger).outlineStyle === "none",
    "Pointer interaction leaves an outline around the update entry");
  check(getComputedStyle(trigger).borderTopColor === "rgba(0, 0, 0, 0)" && getComputedStyle(trigger).boxShadow === "none",
    "Update entry has a persistent border or shadow");
  cases.push("pointer-without-frame");
  await document.fonts.ready;
  check(errors.length === 0, errors.join("; ")); cases.push("browser-errors");
  return { status: "passed", cases, install_calls: installs, check_calls: checks, installer_urls: installerUrls, browser_errors: errors };
}
run().catch((error) => ({ status: "failed", error: String(error), cases, browser_errors: errors }))
  .then((report) => {
    // The assertions are complete; the live System page can update while the
    // browser runner captures the screenshot, just as in the main-page fixture.
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
    return fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) });
  });
