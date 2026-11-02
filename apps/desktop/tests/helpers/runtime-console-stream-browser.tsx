import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, StrictMode, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC } from "@tauri-apps/api/mocks";
import { I18nProvider } from "../../src/i18n";
import { bootstrapApp, readInstanceDetails, readInstanceRuntime, readModuleDetails } from "../../src/api";
import { invokeMock } from "../../src/api-mock";
import { RuntimeSurfaceWorkbench } from "../../src/views/servers/RuntimeSurfaceWorkbench";
import { beginRuntimeConsoleStartupCapture } from "../../src/runtime-console-startup-history";
import { runtimeConsoleHistory } from "../../src/runtime-console-history";
import { bridge } from "./runtime-browser-events";
import type { RuntimeLogStreamEvent } from "../../src/runtime-log-stream";
import type { LogTailSnapshot, RuntimeConsoleSourceSnapshot } from "../../src/types";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const errors: string[] = [];
addEventListener("error", event => errors.push(event.message));
addEventListener("unhandledrejection", event => errors.push(String(event.reason)));
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
fixture.classList.add("server-detail-scroll--runtime");
const root = createRoot(fixture);
let scenarios = 0;
const terminal = () => fixture.querySelector<HTMLElement>("pre.server-runtime-console")!;
const text = () => terminal()?.textContent ?? "";
const bottom = () => terminal().scrollHeight - terminal().clientHeight - terminal().scrollTop;
const frame = () => new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(description);
}
async function settleUntil(predicate: () => boolean, description: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    check(performance.now() < deadline, description);
    await act(async () => { await frame(); });
  }
}
const source = (streamId: string, start: number, revision: number, lines: string[], partial: string[] = [], process = "main"): RuntimeConsoleSourceSnapshot => ({
  log_path: `fixture/${process}.log`, process_key: process, display_name: process, run_id: 1,
  stream_id: streamId, started_at_unix_ms: start, revision, lines, partial_lines: partial,
  total_lines: lines.length, truncated: false
});
const consoleDocument = (sources: RuntimeConsoleSourceSnapshot[]): LogTailSnapshot => ({
  source_path: sources[0]?.log_path ?? null, lines: sources[0]?.lines ?? [], console_sources: sources,
  total_lines: sources.reduce((sum, item) => sum + item.total_lines, 0), truncated: false, read_error: null
});

