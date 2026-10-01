import React, { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "../../src/App";
import { invokeMock } from "../../src/api-mock";
import { mockBootstrap } from "../../src/api-mock/bootstrap";
import { RUNTIME_VIEW_BACKGROUND_POLL_MS, RUNTIME_VIEW_POLL_MS } from "../../src/app-state";
import { I18nProvider, translate } from "../../src/i18n";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { InstanceArchiveResult, InstanceDeletionResult, InstanceSummary } from "../../src/types";
import type { ModuleProgramInventory } from "../../src/storage-management-types";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
// The desktop window is foregrounded during the reported failure. Headless
// Chrome otherwise reports no focus and silently skips installation polling.
Object.defineProperty(document, "hasFocus", { configurable: true, value: () => true });
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const errors: string[] = [];
const scenarios: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
window.alert = window.confirm = window.prompt = () => { throw new Error("Unexpected native dialog"); };

function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(`${description}: ${fixture.textContent}`);
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const result = fixture.querySelector<T>(selector);
  check(result, `Missing ${selector}`);
  return result;
}
function deferred() {
  let resolve!: (value: unknown) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<unknown>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
async function settleUntil(predicate: () => boolean, description: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    check(performance.now() < deadline, description);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function click(target: HTMLElement) { await act(async () => { target.click(); await frame(); }); }
function card(id: string) { return element(`[data-card-kind="instance"][data-card-id="${id}"]`); }
function cardControl(id: string, selector: string) {
  const result = card(id).querySelector<HTMLButtonElement>(selector);
  check(result, `Missing ${id} ${selector}`);
  return result;
}

// Drive production polling at its scheduling boundary so the race is explicit,
// independent of wall-clock timing or whether headless Chrome has window focus.
const intervals = new Map<number, () => void>();
let timerId = 1_000_000;
const originalSetInterval = window.setInterval.bind(window);
const originalClearInterval = window.clearInterval.bind(window);
window.setInterval = (handler: TimerHandler, milliseconds?: number, ...args: unknown[]) => {
  if ([RUNTIME_VIEW_POLL_MS, RUNTIME_VIEW_BACKGROUND_POLL_MS].includes(milliseconds ?? 0)) {
    check(typeof handler === "function", "Runtime poll must use a callable timer");
    const id = timerId++;
    intervals.set(id, () => handler(...args));
    return id;
  }
  return originalSetInterval(handler, milliseconds, ...args);
};
window.clearInterval = (id?: number) => {
  if (id !== undefined && intervals.delete(id)) return;
  originalClearInterval(id);
};

const source = mockBootstrap.state.instances.find((instance) => instance.module_id === "minecraft")!;
const instances: InstanceSummary[] = ["alpha", "beta", "gamma"].map((suffix) => ({ ...source,
  id: `retirement-${suffix}`, name: `Retirement ${suffix}`, status: "Stopped", active_process_count: 0 }));
mockBootstrap.state.instances = instances;
mockBootstrap.state.jobs = [];
const retired = new Set<string>();
const reads = new Set(["read_instance_details_from_storage", "list_instance_backups",
  "read_instance_runtime_overview_from_storage", "read_instance_runtime_window_snapshot",
  "read_instance_log_document_from_storage", "preview_instance_launch", "read_instance_isolation"]);
type Call = { command: string; id: string; operation: ReturnType<typeof deferred> };
const pendingReads: Call[] = [];
const retirements: Call[] = [];
const observedReads: Array<{ command: string; id: string }> = [];
let holdReadsFor: string | null = null;
let refresh: ReturnType<typeof deferred> | null = null;
let refreshRequested = false;
let forbiddenReads = 0;
const forbiddenReadCommands: string[] = [];
let holdNextArchiveInventory = false;
let archiveInventory: ReturnType<typeof deferred> | null = null;
let inventoryLocked = false;
let moduleRefreshes = 0;
let countedModuleRefreshes = 0;
let moduleChangesDuringRetirement = 0;
let postDeletionInventoryChecks = 0;
let systemReadsDuringRetirement = 0;
let creationsDuringRetirement = 0;
const libraryInstallationChecks: string[] = [];
const inventoryContentions: string[] = [];
const inventoryBusy = "instance settings are locked by another process at fixture/.langame/locks/instance-settings/"
  + "archive-inventory.lock; retry after the active operation finishes";

// Register cleanup before any assertions, so a regression still releases React
// subscriptions and every synthetic native request without a secondary error.
Object.assign(globalThis, { __reliabilityFixtureCleanup: async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  await act(async () => { root.unmount(); await frame(); });
  check(intervals.size === 0, "App unmount must release its polling lifecycle");
  await act(async () => {
    holdReadsFor = null;
    archiveInventory?.resolve({ archives: [], pending_deletions: [], issues: [] });
    refresh?.resolve(structuredClone(mockBootstrap));
    for (const read of pendingReads) read.operation.reject(new Error("UNMOUNTED_READ_FIXTURE"));
    for (const retirement of retirements) retirement.operation.reject(new Error("UNMOUNTED_RETIREMENT_FIXTURE"));
    await frame();
  });
  check(forbiddenReads === 0, `Late retirement completion must not resume disposed readers: ${forbiddenReadCommands.join(", ")}`);
  check(errors.length === 0, `Unexpected cleanup errors: ${errors.join("; ")}`);
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
  return { browser_errors: errors, native_dialogs: 0 };
} });

function installNativeBoundary() {
  Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: (command: string, args: Record<string, unknown> = {}) => {
    const input = args.input as { instance_id?: string; module_id?: string } | undefined;
    const id = String(args.instanceId ?? args.instance_id ?? input?.instance_id ?? "");
    // list_instance_archives mutates archive inventory under an exclusive
    // native lock. Library inventory counts require that same inventory;
    // bootstrap only clones app state and does not acquire this lock.
    const countedRead = command === "sync_modules_to_storage"
      || (["refresh_modules", "read_module_details"].includes(command) && args.includePreservedProgramCounts !== false
        && args.include_preserved_program_counts !== false);
    if (command === "refresh_modules") moduleRefreshes++;
    if (command === "refresh_modules" && countedRead) countedModuleRefreshes++;
    if (countedRead && inventoryLocked) {
      inventoryContentions.push(command);
      return Promise.reject(new Error(inventoryBusy));
    }
    if (command === "bootstrap" && refresh) { refreshRequested = true; return refresh.promise; }
    if (command === "bootstrap" && args.includeSystemSnapshot === true && retired.size > 0) {
      systemReadsDuringRetirement++;
    }
    if (command === "create_instance_record" && retired.size > 0) creationsDuringRetirement++;
    if (command === "list_instance_archives") {
      check(!inventoryLocked, "Archive inventory must have a single native owner");
      if (!holdNextArchiveInventory) return Promise.resolve({ archives: [], pending_deletions: [], issues: [] });
      holdNextArchiveInventory = false;
      inventoryLocked = true;
      archiveInventory = deferred();
      return archiveInventory.promise.finally(() => { inventoryLocked = false; });
    }
    if (command === "inspect_instance_removal") return Promise.resolve({ program_path: "fixture/library",
      data_path: `fixture/${id}`, remove_program: false, preserved_program_path: "fixture/library",
      owned_data_paths: [`fixture/${id}/world`, `fixture/${id}/backups`], preserved_external_saves_path: null });
    if (command === "inspect_module_programs") {
      const module = mockBootstrap.state.modules.find((entry) => entry.id === input?.module_id);
      check(module?.install_state === "Installed", "Library status assertion must start from an installed native boundary result");
      const programPath = `fixture/library/${module.id}`;
      const inventory: ModuleProgramInventory = {
        requires_archive_inventory: false,
        installations: [{ id: 1, install_root: programPath, scope: "library", install_state: module.install_state,
          current_version: module.version, used_by: mockBootstrap.state.instances.filter((entry) => entry.module_id === module.id)
            .map(({ id: instanceId, name }) => ({ id: instanceId, name })),
          modification_state: "verified_original", pending_removal: false, size_bytes: 4096 }],
        creation: { can_create: true, action: "independent_install", program_path: programPath,
          additional_bytes: 4096, reason: null }
      };
      return Promise.resolve(inventory);
    }
    if (command === "delete_instance_record" || command === "archive_instance_record") {
      check(instances.some((instance) => instance.id === id), "Mutation must target a synthetic fixture");
      const operation = deferred();
      retirements.push({ command, id, operation });
      retired.add(id);
      return operation.promise;
    }
    if (reads.has(command)) {
      observedReads.push({ command, id });
      if (retired.has(id)) {
        forbiddenReads++;
        forbiddenReadCommands.push(`${command}:${id}`);
        return Promise.reject(new Error("RETIREMENT_MISSING_DIRECTORY: instance directory has moved"));
      }
      if (holdReadsFor === id) {
        const operation = deferred();
        pendingReads.push({ command, id, operation });
        return operation.promise;
      }
    }
    return invokeMock(command, args);
  } } });
}

