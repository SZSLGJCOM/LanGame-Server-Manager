import React, { act, StrictMode, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { mockIPC } from "@tauri-apps/api/mocks";
import { I18nProvider } from "../../src/i18n";
import { RuntimeSurfaceWorkbench } from "../../src/views/servers/RuntimeSurfaceWorkbench";
import type { RuntimeLogStreamEvent } from "../../src/runtime-log-stream";
import type { LogTailSnapshot } from "../../src/types";
import { bridge } from "./runtime-browser-events";

Object.assign(globalThis, { isTauri: true, IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "en-US");
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };

type Props = ComponentProps<typeof RuntimeSurfaceWorkbench>;
const retainedLogs = new Map<string, LogTailSnapshot>();
mockIPC((command, args) => {
  check(command === "read_instance_log_document_from_storage", `Unexpected native command: ${command}`);
  check(args.maxLines === 400, "Native reads must request the bounded console budget");
  const retained = retainedLogs.get(String(args.instanceId));
  check(retained, "Retained log read must target the mounted instance");
  return retained;
});
function properties(instance: string, logPath: string): Props {
  const props: Props = {
    details: {
      summary: { id: instance, name: instance, module_id: "dontstarve", status: "Stopped", active_process_count: 0, autostart: false, bind_ip: "127.0.0.1" },
      active_run: { run_id: 1, log_path: logPath }, ports: [], settings_json: "{}",
      config_file_path: "fixture/instance.json", saves_path: "fixture/saves",
      backup_uses_declared_saves_path: true, auto_backup_on_stop: false, backup_retention_count: 3
    },
    runtime: {
      recent_runs: [], diagnostics: [], health: { status: "stopped", summary: "Stopped" },
      log_tail: { source_path: logPath, lines: [], total_lines: 0, truncated: false, read_error: null },
      players: { query: { status: "unavailable", summary: "Fixture" } },
      performance: {
        status: "idle", summary: "Fixture", process_count: 0, processes: [],
        policy: { priority_class: "normal", apply_to_child_processes: false, startup_stagger_ms: 0, child_process_stagger_ms: 0 },
        preview: { summary: "Fixture", priority_source: "fixture", cpu_affinity_source: "fixture", logical_cpu_count: 1 }
      },
      startup_queue: {
        status: "idle", summary: "Fixture", active_run_count: 0, active_process_count: 0,
        tracked_instance_count: 0, tracked_process_count: 0, next_start_delay_ms: 0,
        projected_effective_stagger_ms: 0, projected_queued_start_count: 0, projected_process_count: 0,
        pending_restart_count: 0, next_restart_delay_ms: 0
      },
      stability: { status: "idle", summary: "Fixture", recent_crash_count: 0, restart_policy_enabled: false, restart_limit: 0, restart_backoff_ms: 0 }
    },
    runtimeWindows: null, startupPending: false, launchHostSurface: "managed_terminal",
    onSendRuntimeCommand: async () => { throw new Error("Fixture must not submit native commands"); },
    onSuppressRuntimeWindows: async () => { throw new Error("Fixture must not operate native windows"); }
  };
  retainedLogs.set(instance, props.runtime!.log_tail);
  return props;
}

function check(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}
const container = document.getElementById("fixture");
check(container, "Fixture container is missing");
const fixture = container;
const consoleText = () => fixture.querySelector("pre.server-runtime-console")?.textContent ?? "";
const pause = () => new Promise<void>((resolve) => setTimeout(resolve, 10));
async function settleUntil(predicate: () => boolean, message: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    check(performance.now() < deadline, message);
    await act(pause);
  }
  await act(async () => { await Promise.resolve(); });
}
async function render(root: Root, props: Props) {
  await act(async () => { root.render(<StrictMode><I18nProvider><RuntimeSurfaceWorkbench {...props} /></I18nProvider></StrictMode>); });
  await settleUntil(() => Boolean(fixture.querySelector("pre.server-runtime-console")), "Workbench did not commit");
}
let emittedByteOffset = 0;
const event = (instance: string, logPath: string, lines: string[]): RuntimeLogStreamEvent => ({
  instance_id: instance, log_path: logPath, lines, byte_offset: ++emittedByteOffset, emitted_at_unix_ms: 1
});
function publish(instance: string, logPath: string, lines: string[]) {
  const retained = retainedLogs.get(instance);
  check(retained?.source_path === logPath, "Published output must target the current retained file");
  const combined = [...retained.lines, ...lines];
  // Retain the complete fixed transcript so the workbench enforces its DOM cap.
  retainedLogs.set(instance, { ...retained, lines: combined, total_lines: combined.length });
  bridge.emit(event(instance, logPath, lines));
}

