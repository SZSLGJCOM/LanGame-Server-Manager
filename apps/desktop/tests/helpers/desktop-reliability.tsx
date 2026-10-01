import { StrictMode, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { I18nProvider } from "../../src/i18n";
import { RuntimeSurfaceWorkbench } from "../../src/views/servers/RuntimeSurfaceWorkbench";
import { RUNTIME_LOG_STREAM_EVENT, type RuntimeLogStreamEvent } from "../../src/runtime-log-stream";

interface Observations {
  event_command_counts: number[];
  dom_command_counts: number[];
  max_tail_lines: number;
  native_bridge: boolean;
  browser_errors: string[];
  dom_nodes_after_unmount?: number;
}

interface Status {
  nonce: string;
  host_pid: number;
  server_pid: number;
  instance_id: string;
  run_id: number;
  log_path: string;
  tail_lines: string[];
  stage: string;
  server_alive: boolean;
  command_count: number;
  busy: boolean;
  error?: string | null;
  observations?: Observations | null;
  recovery: {
    observer_ready: boolean;
    generation: number;
    recoveries: number;
    failures: number;
    page_loads: number;
    paused: boolean;
    last_error?: string | null;
    last_failure_kind?: string | null;
  };
}

type Step = "send_command" | "crash_renderer" | "crash_browser" | "finish";
type Props = ComponentProps<typeof RuntimeSurfaceWorkbench>;
const observations: Observations = {
  event_command_counts: [], dom_command_counts: [], max_tail_lines: 0,
  native_bridge: false, browser_errors: [],
};
let storageKey: string | null = null;
let failed = false;
let stopEvents: UnlistenFn | null = null;
const originalConsoleError = console.error;
const originalConsoleWarn = console.warn;
const submission: { expected: number | null; pending: Promise<Status> | null } = { expected: null, pending: null };

function check(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function persist() {
  if (storageKey) sessionStorage.setItem(storageKey, JSON.stringify(observations));
}

function fail(error: unknown) {
  const message = error instanceof Error ? error.message : String(error);
  observations.browser_errors.push(message);
  failed = true;
  try { persist(); } catch (storageError) {
    observations.browser_errors.push(`Cannot preserve fixture observations: ${String(storageError)}`);
  }
  document.body.dataset.reliability = "failed";
  const status = document.getElementById("status");
  if (status) status.textContent = `Desktop reliability failed: ${observations.browser_errors.join("; ")}`;
  originalConsoleError("DESKTOP_RELIABILITY_FAILED", ...observations.browser_errors);
}

addEventListener("error", (event) => fail(event.message));
addEventListener("unhandledrejection", (event) => fail(event.reason));
console.error = (...args) => { originalConsoleError(...args); fail(args.map(String).join(" ")); };
console.warn = (...args) => {
  originalConsoleWarn(...args);
  if (/Runtime log subscription/i.test(String(args[0]))) fail(args.map(String).join(" "));
};

const pause = () => new Promise<void>((resolve) => setTimeout(resolve, 25));
async function waitUntil(condition: () => boolean, description: string, budget = 15000) {
  const end = performance.now() + budget;
  while (!condition()) {
    check(!failed, observations.browser_errors.join("; "));
    check(performance.now() < end, `${description} exceeded ${budget} ms`);
    await pause();
  }
  check(!failed, observations.browser_errors.join("; "));
}

async function bounded<T>(operation: Promise<T>, description: string): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([operation, new Promise<never>((_, reject) => {
      timer = setTimeout(() => reject(new Error(`${description} exceeded 15 seconds`)), 15000);
    })]);
  } finally { clearTimeout(timer); }
}

function validateStatus(status: Status) {
  check(typeof status.nonce === "string" && status.nonce.length > 0, "Missing native fixture nonce");
  check(Number.isSafeInteger(status.host_pid) && status.host_pid > 0, "Invalid host PID");
  check(Number.isSafeInteger(status.server_pid) && status.server_pid > 0, "Invalid server PID");
  check(status.instance_id === "desktop-reliability" && status.run_id === 1, "Unexpected fixture identity");
  check(typeof status.log_path === "string" && status.log_path.length > 0, "Missing fixture log path");
  check(Array.isArray(status.tail_lines) && status.tail_lines.every((line) => typeof line === "string"), "Invalid native tail");
  check(Number.isInteger(status.command_count) && status.command_count >= 0 && status.command_count <= 3, "Unexpected native command count");
  check(typeof status.busy === "boolean", "Missing native operation state");
  check(status.server_alive, "The synthetic server did not survive the UI lifecycle");
  check(!status.error, `Native fixture failed: ${status.error}`);
  check(!status.recovery.paused, "Native recovery paused before acceptance completed");
}

