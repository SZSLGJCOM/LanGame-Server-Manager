import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, StrictMode, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC } from "@tauri-apps/api/mocks";
import { I18nProvider } from "../../src/i18n";
import { bootstrapApp, readInstanceDetails, readInstanceRuntime, readModuleDetails } from "../../src/api";
import { invokeMock } from "../../src/api-mock";
import { RuntimeSurfaceWorkbench } from "../../src/views/servers/RuntimeSurfaceWorkbench";
import { createRuntimeActionStore } from "../../src/runtime-action-store";
import { runtimeConsoleHistory } from "../../src/runtime-console-history";
import { bridge } from "./runtime-browser-events";
import type { RuntimeLogStreamEvent } from "../../src/runtime-log-stream";
import type { LogTailSnapshot, RuntimeConsoleSourceSnapshot, RuntimeStartupSession } from "../../src/types";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const errors: string[] = [];
const copied: string[] = [];
Object.defineProperty(navigator, "clipboard", { configurable: true, value: {
  writeText: async (value: string) => { copied.push(value); }
} });
addEventListener("error", event => errors.push(event.message));
addEventListener("unhandledrejection", event => errors.push(String(event.reason)));
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
fixture.classList.add("server-detail-scroll--runtime");
let root = createRoot(fixture), scenarios = 0;
const terminal = () => fixture.querySelector<HTMLElement>("pre.server-runtime-console")!;
const region = (name: string) => fixture.querySelector<HTMLElement>(`[data-log-region='${name}']`);
const text = () => terminal()?.textContent ?? "";
const frame = () => new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
function check(condition: unknown, message: string): asserts condition { if (!condition) throw new Error(message); }
async function settleUntil(predicate: () => boolean, message: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) { check(performance.now() < deadline, message); await act(async () => { await frame(); }); }
}
const source = (revision: number, lines: string[], partial_lines: string[] = []): RuntimeConsoleSourceSnapshot => ({
  log_path: "fixture/owned-startup.log", process_key: "startup", display_name: "Startup", run_id: null,
  stream_id: "startup-a", started_at_unix_ms: 101, revision, lines, partial_lines, total_lines: lines.length, truncated: false
});
const session = (lines: string[], extra: Partial<RuntimeStartupSession> = {}): RuntimeStartupSession => ({
  attempt_id: "initial", status: "starting", started_at_unix_ms: 100, finished_at_unix_ms: null,
  source_path: "fixture/owned-startup.log", run_ids: [], error: null, reader_error: null,
  snapshot: { lines, total_lines: lines.length, source_path: "fixture/owned-startup.log" },
  console_source: source(1, lines, ["VALIDATING_10%"]), before_cursor: null, retained_start_cursor: "0", line_generation: "stream:startup-a", ...extra
});