async function run() {
  await Promise.all([import("../../src/i18n-messages"), import("../../src/i18n-messages-zh-cn")]);
  let replayedMounts = 0;
  let renders = 0;
  let maxTailLines = 0;
  let readbacks = 0;
  const cycles = 12;
  for (let cycle = 0; cycle < cycles; cycle += 1) {
    const root = createRoot(fixture);
    const before = bridge.entries.length;
    const instance = `instance-${cycle}`;
    const path = `fixture/run-${cycle}.log`;
    bridge.hold = true;
    try {
      await render(root, properties(instance, path));
      renders += 1;
      check(bridge.entries.length - before === 4, "StrictMode must replay both native listener setups exactly once");
      check(bridge.active.size === 0, "Deferred registration unexpectedly became active");
      await act(async () => { bridge.resolvePending(); });
      check(bridge.active.size === 2, "StrictMode must leave exactly one log and one recovery listener");
      check(bridge.entries[before].released, "StrictMode's retired registration was not released");
      replayedMounts += 1;
      bridge.hold = false;
      await act(async () => { publish(instance, path, ["current output"]); });
      await settleUntil(() => consoleText() === "current output", "Native event did not refresh the retained file");
      check(consoleText() === "current output", "Native event did not reach the real DOM");

      const retired = bridge.entries.slice();
      const replacementPath = `fixture/replacement-${cycle}.log`;
      await render(root, properties(instance, replacementPath));
      renders += 1;
      check(bridge.active.size === 2, "Same-root path change leaked a listener");
      await act(async () => {
        for (const registration of retired) if (registration.name === "runtime-log-stream") registration.deliver(event(instance, replacementPath, ["retired event"]));
        bridge.emit(event(instance, path, ["wrong path"]));
        publish(instance, replacementPath, ["replacement output"]);
      });
      await settleUntil(() => consoleText() === "replacement output", "Replacement file was not read");
      check(consoleText() === "replacement output", "Path change retained a stale event or missed the new stream");

      const replacementInstance = `replacement-${cycle}`;
      const replacement = properties(replacementInstance, replacementPath);
      replacement.onRetryReads = () => {
        readbacks++;
        replacement.runtime = { ...replacement.runtime!, log_tail: {
          source_path: replacementPath, lines: ["authoritative file output after service reset"], total_lines: 1,
          truncated: false, read_error: null
        } };
        retainedLogs.set(replacementInstance, replacement.runtime.log_tail);
        root.render(<StrictMode><I18nProvider><RuntimeSurfaceWorkbench {...replacement} /></I18nProvider></StrictMode>);
      };
      await render(root, replacement);
      renders += 1;
      check(bridge.active.size === 2, "Same-root instance change leaked a listener");
      await act(async () => {
        bridge.emit(event(instance, replacementPath, ["wrong instance"]));
        publish(replacementInstance, replacementPath, ["new instance"]);
      });
      await settleUntil(() => consoleText() === "new instance", "New instance file was not read");
      check(consoleText() === "new instance", "Instance change mixed the retired instance's output");

      for (let batch = 0; batch < 10; batch += 1) {
        await act(async () => {
          publish(replacementInstance, replacementPath,
            Array.from({ length: 100 }, (_, index) => `line ${batch * 100 + index}`));
        });
        await settleUntil(() => consoleText().endsWith(`line ${batch * 100 + 99}`), "Live file read did not include the latest batch");
        const lines = consoleText().split("\n");
        maxTailLines = Math.max(maxTailLines, lines.length);
        check(lines.length <= 400, "Real DOM exceeded the 400-line console budget");
      }
      check(consoleText().split("\n")[0] === "line 600", "Bounded DOM tail lost its expected starting line");
      check(consoleText().endsWith("line 999"), "Bounded DOM tail lost its newest line");
      await act(async () => { bridge.reset(); });
      await settleUntil(() => consoleText() === "authoritative file output after service reset", "Service reset did not finish its file readback");
      check(readbacks === cycle + 1, "A service generation reset must invoke exactly one authoritative readback");
      check(consoleText() === "authoritative file output after service reset", "Reset did not replace the stale live tail with the file readback");
    } finally {
      await act(async () => { root.unmount(); });
      await act(async () => { bridge.resolvePending(); });
    }
    check(bridge.active.size === 0, "Unmount retained a native listener");
    check(fixture.childNodes.length === 0, "Unmount retained DOM nodes");
    await act(async () => {
      for (const registration of bridge.entries) registration.deliver(event(instance, path, ["late unmounted event"]));
    });
    check(readbacks === cycle + 1, "A late recovery event invoked readback after unmount");
    check(fixture.childNodes.length === 0, "A late event remounted the retired workbench");
  }

  // Resolve the real effect's registration only after React has unmounted it.
  const lateRoot = createRoot(fixture);
  bridge.hold = true;
  await render(lateRoot, properties("late", "fixture/late.log"));
  await act(async () => { lateRoot.unmount(); });
  await act(async () => { bridge.resolvePending(); });
  check(bridge.active.size === 0, "Late native registration survived React unmount");
  check(bridge.entries.length === bridge.released, "Not every native registration was released exactly once");
  check(errors.length === 0, `Browser/React errors: ${errors.join("; ")}`);
  return {
    status: "passed", react_version: React.version, cycles, renders,
    strict_mode_replays: replayedMounts, listeners_created: bridge.entries.length,
    listeners_released: bridge.released, listeners_remaining: bridge.active.size,
    dom_nodes_after_unmount: fixture.childNodes.length, max_tail_lines: maxTailLines,
    readbacks,
    browser_errors: errors,
    checks: ["strict_mode_replay", "same_root_path_change", "same_root_instance_change", "late_registration", "late_events", "bounded_dom_tail", "unmount_baseline", "service_generation_readback"]
  };
}

const nonce = new URL(location.href).searchParams.get("nonce");
run().then(
  (report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }),
  (error: unknown) => fetch(`/__reliability_result/${nonce}`, {
    method: "POST", body: JSON.stringify({ status: "failed", error: error instanceof Error ? error.stack : String(error), browser_errors: errors })
  })
);