async function run() {
  const bootstrap = await bootstrapApp({ includeSystemSnapshot: false });
  const original = bootstrap.state.instances[0];
  check(original, "Missing fixture instance");
  const details = { ...await readInstanceDetails(original.id), summary: { ...original, status: "Running" as const, active_process_count: 1 }, active_run: null };
  const moduleDetails = await readModuleDetails(original.module_id);
  const runtime = { ...await readInstanceRuntime(original.id), recent_runs: [],
    log_tail: { source_path: "fixture/main.log", lines: [], total_lines: 0, read_error: null, truncated: false } };
  const initialLines = Array.from({ length: 300 }, (_, index) => `INITIAL_${String(index).padStart(3, "0")}`);
  let retained = consoleDocument([source("main-1", 100, 1, initialLines)]);
  let reads = 0, holdRead = false;
  const logReadArguments: { runId: unknown; source: unknown }[] = [];
  let releaseRead: (() => void) | null = null;
  Object.assign(globalThis, { isTauri: true });
  mockIPC(async (command, args) => {
    if (command === "read_instance_log_document_from_storage") {
      reads++;
      logReadArguments.push({ runId: args.runId, source: args.source });
      const result = retained;
      if (holdRead) await new Promise<void>(resolve => { releaseRead = resolve; });
      return result;
    }
    return invokeMock(command, args as Record<string, unknown>);
  });
  const props: ComponentProps<typeof RuntimeSurfaceWorkbench> = { details, moduleDetails, runtime, runtimeWindows: null,
    startupPending: false, launchHostSurface: "managed_terminal", onRetryReads: () => {} };
  let visible = true;
  const render = () => root.render(<StrictMode><I18nProvider>{visible
    ? <RuntimeSurfaceWorkbench key={props.details.summary.id} {...props} /> : <div>Other workspace</div>}</I18nProvider></StrictMode>);
  await act(async () => { await prepareBrowserLocaleCatalogs(); render(); });
  await settleUntil(() => text().includes("INITIAL_299"), "Initial authoritative console snapshot was not loaded");
  check(terminal().scrollHeight > terminal().clientHeight && bottom() <= 1, "Initial overflowing console must follow its tail");
  const emit = (revision: number, lines: string[], partial: string[] = [], extra: Partial<RuntimeLogStreamEvent> = {}) => {
    const event: RuntimeLogStreamEvent = { instance_id: original.id, log_path: "fixture/main.log", process_key: "main", run_id: 1,
      lines, byte_offset: revision, emitted_at_unix_ms: 100 + revision,
      console: { stream_id: "main-1", started_at_unix_ms: 100, revision, previous_revision: revision - 1,
        reset: revision === 1, partial_lines: partial }, ...extra };
    bridge.emit(event);
  };
  const beforeNormalReads = reads;
  await act(async () => { emit(2, [], ["DOWNLOADING_1%"]); await frame(); });
  check(text().includes("DOWNLOADING_1%"), "An unterminated partial line waited for a newline");
  await act(async () => { emit(3, [], ["DOWNLOADING_2%"]); await frame(); });
  check(!text().includes("DOWNLOADING_1%") && text().includes("DOWNLOADING_2%"), "Partial replacement appended obsolete progress text");
  scenarios++;
  await act(async () => {
    emit(4, ["DOWNLOADING_2%", "NATIVE_REPEAT", "NATIVE_REPEAT"]);
    emit(4, ["DOWNLOADING_2%", "NATIVE_REPEAT", "NATIVE_REPEAT"]);
    await frame();
  });
  check(text().split("DOWNLOADING_2%").length === 2 && text().split("NATIVE_REPEAT").length === 3,
    "Committing the partial or replaying a revision duplicated text, or native repeated rows were removed");
  scenarios++;
  let burstUpdates = 0;
  const observer = new MutationObserver(() => { burstUpdates++; });
  observer.observe(terminal(), { childList: true, characterData: true, subtree: true });
  await act(async () => {
    for (let revision = 5; revision <= 104; revision++) emit(revision, [`BURST_${revision}`]);
    check(!text().includes("BURST_104"), "A stream burst committed DOM updates before the scheduled frame");
    await frame();
  });
  await Promise.resolve(); observer.disconnect();
  check(burstUpdates === 1, `A single-frame burst made ${burstUpdates} document updates`);
  check(text().includes("BURST_104") && bottom() <= 1, "Burst output was dropped or stopped following the tail");
  check(reads === beforeNormalReads, "Normal versioned console events reread the whole file");
  const normalReadbacks = reads - beforeNormalReads;
  scenarios++;
  await act(async () => {
    terminal().scrollTop = 120; terminal().dispatchEvent(new Event("scroll", { bubbles: true }));
  });
  const readingPosition = terminal().scrollTop;
  await act(async () => { emit(105, ["WHILE_READING"]); await frame(); });
  check(terminal().scrollTop === readingPosition && bottom() > 48, "An incremental update moved a user reading older output");
  scenarios++;
  retained = consoleDocument([source("main-1", 100, 107, ["RECOVER_ONE", "RECOVER_REPEAT", "RECOVER_REPEAT"], ["RECOVER_PARTIAL"])]);
  const beforeGap = reads;
  await act(async () => { emit(107, ["RECOVER_REPEAT"], ["RECOVER_PARTIAL"]); });
  await settleUntil(() => text().includes("RECOVER_PARTIAL"), "A missing predecessor did not recover the authoritative document");
  check(reads > beforeGap && !text().includes("WHILE_READING") && text().split("RECOVER_REPEAT").length === 3,
    "Gap recovery lost native repeats or replayed unpositioned old text");
  await act(async () => { emit(108, ["RECOVER_PARTIAL"]); await frame(); });
  check(text().split("RECOVER_PARTIAL").length === 2, "Post-recovery partial completion duplicated text");
  scenarios++;
  // A health poll starts an intentionally late read at revision 107. Revision
  // 109 must stay visible when that older request finishes.
  holdRead = true;
  props.runtime = { ...runtime, log_tail: { ...runtime.log_tail, lines: ["OLD_HEALTH_POLL"] } };
  await act(async () => { render(); });
  await settleUntil(() => releaseRead !== null, "Late-read fixture did not reach its IPC boundary");
  holdRead = false;
  await act(async () => {
    emit(109, ["NEWER_THAN_READ"]);
    releaseRead?.();
    await frame();
  });
  check(text().includes("NEWER_THAN_READ") && !text().includes("OLD_HEALTH_POLL"), "A late snapshot replaced newer console output");
  scenarios++;
  retained = consoleDocument([source("main-2", 200, 1, ["ROTATED_SOURCE"])]);
  await act(async () => { emit(1, ["ROTATED_SOURCE"], [], { console: { stream_id: "main-2", started_at_unix_ms: 200,
    revision: 1, previous_revision: 0, reset: true, partial_lines: [] } }); await frame(); });
  await act(async () => { emit(110, ["LATE_OLD_GENERATION"]); await frame(); });
  check(text().includes("ROTATED_SOURCE") && !text().includes("NEWER_THAN_READ") && !text().includes("LATE_OLD_GENERATION"),
    "Rotation kept the old generation or accepted its delayed delivery");
  scenarios++;
  retained = consoleDocument([source("main-3", 300, 1, ["SERVICE_RECOVERED"])]);
  await act(async () => { bridge.reset(); });
  await settleUntil(() => text().includes("SERVICE_RECOVERED"), "Service event reset did not read the current source generation");
  check(!text().includes("ROTATED_SOURCE"), "Service reset retained stale source output");
  scenarios++;
  // Queue a document then change the selected path before rAF. The callback
  // belonging to the former scope cannot replace the new source.
  retained = consoleDocument([source("other-1", 400, 1, ["OTHER_SCOPE"], [], "other")]);
  await act(async () => {
    emit(2, ["QUEUED_OLD_SCOPE"], [], { console: { stream_id: "main-3", started_at_unix_ms: 300,
      revision: 2, previous_revision: 1, reset: false, partial_lines: [] } });
    props.runtime = { ...runtime, log_tail: { ...runtime.log_tail, source_path: "fixture/other.log" } };
    render();
  });
  await settleUntil(() => text().includes("OTHER_SCOPE"), "Changing the selected source did not read its document");
  await act(async () => { await frame(); });
  check(!text().includes("QUEUED_OLD_SCOPE"), "A cancelled frame published into the replacement source");
  scenarios++;
  const nativeReadStart = logReadArguments.length;
  props.moduleDetails = await readModuleDetails("arksurvivalascended");
  props.details = { ...details, summary: { ...details.summary, module_id: "arksurvivalascended" }, active_run: {
    run_id: 7, log_path: "fixture/ark-console.log", processes: [{ run_id: 7, process_key: "theisland",
      display_name: "The Island", pid: 7, status: "Running", is_primary: true, log_path: "fixture/ark-console.log" }]
  } };
  retained = { source_path: "fixture/native.log", snapshot_revision: 1, lines: ["NATIVE_HEAD", "NATIVE_PART"] };
  props.runtime = { ...runtime, log_tail: { ...runtime.log_tail, source_path: "fixture/native.log" } };
  await act(async () => { render(); });
  await settleUntil(() => text().includes("NATIVE_PART"), "Native document baseline was not loaded");
  check(terminal().dataset.logSource === "game" && terminal().dataset.processKey === "theisland"
    && logReadArguments.slice(nativeReadStart).every(args => args.runId === 7 && args.source === "game"),
    "Native patch acceptance must exercise the actual default ARK game source and its selected map read route");
  const nativeReads = reads;
  const nativePatch: RuntimeLogStreamEvent = { instance_id: original.id, log_path: "fixture/native.log", process_key: "theisland", run_id: 7,
    lines: [], byte_offset: 2, emitted_at_unix_ms: Date.now(), document_patch: { previous_revision: 1, revision: 2,
      start: 1, delete_count: 1, lines: ["NATIVE_PART_DONE", "NATIVE_REPEAT", "NATIVE_REPEAT"], total_lines: 4, truncated: false, read_error: null } };
  await act(async () => { bridge.emit(nativePatch); bridge.emit(nativePatch); await frame(); });
  check(text().includes("NATIVE_PART_DONE") && text().split("NATIVE_REPEAT").length === 3 && reads === nativeReads,
    "Native patches failed to replace the partial row, deduplicate a replay or avoid per-event reads");
  scenarios++;
  retained = { source_path: "fixture/native.log", snapshot_revision: 4,
    lines: ["NATIVE_HEAD", "NATIVE_PART_DONE", "NATIVE_REPEAT", "NATIVE_REPEAT", "NATIVE_MISSING", "NATIVE_GAP"] };
  await act(async () => { bridge.emit({ ...nativePatch, document_patch: { ...nativePatch.document_patch!, previous_revision: 3,
    revision: 4, start: 5, delete_count: 0, lines: ["NATIVE_GAP"], total_lines: 6 } }); });
  await settleUntil(() => text().includes("NATIVE_GAP"), "Native document revision gap did not recover");
  check(text().includes("NATIVE_MISSING") && text().split("NATIVE_REPEAT").length === 3,
    "Native snapshot recovery dropped a missing row or altered true repeated lines");
  scenarios++;
  const startupAt = Date.now();
  props.details = details;
  props.moduleDetails = moduleDetails;
  props.startupPending = true;
  props.startupBoundary = { instanceId: original.id, startedAtUnixMs: startupAt, previousLogPaths: new Set(["fixture/other.log"]) };
  retained = consoleDocument([{ ...source("startup-main", startupAt + 1, 1, ["STARTUP_MAIN"], ["MAIN_PENDING"]),
    log_path: "fixture/startup-main.log" },
    source("startup-caves", startupAt + 2, 1, ["STARTUP_CAVES"], ["CAVES_PENDING"], "caves")]);
  visible = false;
  await act(async () => { render(); });
  const capture = beginRuntimeConsoleStartupCapture(original.module_id, props.startupBoundary);
  await capture.ready;
  await act(async () => { await frame(); });
  visible = true;
  await act(async () => { render(); });
  await settleUntil(() => text().includes("[caves] CAVES_PENDING"), "A late-mounted startup lost its recovered shard partials");
  check(text().includes("[main] STARTUP_MAIN") && text().includes("[main] MAIN_PENDING"), "Startup source recovery did not include both streams");
  await act(async () => {
    bridge.emit({ instance_id: original.id, log_path: "fixture/startup-main.log", process_key: "main", run_id: 1,
      lines: ["MAIN_PENDING"], byte_offset: 2, emitted_at_unix_ms: startupAt + 3,
      console: { stream_id: "startup-main", started_at_unix_ms: startupAt + 1, revision: 2, previous_revision: 1, reset: false, partial_lines: [] } });
    await frame();
  });
  check(text().split("MAIN_PENDING").length === 2 && text().includes("CAVES_PENDING"),
    "The operation capture and visible listener duplicated a committed partial or lost another shard");
  scenarios++;
  // Registration changes the run identity while the managed source stays the
  // same. A short health tail must not flash over the retained full window.
  const handoffLines = Array.from({ length: 400 }, (_, index) => `HANDOFF_${index}`);
  const handoffSource = { ...source("handoff", startupAt + 4, 1, handoffLines), log_path: "fixture/handoff.log" };
  retained = consoleDocument([handoffSource]);
  props.runtime = { ...runtime, log_tail: retained };
  await act(async () => { render(); });
  await settleUntil(() => text().includes("HANDOFF_399"), "Startup handoff fixture did not load");
  holdRead = true;
  releaseRead = null;
  props.startupPending = false;
  props.runtime = { ...runtime, log_tail: consoleDocument([{ ...handoffSource, lines: handoffLines.slice(-32), truncated: true }]) };
  const transitionTexts: string[] = [];
  const handoffObserver = new MutationObserver(() => transitionTexts.push(text()));
  handoffObserver.observe(terminal(), { childList: true, characterData: true, subtree: true });
  await act(async () => { render(); });
  await settleUntil(() => releaseRead !== null, "Handoff document read did not reach IPC");
  check(text() === handoffLines.join("\n"), "Startup completion shrank or retained shard formatting in the console window");
  holdRead = false;
  await act(async () => { releaseRead?.(); await frame(); });
  releaseRead = null;
  holdRead = true;
  props.details = { ...details, active_run: { run_id: 195, log_path: handoffSource.log_path, processes: [{
    run_id: 195, process_key: "main", display_name: "Server", pid: 195, status: "running", is_primary: true,
    log_path: handoffSource.log_path
  }] } };
  await act(async () => { render(); });
  await settleUntil(() => releaseRead !== null, "Registered-run read did not reach IPC");
  check(text() === handoffLines.join("\n"), "Run registration discarded the source's retained window");
  holdRead = false;
  await act(async () => { releaseRead?.(); await frame(); });
  handoffObserver.disconnect();
  check(transitionTexts.every(value => value === handoffLines.join("\n")), "A handoff frame temporarily collapsed the log window");
  scenarios++;
  // A reload can receive the 32-row health snapshot before the 400-row file
  // document at the very same revision; the latter must hydrate the window.
  visible = false;
  await act(async () => { render(); });
  runtimeConsoleHistory.clearInstance(original.id);
  holdRead = true;
  releaseRead = null;
  visible = true;
  await act(async () => { render(); });
  await settleUntil(() => releaseRead !== null, "Reload hydration read did not reach IPC");
  check(text() === handoffLines.slice(-32).join("\n"), "Reload fixture must begin with only the health snapshot");
  holdRead = false;
  await act(async () => { releaseRead?.(); await frame(); });
  await settleUntil(() => text() === handoffLines.join("\n"), "A remount stayed at the short health-tail length");
  scenarios++;
  const finalTranscript = text();
  capture.dispose();
  await act(async () => { root.unmount(); await frame(); });
  check(bridge.active.size === 0, "Console teardown leaked event listeners");
  // The runner takes its screenshot after receiving this report. Preserve the
  // actual observed transcript in a read-only instance of the real component,
  // after proving the live view released every subscription and pending frame.
  const previewRoot = createRoot(fixture);
  await act(async () => { previewRoot.render(<StrictMode><I18nProvider>
    <RuntimeSurfaceWorkbench {...props} startupPending={false} archive={{ runs: [], log: {
      relative_path: "fixture/startup-main.log", text: finalTranscript, truncated: false, issues: []
    } }} />
  </I18nProvider></StrictMode>); });
  check(text() === finalTranscript && bridge.active.size === 0, "The screenshot preview changed the observed transcript or opened a live subscription");
  check(errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  return { status: "passed", scenarios, reads, burst_document_updates: burstUpdates,
    normal_event_readbacks: normalReadbacks, browser_errors: errors };
}

let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error(`Fixture stalled after ${scenarios} scenarios`)), 25000);
})]).finally(() => clearTimeout(watchdog))
  .catch(error => ({ status: "failed", scenarios, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .then(report => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