async function readStatus() {
  const status = await bounded(invoke<Status>("desktop_reliability_status"), "Native fixture status");
  validateStatus(status);
  return status;
}

async function step(status: Status, action: Step, evidence?: Observations) {
  return bounded(invoke<Status>("desktop_reliability_step", {
    nonce: status.nonce, action, ...(evidence ? { observations: evidence } : {}),
  }), `Native fixture ${action}`);
}

async function submittedCommand(): Promise<Status> {
  const pending = submission.pending;
  check(pending, "The native command result is missing");
  return pending;
}

function mergeEvidence(value: unknown) {
  check(value !== null && typeof value === "object", "Invalid saved fixture observations");
  const evidence = value as Record<string, unknown>;
  check(evidence.native_bridge === true, "Saved observations were not obtained through the native bridge");
  for (const key of ["event_command_counts", "dom_command_counts"] as const) {
    const counts = evidence[key];
    check(Array.isArray(counts) && counts.every((count) => Number.isInteger(count) && count >= 1 && count <= 3), `Invalid saved ${key}`);
    observations[key] = [...new Set([...observations[key], ...counts])].sort();
  }
  check(typeof evidence.max_tail_lines === "number" && Number.isInteger(evidence.max_tail_lines)
    && evidence.max_tail_lines >= 0 && evidence.max_tail_lines <= 400, "Invalid saved DOM tail budget");
  observations.max_tail_lines = Math.max(observations.max_tail_lines, evidence.max_tail_lines);
  check(Array.isArray(evidence.browser_errors) && evidence.browser_errors.every((error) => typeof error === "string"), "Invalid saved browser errors");
  observations.browser_errors = [...new Set([...observations.browser_errors, ...evidence.browser_errors])];
  check(observations.browser_errors.length === 0, observations.browser_errors.join("; "));
}

function observeMarkers(lines: string[], key: "event_command_counts" | "dom_command_counts") {
  for (const line of lines) {
    for (const match of line.matchAll(/\bLGSM_RELIABILITY_COMMAND_([123])\b/g)) {
      const count = Number(match[1]);
      if (!observations[key].includes(count)) observations[key].push(count);
    }
  }
  observations[key].sort();
  persist();
}

function activeNativeLogHandlers() {
  // Read-only observation of the pinned Tauri 2.11.5 bridge. Event table entries
  // are non-enumerable and retained after unlisten; the callback Map is authoritative.
  // Fail if its shape changes. No listener, callback, invoke or bridge is replaced.
  const native = window as unknown as {
    __TAURI_INTERNALS__?: { callbacks?: Map<number, unknown> };
    __internal_unstable_listeners_object_id__?: Record<string, Record<string, { handlerId: number }>>;
  };
  const callbacks = native.__TAURI_INTERNALS__?.callbacks;
  check(callbacks instanceof Map, "Pinned native callback registry is unavailable");
  const entries = native.__internal_unstable_listeners_object_id__?.[RUNTIME_LOG_STREAM_EVENT];
  if (!entries) return 0;
  return Object.getOwnPropertyNames(entries).filter((id) => {
    check(Number.isSafeInteger(entries[id]?.handlerId), "Invalid native log callback identity");
    return callbacks.has(entries[id].handlerId);
  }).length;
}

