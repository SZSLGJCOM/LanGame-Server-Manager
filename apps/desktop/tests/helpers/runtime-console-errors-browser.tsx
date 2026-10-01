import React, { act, StrictMode, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC } from "@tauri-apps/api/mocks";
import { I18nProvider } from "../../src/i18n";
import { bootstrapApp, readInstanceDetails, readInstanceLogDocument, readInstanceRuntime, readModuleDetails } from "../../src/api";
import { invokeMock } from "../../src/api-mock";
import { instancePanelReader, type InstancePanelPart } from "../../src/instance-panel-loader";
import { createDefaultAiSettings } from "../../src/ai-settings";
import { ServersView } from "../../src/views/ServersView";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import { bridge } from "./runtime-browser-events";
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
const noOperation = () => {};
let scenarios = 0;
let retries = 0;
const commands: string[] = [];
const copied: string[] = [];
Object.defineProperty(navigator, "clipboard", { configurable: true, value: {
  writeText: async (text: string) => { copied.push(text); }
} });
const rejectedReads = new Map<InstancePanelPart, string>();
let holdReadErrors = false;
let logReads = 0;
const heldReadErrors: Array<() => void> = [];
const nativeLogError = "invalid args `instanceId` for command `read_instance_log_document_from_storage`: command read_instance_log_document_from_storage missing required key instanceId";
const logError = [nativeLogError,
  ...Array.from({ length: 35 }, (_, index) => `诊断详情 ${index + 1}：读取日志快照失败，服务器实时输出和命令输入仍然可用。`),
  `Long path: ${"synthetic-segment-".repeat(35)}`, "LOG_ERROR_END"].join("\n");
const windowError = "Fixture runtime window enumeration failed: access denied. WINDOW_ERROR_END";
const runtimeError = "Fixture runtime overview IPC rejected. RUNTIME_ERROR_END";
const tailError = "Fixture log tail read failed: the synthetic file is unavailable. TAIL_ERROR_END";