async function select(id: string) {
  await settleUntil(() => Boolean(fixture.querySelector(`[data-card-id="${id}"]`)), "Instance card must finish mounting");
  await click(cardControl(id, ".server-list-card-hitarea"));
  const runtimeTab = [...fixture.querySelectorAll<HTMLButtonElement>('[role="tab"]')]
    .find((tab) => tab.textContent?.trim() === translate("zh-CN", "servers.tabs.runtime"));
  if (runtimeTab && runtimeTab.getAttribute("aria-selected") !== "true") await click(runtimeTab);
  await settleUntil(() => Boolean(fixture.querySelector("pre.server-runtime-console")), "Selected CMD must load");
  check(card(id).classList.contains("is-active"), "Selected card must stay active");
}
async function navigate(view: "library" | "servers" | "system") {
  const label = translate("zh-CN", `nav.${view}.label`);
  const target = [...fixture.querySelectorAll<HTMLButtonElement>(".shell-topnav-item")]
    .find((button) => button.querySelector(".shell-topnav-label")?.textContent === label);
  check(target, `${view} navigation must exist`);
  await click(target);
}
async function assertLibraryInstalled(moduleId: string, phase: string) {
  // This checks propagation through the real App and library UI. The native
  // boundary retains its existing installed summary; filesystem preservation
  // is exercised separately by native deletion/probe tests.
  await navigate("library");
  await settleUntil(() => Boolean(fixture.querySelector(`.library-catalog-tile[data-module-id="${moduleId}"]`)),
    "Library catalog must finish mounting");
  await click(element(`.library-catalog-tile[data-module-id="${moduleId}"]`));
  check(element(".library-catalog-focus-status").classList.contains("is-installed"),
    `${phase}: retained library source must remain installed in the catalog`);
  await click(element(".library-catalog-open-details"));
  await settleUntil(() => Boolean(fixture.querySelector('.library-program-inventory[aria-busy="false"]')),
    "Selected library details and inventory must finish loading");
  const installButton = element<HTMLButtonElement>(".library-install-state-button");
  check(installButton.classList.contains("is-installed") && installButton.disabled
    && installButton.textContent?.trim() === translate("zh-CN", "status.install.installed"),
    `${phase}: deleting an instance must not make a retained installation offer a new download`);
  libraryInstallationChecks.push(`${phase}:${moduleId}`);
}
async function openRemoval(id: string, kind: "delete" | "archive") {
  await click(cardControl(id, `.server-list-card-${kind} > button`));
  await settleUntil(() => Boolean(card(id).querySelector(`.server-list-card-${kind} .inline-confirm-submit:not(:disabled)`)),
    "Removal confirmation must finish inspecting its scope");
}
async function submitRemoval(id: string, kind: "delete" | "archive") {
  const before = retirements.length;
  const submit = cardControl(id, `.server-list-card-${kind} .inline-confirm-submit`);
  await act(async () => { submit.click(); submit.click(); await frame(); });
  await settleUntil(() => retirements.length === before + 1, "Confirmation must issue one native mutation");
  check(retirements.length === before + 1, "Repeated confirmation must not duplicate native work");
  return retirements.at(-1)!;
}
function assertPending(id: string, kind: "delete" | "archive") {
  check(!fixture.querySelector("pre.server-runtime-console"), "Retiring instance must unmount CMD before its files disappear");
  const panel = element(".server-detail-panel");
  const status = panel.querySelector<HTMLElement>('[role="status"][aria-busy="true"], [aria-busy="true"] [role="status"]');
  check(status && status.textContent?.trim(), "Selected retirement must display a meaningful accessible pending state");
  const bounds = status.getBoundingClientRect();
  check(bounds.width > 0 && bounds.height > 0 && bounds.left >= 0 && bounds.right <= innerWidth,
    "Pending state must render inside the desktop viewport");
  check(cardControl(id, ".server-list-card-primary-action").disabled, "Retiring instance must not be started");
  const otherKind = kind === "delete" ? "archive" : "delete";
  check(cardControl(id, `.server-list-card-${otherKind} > button`).disabled,
    "A second destructive action must remain disabled during retirement");
  check(card(id).querySelector(`.server-list-card-${kind} [aria-busy="true"]`), "Pending confirmation must retain its busy state");
  check(!fixture.textContent?.includes("RETIREMENT_MISSING_DIRECTORY"), "Stale missing-file errors must never become visible");
}
async function holdCurrentPoll(id: string) {
  check(intervals.size === 1, "Exactly one runtime poll must be active");
  holdReadsFor = id;
  const before = pendingReads.length;
  await act(async () => { [...intervals.values()][0](); await frame(); });
  await settleUntil(() => pendingReads.length >= before + 6, "The real panel reader must start a complete pending read batch");
}
async function rejectStaleReads(id: string) {
  holdReadsFor = null;
  await act(async () => {
    for (const read of pendingReads.filter((entry) => entry.id === id))
      read.operation.reject(new Error("RETIREMENT_MISSING_DIRECTORY: instance directory has moved"));
    await frame();
  });
}
function result(call: Call): InstanceDeletionResult | InstanceArchiveResult {
  const instance = instances.find((entry) => entry.id === call.id)!;
  const common = { instance_id: call.id, instance_name: instance.name, module_id: instance.module_id,
    deleted_at_unix_ms: Date.now(), preserved_external_saves_path: null };
  return call.command === "delete_instance_record"
    ? { ...common, deleted_instance_root: `fixture/${call.id}`,
      program_cleanup: { removed_install_roots: [], preserved_data_paths: [], retained_installs: [] } }
    : { ...common, archive_id: `archive-${call.id}`, external_saves_backup_id: null,
      previous_instance_root: `fixture/${call.id}`, archived_instance_root: `fixture/archive/${call.id}`,
      effective_saves_path: `fixture/${call.id}/world`, saves_archived_with_instance_root: true };
}
async function complete(call: Call) {
  mockBootstrap.state.instances = mockBootstrap.state.instances.filter((instance) => instance.id !== call.id);
  await act(async () => { call.operation.resolve(result(call)); await frame(); });
}