async function run() {
  const bootstrap = await bootstrapApp({ includeSystemSnapshot: false });
  const original = bootstrap.state.instances[0];
  check(original, "Missing fixture instance");
  const details = { ...await readInstanceDetails(original.id), summary: { ...original, status: "Stopped" as const, active_process_count: 0 }, active_run: null };
  const moduleDetails = await readModuleDetails(original.module_id);
  const runtime = { ...await readInstanceRuntime(original.id), recent_runs: [], log_tail: { source_path: null, lines: [] } as LogTailSnapshot };
  const initialLines = [...Array.from({ length: 298 }, (_, index) => `PRECHECK_${String(index).padStart(3, "0")}`), "REPEATED_ROW", "REPEATED_ROW"];
  let receipt = session(initialLines), held = true, reads = 0, failRead = false, holdPage = false;
  let rejectSupersededNative = false, supersededNativeReads = 0;
  let loseDiagnosticAck = false;
  let holdDiagnosticWrite = false;
  const diagnosticReleases: (() => void)[] = [];
  const diagnosticWriteAttempts: string[] = [];
  let nextHeldFailure: string | null = null;
  const heldFailures: (() => void)[] = [];
  let game: LogTailSnapshot = { lines: [] };
  const releases: (() => void)[] = [], pageReleases: (() => void)[] = [], diagnosticIds = new Set<string>();
  let olderLines: string[] = [];
  Object.assign(globalThis, { isTauri: true });
  mockIPC(async (command, args) => {
    if (command === "read_instance_log_document_from_storage") {
      reads++;
      if (rejectSupersededNative && args.runId === 18 && args.source === "game") {
        supersededNativeReads++;
        throw new Error("This map has a newer run; select its retained console output for the older run");
      }
      if (nextHeldFailure) {
        const message = nextHeldFailure; nextHeldFailure = null;
        await new Promise<void>(resolve => heldFailures.push(resolve));
        throw new Error(message);
      }
      if (failRead) { failRead = false; throw new Error("UI_READ_FAILURE"); }
      const current = receipt;
      if (held) await new Promise<void>(resolve => releases.push(resolve));
      if (args.startupBeforeCursor) {
        check(args.startupAttemptId === current.attempt_id && args.startupBeforeCursor === "4000", "Pagination lost its owned cursor");
        if (holdPage) await new Promise<void>(resolve => pageReleases.push(resolve));
        return { ...game, startup_session: { ...current, snapshot: { lines: olderLines.slice(0, 400), total_lines: 400 },
          console_source: null, before_cursor: null } };
      }
      return { ...game, startup_session: current };
    }
    if (command === "record_startup_session_diagnostic") {
      check(args.attemptId === receipt.attempt_id, "Diagnostic crossed startup attempts");
      diagnosticWriteAttempts.push(String(args.diagnosticId));
      if (!diagnosticIds.has(String(args.diagnosticId))) {
        diagnosticIds.add(String(args.diagnosticId));
        const lines = [...receipt.snapshot.lines, `[120] [console-read] ${args.diagnosticId} ${args.message}`];
        receipt = { ...receipt, snapshot: { ...receipt.snapshot, lines, total_lines: lines.length },
          console_source: receipt.console_source ? { ...receipt.console_source,
            revision: Math.max(3, receipt.console_source.revision + 1), lines, total_lines: lines.length,
            partial_lines: receipt.status === "starting" && receipt.console_source.stream_id === "startup-a" ? ["VALIDATING_90%"] : [] } : null };
      }
      if (holdDiagnosticWrite) await new Promise<void>(resolve => diagnosticReleases.push(resolve));
      if (loseDiagnosticAck) { loseDiagnosticAck = false; throw new Error("Diagnostic was stored but its acknowledgement was lost"); }
      return;
    }
    return invokeMock(command, args as Record<string, unknown>);
  });
  const actions = createRuntimeActionStore();
  const operation = actions.begin(original.id, "start", { startup: { moduleId: original.module_id,
    boundary: { instanceId: original.id, startedAtUnixMs: 100, previousLogPaths: new Set() } } });
  check(operation, "Missing startup operation");
  receipt = { ...receipt, attempt_id: operation.startupAttemptId! };
  await operation.ready;
  const props: ComponentProps<typeof RuntimeSurfaceWorkbench> = { details, moduleDetails, runtime, runtimeWindows: null,
    startupPending: true, startupBoundary: operation.boundary, launchHostSurface: "managed_terminal" };
  const render = () => root.render(<StrictMode><I18nProvider><RuntimeSurfaceWorkbench {...props} /></I18nProvider></StrictMode>);
  await act(async () => { await prepareBrowserLocaleCatalogs(); render(); await frame(); });
  const emit = (revision: number, lines: string[], partial_lines: string[] = []) => bridge.emit({ instance_id: original.id,
    process_key: "startup", log_path: receipt.source_path, run_id: null, lines, byte_offset: revision * 100,
    emitted_at_unix_ms: 101 + revision, console: { stream_id: "startup-a", started_at_unix_ms: 101, revision,
      previous_revision: revision - 1, reset: revision === 1, partial_lines } } satisfies RuntimeLogStreamEvent);
  await act(async () => { emit(1, initialLines, ["VALIDATING_10%"]); emit(2, [], ["VALIDATING_90%"]); await frame(); });
  check(text().includes("VALIDATING_90%") && !text().includes("VALIDATING_10%"), "Active progress was appended instead of replaced");
  const beforeRead = { attempt: region("startup")?.dataset.startupAttemptId, revision: region("startup")?.dataset.streamRevision };
  held = false;
  await act(async () => { releases.splice(0).forEach(resolve => resolve()); await frame(); });
  check(region("startup")?.dataset.startupAttemptId === operation.startupAttemptId, "Startup source has no stable attempt identity");
  check(text().includes("VALIDATING_90%") && !text().includes("VALIDATING_10%"), `A delayed initial receipt rolled progress back: ${JSON.stringify({
    progress: fixture.querySelector("[data-startup-progress]")?.textContent, runtime: region("runtime")?.textContent?.slice(-100),
    startup: region("startup")?.textContent?.slice(-100), revision: region("startup")?.dataset.streamRevision, beforeRead, expectedAttempt: operation.startupAttemptId })}`);
  check(text().split("REPEATED_ROW").length - 1 === 2, "Equal complete rows were duplicated or removed");
  scenarios++;

  await act(async () => { failRead = true; props.runtime = { ...runtime, log_tail: { ...runtime.log_tail } }; render(); });
  await settleUntil(() => diagnosticIds.size === 1 && region("startup")?.textContent?.includes("UI_READ_FAILURE") === true,
    "A transient UI failure was not retained in the owned startup history");
  // Reading the receipt preserves the failure; only a successful reread of the
  // original source proves that the active read error has recovered.
  await act(async () => { props.runtime = { ...runtime, log_tail: { ...runtime.log_tail } }; render(); await frame(); });
  await settleUntil(() => !fixture.querySelector("pre > .server-runtime-console-errors"), "Recovered source read error stayed active");
  await act(async () => { failRead = true; props.runtime = { ...runtime, log_tail: { ...runtime.log_tail } }; render(); });
  await settleUntil(() => diagnosticIds.size === 2 && (region("startup")?.textContent?.split("UI_READ_FAILURE").length ?? 0) === 3,
    "A separate recurrence of the same error was mistaken for an IPC replay");
  scenarios++;

  await act(async () => { terminal().scrollTop = 100; terminal().dispatchEvent(new Event("scroll", { bubbles: true })); });
  const topBefore = terminal().scrollTop;
  receipt = { ...receipt, status: "succeeded", finished_at_unix_ms: 200, run_ids: [7], console_source: null,
    snapshot: { lines: [...receipt.snapshot.lines, "[200] [startup] SUCCEEDED"], total_lines: receipt.snapshot.lines.length + 1 } };
  game = { source_path: "fixture/game.log", lines: ["GAME_READY", "GAME_REPEAT", "GAME_REPEAT"], total_lines: 3 };
  await act(async () => { operation.finish(); props.startupPending = false; props.startupBoundary = undefined;
    props.runtime = { ...runtime, recent_runs: [{ run_id: 7, status: "Running", log_path: game.source_path }], log_tail: game }; render(); await frame(); });
  await settleUntil(() => text().includes("SUCCEEDED") && text().includes("GAME_READY"), "Completion lost final startup output or game output");
  check(text().includes("PRECHECK_000") && text().includes("UI_READ_FAILURE"), "Handoff discarded the startup transcript");
  check(!text().includes("VALIDATING_90%"), "Completed startup retained an active progress preview");
  check(region("runtime")?.textContent === game.lines.join("\n"), "Startup source was duplicated into runtime output");
  check(Math.abs(terminal().scrollTop - topBefore) <= 1, "Handoff stole the user's history scroll position");
  scenarios++;

  olderLines = [...receipt.snapshot.lines, ...Array.from({ length: 800 - receipt.snapshot.lines.length }, (_, index) =>
    `HISTORY_${String(index + receipt.snapshot.lines.length).padStart(3, "0")}`)];
  receipt = { ...receipt, snapshot: { lines: olderLines.slice(400), total_lines: 800 }, console_source: null, before_cursor: "4000" };
  await act(async () => { props.runtime = { ...props.runtime!, log_tail: { ...game } }; render(); });
  await settleUntil(() => text().includes("HISTORY_799"), "Latest startup tail was not refreshed");
  await act(async () => { terminal().scrollTop = 100; terminal().dispatchEvent(new Event("scroll", { bubbles: true }));
    fixture.querySelector<HTMLButtonElement>("[data-startup-history-more]")!.click(); });
  await settleUntil(() => text().includes("PRECHECK_000"), "Earlier startup page was not displayed");
  check(terminal().scrollTop > 100, "Prepending history failed to preserve the visible content anchor");
  await act(async () => { props.runtime = { ...props.runtime!, log_tail: { ...game } }; render(); await frame(); });
  check(text().includes("PRECHECK_000") && text().includes("HISTORY_799"), "A running refresh collapsed the expanded startup pages");
  scenarios++;

  receipt = { ...receipt, attempt_id: "pagination-attempt", started_at_unix_ms: 250, source_path: "fixture/pagination-startup.log",
    line_generation: "stream:pagination", retained_start_cursor: "0", snapshot: { lines: olderLines.slice(400), total_lines: 800 }, before_cursor: "4000" };
  await act(async () => { props.runtime = { ...runtime, log_tail: { ...game } }; render(); });
  await settleUntil(() => region("startup")?.dataset.startupAttemptId === "pagination-attempt", "The pagination fixture did not select its new attempt");
  holdPage = true;
  await act(async () => { fixture.querySelector<HTMLButtonElement>("[data-startup-history-more]")!.click(); await frame(); });
  check(pageReleases.length === 1, "The earlier page was not in flight");
  receipt = { ...receipt, line_generation: "retained:4000", retained_start_cursor: "4000",
    snapshot: { lines: olderLines.slice(400), total_lines: 400 }, before_cursor: null };
  await act(async () => { props.runtime = { ...runtime, log_tail: { ...game } }; render(); });
  await settleUntil(() => !fixture.querySelector("[data-startup-history-more]"), "The new retained range was not loaded");
  holdPage = false;
  await act(async () => { pageReleases.splice(0).forEach(resolve => resolve()); await frame(); });
  check(!text().includes("PRECHECK_000") && text().includes("HISTORY_799"), "A late page revived expired rows and displaced the current tail");
  scenarios++;

  for (const status of ["failed", "cancelled"] as const) {
    await act(async () => { root.unmount(); await frame(); });
    runtimeConsoleHistory.clearInstance(original.id);
    root = createRoot(fixture);
    receipt = session([`[300] [startup] ${status.toUpperCase()}_NO_RUN`], { attempt_id: `attempt-${status}`, status,
      started_at_unix_ms: 300, finished_at_unix_ms: 400, console_source: null, run_ids: [],
      error: status === "failed" ? "FAILED_NO_RUN" : null, reader_error: "[310] [reader] RETAINED_READER_ERROR" });
    await act(async () => { render(); await frame(); });
    await settleUntil(() => text().includes(`${status.toUpperCase()}_NO_RUN`), "Reload did not restore the latest no-run attempt");
    check(text().includes("RETAINED_READER_ERROR") && !text().includes("PRECHECK_000"), "Reload lost the reader error or revived another attempt");
    check(region("startup")?.dataset.startupStatus === status, "Reload displayed an incorrect final startup state");
    scenarios++;
  }

  held = true;
  await act(async () => { props.startupPending = true; props.startupBoundary = { instanceId: original.id,
    startupAttemptId: "next-attempt", startedAtUnixMs: 500, previousLogPaths: new Set([receipt.source_path]) }; render(); await frame(); });
  check(!text().includes("CANCELLED_NO_RUN"), "A new attempt displayed the previous startup outcome");
  held = false;
  await act(async () => { releases.splice(0).forEach(resolve => resolve()); await frame(); });
  check(!text().includes("CANCELLED_NO_RUN"), "A delayed old receipt entered the new attempt");
  scenarios++;

  for (const moduleId of ["arksurvivalascended", "rust"]) {
    await act(async () => { root.unmount(); await frame(); });
    runtimeConsoleHistory.clearInstance(original.id);
    root = createRoot(fixture);
    const startupPath = `fixture/${moduleId}/logs/managed-console/run-1000-startup.log`;
    const stdoutPath = `fixture/${moduleId}/logs/managed-console/run-1001-main.log`;
    const nativePath = `fixture/${moduleId}/Saved/Logs/server.log`;
    const oldNative: LogTailSnapshot = { source_path: nativePath, snapshot_revision: 1,
      lines: Array.from({ length: 79 }, (_, index) => `OLD_RUN18_${index}`), total_lines: 79 };
    const stdoutLines = ["Preparing...", "Setting breakpad minidump AppID"];
    const stdout: RuntimeConsoleSourceSnapshot = { ...source(1, stdoutLines), log_path: stdoutPath,
      process_key: "main", display_name: "Main", stream_id: "dual-stdout", started_at_unix_ms: 1001 };
    const nativeLines = ["[game:main] Loading world"];
    receipt = session(nativeLines, { attempt_id: `dual-${moduleId}`, source_path: startupPath, started_at_unix_ms: 1000,
      line_generation: "stream:dual-startup", console_source: { ...source(1, nativeLines), log_path: startupPath,
        stream_id: "dual-startup", started_at_unix_ms: 1000 } });
    game = { source_path: stdoutPath, lines: stdoutLines, total_lines: 2, console_sources: [stdout] };
    props.details = { ...details, summary: { ...details.summary, module_id: moduleId } };
    props.runtime = { ...runtime, recent_runs: [{ run_id: 18, status: "Stopped", log_path: nativePath }], log_tail: oldNative };
    props.startupPending = true;
    props.startupBoundary = { instanceId: original.id, startupAttemptId: receipt.attempt_id,
      startedAtUnixMs: 1000, previousLogPaths: new Set([nativePath]) };
    await act(async () => { render(); await frame(); });
    await settleUntil(() => region("runtime")?.textContent?.includes("Setting breakpad") === true, "Independent stdout was not displayed");
    check(!terminal().hasAttribute("data-run-id"), "Pending stdout was mislabeled with the previous run ID");
    const stdoutBefore = region("runtime")!.textContent;
    const nativeAppend = ["[game:main] World partition loaded", "[game:main] World partition loaded"];
    await act(async () => { bridge.emit({ instance_id: original.id, process_key: "startup", log_path: startupPath,
      run_id: null, lines: nativeAppend, byte_offset: 200, emitted_at_unix_ms: 1002,
      console: { stream_id: "dual-startup", started_at_unix_ms: 1000, revision: 2,
        previous_revision: 1, reset: false, partial_lines: [] } } satisfies RuntimeLogStreamEvent); await frame(); });
    check(region("startup")?.dataset.streamRevision === "2", "Native startup update did not reach the DOM");
    check(text().split("World partition loaded").length - 1 === 2, "Native startup lost equal complete rows");
    check(region("runtime")?.textContent === stdoutBefore, "A native update changed silent process stdout");
    check(text().includes("启动过程（自检 / 更新 / 游戏启动）") && text().includes("进程控制台输出"), "The two real sources were not named accurately");
    check(!text().includes("各来源独立更新") && !text().includes("此来源无新内容时保持原样"), "A phase-dependent explanation was inserted into the real transcript");
    await act(async () => { fixture.querySelector<HTMLButtonElement>(".server-runtime-console-copy-button")!.click(); });
    check(copied.at(-1) === text(), "Copied source labels and logs differ from the displayed transcript");
    scenarios++;

    receipt = { ...receipt, snapshot: { lines: [...nativeLines, ...nativeAppend], total_lines: 3 },
      console_source: { ...receipt.console_source!, revision: 2, lines: [...nativeLines, ...nativeAppend], total_lines: 3, partial_lines: [] } };
    // Reload during startup: no local Start action/capture may be required to
    // reject an old recent_run while the new receipt and run details arrive.
    await act(async () => { root.unmount(); await frame(); });
    runtimeConsoleHistory.clearInstance(original.id);
    root = createRoot(fixture);
    props.startupBoundary = undefined;
    props.runtime = { ...props.runtime, health: { status: "starting", summary: "Starting", reason: { code: "startup_pending", params: {} } }, log_tail: game };
    await act(async () => { render(); await frame(); });
    await settleUntil(() => region("startup")?.dataset.streamRevision === "2", "Reload did not recover the active attempt");
    const diagnosticCountBeforeHandoff = diagnosticIds.size;
    nextHeldFailure = "LATE_HANDOFF_READ_FAILURE";
    await act(async () => { props.runtime = { ...props.runtime!, log_tail: { ...game } }; render(); await frame(); });
    check(heldFailures.length === 1, "The startup read was not held before completion");
    const observedOldRows: string[] = [];
    const observer = new MutationObserver(() => { if (text().includes("OLD_RUN18_")) observedOldRows.push(text()); });
    observer.observe(fixture, { subtree: true, characterData: true, childList: true });
    receipt = { ...receipt, status: "succeeded", finished_at_unix_ms: 1100, run_ids: [21] };
    game = oldNative;
    const supersededBeforeHandoff = supersededNativeReads;
    rejectSupersededNative = true;
    await act(async () => { props.startupPending = false; props.runtime = { ...props.runtime!, log_tail: oldNative }; render(); await frame(); });
    check(supersededNativeReads === supersededBeforeHandoff, "Handoff requested a superseded native run before its new run details arrived");
    await settleUntil(() => region("startup")?.dataset.startupStatus === "succeeded", "Completion receipt did not arrive before run details");
    check(observedOldRows.length === 0 && !text().includes("OLD_RUN18_"), "Completion flashed the previous run before new run details arrived");
    check(!terminal().hasAttribute("data-run-id"), "The handoff still claimed the previous run ID");
    observer.disconnect();
    await act(async () => { heldFailures.splice(0).forEach(resolve => resolve()); await frame(); });
    await settleUntil(() => diagnosticIds.size === diagnosticCountBeforeHandoff + 1
      && region("startup")?.textContent?.includes("LATE_HANDOFF_READ_FAILURE") === true,
    "A disposed startup reader lost its late failure after the receipt succeeded");
    check(receipt.status === "succeeded", "A late read failure changed the successful startup outcome");
    game = { source_path: nativePath, snapshot_revision: 2, lines: ["RUN21_SAVE_WORLD"], total_lines: 1 };
    const currentProcess = { run_id: 21, process_key: "main", display_name: "Current map", is_primary: true,
      status: "Running", log_path: nativePath };
    const oldConsole: LogTailSnapshot = { ...oldNative, source_path: `fixture/${moduleId}/logs/managed-console/run-18-main.log` };
    const historicalProcess = { run_id: 18, process_key: "old-map", display_name: "Historical map", is_primary: false,
      status: "Stopped", log_path: oldConsole.source_path };
    props.details = { ...props.details, active_run: { run_id: 21, status: "Running", log_path: nativePath,
      processes: [currentProcess, historicalProcess] } };
    props.runtime = { ...props.runtime!, recent_runs: [props.details.active_run!], log_tail: game };
    await act(async () => { render(); await frame(); });
    await settleUntil(() => text().includes("RUN21_SAVE_WORLD"), "New run output was blocked after the owned run arrived");
    check(text().includes("游戏运行日志") && !text().includes("进程控制台输出"), "A native file was labeled as process stdout");
    check(terminal().dataset.runId === "21", "The current run identity was not restored");
    scenarios++;

    const diagnosticCountBeforeRuntime = diagnosticIds.size;
    await act(async () => { failRead = true; props.runtime = { ...props.runtime!, log_tail: { ...game } }; render(); await frame(); });
    await settleUntil(() => diagnosticIds.size === diagnosticCountBeforeRuntime + 1
      && region("startup")?.textContent?.includes("runtime-read run=21") === true,
    "An error from the current owned run was not retained with its read identity");
    await act(async () => { props.runtime = { ...props.runtime!, log_tail: { ...game } }; render(); await frame(); });
    await settleUntil(() => !fixture.querySelector("pre > .server-runtime-console-errors"), "Recovered current-run error stayed active");
    await act(async () => { root.unmount(); await frame(); });
    runtimeConsoleHistory.clearInstance(original.id);
    root = createRoot(fixture);
    await act(async () => { render(); await frame(); });
    await settleUntil(() => region("startup")?.textContent?.includes("runtime-read run=21") === true,
      "Reload lost the recovered current-run diagnostic");
    check(text().includes("LATE_HANDOFF_READ_FAILURE"), "Reload lost the late handoff diagnostic");
    scenarios++;

    const diagnosticsBeforeHistory = diagnosticIds.size;
    game = oldConsole;
    const picker = fixture.querySelector<HTMLSelectElement>(".server-runtime-console-target-picker select")!;
    await act(async () => { picker.value = "old-map"; picker.dispatchEvent(new Event("change", { bubbles: true })); await frame(); });
    if (moduleId === "arksurvivalascended") {
      await settleUntil(() => text().includes("This map has a newer run"), "Explicit old native selection hid its real backend rejection");
      const sourcePicker = fixture.querySelector<HTMLSelectElement>(".server-runtime-console-source-select")!;
      await act(async () => { sourcePicker.value = "console"; sourcePicker.dispatchEvent(new Event("change", { bubbles: true })); await frame(); });
    }
    await settleUntil(() => text().includes("OLD_RUN18_0"), "The handoff guard blocked an explicit historical map selection");
    check(terminal().dataset.runId === "18", "Historical selection lost its actual run identity");
    check(diagnosticIds.size === diagnosticsBeforeHistory, "A rejected historical read polluted the current startup attempt");
    scenarios++;

    game = { source_path: nativePath, snapshot_revision: 3, lines: ["RUN21_CURRENT_AGAIN"], total_lines: 1 };
    await act(async () => { picker.value = "main"; picker.dispatchEvent(new Event("change", { bubbles: true })); await frame(); });
    await settleUntil(() => text().includes("RUN21_CURRENT_AGAIN"), "Returning from history did not select the current run");
    const writesBeforeLostAck = diagnosticWriteAttempts.length;
    const diagnosticsBeforeLostAck = diagnosticIds.size;
    await act(async () => { loseDiagnosticAck = true; holdDiagnosticWrite = true; failRead = true;
      props.runtime = { ...props.runtime!, log_tail: { ...game } }; render(); await frame(); });
    await settleUntil(() => diagnosticReleases.length === 1, "The diagnostic write was not held");
    await act(async () => { bridge.emit({ instance_id: original.id, process_key: "main", log_path: nativePath,
      run_id: 21, lines: [], byte_offset: 400, emitted_at_unix_ms: 1200,
      snapshot: { ...game, snapshot_revision: 4, read_error: "UI_READ_FAILURE" } } satisfies RuntimeLogStreamEvent); await frame(); });
    check(diagnosticWriteAttempts.length === writesBeforeLostAck + 1, "Concurrent reports started duplicate writes for the same occurrence");
    await act(async () => { holdDiagnosticWrite = false; diagnosticReleases.splice(0).forEach(resolve => resolve()); await frame(); });
    await act(async () => { failRead = true; props.runtime = { ...props.runtime!, log_tail: { ...game } }; render(); await frame(); });
    await settleUntil(() => diagnosticWriteAttempts.length >= writesBeforeLostAck + 2
      && region("startup")?.textContent?.includes("runtime-read run=21") === true
      && !text().includes("acknowledgement was lost"), "Returning to the active run lost diagnostics or did not recover the write acknowledgement");
    const retryIds = diagnosticWriteAttempts.slice(writesBeforeLostAck);
    check(new Set(retryIds).size === 1 && diagnosticIds.size === diagnosticsBeforeLostAck + 1,
      "Retrying a stored diagnostic with a lost acknowledgement created another occurrence");
    game = { ...game, snapshot_revision: 5 };
    await act(async () => { props.runtime = { ...props.runtime!, log_tail: { ...game } }; render(); await frame(); });
    await settleUntil(() => !fixture.querySelector("pre > .server-runtime-console-errors"), "The current source did not recover after its diagnostic retry");
    scenarios++;
  }
  await act(async () => { root.unmount(); await frame(); });
  check(bridge.active.size === 0, "Startup session teardown leaked subscriptions");
  check(errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  return { status: "passed", scenarios, reads, persisted_diagnostics: diagnosticIds.size, browser_errors: errors };
}

let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => { watchdog = setTimeout(() => reject(new Error(`Fixture stalled after ${scenarios} scenarios`)), 30000); })])
  .finally(() => clearTimeout(watchdog)).catch(error => ({ status: "failed", scenarios, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .then(report => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
