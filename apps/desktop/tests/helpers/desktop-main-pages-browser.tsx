import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import { checkStoragePathLayout } from "./storage-path-layout";
import React, { act, StrictMode, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { bootstrapApp, readModuleDetails } from "../../src/api";
import { invokeMock } from "../../src/api-mock";
import { createDefaultAiSettings } from "../../src/ai-settings";
import { createInitialAppUpdateState } from "../../src/app-update-model";
import { I18nProvider } from "../../src/i18n";
import { AppShell } from "../../src/components/AppShell";
import { useArkToolsInstallationStore } from "../../src/components/ArkToolsInstallationContext";
import type { ArkToolsInstallationStore } from "../../src/ark-tools-installation-store";
import type { ArkToolsInstallProgress } from "../../src/ark-tools-installation";
import { LibraryView } from "../../src/views/LibraryView";
import { SystemView } from "../../src/views/SystemView";
import { StorageInitializationView } from "../../src/components/StorageInitializationView";
import { ViewLoadingState } from "../../src/views/ViewLoadingState";
import { createAppPathSettingsDraft } from "../../src/views/system/app-path-settings-draft";
import { useStorageMigration, type StorageMigrationModel } from "../../src/hooks/useStorageMigration";
import type { StorageMigrationApi, StorageMigrationStatus } from "../../src/storage-migration-types";
import type { AppPathSettingsInput } from "../../src/types";
import type { ModuleProgramInventory } from "../../src/storage-management-types";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
const noOperation = () => {};
let checks = 0;
function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(description);
  checks++;
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const selected = fixture.querySelector<T>(selector);
  if (!selected) throw new Error(`Missing ${selector}`);
  return selected;
}
async function settleUntil(predicate: () => boolean, description: string) {
  const deadline = performance.now() + 8000;
  while (!predicate()) {
    if (performance.now() > deadline) throw new Error(description);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
function visible(target: HTMLElement, label: string) {
  const box = target.getBoundingClientRect();
  check(box.width > 0 && box.height > 0 && box.left >= -1 && box.right <= innerWidth + 1
    && box.top >= -1 && box.bottom <= innerHeight + 1, `${label} is outside viewport: ${JSON.stringify(box.toJSON())}`);
  const hit = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
  check(hit && (target.contains(hit) || hit.contains(target)), `${label} is covered or clipped`);
}
function horizontalFit(selector: string) {
  const target = element(selector);
  const outer = target.getBoundingClientRect();
  const overflow = [...target.querySelectorAll<HTMLElement>("*")].filter((child) => {
    const box = child.getBoundingClientRect();
    return box.width > 0 && (box.left < outer.left - 1 || box.right > outer.right + 1
      || child.scrollWidth > child.clientWidth + 1);
  }).slice(0, 6).map((child) => {
    const { x, y, width, height } = child.getBoundingClientRect();
    return { tag: child.tagName, class: child.className, scrollWidth: child.scrollWidth, clientWidth: child.clientWidth,
      rect: { x, y, width, height } };
  });
  check(target.clientWidth > 0 && target.scrollWidth <= target.clientWidth + 1,
    `${selector} overflows horizontally ${target.scrollWidth}/${target.clientWidth}: ${JSON.stringify(overflow)}`);
}
function shellGeometry() {
  check(document.documentElement.scrollWidth <= innerWidth + 1, "Window overflows horizontally");
  check(document.documentElement.scrollHeight <= innerHeight + 1, "Window content escaped its owned scroll area");
  for (const selector of [".shell-header", ".shell-content-scroll", ".shell-activity-bar"]) horizontalFit(selector);
  visible(element(".shell-theme-icon-button"), "Theme control");
  visible(element(".shell-activity-bar"), "Activity bar");
}
function centeredIn(selector: string, container: string) {
  const target = element(selector); const parent = element(container);
  const rect = target.getBoundingClientRect(); const bounds = parent.getBoundingClientRect();
  const style = getComputedStyle(parent);
  const centerX = bounds.left + parent.clientLeft + (parent.clientWidth + parseFloat(style.paddingLeft) - parseFloat(style.paddingRight)) / 2;
  const centerY = bounds.top + parent.clientTop + (parent.clientHeight + parseFloat(style.paddingTop) - parseFloat(style.paddingBottom)) / 2;
  check(Math.abs(rect.x + rect.width / 2 - centerX) <= 2 && Math.abs(rect.y + rect.height / 2 - centerY) <= 2
    && getComputedStyle(target).textAlign === "center",
    `${selector} must be centered inside ${container}: ${JSON.stringify({ centerX, centerY,
      actualX: rect.x + rect.width / 2, actualY: rect.y + rect.height / 2, scrollTop: parent.scrollTop,
      parentHeight: parent.clientHeight, contentHeight: parent.scrollHeight })}`);
}
async function reach(selector: string) {
  const target = element(selector);
  target.scrollIntoView({ block: "nearest", inline: "nearest" });
  await act(async () => { await new Promise<void>((resolve) => requestAnimationFrame(() => resolve())); });
  visible(target, selector);
  return target;
}
async function pressEnter(target: HTMLElement) {
  await act(async () => { target.focus(); });
  check(document.activeElement === target, "Control did not receive keyboard focus");
  await act(async () => {
    const response = await fetch(`/__reliability_key/${nonce}/Enter`, { method: "POST" });
    check(response.ok, "Native Enter dispatch failed");
  });
}
async function input(selector: string, value: string) {
  await act(async () => {
    const target = element<HTMLInputElement>(selector);
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(target, value);
    target.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

async function compactAction(selector: string) {
  const target = element(selector);
  // The library's entrance animation scales ancestors. Measure the settled
  // border box rather than mistaking a transient transform for control sizing.
  await settleUntil(() => Math.abs(target.getBoundingClientRect().height - target.offsetHeight) < 0.01,
    `${selector} did not settle at its layout size`);
  const style = getComputedStyle(target);
  const height = target.offsetHeight;
  check(style.fontSize === "13px" && style.fontWeight === "500", `${selector} must use compact medium-weight text`);
  check(height >= 28 && height <= 30.5, `${selector} must remain a compact, usable target: ${height}`);
  check(target.scrollWidth <= target.clientWidth + 1 && target.scrollHeight <= target.clientHeight + 1,
    `${selector} clips its label at the compact size`);
  return { height, fontSize: style.fontSize, lineHeight: style.lineHeight };
}

async function run() {
  const fonts = await document.fonts.load('500 13px "Inter"', "LanGame 0123456789");
  check(fonts.length > 0 && fonts.every((face) => face.status === "loaded"), "Bundled Inter must load before layout checks");
  await document.fonts.ready;
  const config = await fetch("/__desktop_layout_page").then((response) => response.json()) as {
    page: "system" | "catalog" | "detail"; locale: "zh-CN" | "en-US";
  };
  localStorage.setItem("langame.locale", config.locale);
  const bootstrap = await bootstrapApp({ includeSystemSnapshot: true });
  const aiSettings = createDefaultAiSettings();
  const activeView = config.page === "system" ? "system" : "library";
  const shellProps: Omit<ComponentProps<typeof AppShell>, "children"> = {
    activeView, theme: "dark", serverCount: bootstrap.state.instances.length, aiSettings, activityText: "界面已就绪",
    appUpdatesEnabled: true, appUpdateState: createInitialAppUpdateState("0.1.0"), assistant: { panelTitle: "助手", tone: "info" }, assistantDraft: "",
    assistantExecution: { status: "idle", promptLabel: null, result: null, error: null },
    assistantMessages: [], assistantConversations: [], assistantActiveConversationId: null,
    assistantInput: { aiSettings, locale: config.locale, activeJobsCount: 0, activeView, bootstrap,
      storageReady: true, libraryPage: config.page === "detail" ? "detail" : "catalog", overlayNames: [],
      runtimeAutoRefreshPaused: false, runtimeRefreshIssue: null, selectedInstanceDetails: null, selectedInstanceId: null,
      selectedModuleId: null, selectedInstanceModuleDetails: null, selectedLaunchPlan: null, selectedLaunchPlanError: null,
      selectedLogDocument: null, selectedModuleDetails: null, selectedRuntime: null, serverWorkspaceSection: "overview", steamCmdStatus: null },
    jobs: [], steamCmdProgress: null, steamCmdMessage: "", steamCmdStopPending: false, steamCmdStopError: null,
    installationStopPendingIds: [], installationStopErrors: {}, onCancelInstallation: noOperation, onCancelSteamCmd: noOperation,
    runtimeRefreshIssue: null, runtimeAutoRefreshPaused: false, runtimePollIntervalMs: 5000, runtimeRefreshFailureLimit: 3,
    onResumeRuntimeAutoRefresh: noOperation, onAssistantAction: noOperation, onAssistantDeleteConversation: noOperation,
    onAssistantDraftChange: noOperation, onAssistantNewConversation: noOperation, onAssistantSelectConversation: noOperation,
    onAssistantRunPrompt: noOperation, onAssistantSendMessage: noOperation, onCheckAppUpdate: noOperation,
    onClearAiSecret: async (next) => next, onInstallAppUpdate: noOperation, onSaveAiSettings: async (next) => next,
    onSelectView: noOperation, onThemeChange: noOperation
  };
  let openedInstances = 0;
  let directoryPicks = 0;
  let chosenDirectory: string | null = null;
  let pickerStart: string | null | undefined;
  let saveError: string | null = null;
  let saveWait: Promise<void> | null = null;
  let previewWait: Promise<void> | null = null;
  const pathWrites: AppPathSettingsInput[] = [];
  const pathAttempts: typeof pathWrites = [];
  let openedModules = 0;
  let returnedToCatalog = 0;
  const created: Array<{ name: string; module_id: string }> = [];
  const systemProps: Omit<ComponentProps<typeof SystemView>, "storageMigration"> = {
    snapshot: bootstrap.state.snapshot, instances: bootstrap.state.instances, bindAddressCandidates: [], appSettings: bootstrap.state.settings,
    steamCmdStatus: null, steamCmdBusy: false, steamCmdProgress: null, steamCmdMessage: "",
    onOpenInstance: () => { openedInstances++; },
    onPickDirectory: async (current) => { directoryPicks++; pickerStart = current; return chosenDirectory; },
    onEnsureSteamCmd: noOperation, onUninstallSteamCmd: noOperation
  };
  let migrationSnapshot: StorageMigrationStatus | null = null;
  let migrationModel: StorageMigrationModel;
  let migrationSequence = 0;
  let systemVisible = true;
  let arkStore: ArkToolsInstallationStore;
  let arkProgress: ArkToolsInstallProgress = { instanceId: "fixture-ark", operationId: "fixture-ark-install", active: true,
    phase: "downloading", source: "fixture", downloadedBytes: 50, totalBytes: 100, elapsedSeconds: 1, error: null };
  function ArkActivityFixture() { arkStore = useArkToolsInstallationStore(); return null; }
  // Explicit synthetic service boundary; the real observer and panel run unchanged.
  const migrationApi: StorageMigrationApi = {
    available: () => config.page === "system",
    read: async () => migrationSnapshot,
    preview: async input => {
      if (previewWait) await previewWait;
      const current = createAppPathSettingsDraft(systemProps.appSettings);
      migrationSnapshot = { operation_id: `layout-migration-${++migrationSequence}`, phase: "ready", input: { ...input },
        roots: (Object.keys(input) as Array<keyof AppPathSettingsInput>).filter(key => input[key] !== current[key])
          .map(key => ({ source: current[key], target: input[key] })),
        total_files: 3, total_bytes: 4096, completed_files: 0, completed_bytes: 0, reused_files: 0,
        conflicts: [], conflict_count: 0, can_cancel: true, recovery_required: false,
        error: null, cleanup_warnings: [], preserved_destinations: [] };
      return { ...migrationSnapshot, phase: "checking" };
    },
    execute: async operationId => {
      check(migrationSnapshot?.operation_id === operationId && migrationSnapshot.phase === "ready", "Migration must use its reviewed identity");
      const next = { ...migrationSnapshot.input };
      pathAttempts.push(next);
      migrationSnapshot = { ...migrationSnapshot, phase: "copying" };
      const complete = () => {
        check(migrationSnapshot?.operation_id === operationId, "A deferred result changed migration identity");
        migrationSnapshot = { ...migrationSnapshot, phase: saveError ? "failed" : "completed", can_cancel: false,
          error: saveError, completed_files: saveError ? 0 : 3, completed_bytes: saveError ? 0 : 4096,
          preserved_destinations: saveError ? [] : migrationSnapshot.roots.slice(0, 1).map(root => ({
            original_path: root.target, backup_path: `${root.target}.previous-contents` })) };
        if (!saveError) pathWrites.push(next);
      };
      if (saveWait) void saveWait.then(() => { complete(); migrationModel.recheck(); });
      else complete();
      return migrationSnapshot;
    },
    cancel: async operationId => {
      check(migrationSnapshot?.operation_id === operationId, "Cancellation must use the existing migration");
      migrationSnapshot = { ...migrationSnapshot, phase: "cancelled", can_cancel: false };
      return migrationSnapshot;
    }
  };
  const sampleDetails = await readModuleDetails("minecraft");
  // Synthetic identities prevent remote store/media requests; the page, real
  // catalog motion, details component and all interaction code run unchanged.
  const modules = Array.from({ length: 12 }, (_, index) => ({ ...sampleDetails.summary,
    id: `layout-library-${index}`, name: `Synthetic Library Game ${index + 1}`, install_state: "Installed" as const, steam_app_id: null }));
  if (config.page === "detail") {
    // Program ownership is a native boundary; give this synthetic installed
    // game an explicit inventory instead of asking the preview to inspect files.
    const inventory: ModuleProgramInventory = {
      requires_archive_inventory: false,
      installations: [{ id: 1, install_root: "D:/Fixture/library", scope: "library", install_state: "Installed",
        current_version: "fixture-version", used_by: [], modification_state: "unverified", pending_removal: false, size_bytes: 1024 }],
      creation: { can_create: true, action: "existing_install", program_path: "D:/Fixture/library", additional_bytes: 0, reason: null }
    };
    Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: (command: string, args: { input?: { module_id?: string } } = {}) => {
      if (command === "inspect_module_programs" && args.input?.module_id === modules[0].id) return Promise.resolve(inventory);
      return invokeMock(command, args);
    } } });
  }
  const libraryProps: ComponentProps<typeof LibraryView> = {
    steamCmdStatus: systemProps.steamCmdStatus, steamCmdBusy: false,
    mode: config.page === "detail" ? "detail" : "catalog", modules, selectedModuleId: modules[0].id,
    selectedModuleDetails: { ...sampleDetails, summary: modules[0] }, search: "", catalogFocusId: modules[0].id,
    catalogScrollLeft: 0, jobs: [], creatingModuleIds: new Set(),
    onSearchChange: (search) => { libraryProps.search = search; render(); },
    onCatalogFocusChange: noOperation, onCatalogScrollLeftChange: noOperation,
    onOpenModule: () => { openedModules++; }, onBackToCatalog: () => { returnedToCatalog++; },
    onInstall: noOperation, onUninstall: noOperation, onCreateServer: async (value) => { created.push(value); }
  };
  let pageState: React.ReactNode = null;
  function Harness() {
    migrationModel = useStorageMigration(async status => {
      systemProps.appSettings = { ...systemProps.appSettings, ...status.input };
    }, systemProps.appSettings, migrationApi);
    return <AppShell {...shellProps} storageMigration={migrationModel}>
      <ArkActivityFixture />
      {pageState ?? (config.page === "system" ? systemVisible ? <SystemView {...systemProps} storageMigration={migrationModel} />
        : <div className="fixture-away-page">Another workspace</div> : <LibraryView {...libraryProps} />)}
    </AppShell>;
  }
  function render() {
    root.render(<StrictMode><I18nProvider><Harness /></I18nProvider></StrictMode>);
  }
  await act(async () => { await prepareBrowserLocaleCatalogs(); render(); });
  const pageSelector = config.page === "system" ? ".system-dashboard-page"
    : config.page === "catalog" ? ".library-home-page" : ".library-detail-page";
  await settleUntil(() => Boolean(fixture.querySelector(pageSelector)), `${config.page} page did not mount`);
  await settleUntil(() => element(".shell-locale-button").textContent === (config.locale === "en-US" ? "ZH" : "EN"),
    "Requested interface language did not load");
  await act(async () => { await new Promise<void>((resolve) => requestAnimationFrame(() => resolve())); });
  await settleUntil(() => !migrationModel.recovering, "Migration recovery did not settle");
  shellGeometry();
  horizontalFit(pageSelector);
  const controls: Record<string, Awaited<ReturnType<typeof compactAction>>> = {};
  if (config.page === "system") {
    let retries = 0;
    const onRetry = () => { retries++; };
    pageState = <StorageInitializationView state={{ status: "pending", attempt: 0 }} onRetry={onRetry} />;
    await act(async () => { render(); });
    centeredIn(".storage-initialization-content", ".shell-content-scroll");
    const failure = { status: "failed" as const, attempt: 0, error: "Storage could not be opened.",
      databasePath: null, logPath: null, logDirectory: null };
    pageState = <StorageInitializationView state={failure} onRetry={onRetry} />;
    await act(async () => { render(); });
    centeredIn(".storage-initialization-content", ".shell-content-scroll");
    pageState = <StorageInitializationView state={{ ...failure, error: "Diagnostic details.\n".repeat(60) }} onRetry={onRetry} />;
    await act(async () => { render(); });
    await pressEnter(await reach(".storage-initialization-details summary"));
    check(element<HTMLDetailsElement>(".storage-initialization-details").open, "Full diagnostics must expand");
    await pressEnter(await reach(".storage-initialization-retry-button"));
    check(retries === 1, "Long diagnostics must keep the retry action reachable");
    shellGeometry();
    pageState = <ViewLoadingState error={null} canRetry={false} onRetry={onRetry} />;
    await act(async () => { render(); });
    centeredIn(".view-loading-state > p", ".shell-content-scroll");
    pageState = <ViewLoadingState error={new Error("Fixture view load failure")} canRetry onRetry={onRetry} />;
    await act(async () => { render(); });
    await pressEnter(await reach(".view-loading-state button"));
    check(Number(retries) === 2, "Centered view-load failures must retain retry behavior");
    pageState = null;
    await act(async () => { render(); });
    await settleUntil(() => Boolean(fixture.querySelector(".system-dashboard-page")), "The system page must return after a whole-page state");
    horizontalFit(".system-command-grid");
    await reach(".system-panel-head h2");
    check(getComputedStyle(element(".system-panel-head h2")).fontSize === "16px", "System section titles must use the compact heading role");
    controls.directoryPicker = await compactAction(".system-path-picker-button");
    checkStoragePathLayout(fixture, check);
    for (const metric of fixture.querySelectorAll<HTMLElement>(".system-top-metric")) {
      metric.scrollIntoView({ block: "nearest" });
      visible(metric, "System metric");
    }
    const target = await reach(".system-instance-item");
    await pressEnter(target);
    check(openedInstances === 1, "System instance navigation lost its callback");
    await pressEnter(await reach(".system-path-picker-button"));
    check(directoryPicks === 1, "System directory picker is not keyboard operable");
    const archiveRow = '.system-path-row[data-path-key="archives_root"]';
    const pathAction = (index: number) => `${archiveRow} .system-runtime-path-actions button:nth-child(${index})`;
    const choosePath = (key: string) => `.system-path-row[data-path-key="${key}"] .system-path-picker-button`;
    const displayedPath = (key: string) => element(`.system-path-row[data-path-key="${key}"] .system-runtime-path`).textContent;
    const runtimeAction = () => element<HTMLButtonElement>(".system-runtime-check-button");
    const completed = () => settleUntil(() => migrationModel.status?.phase === "completed" && !migrationModel.pending
      && !migrationModel.refreshing && !runtimeAction().disabled, "Automatic migration did not finish");
    const openActivity = async () => { if (!fixture.querySelector(".shell-activity-panel")) await pressEnter(await reach(".shell-activity-expand")); };
    const closeActivity = async () => { if (fixture.querySelector(".shell-activity-panel")) await pressEnter(await reach(".shell-activity-panel-close")); };
    const originalPaths = systemProps.appSettings;
    const copiedPaths: string[] = [];
    const openedPaths: string[] = [];
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: async (value: string) => { copiedPaths.push(value); } } });
    Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: (command: string, args: { path?: string } = {}) => {
      if (command === "open_local_path") { openedPaths.push(args.path!); return Promise.resolve(); }
      if (command === "read_ark_tools_install_progress") return Promise.resolve(arkProgress);
      return invokeMock(command, args);
    } } });
    check([...fixture.querySelectorAll(".system-path-row")].map(row => row.getAttribute("data-path-key")).join(",")
      === "games_root,servers_root,archives_root,steamcmd_root", "Archive directory is the third ordinary path row before SteamCMD");
    check(!fixture.querySelector(".system-storage-management,.storage-management-panel,.system-path-editor-footer,.system-path-save-button,.system-path-discard-button,.system-path-migration-review"),
      "Idle system paths must have four rows without a permanent migration form or retired storage interface");
    await pressEnter(await reach(pathAction(1))); await pressEnter(await reach(pathAction(2)));
    check(copiedPaths[0] === originalPaths.archives_root && openedPaths[0] === originalPaths.archives_root,
      "Archive copy/open controls use the configured archive directory exactly");
    check(!fixture.querySelector(pathAction(3)) && !fixture.querySelector(choosePath("archives_root"))
      && element(`${archiveRow} .system-runtime-label`).textContent === (config.locale === "zh-CN" ? "实例归档区" : "Instance archives"),
      "Archives must display only their label, path and copy/open controls");
    await pressEnter(await reach(choosePath("steamcmd_root")));
    check(pickerStart === originalPaths.steamcmd_root && migrationSequence === 0 && !fixture.querySelector(".system-storage-dialog"),
      "Cancelling a native chooser must not create a migration");
    let finishPreview = () => {};
    previewWait = new Promise<void>(resolve => { finishPreview = resolve; });
    chosenDirectory = "  D:/Fixture steamcmd  ";
    await pressEnter(await reach(choosePath("steamcmd_root")));
    check(pathWrites.length === 0 && systemProps.appSettings.steamcmd_root === originalPaths.steamcmd_root
      && displayedPath("steamcmd_root") === originalPaths.steamcmd_root && migrationModel.draft?.steamcmd_root === "D:/Fixture steamcmd"
      && !fixture.querySelector(".system-path-unapplied"),
      "Automatic preflight must retain configured paths while checking the separate trimmed target");
    check(!fixture.querySelector("dialog,.system-migration-apply") && runtimeAction().disabled
      && element<HTMLButtonElement>(pathAction(1)).disabled && element<HTMLButtonElement>(pathAction(2)).disabled,
      "Pending paths must guard runtime, copy and open actions without opening confirmation");
    systemProps.appSettings = { ...systemProps.appSettings };
    await act(async () => { render(); });
    check(displayedPath("steamcmd_root") === originalPaths.steamcmd_root && migrationModel.draft?.steamcmd_root === "D:/Fixture steamcmd"
      && pathWrites.length === 0, "A refreshed settings snapshot changed the configured path or erased the pending selection");
    await act(async () => { finishPreview(); }); previewWait = null;
    await completed();
    const saved = pathWrites[0];
    check(pathWrites.length === 1 && Object.keys(saved).sort().join(",") === "archives_root,games_root,servers_root,steamcmd_root"
      && saved.archives_root === originalPaths.archives_root && saved.games_root === originalPaths.games_root
      && saved.servers_root === originalPaths.servers_root && saved.steamcmd_root === "D:/Fixture steamcmd",
      "A selection automatically submits all four paths while preserving unchanged roots");
    await pressEnter(await reach(pathAction(1))); await pressEnter(await reach(pathAction(2)));
    check(displayedPath("archives_root") === saved.archives_root && copiedPaths.at(-1) === saved.archives_root
      && openedPaths.at(-1) === saved.archives_root, "Configured path controls follow the confirmed migration");
    chosenDirectory = " D:/Fixture steamcmd "; const sequenceBeforeSame = migrationSequence;
    await pressEnter(await reach(choosePath("steamcmd_root")));
    check(pathWrites.length === 1 && migrationSequence === sequenceBeforeSame && !fixture.querySelector(".system-storage-dialog"),
      "Selecting the same folder must not create another migration");
    chosenDirectory = "D:/Next steamcmd"; saveError = "STEAMCMD_PATH_WRITE_FAILED";
    await pressEnter(await reach(choosePath("steamcmd_root")));
    await settleUntil(() => migrationModel.status?.phase === "failed", "Migration failure did not become visible");
    await openActivity();
    check(pathWrites.length === 1 && displayedPath("steamcmd_root") === "D:/Fixture steamcmd" && migrationModel.draft?.steamcmd_root === "D:/Next steamcmd"
      && systemProps.appSettings.steamcmd_root === "D:/Fixture steamcmd"
      && element(".shell-activity-panel").textContent?.includes(saveError)
      && !element(".system-runtime-panel").textContent?.includes(saveError),
      "Failed migration must show the configured path and retain its target and diagnostic only for activity recovery");
    const picksBeforeRetry = directoryPicks;
    saveError = null; await pressEnter(await reach(".shell-activity-panel .system-migration-retry")); await completed(); await closeActivity();
    check(pathWrites.length === 2 && !fixture.textContent?.includes("STEAMCMD_PATH_WRITE_FAILED") && directoryPicks === picksBeforeRetry,
      "Successful retry must clear failure without selecting paths again");
    chosenDirectory = "D:/Discarded steamcmd"; saveError = "TARGET_BUSY";
    await pressEnter(await reach(choosePath("steamcmd_root")));
    await settleUntil(() => migrationModel.status?.phase === "failed", "Discard case did not fail");
    systemProps.appSettings = { ...systemProps.appSettings, games_root: "D:/Refreshed server files" };
    await act(async () => { render(); });
    await openActivity(); await pressEnter(await reach(".shell-activity-panel .system-migration-discard")); await closeActivity();
    check(pathWrites.length === 2 && displayedPath("steamcmd_root") === "D:/Next steamcmd"
      && displayedPath("games_root") === "D:/Refreshed server files" && !runtimeAction().disabled,
      "Discard must retain the latest configuration and clear pending paths");
    saveError = null;

    systemProps.appSettings = { ...originalPaths, servers_root: "E:\\LanGame\\instances", archives_root: "E:\\LanGame\\instances\\.trash" };
    await act(async () => { render(); });
    let finishSave = () => {};
    saveWait = new Promise<void>(resolve => { finishSave = resolve; });
    const attemptsBeforeSave = pathAttempts.length;
    chosenDirectory = " D:\\LanGame\\instances ";
    await pressEnter(await reach(choosePath("servers_root")));
    await settleUntil(() => migrationModel.status?.phase === "copying", "A checked workspace did not automatically migrate");
    check(displayedPath("servers_root") === "E:\\LanGame\\instances" && displayedPath("archives_root") === "E:\\LanGame\\instances\\.trash"
      && migrationModel.draft?.servers_root === "D:\\LanGame\\instances" && migrationModel.draft?.archives_root === "D:\\LanGame\\instances\\.trash"
      && systemProps.appSettings.servers_root === "E:\\LanGame\\instances" && pathWrites.length === 2,
      "E: to D: must preflight the workspace and its default archives together without an intermediate write");
    check(pathAttempts.length === attemptsBeforeSave + 1 && pathWrites.length === 2 && runtimeAction().disabled
      && [...fixture.querySelectorAll<HTMLButtonElement>(".system-path-row button")].every(button => button.disabled),
      "Running migration must lock targets and prevent duplicate submission");
    check(!fixture.querySelector(".system-storage-dialog"), "Automatic migration opens a confirmation dialog");
    systemVisible = false;
    await act(async () => { render(); });
    check(Boolean(fixture.querySelector(".fixture-away-page")) && Boolean(fixture.querySelector(".shell-activity-bar .system-migration-activity")),
      "Leaving system settings must preserve migration activity");
    await act(async () => { arkStore.recover("fixture-ark", "Fixture ARK"); });
    await settleUntil(() => Boolean(fixture.querySelector(".shell-activity-progress")?.textContent?.includes("Fixture ARK")),
      "Migration must not replace the existing ARK activity");
    await pressEnter(await reach(".shell-activity-expand"));
    check(Boolean(fixture.querySelector(".shell-activity-panel .system-migration-activity"))
      && Boolean(fixture.querySelector(".shell-activity-panel")?.textContent?.includes("Fixture ARK")),
      "Global task details retain both ARK and storage migration progress");
    await pressEnter(await reach(".shell-activity-panel-close"));
    arkProgress = { ...arkProgress, active: false, phase: "failed", error: "fixture observation complete" };
    await settleUntil(() => arkStore.getSnapshot().some(entry => !entry.observing && entry.progress?.phase === "failed"),
      "Synthetic ARK observation did not settle");
    await act(async () => { arkStore.dismiss(arkProgress.operationId); });
    systemVisible = true;
    await act(async () => { render(); });
    check(displayedPath("servers_root") === "E:\\LanGame\\instances" && migrationModel.draft?.servers_root === "D:\\LanGame\\instances",
      "Returning to system settings changed the configured path or lost the submitted target");
    await act(async () => { finishSave(); }); saveWait = null;
    await completed();
    check(pathWrites.length === 3 && pathWrites.at(-1)?.servers_root === "D:\\LanGame\\instances"
      && pathWrites.at(-1)?.archives_root === "D:\\LanGame\\instances\\.trash"
      && pathWrites.at(-1)?.games_root === originalPaths.games_root && pathWrites.at(-1)?.steamcmd_root === originalPaths.steamcmd_root
      && !fixture.querySelector(".system-path-migration-review,.system-migration-open"),
      "Successful migration must apply the full combination and return to the compact main panel");

    systemProps.appSettings = { ...originalPaths, servers_root: "E:\\LanGame\\instances", archives_root: "E:\\Custom archives" };
    await act(async () => { render(); });
    let finishExternalPreview = () => {};
    previewWait = new Promise<void>(resolve => { finishExternalPreview = resolve; });
    chosenDirectory = "D:\\Custom instances";
    await pressEnter(await reach(choosePath("servers_root")));
    check(displayedPath("servers_root") === "E:\\LanGame\\instances" && displayedPath("archives_root") === "E:\\Custom archives"
      && migrationModel.draft?.servers_root === "D:\\Custom instances" && migrationModel.draft.archives_root === "D:\\Custom instances\\.trash"
      && pathWrites.length === 3 && !fixture.querySelector(choosePath("archives_root")),
      "External archives must be included in the pending workspace migration without a separate chooser or early path publication");
    check(!element(".shell-activity-bar").textContent?.includes(config.locale === "zh-CN" ? "同盘" : "volume"),
      "Automatic archive migration must not direct the user to choose another volume");
    await act(async () => { finishExternalPreview(); }); previewWait = null;
    await completed();
    check(pathWrites.length === 4 && pathWrites[3].servers_root === "D:\\Custom instances"
      && pathWrites[3].archives_root === "D:\\Custom instances\\.trash"
      && displayedPath("archives_root") === "D:\\Custom instances\\.trash",
      "The workspace and its old external archives must be committed by the same migration");
    chosenDirectory = null; const previewsBeforeCancel = migrationSequence;
    await pressEnter(await reach(choosePath("servers_root")));
    check(displayedPath("servers_root") === systemProps.appSettings.servers_root && migrationModel.draft === null
      && displayedPath("archives_root") === "D:\\Custom instances\\.trash" && migrationSequence === previewsBeforeCancel,
      "Cancelling a chooser must preserve the committed workspace and archive destination");
    for (const [key, next] of [["games_root", "D:\\Server files"], ["steamcmd_root", "D:\\Tools\\steamcmd"]]) {
      chosenDirectory = next; await pressEnter(await reach(choosePath(key))); await completed();
    }
    check(pathWrites.length === 6 && JSON.stringify(pathWrites.at(-1)) === JSON.stringify({
      games_root: "D:\\Server files", servers_root: "D:\\Custom instances", archives_root: "D:\\Custom instances\\.trash", steamcmd_root: "D:\\Tools\\steamcmd"
    }), "Server-file and SteamCMD selections must preserve the committed workspace and archive path");
    for (const fallback of [
      { workspace: "D:/LanGame/instances/", expected: "D:/LanGame/instances/.trash", missing: true },
      { workspace: "D:\\LanGame\\instances\\", expected: "D:\\LanGame\\instances\\.trash", missing: false }
    ]) {
      systemProps.appSettings = { ...originalPaths, servers_root: fallback.workspace, archives_root: "  " };
      if (fallback.missing) Reflect.deleteProperty(systemProps.appSettings, "archives_root");
      await act(async () => { render(); });
      const path = await reach(`${archiveRow} .system-runtime-path`);
      check(path.textContent === fallback.expected && path.title === fallback.expected, "Unset archives show the concrete default and full tooltip");
      await pressEnter(await reach(pathAction(1))); await pressEnter(await reach(pathAction(2)));
      check(copiedPaths.at(-1) === fallback.expected && openedPaths.at(-1) === fallback.expected, "Default archive copy/open use its displayed path");
      const previousWrites = pathWrites.length;
      check(!fixture.querySelector(choosePath("archives_root")), "Default archives must not offer a separate chooser");
      chosenDirectory = "D:/Updated server files";
      await pressEnter(await reach(choosePath("games_root"))); await completed();
      check(pathWrites.length === previousWrites + 1 && pathWrites.at(-1)?.archives_root === fallback.expected,
        "Changing another path must preserve the effective default archive location");
    }
    const retainedTarget = migrationModel.status?.preserved_destinations[0];
    check(retainedTarget && !fixture.querySelector(".system-migration-preserved")
      && element(".shell-activity-bar").textContent?.includes(config.locale === "zh-CN" ? "目录迁移已完成" : "Folder migration completed"),
      "A preserved target must leave only the normal completion label in the compact activity bar");
    await openActivity();
    check(element(".shell-activity-panel .system-migration-preserved code").textContent === retainedTarget.backup_path
      && !fixture.querySelector(".shell-activity-panel .system-migration-warnings"), "Activity details must expose the backup without misreporting a cleanup warning");
    const attemptsBeforeOpen = pathAttempts.length;
    await pressEnter(await reach(".shell-activity-panel .system-migration-backup-open"));
    check(openedPaths.at(-1) === retainedTarget.backup_path && pathAttempts.length === attemptsBeforeOpen,
      "The backup action must open only the preserved target, without repeating migration");
    await closeActivity();
    systemProps.instances = [];
    systemProps.snapshot = { ...systemProps.snapshot, running_instances: 0, total_online_players: 0,
      total_player_capacity: 0, player_count_queried_instances: 0, player_count_queryable_instances: 0,
      instance_process_count: 0, instance_process_memory_bytes: 0, instance_process_threads: 0, instance_process_handles: 0 };
    shellProps.serverCount = 0;
    await act(async () => { render(); });
    await reach(".system-instance-empty");
    centeredIn(".system-instance-empty", ".system-instance-list");
    const emptyBounds = element(".system-instance-empty").getBoundingClientRect();
    const listBounds = element(".system-instance-list").getBoundingClientRect();
    check(emptyBounds.top >= listBounds.top && emptyBounds.bottom <= listBounds.bottom,
      "Path settings must not compress the instance overview until its content is clipped");
    shellGeometry();
    await reach(".system-runtime-panel");
    horizontalFit(".system-runtime-panel");
    check(!fixture.querySelector("dialog,.system-path-migration-review,.system-path-editor-footer,.system-path-unapplied"), "Completed migration must leave only the four configured path rows");
  } else if (config.page === "catalog") {
    await reach(".library-catalog-search-input");
    await reach(".library-catalog-focus-title");
    controls.openDetails = await compactAction(".library-catalog-open-details");
    await pressEnter(await reach(".library-catalog-open-details"));
    check(openedModules === 1, "Catalog details action lost its callback");
    const rail = element(".library-catalog-rail");
    check(rail.scrollWidth > rail.clientWidth, "Large catalog must keep its intentional local horizontal scroll");
    // A page action moves selection by the visible-card count. Its next target
    // can still fit without scrolling, so exercise navigation through the end.
    const next = element<HTMLButtonElement>(".library-catalog-rail-nav.is-right");
    for (let step = 0; step < modules.length && !next.disabled; step++) {
      const previous = rail.getAttribute("aria-activedescendant");
      await pressEnter(await reach(".library-catalog-rail-nav.is-right"));
      check(rail.getAttribute("aria-activedescendant") !== previous, "Catalog pagination did not move selection");
    }
    check(next.disabled, "Catalog navigation cannot reach the final game");
    await settleUntil(() => rail.scrollLeft > 0, "Catalog pagination cannot expose more games");
    shellGeometry();
    await input(".library-catalog-search-input", "Synthetic Library Game 12");
    await settleUntil(() => fixture.querySelectorAll(".library-catalog-tile").length === 1, "Catalog search did not filter the real page");
    await reach(".library-catalog-open-details");
    await input(".library-catalog-search-input", "");
    await settleUntil(() => fixture.querySelectorAll(".library-catalog-tile").length === 12, "Cleared catalog search lost modules");
    await input(".library-catalog-search-input", "no matching library game");
    await reach(".library-catalog-empty");
    centeredIn(".library-catalog-empty", ".library-catalog-rail-shell");
  } else {
    await reach(".workspace-title");
    controls.back = await compactAction(".library-back-button");
    controls.create = await compactAction('.library-create-server-form button[type="submit"]');
    await pressEnter(await reach(".library-back-button"));
    check(returnedToCatalog === 1, "Detail back action is unreachable");
    horizontalFit(".library-detail-header");
    horizontalFit(".library-sidebar-server-actions");
    const mediaBox = element(".library-detail-stage-media").getBoundingClientRect();
    const sidebarBox = element(".library-sidebar-summary").getBoundingClientRect();
    if (innerWidth <= 900) {
      check(sidebarBox.top >= mediaBox.bottom, "Small desktop details must stack controls below the media");
      check(Math.abs(sidebarBox.width - mediaBox.width) <= 1, "Stacked detail sections must use the same available width");
    } else {
      check(sidebarBox.height <= mediaBox.height + 1,
        "The detail sidebar must not exceed the media card inside the full desktop shell");
    }
    await reach(".library-create-server-form .text-input");
    await input(".library-create-server-form .text-input", "布局验证服务器");
    await pressEnter(await reach('.library-create-server-form button[type="submit"]'));
    check(created.length === 1 && created[0].name === "布局验证服务器" && created[0].module_id === modules[0].id,
      "Detail creation action lost its selected game or edited name");
    // Synthetic games have no store record, so the real detail page owns a
    // media empty state and omits its optional store story.
    await reach(".library-media-fallback");
    check(!fixture.querySelector(".library-story-panel"), "Unknown games must not inherit another game's store story");
    libraryProps.selectedModuleId = null;
    libraryProps.selectedModuleDetails = null;
    await act(async () => { render(); });
    await reach(".library-detail-empty .empty-state");
    centeredIn(".library-detail-empty .empty-state", ".shell-content-scroll");
    const emptyScroll = element(".shell-content-scroll");
    check(emptyScroll.scrollHeight <= emptyScroll.clientHeight + 1, "The whole-page empty state must not retain decorative overflow");
    check(!fixture.querySelector(".library-detail-empty .panel-card"), "A whole-page empty state must not add a fixed-height card");
    shellGeometry();
  }
  for (const scroll of fixture.querySelectorAll<HTMLElement>(".shell-content-scroll, .system-command-grid, .library-catalog-rail")) {
    scroll.scrollTop = 0; scroll.scrollLeft = 0;
  }
  if (config.page === "system") {
    await reach(".system-runtime-panel");
  }
  check(errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, page: config.page, locale: config.locale,
    viewport: { width: innerWidth, height: innerHeight }, controls, browser_errors: errors };
}

let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error(`Main page checks stalled after ${checks} assertions`)), 30000);
})]).finally(() => {
  clearTimeout(watchdog);
  // Leave the rendered page alive for capture after the test's act scope ends.
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
}).catch((error) => ({ status: "failed", checks,
  error: `After ${checks} assertions: ${error instanceof Error ? error.stack : String(error)}`, browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
