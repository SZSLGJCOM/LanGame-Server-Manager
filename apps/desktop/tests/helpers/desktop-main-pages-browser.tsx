import React, { act, StrictMode, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { bootstrapApp, readModuleDetails } from "../../src/api";
import { invokeMock } from "../../src/api-mock";
import { createDefaultAiSettings } from "../../src/ai-settings";
import { createInitialAppUpdateState } from "../../src/app-update-model";
import { I18nProvider } from "../../src/i18n";
import { AppShell } from "../../src/components/AppShell";
import { LibraryView } from "../../src/views/LibraryView";
import { SystemView } from "../../src/views/SystemView";
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
    && getComputedStyle(target).textAlign === "center", `${selector} is horizontally and vertically centered inside ${container}`);
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
  const pathWrites: Array<Parameters<ComponentProps<typeof SystemView>["onSaveAppSettings"]>[0]> = [];
  let openedModules = 0;
  let returnedToCatalog = 0;
  const created: Array<{ name: string; module_id: string }> = [];
  const systemProps: ComponentProps<typeof SystemView> = {
    snapshot: bootstrap.state.snapshot, instances: bootstrap.state.instances, bindAddressCandidates: [], appSettings: bootstrap.state.settings,
    steamCmdStatus: null, steamCmdBusy: false, steamCmdProgress: null, steamCmdMessage: "",
    onOpenInstance: () => { openedInstances++; },
    onPickDirectory: async (current) => { directoryPicks++; pickerStart = current; return chosenDirectory; },
    onSaveAppSettings: async (next) => {
      if (saveError) throw new Error(saveError);
      systemProps.appSettings = { ...systemProps.appSettings, ...next };
      pathWrites.push(next); render();
    }, onEnsureSteamCmd: noOperation, onUninstallSteamCmd: noOperation
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
  function render() {
    root.render(<StrictMode><I18nProvider><AppShell {...shellProps}>
      {config.page === "system" ? <SystemView {...systemProps} /> : <LibraryView {...libraryProps} />}
    </AppShell></I18nProvider></StrictMode>);
  }
  await act(async () => { render(); });
  const pageSelector = config.page === "system" ? ".system-dashboard-page"
    : config.page === "catalog" ? ".library-home-page" : ".library-detail-page";
  await settleUntil(() => Boolean(fixture.querySelector(pageSelector)), `${config.page} page did not mount`);
  await settleUntil(() => element(".shell-locale-button").textContent === (config.locale === "en-US" ? "ZH" : "EN"),
    "Requested interface language did not load");
  await act(async () => { await new Promise<void>((resolve) => requestAnimationFrame(() => resolve())); });
  shellGeometry();
  horizontalFit(pageSelector);
  const controls: Record<string, Awaited<ReturnType<typeof compactAction>>> = {};
  if (config.page === "system") {
    horizontalFit(".system-command-grid");
    await reach(".system-panel-head h2");
    check(getComputedStyle(element(".system-panel-head h2")).fontSize === "16px", "System section titles must use the compact heading role");
    controls.directoryPicker = await compactAction(".system-path-picker-button");
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
    const originalPaths = systemProps.appSettings;
    const copiedPaths: string[] = [];
    const openedPaths: string[] = [];
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: async (value: string) => { copiedPaths.push(value); } } });
    Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: (command: string, args: { path?: string } = {}) => {
      if (command === "open_local_path") { openedPaths.push(args.path!); return Promise.resolve(); }
      return invokeMock(command, args);
    } } });
    check([...fixture.querySelectorAll(".system-path-row")].map((row) => row.getAttribute("data-path-key")).join(",")
      === "games_root,servers_root,archives_root,steamcmd_root", "Archive directory is the third ordinary path row before SteamCMD");
    check(!fixture.querySelector(".system-storage-management,.storage-management-panel"), "System has no retired storage management interface");
    await pressEnter(await reach(pathAction(1))); await pressEnter(await reach(pathAction(2)));
    check(copiedPaths[0] === originalPaths.archives_root && openedPaths[0] === originalPaths.archives_root,
      "Archive copy/open controls use the configured archive directory exactly");
    await pressEnter(await reach(pathAction(3)));
    check(pickerStart === originalPaths.archives_root && pathWrites.length === 0, "Cancelled archive chooser starts at current directory and does not save");
    chosenDirectory = "  D:/Fixture archives  ";
    await pressEnter(await reach(pathAction(3)));
    await settleUntil(() => pathWrites.length === 1 && !element<HTMLButtonElement>(pathAction(3)).disabled, "Archive path save did not complete");
    const saved = pathWrites[0];
    check(Object.keys(saved).sort().join(",") === "archives_root,games_root,servers_root,steamcmd_root"
      && saved.archives_root === "D:/Fixture archives" && saved.games_root === originalPaths.games_root
      && saved.servers_root === originalPaths.servers_root && saved.steamcmd_root === originalPaths.steamcmd_root,
      "Archive chooser saves the trimmed path while preserving all other configured roots");
    check(element(`${archiveRow} .system-runtime-path`).textContent === "D:/Fixture archives", "Saved archive path is displayed from updated settings");
    await pressEnter(await reach(pathAction(1))); await pressEnter(await reach(pathAction(2)));
    check(copiedPaths[1] === "D:/Fixture archives" && openedPaths[1] === "D:/Fixture archives", "Copy and open follow the successfully updated archive path");
    chosenDirectory = " D:/Fixture archives "; await pressEnter(await reach(pathAction(3)));
    check(pathWrites.length === 1, "Choosing the same archive directory does not submit another save");
    chosenDirectory = "D:/Next archives"; saveError = "ARCHIVE_PATH_WRITE_FAILED";
    await pressEnter(await reach(pathAction(3)));
    await settleUntil(() => Boolean(fixture.textContent?.includes(saveError!)), "Archive path save error lost its original cause");
    check(pathWrites.length === 1 && element(`${archiveRow} .system-runtime-path`).textContent === "D:/Fixture archives",
      "Failed archive path save keeps the last successful path and remains retryable");
    saveError = null; await pressEnter(await reach(pathAction(3)));
    await settleUntil(() => pathWrites.length === 2, "Archive path save could not be retried");
    check(!fixture.textContent?.includes("ARCHIVE_PATH_WRITE_FAILED"), "Successful path retry clears its previous failure");
    for (const fallback of [
      { workspace: "D:/LanGame/instances/", expected: "D:/LanGame/instances/.trash", missing: true },
      { workspace: "D:\\LanGame\\instances\\", expected: "D:\\LanGame\\instances\\.trash", missing: false }
    ]) {
      systemProps.appSettings = { ...originalPaths, servers_root: fallback.workspace, archives_root: "  " };
      if (fallback.missing) Reflect.deleteProperty(systemProps.appSettings, "archives_root");
      await act(async () => { render(); });
      const displayedPath = await reach(`${archiveRow} .system-runtime-path`);
      check(displayedPath.textContent === fallback.expected && displayedPath.title === fallback.expected,
        "Unset archive directory displays its concrete default location, including the full-path tooltip");
      await pressEnter(await reach(pathAction(1))); await pressEnter(await reach(pathAction(2)));
      check(copiedPaths.at(-1) === fallback.expected && openedPaths.at(-1) === fallback.expected,
        "Default archive copy and open use exactly the displayed directory");
      chosenDirectory = null;
      const previousWrites = pathWrites.length;
      await pressEnter(await reach(pathAction(3)));
      check(pickerStart === fallback.expected && pathWrites.length === previousWrites,
        "Unset archive chooser starts at the concrete default and cancellation does not save");
      chosenDirectory = "D:/Updated server files";
      await pressEnter(await reach('.system-path-row[data-path-key="games_root"] .system-path-picker-button'));
      await settleUntil(() => pathWrites.length === previousWrites + 1, "Other directory change did not save");
      check(pathWrites.at(-1)?.archives_root === fallback.expected,
        "Changing another directory preserves the effective archive location instead of sending an empty value");
    }
    systemProps.instances = [];
    systemProps.snapshot = { ...systemProps.snapshot, running_instances: 0, total_online_players: 0,
      total_player_capacity: 0, player_count_queried_instances: 0, player_count_queryable_instances: 0,
      instance_process_count: 0, instance_process_memory_bytes: 0, instance_process_threads: 0, instance_process_handles: 0 };
    shellProps.serverCount = 0;
    await act(async () => { render(); });
    await reach(".system-instance-empty");
    centeredIn(".system-instance-empty", ".system-instance-list");
    shellGeometry();
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
    check(sidebarBox.height <= mediaBox.height + 1,
      "The detail sidebar must not exceed the media card inside the full desktop shell");
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
    await reach(".panel-card--centered .empty-state");
    centeredIn(".panel-card--centered .empty-state", ".panel-card--centered");
    shellGeometry();
  }
  for (const scroll of fixture.querySelectorAll<HTMLElement>(".shell-content-scroll, .system-command-grid, .library-catalog-rail")) {
    scroll.scrollTop = 0; scroll.scrollLeft = 0;
  }
  if (config.page === "system") await reach(".system-instance-empty");
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
