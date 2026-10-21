import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, StrictMode, useState, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC } from "@tauri-apps/api/mocks";
import { I18nProvider } from "../../src/i18n";
import { bootstrapApp, readInstanceDetails, readInstanceRuntime, readModuleDetails } from "../../src/api";
import { invokeMock } from "../../src/api-mock";
import { createDefaultAiSettings } from "../../src/ai-settings";
import { ServersView } from "../../src/views/ServersView";
import { RuntimeSurfaceWorkbench } from "../../src/views/servers/RuntimeSurfaceWorkbench";
import { useSteamWorkshopAccount } from "../../src/views/servers/useSteamWorkshopAccount";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import { bridge } from "./runtime-browser-events";
import type { LogTailSnapshot } from "../../src/types";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const errors: string[] = [];
addEventListener("error", event => errors.push(event.message));
addEventListener("unhandledrejection", event => errors.push(String(event.reason)));
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const noOperation = () => {};
let scenarios = 0;
let starts = 0;
const consoleNode = () => fixture.querySelector<HTMLElement>("pre.server-runtime-console");
const text = () => consoleNode()?.textContent ?? "";
const runtimeListenerCount = () => bridge.entries.filter(entry => bridge.active.has(entry.id)
  && ["runtime-log-stream", "runtime-service-events-reset"].includes(entry.name)).length;
function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(description);
}
async function settleUntil(predicate: () => boolean, description: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    check(performance.now() < deadline, description);
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 10)); });
  }
}
async function click(selector: string) {
  const element = fixture.querySelector<HTMLButtonElement>(selector);
  check(element && !element.disabled && element.getClientRects().length > 0, `Missing visible enabled control: ${selector}`);
  await act(async () => { element.scrollIntoView({ block: "nearest" }); element.click(); });
}
async function tab(id: "runtime" | "settings" | "maintenance") {
  await click(`.server-detail-tabs [role="tab"][id$="-${id}"]`);
  await settleUntil(() => Boolean(fixture.querySelector(`.server-detail-tabs [role="tab"][id$="-${id}"][aria-selected="true"]`)),
    `The ${id} tab did not become selected`);
}
const snapshot = (path: string, lines: string[]): LogTailSnapshot => ({ source_path: path, lines,
  total_lines: lines.length, truncated: false, read_error: null });

function SteamStartupHarness({ workbench, active }: {
  workbench: ComponentProps<typeof RuntimeSurfaceWorkbench>; active: boolean
}) {
  const account = useSteamWorkshopAccount({ kind: "instance", id: workbench.details.summary.id });
  const [steamSelected, setSteamSelected] = useState(false);
  return <>
    <button type="button" id="startup-harness-focus">Other workspace focus</button>
    <RuntimeSurfaceWorkbench {...workbench} active={active} steamAccount={account}
      steamCmdSelected={steamSelected} onSteamCmdSelected={setSteamSelected} />
  </>;
}

async function source(selector: string, value: "server" | "steamcmd") {
  const select = fixture.querySelector<HTMLSelectElement>(selector);
  check(select && !select.disabled && select.getClientRects().length > 0, `Missing visible console source: ${selector}`);
  await act(async () => { select.value = value; select.dispatchEvent(new Event("change", { bubbles: true })); });
}