function properties(status: Status): Props {
  return {
    details: {
      summary: { id: status.instance_id, name: "Fixture server", module_id: "demo", status: "running", active_process_count: 1, autostart: false, bind_ip: "127.0.0.1" },
      active_run: { run_id: status.run_id, pid: status.server_pid, log_path: status.log_path },
      ports: [], settings_json: "{}", config_file_path: "fixture/instance.json", saves_path: "fixture/saves",
      backup_uses_declared_saves_path: true, auto_backup_on_stop: false, backup_retention_count: 3,
    },
    moduleDetails: {
      summary: { id: "demo", name: "Fixture server", version: "1.0.0", install_state: "installed", supported_platforms: ["windows"] },
      default_ports: [],
      runtime: {
        player_actions: [{ id: "fixture_command", label: "Fixture command", transport: "stdin", command_template: "LGSM_RELIABILITY_COMMAND" }],
      },
    },
    runtime: {
      recent_runs: [], diagnostics: [], health: { status: "ready", summary: "Synthetic process ready" },
      log_tail: { source_path: status.log_path, lines: status.tail_lines.slice(-400), total_lines: status.tail_lines.length, truncated: status.tail_lines.length > 400, read_error: null },
      players: { query: { status: "unavailable", summary: "Fixture" } },
      performance: {
        applied_resource_limits: null,
        status: "idle", summary: "Fixture", process_count: 1, processes: [],
        policy: { resource_limits: { cpu_percent: null, memory_limit_mib: null, host_memory_reserve_mib: 2048 }, priority_class: "normal", apply_to_child_processes: false, startup_stagger_ms: 0, child_process_stagger_ms: 0 },
        preview: { summary: "Fixture", priority_source: "fixture", cpu_affinity_source: "fixture", logical_cpu_count: 1 },
      },
      startup_queue: {
        status: "idle", summary: "Fixture", active_run_count: 1, active_process_count: 1,
        tracked_instance_count: 1, tracked_process_count: 1, next_start_delay_ms: 0,
        projected_effective_stagger_ms: 0, projected_queued_start_count: 0, projected_process_count: 1,
        pending_restart_count: 0, next_restart_delay_ms: 0,
      },
      stability: { status: "idle", summary: "Fixture", recent_crash_count: 0, restart_policy_enabled: false, restart_limit: 0, restart_backoff_ms: 0 },
    },
    runtimeWindows: null, startupPending: false, launchHostSurface: "managed_terminal",
    onSendRuntimeCommand: async (instance, command, processKey, options) => {
      check(instance === status.instance_id, "Command targeted another instance");
      check(processKey === null && options?.transport === "stdin", "Command did not select the fixture's native input channel");
      check(submission.expected !== null && command === `fixture-command-${submission.expected}`, "The real form submitted an unexpected command");
      check(submission.pending === null, "The command form submitted twice");
      submission.pending = step(status, "send_command");
      await submission.pending;
      return null;
    },
    onSuppressRuntimeWindows: async () => { throw new Error("The fixture must not alter native game windows"); },
  };
}

