import React, { act, StrictMode, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { bootstrapApp } from "../../src/api";
import { invokeMock } from "../../src/api-mock";
import { createDefaultAiSettings } from "../../src/ai-settings";
import { I18nProvider } from "../../src/i18n";
import { ServersView } from "../../src/views/ServersView";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
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
const cause = "private runtime refresh failed at D:/fixture/instance/runtime: required private runtime is missing";
let isolationReads = 0;
let removalInspections = 0;
let deleteCalls = 0;
let retries = 0;

function check(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const value = fixture.querySelector<T>(selector);
  check(value, `Missing ${selector}`);
  return value;
}
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
async function settleUntil(predicate: () => boolean, message: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    check(performance.now() < deadline, message);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function click(target: HTMLElement) { await act(async () => { target.click(); await frame(); }); }
function fits(target: HTMLElement) {
  const rect = target.getBoundingClientRect();
  check(rect.width > 0 && rect.height > 0 && rect.left >= 0 && rect.right <= innerWidth + 1
    && rect.top >= 0 && rect.bottom <= innerHeight + 1, `Recovery control exceeds viewport: ${JSON.stringify(rect)}`);
}

async function run() {
  const bootstrap = await bootstrapApp({ includeSystemSnapshot: false });
  const original = bootstrap.state.instances.find((instance) => instance.module_id === "minecraft");
  check(original, "Mock must include a Minecraft instance");
  const instance = { ...original, name: "已移除文件的实例", status: "Stopped" as const, active_process_count: 0 };
  Object.assign(window, { isTauri: true });
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: { invoke: async (command: string, args: Record<string, unknown> = {}) => {
    if (command === "list_instance_archives") return { archives: [], pending_deletions: [], issues: [] };
    if (command === "inspect_instance_removal") {
      removalInspections++;
      return { program_path: "D:/fixture/instance/runtime", data_path: "D:/fixture/instance", remove_program: true,
        owned_data_paths: [], preserved_program_path: null, preserved_external_saves_path: null };
    }
    if (command === "read_instance_isolation") {
      isolationReads++;
      return { instance_id: instance.id, mode: "damaged", runtime_path: "D:/fixture/instance/runtime",
        data_path: "D:/fixture/instance", config_path: "D:/fixture/instance/config", saves_path: "D:/fixture/instance/saves",
        conflicts: [], issues: [cause] };
    }
    return invokeMock(command, args);
  } } });
  const props: ComponentProps<typeof ServersView> = {
    aiSettings: createDefaultAiSettings(), assistantCanRun: false, bindAddressCandidates: [], instances: [instance],
    moduleInstallations: {}, instanceLaunchPlans: {}, instanceLaunchFailures: {}, section: "overview",
    onWorkspaceSectionChange: noOperation, selectedInstanceId: instance.id, selectedDetails: null,
    panelLoadState: { instanceId: instance.id, pending: ["details"], errors: {} },
    selectedBackups: [], selectedModuleDetails: null, selectedModuleDetailsError: null,
    runtime: null, runtimeWindows: null, launchPlan: null, launchPlanError: null,
    refreshIssue: { message: "Previous selection failure", failedAt: 1, consecutiveFailures: 1 },
    onActivity: noOperation, onResumeAutoRefresh: () => { retries++; }, onSelectInstance: noOperation,
    onStart: noOperation, onStop: noOperation, onInstallModule: async () => {}, onOpenModuleLibrary: noOperation,
    onCreateInstance: noOperation, onPickDirectory: async () => null,
    onImportDontStarveWorldData: async () => { throw new Error("Unexpected import"); },
    onOpenLocalPath: noOperation, onSendRuntimeCommand: async () => null, onSuppressRuntimeWindows: async () => {},
    onCreateBackup: noOperation, onArchivesChanged: async () => {}, onArchiveInstance: async () => { throw new Error("Unexpected archive"); },
    onDeleteInstance: async (id) => {
      check(id === instance.id, "Recovery deletion must target the selected instance");
      deleteCalls++;
      props.instances = [];
      props.selectedInstanceId = null;
      props.panelLoadState = null;
      render();
    },
    onRestoreBackup: noOperation, onRenameBackup: async () => true, onDeleteBackup: async () => {},
    onSaveSettings: async () => undefined, onSaveAutostart: async () => {},
    onApplyPlayerAccessMutation: async () => { throw new Error("Unexpected player mutation"); }
  };
  function render() {
    root.render(<StrictMode><I18nProvider><InstanceSettingsSaveProvider><ServersView {...props} />
    </InstanceSettingsSaveProvider></I18nProvider></StrictMode>);
  }
  await act(async () => { render(); });
  await settleUntil(() => Boolean(fixture.querySelector(".instance-unavailable")), "Unavailable panel must finish mounting");
  check(element(".instance-unavailable").getAttribute("aria-busy") === "true", "Pending details must remain a true loading state");
  check(element(".instance-unavailable h3").textContent === "加载中…", "Pending details must have the localized loading state");
  check(!fixture.querySelector(".instance-unavailable-actions"), "Loading must not show failure actions");
  check(isolationReads === 0, "Pending details must not trigger eager diagnostics");

  props.panelLoadState = { instanceId: instance.id, pending: [], errors: { details: cause } };
  await act(async () => { render(); });
  const recovery = element(".instance-unavailable");
  check(recovery.getAttribute("aria-busy") === "false", "Failed details must finish loading");
  check(element(".instance-unavailable h3").textContent === "暂时无法读取此实例", "Failure title must explain the state");
  check(element(".workspace-metrics").textContent?.includes("无法读取"), "Metric must stop reporting loading after failure");
  check(!element<HTMLDetailsElement>(".instance-unavailable-diagnostics").open, "Diagnostics must be collapsed initially");
  check(!fixture.textContent?.includes(cause) && isolationReads === 0, "Raw error and paths must stay out of the initial recovery screen");
  fits(element(".instance-unavailable-actions"));
  await click(element(".instance-unavailable-actions > button"));
  check(retries === 1, "Retry must invoke the real callback exactly once");

  await click(element(".instance-unavailable-diagnostics > summary"));
  await settleUntil(() => isolationReads > 0 && Boolean(fixture.querySelector(".instance-isolation-content")), "Expanded diagnostics must load");
  check(fixture.textContent?.includes(cause), "Opt-in diagnostics must preserve the failure cause");
  await click(element(".instance-unavailable-diagnostics > summary"));
  await settleUntil(() => !fixture.querySelector(".instance-isolation-panel"), "Closing diagnostics must dispose its reader");

  for (const status of ["Running", "Starting", "Stopping"] as const) {
    props.instances = [{ ...instance, status, active_process_count: status === "Running" ? 1 : 0 }];
    await act(async () => { render(); });
    check(element<HTMLButtonElement>(".instance-unavailable-actions .inline-confirm-action > button").disabled,
      `Failed details must not allow deletion while ${status}`);
  }
  props.instances = [instance];
  await act(async () => { render(); });
  await settleUntil(() => !element<HTMLButtonElement>(".instance-unavailable-actions .inline-confirm-action > button").disabled,
    "Stopped unavailable instance must be deletable");
  await click(element(".instance-unavailable-actions .inline-confirm-action > button"));
  await settleUntil(() => !element<HTMLButtonElement>(".instance-unavailable .inline-confirm-submit").disabled, "Deletion scope must be inspected");
  check(removalInspections === 1 && deleteCalls === 0, "Inspecting deletion must not delete anything");
  fits(element(".instance-unavailable .inline-confirm-submit"));
  await click(element(".instance-unavailable .inline-confirm-submit"));
  await settleUntil(() => !fixture.querySelector(".instance-unavailable"), "Successful deletion must reach the empty instance view");
  check(deleteCalls === 1 && !fixture.querySelector(`[data-card-id="${instance.id}"]`), "Confirmed removal must remove the record exactly once");

  // Retain the concise failure surface for the optional screenshot.
  props.instances = [instance];
  props.selectedInstanceId = instance.id;
  props.panelLoadState = { instanceId: instance.id, pending: [], errors: { details: cause } };
  await act(async () => { render(); });
  check(errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  return { status: "passed", retries, delete_calls: deleteCalls, removal_inspections: removalInspections,
    diagnostics_on_demand: true, running_deletion_blocked: true, browser_errors: errors };
}

void run().catch((error) => ({ status: "failed", error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .then((report) => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
    return fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) });
  });