function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(description);
}
async function settleUntil(predicate: () => boolean, description: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    check(performance.now() < deadline, description);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function key(name: "Tab" | "Enter") {
  const response = await fetch(`/__reliability_key/${nonce}/${name}`, { method: "POST" });
  check(response.ok, `Native ${name} dispatch failed`);
}
const consoleNode = () => fixture.querySelector<HTMLElement>("pre.server-runtime-console")!;
function bounds() {
  return [".server-runtime-console-frame", "pre.server-runtime-console", ".server-runtime-console-command-form"]
    .map((selector) => {
      const element = fixture.querySelector<HTMLElement>(selector);
      check(element, `Missing ${selector}`);
      const rect = element.getBoundingClientRect();
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
}
function errorsInside(expected: string[], preserveOutput = true) {
  const terminal = consoleNode();
  const alert = terminal.querySelector<HTMLElement>(".server-runtime-console-errors[role='alert']");
  check(alert, "Read failures must be an accessible alert inside the console's scroll content");
  for (const message of expected) check(alert.textContent?.includes(message), `Console lost complete error: ${message}`);
  check(!fixture.querySelector(".server-runtime-read-error"), "A read failure still occupies space outside the console");
  check([...fixture.querySelectorAll("[role='alert']")].every((element) => terminal.contains(element)),
    "Runtime read errors must not render additional alert panels above the terminal");
  check(getComputedStyle(terminal).overflowY === "auto", "Errors must use the console scroll area");
  check(terminal.scrollWidth <= terminal.clientWidth + 1, "Long error details must wrap within the console width");
  if (preserveOutput) check(terminal.textContent?.includes("existing output retained"), "Read error removed existing console output");
  const retry = alert.querySelector<HTMLButtonElement>(".server-runtime-console-retry");
  check(retry && !retry.disabled, "Read error must offer an enabled retry inside the console");
  terminal.scrollTop = terminal.scrollHeight;
  retry.scrollIntoView({ block: "nearest" });
  const retryRect = retry.getBoundingClientRect();
  const consoleRect = terminal.getBoundingClientRect();
  check(retryRect.top >= consoleRect.top - 1 && retryRect.bottom <= consoleRect.bottom + 1,
    "Retry must be reachable by scrolling inside the console");
  return retry;
}

async function run() {
  const bootstrap = await bootstrapApp({ includeSystemSnapshot: false });
  const original = bootstrap.state.instances[0];
  check(original, "Development fixture must provide an instance");
  const boundedPreview = await readInstanceLogDocument(original.id, 1);
  check(boundedPreview.lines.length === 1, "Development log reads must honor the real API maxLines argument");
  // A synthetic game prevents unrelated remote cover art requests in this local IPC fixture.
  const instance = { ...original, module_id: "fixture-console", name: "运行日志故障复现", status: "Running" as const, active_process_count: 1 };
  const details = { ...await readInstanceDetails(instance.id), summary: instance, active_run: null };
  const nativeModule = await readModuleDetails("dontstarve");
  const moduleDetails = { ...nativeModule, summary: { ...nativeModule.summary, id: instance.module_id } };
  const runtime = { ...await readInstanceRuntime(instance.id), recent_runs: [], diagnostics: [],
    health: { status: "ready", summary: "Fixture ready" },
    log_tail: { source_path: "fixture/runtime.log", lines: ["existing output retained", "Server ready for commands"],
      total_lines: 2, truncated: false, read_error: null } };
  const runtimeWindows = { instance_id: instance.id, observed_at_unix_ms: 1, status: "ready", summary: "Fixture",
    inspected_process_count: 1, windows: [] };
  let retainedTailError: string | null = null;
  let retryFlight: Promise<void> | null = null;
  const props: ComponentProps<typeof ServersView> = {
    aiSettings: createDefaultAiSettings(), assistantCanRun: false, bindAddressCandidates: [], instances: [instance],
    moduleInstallations: { [instance.module_id]: { installState: "Installed", hasManagedInstallSource: true } },
    instanceLaunchPlans: {}, instanceLaunchFailures: {}, section: "overview", onWorkspaceSectionChange: noOperation,
    selectedInstanceId: instance.id, selectedDetails: details, selectedBackups: [], selectedModuleDetails: moduleDetails,
    runtime, runtimeWindows, launchPlan: null, launchPlanError: null,
    refreshIssue: null, onActivity: noOperation,
    onResumeAutoRefresh: () => { retries++; rejectedReads.clear(); retryFlight = refresh(); },
    onSelectInstance: noOperation, onStart: noOperation, onStop: noOperation, onInstallModule: async () => {},
    onOpenModuleLibrary: noOperation, onCreateInstance: noOperation, onPickDirectory: async () => null,
    onImportDontStarveWorldData: async () => { throw new Error("Unexpected world import"); },
    onOpenLocalPath: noOperation, onSendRuntimeCommand: async (id, command) => {
      check(id === instance.id, "Command must target the mounted instance");
      commands.push(command);
      return { instance_id: id, process_key: "main", display_name: "Fixture", pid: 1,
        command, write_confirmation_pending: false, submitted_at_unix_ms: Date.now() };
    }, onSuppressRuntimeWindows: async () => {}, onCreateBackup: noOperation,
    onArchivesChanged: async () => {}, onArchiveInstance: async () => {}, onDeleteInstance: async () => {}, onRestoreBackup: noOperation, onRenameBackup: async () => true,
    onDeleteBackup: async () => {}, onSaveSettings: async () => undefined, onSaveAutostart: async () => {},
    onApplyPlayerAccessMutation: async () => { throw new Error("Unexpected player mutation"); }
  };
  const render = () => root.render(<StrictMode><I18nProvider><InstanceSettingsSaveProvider>
    <ServersView {...props} />
  </InstanceSettingsSaveProvider></I18nProvider></StrictMode>);
  async function refresh() {
    await instancePanelReader.load(instance.id, { signal: new AbortController().signal,
      onProgress: (state) => { props.panelLoadState = state; render(); },
      onUpdate: (patch) => {
        if (patch.runtime) props.runtime = patch.runtime;
        if (patch.runtimeWindows) props.runtimeWindows = patch.runtimeWindows;
        render();
      } });
  }
  // Only the native boundary is substituted. Production API routing, panel reader,
  // React components, translations, and application CSS execute unchanged.
  Object.assign(globalThis, { isTauri: true });
  mockIPC(async (command, args) => {
    const part = ({ read_instance_runtime_overview_from_storage: "runtime",
      read_instance_runtime_window_snapshot: "runtimeWindows", read_instance_log_document_from_storage: "logDocument" } as const)
      [command as "read_instance_runtime_overview_from_storage" | "read_instance_runtime_window_snapshot" | "read_instance_log_document_from_storage"];
    if (part === "logDocument") logReads++;
    if (part && rejectedReads.has(part)) {
      if (holdReadErrors) await new Promise<void>((resolve) => { heldReadErrors.push(resolve); });
      throw new Error(rejectedReads.get(part)!);
    }
    if (part === "runtime") return runtime;
    if (part === "runtimeWindows") return runtimeWindows;
    if (part === "logDocument") return { ...runtime.log_tail, read_error: retainedTailError };
    return invokeMock(command, args as Record<string, unknown>);
  });
  await act(async () => { render(); });
  await settleUntil(() => Boolean(consoleNode()), "Complete ServersView did not mount its runtime console");
  const baseline = bounds();
  check(baseline.every((box) => box.height > 20), `Console baseline has no usable area: ${JSON.stringify(baseline)}`);
  const cases: Array<{ failures: Array<[InstancePanelPart, string]>; emptyRuntime?: boolean }> = [
    { failures: [["logDocument", logError]] },
    { failures: [["runtimeWindows", windowError]] },
    { failures: [["runtime", runtimeError]] },
    { failures: [["runtime", runtimeError]], emptyRuntime: true },
    { failures: [["logDocument", logError], ["runtimeWindows", windowError], ["runtime", runtimeError]] }
  ];
  for (const scenario of cases) {
    rejectedReads.clear();
    for (const [part, error] of scenario.failures) rejectedReads.set(part, error);
    if (scenario.emptyRuntime) props.runtime = null;
    await act(async () => { await refresh(); });
    const label = scenario.failures.map(([part]) => part).join("+");
    unchanged(baseline, label);
    if (scenarios === 0) {
      const terminal = consoleNode();
      check(terminal.scrollHeight > terminal.clientHeight && terminal.scrollTop > 0
        && terminal.scrollHeight - terminal.clientHeight - terminal.scrollTop <= 1,
      "A new read failure must scroll its complete details into reach");
      terminal.scrollTop = 0;
      const readsBeforeEvent = logReads;
      runtime.log_tail.lines.push("output received while reviewing the read failure");
      runtime.log_tail.total_lines++;
      await act(async () => { bridge.emit({ instance_id: instance.id, log_path: runtime.log_tail.source_path,
        lines: ["output received while reviewing the read failure"], byte_offset: 100, emitted_at_unix_ms: 1 }); });
      check(logReads > readsBeforeEvent, "A native event must invalidate the authoritative file read even while reads fail");
      check(!consoleNode().textContent?.includes("output received while reviewing the read failure"),
        "A failed read must not splice an unpositioned event into the retained snapshot");
      check(terminal.scrollTop === 0, "Unchanged errors and later output must preserve the user's scroll position");
      unchanged(baseline, "native output received during read failure");
    }
    if (scenarios === 0 || scenario.failures.length > 1) {
      // Automatic refresh clears reader errors while IPC is pending, then delivers
      // failures independently. Repeated failures must not pull the user off old output.
      consoleNode().scrollTop = 0;
      holdReadErrors = true;
      let repeatedRefresh: Promise<void> | undefined;
      await act(async () => { repeatedRefresh = refresh(); await Promise.resolve(); });
      check(heldReadErrors.length === scenario.failures.length, "Deferred IPC must hold each failing read");
      holdReadErrors = false;
      while (heldReadErrors.length > 0) {
        await act(async () => {
          heldReadErrors.shift()!();
          if (heldReadErrors.length === 0) await repeatedRefresh;
          else await Promise.resolve();
        });
        check(consoleNode().scrollTop === 0,
          "Automatic refresh repeated an existing read failure and stole the user's scroll position");
      }
      unchanged(baseline, "staggered repeated IPC failures");
    }
    const retry = errorsInside(scenario.failures.map(([, message]) => message), !scenario.emptyRuntime);
    unchanged(baseline, `${label} after scrolling`);
    if (scenarios === 0) {
      await act(async () => { fixture.querySelector<HTMLButtonElement>(".server-runtime-console-copy-button")!.click(); });
      check(copied.length === 1 && copied[0].includes(logError) && copied[0].includes("existing output retained"),
        "Copy console must preserve output and complete visible error details");
      const input = fixture.querySelector<HTMLInputElement>(".server-runtime-console-command-input")!;
      check(!input.disabled, "A read failure must not disable the managed command input");
      const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
      await act(async () => { input.focus(); setValue.call(input, "status"); input.dispatchEvent(new Event("input", { bubbles: true })); });
      await act(async () => { await key("Enter"); });
      check(commands.length === 1 && commands[0] === "status", "Read failure blocked command submission");
      check(input.value === "", "Successful command submission did not clear the input");
    }
    await act(async () => {
      if (scenarios === 0) {
        fixture.querySelector<HTMLButtonElement>(".server-runtime-console-copy-button")!.focus();
        await key("Tab");
        check(document.activeElement === retry, "Keyboard Tab must reach the in-console retry control");
        await key("Enter");
      } else retry.click();
      await retryFlight;
    });
    check(!consoleNode().querySelector("[role='alert']"), "Successful retry must clear the read failure");
    check(consoleNode().textContent?.includes("existing output retained"), "Recovery must retain normal console output");
    check(consoleNode().textContent?.split("output received while reviewing the read failure").length === 2,
      "Recovery must display the native output produced during the failure exactly once");
    unchanged(baseline, `${label} recovery`);
    scenarios++;
  }
  retainedTailError = tailError;
  props.runtime = { ...runtime, log_tail: { ...runtime.log_tail, read_error: tailError } };
  await act(async () => { render(); });
  unchanged(baseline, "log_tail.read_error");
  errorsInside([tailError]);
  scenarios++;
  retainedTailError = null;
  props.runtime = runtime;
  await act(async () => { render(); });
  check(!consoleNode().querySelector("[role='alert']"), "A recovered log tail must remove the failure");
  rejectedReads.set("logDocument", nativeLogError);
  await act(async () => { await refresh(); });
  errorsInside([nativeLogError]);

  // Exercise the real Stop -> Start UI and parent click-time boundary. The
  // previous producer can deliver its final shard output after Start is clicked.
  rejectedReads.clear();
  props.panelLoadState = null;
  props.runtime = { ...runtime, recent_runs: [{ run_id: 1, status: "stopped", log_path: runtime.log_tail.source_path,
    processes: [{ log_path: "fixture/old-registered-caves.log" }] }] };
  let starts = 0;
  let stops = 0;
  let finishStart: (() => void) | undefined;
  props.onStop = async () => {
    stops++;
    const stopped = { ...instance, status: "Stopped" as const, active_process_count: 0 };
    props.instances = [stopped];
    props.selectedDetails = { ...details, summary: stopped };
    render();
  };
  props.onStart = async () => {
    starts++;
    await new Promise<void>((resolve) => { finishStart = resolve; });
  };
  const actionButton = () => fixture.querySelector<HTMLButtonElement>(".server-list-card-primary-action")!;
  const emit = (log_path: string, line: string, process_key = "main", emitted_at_unix_ms = Date.now()) =>
    bridge.emit({ instance_id: instance.id, log_path, process_key, lines: [line], byte_offset: 100, emitted_at_unix_ms });
  await act(async () => { render(); });
  await act(async () => { emit("fixture/observed-only-caves.log", "previous unselected shard"); });
  await act(async () => { actionButton().click(); });
  check(stops === 1 && !actionButton().disabled, "Stop must complete before the next startup");
  await act(async () => { actionButton().click(); });
  await settleUntil(() => starts === 1, "Start did not reach the native lifecycle boundary");
  check(actionButton().disabled, "Start must remain pending during native launch");
  check(!consoleNode().textContent?.includes("existing output retained"), "Start retained the previous session's snapshot");
  await act(async () => {
    emit(runtime.log_tail.source_path, "LATE_OLD_MAIN");
    emit("fixture/old-registered-caves.log", "LATE_OLD_REGISTERED_SHARD");
    emit("fixture/observed-only-caves.log", "LATE_OLD_OBSERVED_SHARD");
    emit("fixture/queued-unknown.log", "QUEUED_OLD_EVENT", "main", 0);
    emit("fixture/run-2-main.log", "NEW_MAIN");
    emit("fixture/run-2-caves.log", "NEW_CAVES", "caves");
  });
  check(!/LATE_OLD_|QUEUED_OLD_EVENT/.test(consoleNode().textContent ?? ""), "Previous session output polluted the new startup");
  check(consoleNode().textContent?.includes("[main] NEW_MAIN") && consoleNode().textContent?.includes("[caves] NEW_CAVES"),
    "Startup must retain both current shard streams");
  props.runtime = { ...runtime, log_tail: { ...runtime.log_tail, source_path: "fixture/run-2-main.log", lines: ["NEW_MAIN"] } };
  await act(async () => { render(); });
  await act(async () => { emit("fixture/run-2-extra.log", "NEW_EXTRA", "extra"); });
  check(consoleNode().textContent?.includes("[extra] NEW_EXTRA") && consoleNode().textContent?.includes("[caves] NEW_CAVES"),
    "A new primary poll must preserve the aggregate and allow later shards");
  await act(async () => { finishStart!(); });
  scenarios++;
  // Restore the error scenario for the optional screenshot/report bounds.
  props.instances = [instance];
  props.selectedDetails = details;
  props.runtime = runtime;
  rejectedReads.set("logDocument", nativeLogError);
  await act(async () => { await refresh(); });
  errorsInside([nativeLogError]);
  check(errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  return { status: "passed", scenarios, retries, commands_sent: commands.length, baseline,
    final_bounds: bounds(), browser_errors: errors };
}

let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error(`Interaction stalled after ${scenarios} scenarios`)), 25_000);
})]).finally(() => clearTimeout(watchdog))
  .catch((error) => ({ status: "failed", scenarios, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