async function run() {
  check(isTauri(), "This fixture requires the real Tauri bridge");
  let status = await readStatus();
  storageKey = `langame-desktop-reliability:${status.nonce}`;
  const saved = sessionStorage.getItem(storageKey);
  if (saved) mergeEvidence(JSON.parse(saved));
  if (status.observations) mergeEvidence(status.observations);
  check(activeNativeLogHandlers() === 0, "A fresh fixture page already has log listeners");
  observations.native_bridge = true;
  persist();
  const fixture = document.getElementById("fixture");
  check(fixture, "Missing fixture container");
  const root = createRoot(fixture);
  const sampleDOM = () => {
    const consoleNode = fixture.querySelector("pre.server-runtime-console");
    if (!consoleNode) return;
    check(!consoleNode.querySelector(".server-runtime-console-errors"), "The real console reported a log read error");
    check(Array.from(consoleNode.children).every((child) =>
      child.matches("span.server-runtime-command-feedback[role='status']")), "Unexpected content inside the real console");
    const logText = Array.from(consoleNode.childNodes)
      .filter((node) => node.nodeType === Node.TEXT_NODE)
      .map((node) => node.textContent ?? "").join("");
    const lines = logText.split("\n");
    check(lines.length <= 400, "The real DOM exceeded the 400-line tail budget");
    observations.max_tail_lines = Math.max(observations.max_tail_lines, lines.length);
    observeMarkers(lines, "dom_command_counts");
  };
  const domObserver = new MutationObserver(() => {
    if (!failed) try { sampleDOM(); } catch (error) { fail(error); }
  });
  domObserver.observe(fixture, { childList: true, subtree: true, characterData: true });
  stopEvents = await bounded(listen<RuntimeLogStreamEvent>(RUNTIME_LOG_STREAM_EVENT, ({ payload }) => {
    if (failed || payload.instance_id !== status.instance_id || payload.log_path !== status.log_path) return;
    observeMarkers(payload.lines, "event_command_counts");
  }), "Native runtime log subscription");
  root.render(<StrictMode><I18nProvider><RuntimeSurfaceWorkbench {...properties(status)} /></I18nProvider></StrictMode>);
  await waitUntil(() => activeNativeLogHandlers() === 2 && Boolean(fixture.querySelector("pre.server-runtime-console")), "Real console mount and native subscriptions");
  sampleDOM();

  for (let stepIndex = 0; stepIndex < 12; stepIndex += 1) {
    status = await readStatus();
    const description = document.getElementById("status");
    if (description) description.textContent = `Native desktop fixture: ${status.stage}`;
    if (status.busy || !status.recovery.observer_ready || status.stage.endsWith("_recovery")) {
      const recoveryEnd = performance.now() + 15000;
      while (status.busy || !status.recovery.observer_ready || status.stage.endsWith("_recovery")) {
        check(performance.now() < recoveryEnd, "Native recovery did not become ready");
        await pause();
        status = await readStatus();
      }
    }
    const commandCounts: Record<string, number | undefined> = {
      initial_command: 1, after_renderer_command: 2, after_browser_command: 3,
    };
    const count = commandCounts[status.stage];
    if (count) {
      check(status.command_count === count - 1, "A completed command must not be replayed");
      submission.expected = count;
      submission.pending = null;
      const input = fixture.querySelector<HTMLInputElement>("input.server-runtime-console-command-input");
      const form = fixture.querySelector<HTMLFormElement>("form.server-runtime-console-command-form");
      check(input && form && !input.disabled, "The real command form is not enabled");
      const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
      check(setValue, "Native input value setter is unavailable");
      setValue.call(input, `fixture-command-${count}`);
      input.dispatchEvent(new Event("input", { bubbles: true }));
      await pause();
      form.requestSubmit();
      await waitUntil(() => submission.pending !== null, "Real command form submission");
      const result = await submittedCommand();
      validateStatus(result);
      check(result.command_count === count, "Native command count did not advance exactly once");
      await waitUntil(() => observations.event_command_counts.includes(count)
        && observations.dom_command_counts.includes(count), "Native output marker in both event bridge and real DOM");
      continue;
    }
    if (status.stage === "renderer_crash" || status.stage === "browser_crash") {
      check(status.command_count === (status.stage === "renderer_crash" ? 1 : 2), "Fault stage command count mismatch");
      persist();
      const action = status.stage === "renderer_crash" ? "crash_renderer" : "crash_browser";
      // Rust retains only this synthetic checkpoint: sessionStorage alone does not
      // survive destruction and recreation of a browsing context.
      void step(status, action, observations).catch(fail);
      setTimeout(() => fail(`Native ${action} did not replace this page`), 15000);
      return;
    }
    if (status.stage === "finish") {
      check(JSON.stringify(observations.event_command_counts) === "[1,2,3]", "Incomplete native marker history");
      check(JSON.stringify(observations.dom_command_counts) === "[1,2,3]", "Incomplete DOM marker history");
      check(status.recovery.recoveries >= 2 && status.recovery.failures >= 2, "Both native failures must recover");
      check(observations.max_tail_lines === 400, "The fixture did not reach the DOM tail budget");
      domObserver.disconnect();
      root.unmount();
      await waitUntil(() => fixture.childNodes.length === 0 && activeNativeLogHandlers() === 1, "Component unmount and native listener cleanup");
      await stopEvents();
      stopEvents = null;
      await waitUntil(() => activeNativeLogHandlers() === 0, "Fixture observer cleanup");
      observations.dom_nodes_after_unmount = fixture.childNodes.length;
      check(observations.browser_errors.length === 0, observations.browser_errors.join("; "));
      persist();
      document.body.dataset.reliability = "finishing";
      await step(status, "finish", observations);
      return;
    }
    throw new Error(`Unexpected native fixture stage: ${status.stage}`);
  }
  throw new Error("Native fixture exceeded its bounded stage count");
}

void run().catch(fail);