async function run() {
  const bootstrap = await bootstrapApp({ includeSystemSnapshot: false });
  const original = bootstrap.state.instances.find(instance => instance.module_id === "minecraft");
  check(original, "Browser mock must provide a Minecraft instance");
  const instance = { ...original, status: "Stopped" as const, active_process_count: 0 };
  const details = { ...await readInstanceDetails(instance.id), summary: instance, active_run: null };
  const moduleDetails = await readModuleDetails(instance.module_id);
  const runtime = { ...await readInstanceRuntime(instance.id), recent_runs: [],
    log_tail: snapshot("fixture/tabs-previous.log", ["PREVIOUS_RUN_OUTPUT"]) };
  let retained = runtime.log_tail;
  let finishStart: (() => void) | undefined;
  Object.assign(globalThis, { isTauri: true });
  mockIPC((command, args) => command === "read_instance_log_document_from_storage" ? retained
    : invokeMock(command, args as Record<string, unknown>));
  const props: ComponentProps<typeof ServersView> = {
    aiSettings: createDefaultAiSettings(), assistantCanRun: false, bindAddressCandidates: [], instances: [instance],
    moduleInstallations: { [instance.module_id]: { installState: "Installed", hasManagedInstallSource: true } },
    instanceLaunchPlans: {}, instanceLaunchFailures: {}, section: "overview", onWorkspaceSectionChange: noOperation,
    selectedInstanceId: instance.id, selectedDetails: details, selectedBackups: [], selectedModuleDetails: moduleDetails,
    selectedModuleDetailsError: null, runtime, runtimeWindows: null, launchPlan: null, launchPlanError: null,
    refreshIssue: null, onActivity: noOperation, onResumeAutoRefresh: noOperation, onSelectInstance: noOperation,
    onStart: async id => {
      check(id === instance.id, "Start targeted the wrong instance");
      starts++;
      await new Promise<void>(resolve => { finishStart = resolve; });
    },
    onStop: noOperation, onInstallModule: async () => {}, onOpenModuleLibrary: noOperation,
    onCreateInstance: noOperation, onPickDirectory: async () => null,
    onImportDontStarveWorldData: async () => { throw new Error("Unexpected world import"); },
    onOpenLocalPath: noOperation, onSendRuntimeCommand: async () => null, onSuppressRuntimeWindows: async () => {},
    onCreateBackup: noOperation, onDeleteInstance: noOperation, onArchivesChanged: async () => {},
    onArchiveInstance: noOperation, onRestoreBackup: noOperation, onRenameBackup: async () => false,
    onDeleteBackup: noOperation, onSaveSettings: async () => undefined, onSaveAutostart: async () => {},
    onApplyPlayerAccessMutation: async () => { throw new Error("Unexpected player mutation"); }
  };
  const render = () => root.render(<StrictMode><I18nProvider><InstanceSettingsSaveProvider>
    <ServersView {...props} />
  </InstanceSettingsSaveProvider></I18nProvider></StrictMode>);
  const emit = (lines: string[]) => bridge.emit({ instance_id: instance.id,
    log_path: "fixture/tabs-startup.log", lines, byte_offset: 100, emitted_at_unix_ms: Date.now() });
  await act(async () => { await prepareBrowserLocaleCatalogs(); render(); });
  await settleUntil(() => Boolean(consoleNode()) && runtimeListenerCount() === 2,
    "The actual ServersView runtime console and event subscriptions did not mount");
  await click(".server-list-card-primary-action");
  await settleUntil(() => Boolean(finishStart), "The production start action did not reach onStart");
  check(starts === 1 && fixture.querySelector(".server-list-card-primary-action")?.getAttribute("aria-busy") === "true",
    "The start action must stay pending while its operation is unfinished");
  await act(async () => { emit(["BEFORE_LEAVING_STARTUP_PROGRESS"]); });
  check(text().includes("BEFORE_LEAVING_STARTUP_PROGRESS") && !text().includes("PREVIOUS_RUN_OUTPUT"),
    "Starting through ServersView did not display the current startup output");
  const startupConsole = consoleNode();
  scenarios++;

  await tab("settings");
  await settleUntil(() => Boolean(fixture.querySelector(".server-detail-scroll--settings .configuration-workspace")),
    "The actual configuration workspace did not mount");
  check(consoleNode() === startupConsole && runtimeListenerCount() === 2 && consoleNode()!.getClientRects().length === 0,
    "Selecting configuration during startup must hide the console while retaining its event subscriptions");
  await act(async () => { emit(["WHILE_CONFIGURATION_OPEN_PROGRESS"]); });
  await tab("runtime");
  check(consoleNode() === startupConsole && consoleNode()!.getClientRects().length > 0
    && text().includes("BEFORE_LEAVING_STARTUP_PROGRESS") && text().includes("WHILE_CONFIGURATION_OPEN_PROGRESS"),
    "Returning from configuration lost progress emitted while the runtime console was hidden");
  scenarios++;

  await tab("maintenance");
  await settleUntil(() => Boolean(fixture.querySelector(".server-detail-scroll--maintenance .configuration-workspace")),
    "The actual maintenance workspace did not mount");
  check(consoleNode() === startupConsole && runtimeListenerCount() === 2 && consoleNode()!.getClientRects().length === 0,
    "Selecting maintenance during startup must hide the console while retaining its event subscriptions");
  await act(async () => { emit(["WHILE_MAINTENANCE_OPEN_PROGRESS"]); });
  await tab("runtime");
  check(consoleNode() === startupConsole && consoleNode()!.getClientRects().length > 0
    && text().includes("BEFORE_LEAVING_STARTUP_PROGRESS") && text().includes("WHILE_CONFIGURATION_OPEN_PROGRESS")
    && text().includes("WHILE_MAINTENANCE_OPEN_PROGRESS"),
    "Returning from maintenance lost the startup transcript or output emitted in another tab");
  scenarios++;

  await tab("maintenance");
  check(runtimeListenerCount() === 2, "Pending startup must still own its listeners before completion");
  retained = snapshot("fixture/tabs-startup.log", ["STARTUP_COMPLETE"]);
  await act(async () => {
    const running = { ...instance, status: "Running" as const, active_process_count: 1 };
    props.instances = [running];
    props.selectedDetails = { ...details, summary: running };
    props.runtime = { ...runtime, log_tail: retained };
    render();
    finishStart!();
  });
  await settleUntil(() => !consoleNode() && runtimeListenerCount() === 0,
    "Completing startup outside the runtime tab must unmount the hidden console and release its subscriptions");
  check(fixture.querySelector('.server-detail-tabs [id$="-maintenance"][aria-selected="true"]'),
    "Completing startup unexpectedly changed the user's selected tab");
  scenarios++;

  await act(async () => { root.render(null); });
  check(runtimeListenerCount() === 0, "Unmounting ServersView leaked runtime listeners");

  const rustModule = await readModuleDetails("rust");
  const steamWorkbench: ComponentProps<typeof RuntimeSurfaceWorkbench> = { details: { ...details,
    summary: { ...instance, module_id: "rust" } }, runtime, moduleDetails: rustModule, runtimeWindows: null,
    startupPending: true, startupBoundary: { instanceId: instance.id, startedAtUnixMs: Date.now(),
      previousLogPaths: new Set(["fixture/tabs-previous.log"]) }, launchHostSurface: "managed_terminal" };
  let harnessActive = true;
  const renderHarness = () => root.render(<StrictMode><I18nProvider>
    <SteamStartupHarness workbench={steamWorkbench} active={harnessActive} />
  </I18nProvider></StrictMode>);
  await act(async () => { renderHarness(); });
  await settleUntil(() => Boolean(fixture.querySelector(".server-steamcmd-source")) && runtimeListenerCount() === 2,
    "The startup harness did not expose the real SteamCMD source selector");
  await act(async () => { emit(["BEFORE_STEAMCMD_STARTUP_PROGRESS"]); });
  const steamStartupConsole = consoleNode();
  check(steamStartupConsole?.textContent?.includes("BEFORE_STEAMCMD_STARTUP_PROGRESS"),
    "The SteamCMD-capable startup did not receive its initial server log");
  await source(".server-runtime-console-card:not(.server-steamcmd-console) .server-steamcmd-source", "steamcmd");
  const steamConsole = fixture.querySelector<HTMLElement>(".server-steamcmd-console pre");
  check(consoleNode() === steamStartupConsole && steamStartupConsole.getClientRects().length === 0
    && steamConsole && steamConsole.getClientRects().length > 0 && runtimeListenerCount() === 2,
    "Selecting SteamCMD during startup must retain the hidden server console and its subscriptions");
  await act(async () => { emit(["WHILE_STEAMCMD_OPEN_PROGRESS"]); });
  await source(".server-steamcmd-console .server-steamcmd-source", "server");
  check(consoleNode() === steamStartupConsole && steamStartupConsole.getClientRects().length > 0
    && text().includes("BEFORE_STEAMCMD_STARTUP_PROGRESS") && text().includes("WHILE_STEAMCMD_OPEN_PROGRESS"),
    "Returning from SteamCMD lost the original server console or progress emitted while it was hidden");
  scenarios++;

  await source(".server-runtime-console-card:not(.server-steamcmd-console) .server-steamcmd-source", "steamcmd");
  const outsideFocus = fixture.querySelector<HTMLButtonElement>("#startup-harness-focus");
  check(outsideFocus, "The harness focus target did not mount");
  outsideFocus.focus();
  check(document.activeElement === outsideFocus, "The harness must establish focus outside both consoles");
  harnessActive = false;
  await act(async () => { renderHarness(); });
  const hiddenConsoles = [...fixture.querySelectorAll<HTMLElement>("pre.server-runtime-console")];
  const hiddenSteamInput = fixture.querySelector<HTMLInputElement>(".server-steamcmd-console input");
  check(hiddenConsoles.length === 2 && hiddenConsoles.every(node => node.getClientRects().length === 0)
    && consoleNode() === steamStartupConsole && runtimeListenerCount() === 2,
    "An inactive SteamCMD-selected startup must hide both consoles while retaining the startup listener");
  check(hiddenSteamInput?.disabled && document.activeElement === outsideFocus,
    "An inactive anonymous SteamCMD console must not take focus into its hidden input");
  scenarios++;

  await act(async () => { root.unmount(); });
  check(runtimeListenerCount() === 0, "The startup tabs fixture leaked runtime listeners");
  check(errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  return { status: "passed", scenarios, starts, browser_errors: errors };
}

let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error(`Fixture stalled after ${scenarios} scenarios`)), 25000);
})]).finally(() => clearTimeout(watchdog))
  .catch(error => ({ status: "failed", scenarios, starts, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .then(report => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
