import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "../../src/App";
import { invokeMock } from "../../src/api-mock";
import { mockBootstrap } from "../../src/api-mock/bootstrap";
import { RUNTIME_VIEW_BACKGROUND_POLL_MS, RUNTIME_VIEW_POLL_MS } from "../../src/app-state";
import { I18nProvider } from "../../src/i18n";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
Object.defineProperty(document, "hasFocus", { configurable: true, value: () => true });
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
function check(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
async function settleUntil(predicate: () => boolean, message: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    check(performance.now() < deadline, message);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}

// Trigger the production polling boundary deterministically until it pauses.
const intervals = new Map<number, () => void>();
let timerId = 1_000_000;
const originalSetInterval = window.setInterval.bind(window);
const originalClearInterval = window.clearInterval.bind(window);
window.setInterval = (handler: TimerHandler, milliseconds?: number, ...args: unknown[]) => {
  if ([RUNTIME_VIEW_POLL_MS, RUNTIME_VIEW_BACKGROUND_POLL_MS].includes(milliseconds ?? 0)) {
    check(typeof handler === "function", "Runtime poll must be a function");
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
const instance = mockBootstrap.state.instances.find((entry) => entry.module_id === "minecraft")!;
mockBootstrap.state.instances = [{ ...instance, status: "Stopped", active_process_count: 0 }];
mockBootstrap.state.jobs = [];
let detailReads = 0;
let inventoryReads = 0;
let deleteCalls = 0;
let inventoryUnavailable = false;
let pendingPanelReads = 0;
const panelCommands = new Set(["read_instance_details_from_storage", "list_instance_backups",
  "read_instance_runtime_overview_from_storage", "read_instance_runtime_window_snapshot",
  "read_instance_log_document_from_storage", "preview_instance_launch"]);

Object.assign(globalThis, { __reliabilityFixtureCleanup: async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  await act(async () => { root.unmount(); await frame(); });
  check(intervals.size === 0, "Unmount must release polling");
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
  return { browser_errors: errors, native_dialogs: 0 };
} });

async function run() {
  // Bootstrap in the browser host, then substitute only native calls. The
  // entire App, real panel reader, pause policy and selection handling run.
  await act(async () => { await prepareBrowserLocaleCatalogs(); await import("../../src/views/ServerWorkspaceView"); root.render(<StrictMode><I18nProvider><InstanceSettingsSaveProvider><App />
  </InstanceSettingsSaveProvider></I18nProvider></StrictMode>); });
  await settleUntil(() => fixture.querySelector(".shell-topnav-badge")?.textContent === "1", "App bootstrap must finish");
  Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command: string, args: Record<string, unknown> = {}) => {
    if (panelCommands.has(command)) pendingPanelReads++;
    try {
    if (command === "list_instances_from_storage") {
      inventoryReads++;
      if (inventoryUnavailable) throw new Error("INVENTORY_UNAVAILABLE_FIXTURE");
      return structuredClone(mockBootstrap.state.instances);
    }
    if (command === "read_instance_details_from_storage") {
      detailReads++;
      throw new Error("MISSING_RUNTIME_FIXTURE: required private runtime is missing");
    }
    if (command === "delete_instance_record" || command === "archive_instance_record") {
      deleteCalls++;
      throw new Error("Automatic reconciliation must not submit a manual retirement request");
    }
    return await invokeMock(command, args);
    } finally {
      if (panelCommands.has(command)) pendingPanelReads--;
    }
  } } });
  const servers = [...fixture.querySelectorAll<HTMLButtonElement>(".shell-topnav-item")]
    .find((button) => button.querySelector(".shell-topnav-label")?.textContent === "服务器");
  check(servers, "Server navigation must exist");
  await act(async () => { servers.click(); await frame(); });
  await settleUntil(() => Boolean(fixture.querySelector('.instance-unavailable[aria-busy="false"]')), "Missing runtime must show recovery state");
  for (let attempt = 0; attempt < 4 && intervals.size > 0; attempt++) {
    await settleUntil(() => pendingPanelReads === 0, "Previous native panel batch must settle before the next poll");
    await act(async () => { for (const tick of intervals.values()) tick(); await frame(); });
  }
  await settleUntil(() => intervals.size === 0 && detailReads >= 3,
    `Repeated failures must pause detail polling; detailReads=${detailReads}, inventoryReads=${inventoryReads}, intervals=${intervals.size}`);
  check(intervals.size === 0, "The recovery check must happen after polling stopped");

  inventoryUnavailable = true;
  const readsBeforeFailure = inventoryReads;
  await act(async () => { window.dispatchEvent(new Event("focus")); await frame(); });
  await settleUntil(() => inventoryReads > readsBeforeFailure, "Focus must reconcile while detail polling is paused");
  check(fixture.querySelector(`[data-card-id="${instance.id}"]`), "Inventory failure must preserve the instance card");
  inventoryUnavailable = false;
  mockBootstrap.state.instances = [];
  await act(async () => { document.dispatchEvent(new Event("visibilitychange")); await frame(); });
  await settleUntil(() => !fixture.querySelector(`[data-card-id="${instance.id}"]`) && !fixture.querySelector(".instance-unavailable"),
    "Confirmed missing records must clear both the card and selected failure panel");
  await settleUntil(() => intervals.size === 1, "Clearing the last instance must reset the paused refresh lifecycle");
  check(fixture.querySelector(".shell-topnav-badge")?.textContent === "0", "Navigation instance count must follow the reconciled list");
  check(deleteCalls === 0, "Foreground reconciliation must never invoke an archive or file deletion from the UI");
  check(errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  return { status: "passed", detail_reads: detailReads, inventory_reads: inventoryReads,
    paused_reconciliation: true, inventory_failure_preserved: true, manual_deletion_calls: deleteCalls, browser_errors: errors };
}

void run().catch((error) => ({ status: "failed", error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .then((report) => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
    return fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) });
  });
