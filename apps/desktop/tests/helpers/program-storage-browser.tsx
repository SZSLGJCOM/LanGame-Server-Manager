import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { createInstance } from "../../src/api";
import { invokeMock } from "../../src/api-mock";
import { I18nProvider, useI18n } from "../../src/i18n";
import { ActivityNoticeTarget } from "../../src/components/ActivityNotice";
import { LibraryServerActions } from "../../src/views/library/LibraryServerActions";
import { formatInstallState, hasStoredProgram } from "../../src/views/library/library-shared";
import { InstanceProgramMaintenance } from "../../src/views/servers/InstanceProgramMaintenance";
import type { InstanceDetails, InstanceIsolationReport, ModuleDetails, SteamCmdStatus } from "../../src/types";
import type { ModuleProgramInventory } from "../../src/storage-management-types";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "en-US");
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100vh;padding:24px;box-sizing:border-box;display:flex;flex-direction:column";
const root = createRoot(fixture);
const errors: string[] = [];
const checks: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(description);
  checks.push(description);
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const result = fixture.querySelector<T>(selector);
  if (!result) throw new Error(`Missing ${selector}: ${fixture.textContent}`);
  return result;
}
function deferred() {
  let resolve!: (value: unknown) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const calls: Array<{ command: string; args: Record<string, unknown>; operation: ReturnType<typeof deferred> }> = [];
let module: ModuleDetails;
let details: InstanceDetails;
let report: InstanceIsolationReport;
let page: "create" | "maintenance" = "create";
let instanceName = "Program fixture";
let changed = 0;
const initialInspection = deferred();
let nextInspection: ReturnType<typeof deferred> | null = initialInspection;
let inspectionFailure = false;
let installationUsed = false;
const inspectionRequests: string[] = [];
let activeInspections = 0;
let maximumInspections = 0;
function installationFixture(mode: string = "independent"): ModuleProgramInventory {
  const libraryInstalled = module.summary.install_state === "Installed";
  const hasIndependentInstall = (module.summary.instance_program_count ?? 0) > 0;
  const sourcePath = libraryInstalled ? "fixture/library" : "fixture/instances/owner/runtime";
  const action = libraryInstalled && !installationUsed ? "existing_install"
    : libraryInstalled && mode === "shared" ? "shared_install" : "independent_install";
  return { installations: libraryInstalled || hasIndependentInstall ? [{ id: 1, install_root: sourcePath,
    scope: libraryInstalled ? "library" : "instance", install_state: "Installed", current_version: "fixture-version",
    used_by: installationUsed || hasIndependentInstall ? [{ id: "fixture-owner", name: "Fixture owner" }] : [],
    modification_state: installationUsed || hasIndependentInstall ? "modified_or_used" : "unverified",
    pending_removal: false,
    size_bytes: 100 * 1024 ** 3 }] : [],
    creation: { can_create: hasStoredProgram(module.summary), action,
      program_path: libraryInstalled || hasIndependentInstall ? sourcePath : "",
      additional_bytes: action === "independent_install" ? libraryInstalled || hasIndependentInstall ? 100 * 1024 ** 3 : null : 0,
      reason: null } };
}
let switchLocale: ReturnType<typeof useI18n>["setLocale"];
let steamCmdStatus: SteamCmdStatus;
function Harness() {
  const { t, setLocale } = useI18n();
  switchLocale = setLocale;
  const [target, setTarget] = useState<HTMLDivElement | null>(null);
  const [creating, setCreating] = useState(false);
  return <ActivityNoticeTarget.Provider value={{ element: target, dismissLabel: "Close" }}>
    <main style={{ flex: 1, minHeight: 0, overflowY: "auto" }}>
      <section className={`operation-content${page === "create" ? " library-detail-page" : ""}`} style={{ maxWidth: page === "create" ? 320 : 700 }}>
        {page === "create" ? <LibraryServerActions selected={module.summary} selectedModuleDetails={module}
          steamCmdStatus={steamCmdStatus} steamCmdBusy={false}
          installLabel={formatInstallState(module.summary.install_state, t)} installBusy={false} installStatusClass="is-success"
          creating={creating}
          instanceName={instanceName} onInstanceNameChange={(value) => { instanceName = value; draw(); }}
          onInstall={() => {}} onUninstall={() => {}} onCreateServer={async (input) => {
            setCreating(true); try { await createInstance(input); } finally { setCreating(false); }
          }} /> : <InstanceProgramMaintenance key={report.mode} details={details} report={report} jobs={[]}
            onChanged={() => { changed++; }} />}
      </section>
    </main>
    <footer className="shell-activity-bar"><span className="shell-activity-label">Activity</span>
      <div className="shell-activity-notices" ref={setTarget} /></footer>
  </ActivityNoticeTarget.Provider>;
}
function draw() { root.render(<I18nProvider><Harness /></I18nProvider>); }
async function click(selector: string) { await act(async () => { element<HTMLButtonElement>(selector).click(); }); }
async function waitFor(selector: string) {
  const deadline = performance.now() + 5000;
  while (!fixture.querySelector(selector)) {
    if (performance.now() >= deadline) throw new Error(`Waiting for ${selector}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
function notice() {
  const result = element(".shell-activity-notice");
  const footer = element(".shell-activity-bar");
  const box = footer.getBoundingClientRect();
  const noticeBox = result.getBoundingClientRect();
  check(footer.contains(result) && noticeBox.top >= box.top && noticeBox.bottom <= box.bottom, "Feedback remains in fixed activity bar");
  check(!element("main").querySelector(".shell-activity-notice"), "Operation feedback adds no content block");
  return result;
}
async function run() {
  steamCmdStatus = await invokeMock<SteamCmdStatus>("ensure_steamcmd_ready", { operationId: "program-storage-fixture" });
  check(steamCmdStatus.ready, "Program operations have a prepared SteamCMD fixture");
  module = await invokeMock<ModuleDetails>("read_module_details", { moduleId: "minecraft" });
  module = { ...module, summary: { ...module.summary, install_state: "Installed", instance_program_count: 0, archived_program_count: 0 } };
  details = await invokeMock<InstanceDetails>("read_instance_details_from_storage", { instanceId: "srv-dst-terminal-error" });
  details = { ...details, summary: { ...details.summary, id: "program-fixture", module_id: "minecraft", status: "Stopped", active_process_count: 0 }, active_run: null };
  report = { instance_id: details.summary.id, mode: "shared", runtime_path: "fixture/library", data_path: "fixture/instance", config_path: "fixture/config", saves_path: "fixture/saves", conflicts: [], issues: [] };
  Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: (command: string, args: Record<string, unknown>) => {
    if (command === "inspect_module_programs") {
      const input = args.input as { module_id: string; program_mode: string };
      inspectionRequests.push(input.module_id);
      maximumInspections = Math.max(maximumInspections, ++activeInspections);
      const pending = nextInspection;
      nextInspection = null;
      const result = inspectionFailure ? Promise.reject(new Error("Ownership inspection fixture failure"))
        : pending ? pending.promise : Promise.resolve(installationFixture(input.program_mode));
      return result.finally(() => { activeInspections--; });
    }
    if (command === "create_instance_record" || command === "update_instance_program") {
      const operation = deferred(); calls.push({ command, args, operation }); return operation.promise;
    }
    throw new Error(`Unexpected program-storage command: ${command}`);
  } } });
  await act(async () => { await prepareBrowserLocaleCatalogs(); draw(); });
  await waitFor("select");
  const select = element<HTMLSelectElement>("select");
  check(select.value === "shared" && select.options.length === 2, "Shared capability exposes shared default and independent option");
  check(fixture.querySelectorAll("select").length === 1 && !fixture.textContent?.includes("Program content"),
    "Creation only offers program ownership; clean program preparation needs no source selection");
  check(element<HTMLButtonElement>('button[type="submit"]').disabled && calls.length === 0,
    "Creation is blocked while the installation plan is unresolved");
  await act(async () => { initialInspection.resolve(installationFixture("shared")); });
  check(!element<HTMLButtonElement>('button[type="submit"]').disabled && fixture.textContent?.includes("Use in place")
    && fixture.textContent?.includes("Unverified") && !fixture.textContent?.includes("No instances currently use"),
    "First instance preview permits in-place use while clearly retaining creation-time verification");
  const formBefore = element("form").getBoundingClientRect();
  check(!element<HTMLButtonElement>('button[aria-label="Check for updates"]').disabled,
    "Installed program maintenance is available before creation");
  await click('button[type="submit"]');
  check(calls[0].command === "create_instance_record" && calls[0].args.programMode === "shared"
    && !("programSource" in calls[0].args), "Shared choice crosses the creation API without a source override");
  check(element<HTMLButtonElement>('button[type="submit"]').disabled && element<HTMLSelectElement>("select").disabled, "Creating prevents duplicate submission and mode changes");
  check([...fixture.querySelectorAll<HTMLButtonElement>(".library-sidebar-maintenance-row button")].every((button) => button.disabled),
    "Pending creation prevents installation, verification and removal before a repair job is discovered");
  check(notice().textContent?.includes("Repairs will download when needed")
    && notice().textContent?.includes("creation will continue automatically"),
    "Original creation explains automatic repair and continuation");
  check(element("form").getBoundingClientRect().height === formBefore.height, "Creation progress leaves form geometry unchanged");
  installationUsed = true;
  await act(async () => { calls[0].operation.resolve({}); });
  await act(async () => { select.value = "independent"; select.dispatchEvent(new Event("change", { bubbles: true })); });
  await click('button[type="submit"]');
  check(calls[1].args.programMode === "independent" && !("programSource" in calls[1].args), "Independent creation cannot request modified program reuse");
  await act(async () => { calls[1].operation.resolve({}); });
  module = await invokeMock<ModuleDetails>("read_module_details", { moduleId: "palworld" });
  module = { ...module, summary: { ...module.summary, install_state: "NotInstalled", instance_program_count: 1, archived_program_count: 0 } };
  await act(async () => { draw(); });
  check(!fixture.querySelector("select"), "Independent-only module has no redundant mode chooser");
  check(!element<HTMLButtonElement>('button[type="submit"]').disabled, "Existing instance permits another clean creation after library adoption");
  check(!element<HTMLButtonElement>(".library-install-state-button").disabled && !element(".library-install-state-button").classList.contains("is-installed"), "Creation readiness does not mislabel absent library program as installed");

  page = "maintenance";
  await act(async () => { draw(); });
  const maintenanceHeight = element(".operation-content").getBoundingClientRect().height;
  await click(".button-row .inline-confirm-action > button");
  await click(".inline-confirm-submit");
  check(calls[2].command === "update_instance_program" && calls[2].args.instanceId === details.summary.id, "Shared maintenance targets this instance's actual program version");
  notice();
  await act(async () => { calls[2].operation.reject(new Error("Steam HTTP 429 fixture failure RETRY_MESSAGE_END")); });
  check(notice().textContent?.includes("RETRY_MESSAGE_END"), "Complete failure and retry state remain in bottom bar");
  check(element(".operation-content").getBoundingClientRect().height === maintenanceHeight, "Error text does not increase maintenance content height");
  await click(".shell-activity-notice-actions button");
  check(calls[3].command === "update_instance_program" && calls[3].args.instanceId === details.summary.id, "Retry resubmits the same shared program operation");
  await act(async () => { calls[3].operation.resolve({}); });
  check(changed === 1 && notice().classList.contains("is-success"), "Completed maintenance refreshes data and reports success below");
  await click(".shell-activity-notice-close");
  check(!fixture.querySelector(".shell-activity-notice"), "Maintenance feedback can be dismissed");

  report = { ...report, mode: "private", runtime_path: "fixture/instance/runtime" };
  await act(async () => { draw(); });
  const buttons = fixture.querySelectorAll<HTMLButtonElement>(".button-row .inline-confirm-action > button");
  await act(async () => { buttons[1].click(); });
  await click(".inline-confirm-submit");
  check(calls[4].command === "update_instance_program" && calls[4].args.instanceId === details.summary.id && calls[4].args.validate === true, "Independent verification targets only this instance");
  await act(async () => { calls[4].operation.resolve({}); });
  const automaticSettings = details.settings_json;
  details = { ...details, settings_json: JSON.stringify({ ...JSON.parse(automaticSettings), program_update: { policy: "pinned" } }) };
  await act(async () => { draw(); });
  check([...fixture.querySelectorAll<HTMLButtonElement>(".button-row button")].every((button) => button.disabled),
    "Pinned instance disables update and verification before either command can be dispatched");
  const pinnedHelp = element(".button-row").getAttribute("aria-describedby");
  check(pinnedHelp && document.getElementById(pinnedHelp)?.textContent?.includes("keeps its current version"),
    "Pinned maintenance explains the required policy change accessibly");
  await click(".button-row button");
  check(calls.length === 5, "Disabled pinned maintenance dispatched an update");
  details = { ...details, settings_json: automaticSettings };
  await act(async () => { draw(); });
  check([...fixture.querySelectorAll<HTMLButtonElement>(".button-row button")].every((button) => !button.disabled),
    "Removing the pin restores stopped-instance program maintenance");

  details = { ...details, summary: { ...details.summary, status: "Running", active_process_count: 1 } };
  await act(async () => { draw(); });
  check([...fixture.querySelectorAll<HTMLButtonElement>(".button-row button")].every((button) => button.disabled), "Running instance disables program maintenance");
  page = "create";
  module = await invokeMock<ModuleDetails>("read_module_details", { moduleId: "minecraft" });
  module = { ...module, summary: { ...module.summary, install_state: "Installed" } };
  await act(async () => { draw(); });
  const mode = element<HTMLSelectElement>("select");
  await act(async () => { mode.value = "independent"; mode.dispatchEvent(new Event("change", { bubbles: true })); });
  check(fixture.querySelectorAll("select").length === 1,
    "Selecting independent ownership does not add a program content choice");
  check(fixture.textContent?.includes("Automatically verify server files")
    && fixture.textContent?.includes("Old settings, saves and mods are not inherited"),
    "Creation help explains automatic clean preparation and new instance data");
  await click('button[type="submit"]');
  check(calls[5].args.programMode === "independent" && !("programSource" in calls[5].args), "Independent creation uses backend-owned clean preparation");
  check(mode.disabled, "Creating freezes the program ownership choice");
  check(notice().textContent?.includes("Repairs will download when needed"),
    "Independent creation automatically prepares clean originals when needed");
  await act(async () => { calls[5].operation.resolve({}); });
  await act(async () => { mode.value = "shared"; mode.dispatchEvent(new Event("change", { bubbles: true })); });
  check(fixture.querySelectorAll("select").length === 1, "Shared mode also requires no source choice");
  await click('button[type="submit"]');
  check(calls[6].args.programMode === "shared" && !("programSource" in calls[6].args), "Shared creation retains backend-owned clean preparation");
  await act(async () => { calls[6].operation.resolve({}); });
  await act(async () => { await import("../../src/i18n-messages-zh-cn"); switchLocale("zh-CN"); });
  await waitFor("select");
  await act(async () => {
    const currentMode = element<HTMLSelectElement>("select");
    currentMode.value = "independent"; currentMode.dispatchEvent(new Event("change", { bubbles: true }));
  });
  check(!fixture.textContent?.includes("保留修改") && !fixture.textContent?.includes("程序内容")
    && fixture.textContent?.includes("自动校验服务器程序") && fixture.textContent?.includes("不继承旧配置、存档和模组"),
    "Chinese creation explains clean defaults without an obsolete source choice");
  module = await invokeMock<ModuleDetails>("read_module_details", { moduleId: "palworld" });
  module = { ...module, summary: { ...module.summary, install_state: "NotInstalled", instance_program_count: 0, archived_program_count: 1 } };
  await act(async () => { draw(); });
  check(!fixture.querySelector("select") && !element<HTMLButtonElement>('button[type="submit"]').disabled,
    "An archived independent program permits verified preparation without offering missing local library files");
  check(element(".library-program-inventory").textContent?.includes("程序库：未安装")
    && element(".library-program-inventory").textContent?.includes("归档保留程序：1 份"), "Archive help does not mislabel the library as installed");
  check(fixture.textContent?.includes("仍保留并占用磁盘空间") && fixture.textContent?.includes("服务器 → 归档"), "Archive retention and recovery location are visible");
  await click('button[type="submit"]');
  check(!("programSource" in calls[7].args) && calls[7].args.programMode === "independent", "Archived programs use the same clean creation API");
  await act(async () => { calls[7].operation.resolve({}); });
  module = { ...module, summary: { ...module.summary, archived_program_count: 0 } };
  await act(async () => { draw(); });
  check(element<HTMLButtonElement>('button[type="submit"]').disabled, "A module with no stored program requires the explicit install action");
  module = { ...module, summary: { ...module.summary, archived_program_count: 1 } };
  await act(async () => { draw(); });
  const metrics = fixture.querySelectorAll<HTMLElement>(".library-program-metric");
  check(metrics.length >= 2, "Program information stays available as compact metrics");
  for (const metric of metrics) {
    check(metric.getBoundingClientRect().right <= element("section").getBoundingClientRect().right,
      "Program and archive metrics fit the production-width sidebar");
  }
  inspectionFailure = true;
  module = { ...module, summary: { ...module.summary, id: "failed-inspection" } };
  await act(async () => { draw(); });
  check(element<HTMLButtonElement>('button[type="submit"]').disabled
    && element(".library-program-inventory").textContent?.includes("Ownership inspection fixture failure"),
    "Failed inspection cannot reuse a previous module's creation readiness");
  inspectionFailure = false;
  await click(".library-program-inventory button");
  check(!element<HTMLButtonElement>('button[type="submit"]').disabled, "A successful explicit retry restores creation readiness");
  const staleInspection = deferred();
  const previousInspections = inspectionRequests.length;
  nextInspection = staleInspection;
  module = { ...module, summary: { ...module.summary, id: "stale-inspection" } };
  await act(async () => { draw(); });
  check(element<HTMLButtonElement>('button[type="submit"]').disabled, "Changing modules invalidates the previous plan immediately");
  module = { ...module, summary: { ...module.summary, id: "current-inspection" } };
  await act(async () => { draw(); });
  check(inspectionRequests.length === previousInspections + 1, "A new selection queues behind the owned native scan");
  await act(async () => { staleInspection.resolve({ ...installationFixture(), creation: {
    can_create: false, action: "independent_install", program_path: "STALE_PROGRAM_PATH", additional_bytes: null, reason: "STALE_REASON"
  } }); });
  check(!element<HTMLButtonElement>('button[type="submit"]').disabled && !fixture.textContent?.includes("STALE_PROGRAM_PATH"),
    "A late previous-module inspection cannot replace the current plan");
  check(maximumInspections === 1 && inspectionRequests.at(-1) === "current-inspection",
    "Installation inspection concurrency stays bounded and only the current selection is read next");
  const refreshedInventory = installationFixture();
  const summaryLag = deferred();
  nextInspection = summaryLag;
  module = { ...module, summary: { ...module.summary, id: "stale-bootstrap", install_state: "NotInstalled",
    instance_program_count: 0, archived_program_count: 0 } };
  await act(async () => { draw(); });
  await act(async () => { summaryLag.resolve({ ...refreshedInventory, creation: { ...refreshedInventory.creation, can_create: true } }); });
  check(!element<HTMLButtonElement>('button[type="submit"]').disabled,
    "The fresh backend creation plan remains authoritative when bootstrap program counts lag behind");
  module = await invokeMock<ModuleDetails>("read_module_details", { moduleId: "palworld" });
  module = { ...module, summary: { ...module.summary, install_state: "Installed", instance_program_count: 0, archived_program_count: 0 } };
  installationUsed = innerWidth >= 1000;
  await act(async () => { draw(); });
  check(element(".library-program-inventory").textContent?.includes(installationUsed ? "复制一份独立程序" : "原位置使用"),
    "Final screenshot shows a consistent first-instance or additional independent-installation plan");
  element<HTMLButtonElement>('button[type="submit"]').scrollIntoView({ block: "nearest" });
  const createBounds = element('button[type="submit"]').getBoundingClientRect();
  const scrollBounds = element("main").getBoundingClientRect();
  check(createBounds.top >= scrollBounds.top && createBounds.bottom <= scrollBounds.bottom,
    "The creation action remains reachable through local scrolling at short desktop heights");
  check(document.documentElement.scrollWidth <= innerWidth, "Creation controls fit the viewport");
  // Settle the provider's documented secondary-locale preload while retaining
  // the real rendered state for the browser runner's optional screenshot.
  await act(async () => { await import("../../src/i18n-messages-zh-cn"); });
  check(errors.length === 0, "No browser or React errors");
  return { status: "passed", checks, calls: calls.map(({ command, args }) => ({ command, args })), browser_errors: errors };
}
void run().catch((error) => ({ status: "failed", error: String(error?.stack ?? error), checks, browser_errors: errors }))
  .then((result) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(result) }));