async function run() {
  // Browser bootstrap avoids testing unrelated native window/shutdown adapters.
  // All retirement actions, panel reads and CMD use real App components and the
  // controlled native IPC boundary below; no filesystem or game process runs.
  await act(async () => { root.render(<StrictMode><I18nProvider><InstanceSettingsSaveProvider>
    <App />
  </InstanceSettingsSaveProvider></I18nProvider></StrictMode>); });
  await settleUntil(() => fixture.querySelector(".shell-topnav-badge")?.textContent === "3", "App must finish bootstrap");
  installNativeBoundary();
  await assertLibraryInstalled("minecraft", "before-delete");
  await navigate("servers");
  await select(instances[0].id);
  const alpha = instances[0].id;
  await openRemoval(alpha, "delete");
  await holdCurrentPoll(alpha);
  const failed = await submitRemoval(alpha, "delete");
  assertPending(alpha, "delete");
  await rejectStaleReads(alpha);
  assertPending(alpha, "delete");
  const pendingReadCount = observedReads.length;
  await click(cardControl(alpha, ".server-list-card-hitarea"));
  assertPending(alpha, "delete");
  await act(async () => { for (const tick of intervals.values()) tick(); await frame(); });
  check(observedReads.length === pendingReadCount, "Retirement must suspend reads while native work remains pending");
  retired.delete(alpha);
  await act(async () => { failed.operation.reject(new Error("DELETE_DENIED_FIXTURE: owned file is locked")); await frame(); });
  await settleUntil(() => Boolean(fixture.querySelector("pre.server-runtime-console")), "Failed deletion must restore normal CMD");
  check(card(alpha).classList.contains("is-active") && !cardControl(alpha, ".server-list-card-hitarea").disabled,
    "Failed deletion must restore the selected instance");
  check(element(".shell-activity-bar").textContent?.includes("DELETE_DENIED_FIXTURE"), "Real failure must be visible in activity feedback");
  check(!fixture.textContent?.includes("RETIREMENT_MISSING_DIRECTORY"), "Stale read failure must not replace the real deletion failure");
  scenarios.push("delete-failure");

  const retry = card(alpha).querySelector<HTMLButtonElement>(".server-list-card-delete .inline-confirm-submit:not(:disabled)");
  if (!retry) await openRemoval(alpha, "delete");
  const succeeded = await submitRemoval(alpha, "delete");
  assertPending(alpha, "delete");
  refresh = deferred();
  refreshRequested = false;
  await complete(succeeded);
  await settleUntil(() => refreshRequested, "Successful mutation must refresh registered instances");
  const completedCard = fixture.querySelector<HTMLElement>(`[data-card-id="${alpha}"]`);
  check(!completedCard || completedCard.querySelector<HTMLButtonElement>(".server-list-card-primary-action")?.disabled,
    "Native completion must remove or lock stale instance controls until refresh finishes");
  holdNextArchiveInventory = true;
  await act(async () => { const flight = refresh!; refresh = null; flight.resolve(structuredClone(mockBootstrap)); await frame(); });
  await settleUntil(() => inventoryLocked, "Deletion completion must start its final archive inventory refresh");
  const refreshesBeforeUnlock = moduleRefreshes;
  const countRefreshesBeforeUnlock = countedModuleRefreshes;
  await act(async () => { window.dispatchEvent(new Event("focus")); await frame(); });
  await settleUntil(() => moduleRefreshes > refreshesBeforeUnlock,
    "Light installation polling must complete while archive inventory remains locked");
  check(inventoryContentions.length === 0,
    `Counted library refresh must wait for post-deletion archive inventory; contended native calls: ${inventoryContentions.join(", ")}`);
  check(!element(".shell-activity-bar").textContent?.includes("刷新游戏库失败"),
    "Successful deletion must not produce the reported library refresh failure");
  check(countedModuleRefreshes === countRefreshesBeforeUnlock, "Count polling must not enter native inventory while its lock is held");
  await act(async () => { archiveInventory!.resolve({ archives: [], pending_deletions: [], issues: [] }); await frame(); });
  await settleUntil(() => !inventoryLocked && countedModuleRefreshes > countRefreshesBeforeUnlock,
    "Queued program counts must refresh after inventory releases its lock");
  check(!element(".shell-activity-bar").textContent?.includes("刷新游戏库失败"),
    "Reconciled successful deletion must retain successful activity feedback");
  postDeletionInventoryChecks++;
  await settleUntil(() => !fixture.querySelector(`[data-card-id="${alpha}"]`), "Successful delete must remove its card");
  check(!fixture.textContent?.includes("RETIREMENT_MISSING_DIRECTORY"), "Successful delete must not leave a false error");
  scenarios.push("delete-success");
  await assertLibraryInstalled("minecraft", "after-delete");
  await assertLibraryInstalled("corekeeper", "other-module");
  await assertLibraryInstalled("minecraft", "return-to-module");
  await navigate("servers");

  const beta = instances[1].id;
  const gamma = instances[2].id;
  await select(beta);
  await openRemoval(beta, "archive");
  const archived = await submitRemoval(beta, "archive");
  assertPending(beta, "archive");
  await select(gamma);
  check(card(gamma).classList.contains("is-active"), "An unrelated instance must remain selectable during retirement");
  mockBootstrap.state.instances = mockBootstrap.state.instances.map((entry) => entry.id === gamma
    ? { ...entry, name: "Unrelated instance refreshed during archive" } : entry);
  await act(async () => { for (const tick of intervals.values()) tick(); await frame(); });
  await settleUntil(() => card(gamma).textContent?.includes("Unrelated instance refreshed during archive") === true,
    "Unrelated instance runtime inventory must continue refreshing during archive");
  const systemReadsBefore = systemReadsDuringRetirement;
  await navigate("system");
  await settleUntil(() => systemReadsDuringRetirement > systemReadsBefore
    && fixture.querySelector('.system-core-stage[aria-busy="false"]') !== null,
    "System telemetry must refresh and settle while archive remains pending");
  await navigate("library");
  await settleUntil(() => Boolean(fixture.querySelector('.library-catalog-tile[data-module-id="corekeeper"]')),
    "Library catalog remains available during archive");
  await click(element('.library-catalog-tile[data-module-id="corekeeper"]'));
  const countsBeforeArchiveRefresh = countedModuleRefreshes;
  for (const state of ["NotInstalled", "Installed"] as const) {
    mockBootstrap.state.modules = mockBootstrap.state.modules.map((module) => module.id === "corekeeper"
      ? { ...module, install_state: state } : module);
    await act(async () => { window.dispatchEvent(new Event("focus")); await frame(); });
    await settleUntil(() => element(".library-catalog-focus-status").classList.contains("is-installed") === (state === "Installed"),
      "Changed installation status must reach the visible catalog before archive completion");
    moduleChangesDuringRetirement++;
  }
  check(countedModuleRefreshes === countsBeforeArchiveRefresh,
    "Archive still owns inventory while unrelated installation status changes remain visible");
  await assertLibraryInstalled("corekeeper", "during-archive");
  const createButton = element<HTMLButtonElement>('.library-create-server-form button[type="submit"]');
  check(!createButton.disabled, "Unrelated existing-library creation must not wait for archive completion");
  await click(createButton);
  await settleUntil(() => creationsDuringRetirement === 1, "Creation must reach the native boundary before archive completion");
  await navigate("servers");
  await select(gamma);
  scenarios.push("unrelated-work-during-archive");
  await complete(archived);
  await settleUntil(() => !fixture.querySelector(`[data-card-id="${beta}"]`), "Successful archive must remove its normal card");
  check(card(gamma).classList.contains("is-active") && Boolean(fixture.querySelector("pre.server-runtime-console")),
    "Retirement completion must preserve the newer selection and CMD");
  scenarios.push("archive-navigation");
  check(forbiddenReads === 0, "No reader may access a retired instance after mutation starts");

  // Keep a real pending screen available to the runner's optional screenshot;
  // cleanup also verifies disposal while native retirement is still in flight.
  await openRemoval(gamma, "delete");
  await submitRemoval(gamma, "delete");
  assertPending(gamma, "delete");
  scenarios.push("pending-unmount");
  check(errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  return { status: "passed", scenarios, retirement_requests: retirements.length,
    discarded_reads: pendingReads.length, reads_after_retirement_started: forbiddenReads,
    post_deletion_inventory_checks: postDeletionInventoryChecks, inventory_contentions: inventoryContentions,
    system_reads_during_retirement: systemReadsDuringRetirement, creations_during_retirement: creationsDuringRetirement,
    module_changes_during_retirement: moduleChangesDuringRetirement,
    library_installation_checks: libraryInstallationChecks, browser_errors: errors };
}

void run().catch((error) => ({ status: "failed", scenarios,
  error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .then((report) => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
    return fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) });
  });
