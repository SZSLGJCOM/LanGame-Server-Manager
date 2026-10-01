import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, StrictMode, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC } from "@tauri-apps/api/mocks";
import { I18nProvider } from "../../src/i18n";
import { bootstrapApp, readInstanceDetails, readInstanceRuntime, readModuleDetails } from "../../src/api";
import { invokeMock } from "../../src/api-mock";
import { RuntimeSurfaceWorkbench } from "../../src/views/servers/RuntimeSurfaceWorkbench";
import { bridge } from "./runtime-browser-events";
import type { InstanceRuntimeCommandResult, LogTailSnapshot, RuntimeCommandDispatchOptions } from "../../src/types";
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
const consoleNode = () => fixture.querySelector<HTMLElement>("pre.server-runtime-console")!;
const text = () => consoleNode()?.textContent ?? "";
const bottomDistance = () => consoleNode().scrollHeight - consoleNode().clientHeight - consoleNode().scrollTop;
function checkAtBottom(description: string) {
  const terminal = consoleNode();
  check(terminal.clientHeight > 0 && terminal.scrollHeight > terminal.clientHeight,
    "Scroll assertions require real overflowing browser layout");
  check(bottomDistance() <= 1, `${description} (distance ${bottomDistance()})`);
}
async function scrollTo(top: number) {
  await act(async () => {
    consoleNode().scrollTop = top;
    consoleNode().dispatchEvent(new Event("scroll", { bubbles: true }));
  });
}
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
const lines = (prefix: string, count: number) => Array.from({ length: count }, (_, i) => `${prefix}_${String(i).padStart(3, "0")}`);
const snapshot = (path: string, output: string[]): LogTailSnapshot => ({ source_path: path, lines: output,
  total_lines: output.length, truncated: false, read_error: null });

async function run() {
  const bootstrap = await bootstrapApp({ includeSystemSnapshot: false });
  const original = bootstrap.state.instances[0];
  check(original, "Missing fixture instance");
  const instance = { ...original, status: "Running" as const, active_process_count: 1 };
  const details = { ...await readInstanceDetails(instance.id), summary: instance, active_run: null };
  const [moduleDetails, rustModuleDetails] = await Promise.all([
    readModuleDetails(instance.module_id), readModuleDetails("rust")
  ]);
  const runtime = { ...await readInstanceRuntime(instance.id), recent_runs: [],
    log_tail: snapshot("fixture/run-a.log", lines("BOOT", 100)) };
  let retained = runtime.log_tail;
  let readFailure = false;
  let holdRead = false;
  let releaseRead: (() => void) | undefined;
  let reads = 0;
  const requestedRunIds: unknown[] = [];
  let separateNativeSource = false;
  const processDocuments = new Map<number, LogTailSnapshot>();
  let nextResponse: Partial<InstanceRuntimeCommandResult> = {};
  const dispatches: Array<{ instanceId: string; command: string; options?: RuntimeCommandDispatchOptions }> = [];
  Object.assign(globalThis, { isTauri: true });
  mockIPC(async (command, args) => {
    if (command === "read_instance_log_document_from_storage") {
      reads++;
      requestedRunIds.push(args.runId);
      const result = processDocuments.get(Number(args.runId)) ?? (separateNativeSource && args.runId != null
        ? snapshot("fixture/launcher.log", ["MANAGED_ENTRYPOINT_ONLY"]) : retained);
      if (holdRead) await new Promise<void>(resolve => { releaseRead = resolve; });
      if (readFailure) throw new Error("History recovery failed in fixture");
      return result;
    }
    return invokeMock(command, args as Record<string, unknown>);
  });
  const props: ComponentProps<typeof RuntimeSurfaceWorkbench> = { details, runtime, moduleDetails,
    runtimeWindows: null, startupPending: false, launchHostSurface: "managed_terminal",
    onRetryReads: () => {}, onSendRuntimeCommand: async (id, command, _processKey, options) => {
      dispatches.push({ instanceId: id, command, options });
      return { instance_id: id, process_key: "main", display_name: "Fixture", pid: 1, command,
        submitted_at_unix_ms: Date.now(), write_confirmation_pending: false, ...nextResponse };
    } };
  const render = () => root.render(<StrictMode><I18nProvider><RuntimeSurfaceWorkbench {...props} /></I18nProvider></StrictMode>);
  await act(async () => { await prepareBrowserLocaleCatalogs(); render(); });
  await settleUntil(() => text().includes("BOOT_000"), "Initial snapshot is missing");
  checkAtBottom("Opening an instance must reveal its latest retained output");
  const emit = (output: string[], offset: number, path = "fixture/run-a.log", emittedAt = Date.now(), persist = true) => {
    // The real publisher reads persisted bytes before delivering their event.
    if (persist && retained.source_path === path) retained = snapshot(path, [...retained.lines, ...output]);
    bridge.emit({ instance_id: instance.id, process_key: "main", log_path: path, lines: output,
      byte_offset: offset, emitted_at_unix_ms: emittedAt });
  };
  // Rust run 95: the file already contained a six-line Navmesh batch before
  // its delayed event arrived. Native paired lines must stay paired, not double.
  const navmesh = ["[RustNav] underwater_lab", "[RustNav] underwater_lab",
    "[RustNav] oilrig_1", "[RustNav] oilrig_2", "[RustNav] oilrig_1", "[RustNav] oilrig_2"];
  retained = snapshot("fixture/run-a.log", [...runtime.log_tail.lines, ...navmesh]);
  props.runtime = { ...runtime, log_tail: snapshot("fixture/run-a.log", retained.lines.slice(-32)) };
  await act(async () => { render(); });
  await settleUntil(() => text().includes("[RustNav] underwater_lab"), "Authoritative Navmesh snapshot did not load");
  await act(async () => { emit(navmesh, 2800, "fixture/run-a.log", Date.now(), false); });
  check(text().split("[RustNav] underwater_lab").length === 3
    && text().split("[RustNav] oilrig_1").length === 3
    && text().split("[RustNav] oilrig_2").length === 3,
    "A delayed Navmesh event duplicated a batch already present in the file snapshot");
  scenarios++;
  const live = lines("LIVE", 100);
  await act(async () => { emit(live, 3000); });
  check(text().includes("LIVE_000") && text().includes("LIVE_099"), "Live event did not reach the real console component");
  retained = snapshot("fixture/run-a.log", [...runtime.log_tail.lines, ...live]);
  props.runtime = { ...runtime, log_tail: snapshot("fixture/run-a.log", live.slice(-32)) };
  await act(async () => { render(); });
  check(text().includes("BOOT_000") && text().includes("LIVE_000") && text().includes("LIVE_099"),
    "A 32-line health poll discarded previously received console history");
  checkAtBottom("Live events and health polls must follow output while the user stays at the bottom");
  scenarios++;

  const finalEmissionTime = Date.now();
  await act(async () => {
    emit(["FINAL_COMPLETE_LINE"], 3000, "fixture/run-a.log", finalEmissionTime);
    emit(["FINAL_UNTERMINATED_LINE"], 3000, "fixture/run-a.log", finalEmissionTime);
  });
  check(text().includes("FINAL_COMPLETE_LINE") && text().includes("FINAL_UNTERMINATED_LINE"),
    "A final unterminated line sharing its preceding batch's cursor and timestamp was lost");
  scenarios++;

  await act(async () => {
    emit(["IDENTICAL_FINAL_FRAGMENT"], 3020, "fixture/run-a.log", finalEmissionTime);
    emit(["IDENTICAL_FINAL_FRAGMENT"], 3020, "fixture/run-a.log", finalEmissionTime);
  });
  check(text().split("IDENTICAL_FINAL_FRAGMENT").length === 3,
    "Two real deliveries with identical content and metadata must not be guessed to be duplicates");
  scenarios++;

  await act(async () => { bridge.emit({ instance_id: instance.id, log_path: "fixture/run-a.log",
    lines: [], stream_error: "[LanGame] FINAL_DRAIN_INCOMPLETE", byte_offset: 3020, emitted_at_unix_ms: Date.now() }); });
  check(text().includes("FINAL_DRAIN_INCOMPLETE"), "A diagnostic emitted at the previous cursor was lost");
  check(!retained.lines.some(line => line.includes("FINAL_DRAIN_INCOMPLETE")),
    "A stream diagnostic must remain distinct from the persisted native transcript");
  scenarios++;

  retained = snapshot("fixture/run-a.log", ["SAME_PATH_ROTATED_FIRST_LINE"]);
  await act(async () => { emit(["SAME_PATH_ROTATED_FIRST_LINE"], 20, "fixture/run-a.log", Date.now(), false); });
  check(text().includes("SAME_PATH_ROTATED_FIRST_LINE"), "A same-path file rotation required an unrelated service reset");
  scenarios++;

  await act(async () => { emit(lines("BOUNDED", 420), 10000); });
  check(!text().includes("BOOT_000") && !text().includes("BOUNDED_000") && text().includes("BOUNDED_419"),
    "Console history must stay bounded at 400 retained lines");
  checkAtBottom("A full 400-line console must follow its newest output");
  const cappedHeight = consoleNode().scrollHeight;
  await act(async () => { emit(["CAPPED_NEXT_LINE"], 10050); });
  check(consoleNode().scrollHeight === cappedHeight && text().includes("CAPPED_NEXT_LINE"),
    "The bounded append must replace a line without increasing the real scroll height");
  checkAtBottom("A same-height bounded append must remain at the bottom");
  await scrollTo(120);
  const readingPosition = consoleNode().scrollTop;
  await act(async () => { emit(["CAPPED_WHILE_READING"], 10100); });
  check(consoleNode().scrollTop === readingPosition && bottomDistance() > 48,
    "New output must not take the console away from a user reading older lines");
  await scrollTo(consoleNode().scrollHeight - consoleNode().clientHeight - 24);
  await act(async () => { emit(["CAPPED_RESUME_FOLLOW"], 10150); });
  checkAtBottom("Scrolling back near the bottom must resume following on the next line");
  scenarios++;

  retained = snapshot("fixture/run-a.log", lines("RECOVERED", 75));
  const priorReads = reads;
  await act(async () => { bridge.reset(); });
  await settleUntil(() => reads > priorReads && text().includes("RECOVERED_000"), "Event reset did not reload retained history");
  check(!text().includes("BOUNDED_419"), "Reset retained stale-generation content");
  await act(async () => { emit(["AFTER_RESET"], 20); });
  check(text().includes("AFTER_RESET"), "Event reset prevented newly delivered output from being retained");
  scenarios++;

  retained = snapshot("fixture/run-a.log", ["STALE_READ_RESULT"]);
  holdRead = true;
  await act(async () => { bridge.reset(); });
  await settleUntil(() => Boolean(releaseRead), "Recovery did not start its held read");
  retained = snapshot("fixture/run-a.log", ["LIVE_DURING_RECOVERY"]);
  await act(async () => { emit(["LIVE_DURING_RECOVERY"], 30, "fixture/run-a.log", Date.now(), false); });
  holdRead = false;
  await act(async () => { releaseRead!(); });
  check(text().includes("LIVE_DURING_RECOVERY") && !text().includes("STALE_READ_RESULT"),
    "A late retained-file response overwrote newer live output");
  scenarios++;

  await scrollTo(0);
  retained = snapshot("fixture/run-b.log", ["NEW_RUN_ONLY", ...lines("NEW_SOURCE", 160)]);
  props.runtime = { ...runtime, log_tail: retained };
  await act(async () => { render(); });
  await settleUntil(() => text().includes("NEW_RUN_ONLY"), "Source change did not load new history");
  check(!text().includes("RECOVERED_000") && !text().includes("AFTER_RESET"), "New run retained previous run output");
  checkAtBottom("Switching log sources must start at the new source's latest output");
  scenarios++;

  readFailure = true;
  await act(async () => { bridge.reset(); });
  await settleUntil(() => text().includes("History recovery failed in fixture"), "Recovery failure was silently hidden");
  readFailure = false;
  retained = snapshot("fixture/run-b.log", ["RECOVERY_RETRY_OK"]);
  await act(async () => { fixture.querySelector<HTMLButtonElement>(".server-runtime-console-retry")!.click(); });
  await settleUntil(() => text().includes("RECOVERY_RETRY_OK") && !text().includes("History recovery failed in fixture"),
    "Retry did not recover retained output and clear its read error");
  scenarios++;

  retained = snapshot("fixture/native-server.log", lines("NATIVE_INITIAL", 100));
  props.runtime = { ...runtime, log_tail: snapshot(retained.source_path!, retained.lines.slice(-32)) };
  await act(async () => { render(); });
  await settleUntil(() => text().includes("NATIVE_INITIAL_000"), "Native-file initial history was not read");
  check(bridge.active.size === 2, "Native-file scenario requires a successfully registered event bridge");
  const readsBeforeNativePoll = reads;
  retained = snapshot("fixture/native-server.log", [...retained.lines, ...lines("NATIVE_POLL_ONLY", 80)]);
  props.runtime = { ...runtime, log_tail: snapshot(retained.source_path!, retained.lines.slice(-32)) };
  await act(async () => { render(); });
  await settleUntil(() => reads > readsBeforeNativePoll && text().includes("NATIVE_POLL_ONLY_079"),
    "A successfully registered listener without events froze same-path native-file polling");
  check(text().includes("NATIVE_INITIAL_000") && text().includes("NATIVE_POLL_ONLY_000"),
    "Native-file polling must refresh the bounded 400-line document instead of shrinking to the 32-line overview");
  scenarios++;

  separateNativeSource = true;
  const selectionReadStart = requestedRunIds.length;
  props.details = { ...details, active_run: { run_id: 42, log_path: "fixture/launcher.log", processes: [{
    run_id: 42, process_key: "main", display_name: "Primary", pid: 42, status: "Running",
    is_primary: true, log_path: "fixture/launcher.log"
  }] } };
  retained = snapshot("fixture/native-server.log", [...retained.lines, "DEFAULT_NATIVE_SOURCE_REFRESHED"]);
  props.runtime = { ...runtime, log_tail: snapshot(retained.source_path!, retained.lines.slice(-32)) };
  await act(async () => { render(); });
  await settleUntil(() => text().includes("DEFAULT_NATIVE_SOURCE_REFRESHED"),
    "The primary console used the explicit launcher run instead of the resolved native log source");
  check(requestedRunIds.length > selectionReadStart && requestedRunIds.slice(selectionReadStart).every(runId => runId == null),
    "Non-ARK primary reads must leave runId unspecified so the backend resolves the authoritative native file");
  await act(async () => {
    emit(["WRONG_MANAGED_SOURCE_EVENT"], 5000, "fixture/launcher.log");
    emit(["CURRENT_NATIVE_SOURCE_EVENT"], 50, "fixture/native-server.log");
  });
  check(text().includes("CURRENT_NATIVE_SOURCE_EVENT") && !text().includes("WRONG_MANAGED_SOURCE_EVENT")
    && !text().includes("MANAGED_ENTRYPOINT_ONLY"), "Primary console mixed its native source with managed launcher output");
  scenarios++;

  const submit = async (response: Partial<InstanceRuntimeCommandResult>, command: string) => {
    nextResponse = response;
    const input = fixture.querySelector<HTMLInputElement>(".server-runtime-console-command-input")!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, command);
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    input.focus();
    await act(async () => {
      const sent = await fetch(`/__reliability_key/${nonce}/Enter`, { method: "POST" });
      check(sent.ok, "Native Enter dispatch failed");
    });
  };
  await scrollTo(0);
  await submit({ response_text: "NATIVE_REPLY: 0 players" }, "status");
  await settleUntil(() => text().includes("NATIVE_REPLY: 0 players"), "Native command reply was not shown in the console");
  checkAtBottom("A requested command reply must be visible even after reading older output");
  scenarios++;

  await submit({ write_confirmation_pending: true, response_text: "Runtime command accepted; stdin write confirmation is pending." }, "status");
  await settleUntil(() => text().includes("等待写入确认"), "Pending stdin write was presented as a confirmed native reply");
  check(!text().includes("原生响应\nRuntime command accepted"), "Manager receipt was mislabeled as native output");
  scenarios++;

  props.moduleDetails = rustModuleDetails;
  props.details = { ...props.details, summary: { ...instance, module_id: "rust" },
    settings_json: JSON.stringify({ rcon_password: "console-pass", rcon_web: true }) };
  await act(async () => { render(); });
  const previousDispatches = dispatches.length;
  check(!fixture.querySelector<HTMLInputElement>(".server-runtime-console-command-input")!.disabled,
    "Rust's configured WebSocket RCON route must enable the real command input");
  await submit({ response_text: "RUST_NATIVE_REPLY: 0 players" }, "global.status");
  await settleUntil(() => dispatches.length === previousDispatches + 1 && text().includes("RUST_NATIVE_REPLY: 0 players"),
    "Rust command did not dispatch once and display its native response");
  const rustDispatch = dispatches[dispatches.length - 1];
  check(rustDispatch.instanceId === instance.id && rustDispatch.command === "global.status"
    && rustDispatch.options?.transport === "websocket_rcon" && rustDispatch.options.portName === "rcon"
    && rustDispatch.options.passwordSettingKey === "rcon_password",
    "Rust's real module declaration must select websocket_rcon with the rcon endpoint and password setting key");
  check(!JSON.stringify(rustDispatch.options).includes("console-pass"),
    "Command routing must pass the password setting key, not copy its secret into dispatch options");
  scenarios++;

  separateNativeSource = false;
  processDocuments.set(43, snapshot("fixture/shard.log", lines("SHARD", 200)));
  props.details = { ...props.details, active_run: { ...props.details.active_run!, processes: [
    ...props.details.active_run!.processes!,
    { run_id: 43, process_key: "caves", display_name: "Caves", pid: 43, status: "Running",
      is_primary: false, log_path: "fixture/shard.log" }
  ] } };
  await act(async () => { render(); });
  readFailure = true;
  await act(async () => { bridge.reset(); });
  await settleUntil(() => text().includes("History recovery failed in fixture"), "Primary read failure was not displayed");
  readFailure = false;
  holdRead = true;
  releaseRead = undefined;
  await scrollTo(0);
  await act(async () => { fixture.querySelectorAll<HTMLButtonElement>(".server-runtime-console-tab")[1].click(); });
  await settleUntil(() => Boolean(releaseRead), "Selected shard did not start its held read");
  check(!text().includes("History recovery failed in fixture"), "The previous process read error leaked into the selected shard before its first response");
  holdRead = false;
  await act(async () => { releaseRead!(); });
  await settleUntil(() => text().includes("SHARD_199"), "Selected process did not load its retained output");
  checkAtBottom("Selecting another process must reveal its latest output");
  readFailure = true;
  await act(async () => { bridge.reset(); });
  await settleUntil(() => text().includes("History recovery failed in fixture"), "Shard read failure was not displayed");
  readFailure = false;
  holdRead = true;
  releaseRead = undefined;
  await act(async () => { fixture.querySelector<HTMLButtonElement>(".server-runtime-console-retry")!.click(); });
  await settleUntil(() => Boolean(releaseRead), "Same-scope retry did not start its held read");
  check(text().includes("History recovery failed in fixture"), "A pending same-scope retry cleared its unresolved read error");
  holdRead = false;
  await act(async () => { releaseRead!(); });
  await settleUntil(() => !text().includes("History recovery failed in fixture") && text().includes("SHARD_199"),
    "A successful same-scope retry did not clear its error");
  scenarios++;
  readFailure = true;
  await act(async () => { bridge.reset(); });
  await settleUntil(() => text().includes("History recovery failed in fixture"), "Second shard read failure was not displayed");
  readFailure = false;
  holdRead = true;
  releaseRead = undefined;
  await scrollTo(0);
  await act(async () => {
    const target = fixture.querySelector<HTMLSelectElement>(".server-runtime-console-target-select")!;
    target.value = "main";
    target.dispatchEvent(new Event("change", { bubbles: true }));
  });
  await settleUntil(() => Boolean(releaseRead), "Command target change did not start its held read");
  check(!text().includes("History recovery failed in fixture"), "The previous shard read error leaked through a command target change");
  holdRead = false;
  await act(async () => { releaseRead!(); });
  await settleUntil(() => text().includes("DEFAULT_NATIVE_SOURCE_REFRESHED"), "Primary process did not reload its output");
  checkAtBottom("Returning to a process must restore default tail following");
  scenarios++;
  scenarios++;

  await scrollTo(0);
  props.details = { ...props.details, active_run: { ...props.details.active_run!, run_id: 44,
    processes: [{ ...props.details.active_run!.processes![0], run_id: 44 }] } };
  await act(async () => { render(); });
  checkAtBottom("A new run must reset following even when it reuses the same process and log path");
  scenarios++;

  await scrollTo(0);
  props.details = { ...props.details, summary: { ...props.details.summary, id: "fixture-second-instance" } };
  await act(async () => { render(); });
  await settleUntil(() => text().includes("NATIVE_INITIAL_000"), "New instance did not load its retained output");
  checkAtBottom("Changing instances must reset a previous instance's paused following state");
  scenarios++;

  props.runtime = { ...props.runtime!, health: { status: "ready", summary: "Fixture ready" }, diagnostics: [] };
  await act(async () => { render(); });
  const healthLight = () => fixture.querySelector<HTMLElement>(".server-runtime-health-light")!;
  const readyLabel = healthLight().getAttribute("aria-label");
  check(healthLight().classList.contains("is-ready"), "A ready server must have a ready indicator");
  props.runtime = { ...props.runtime, diagnostics: [{ code: "expected_configuration", severity: "info",
    summary: "Expected local configuration", actionable: false }] };
  await act(async () => { render(); });
  check(healthLight().classList.contains("is-ready") && healthLight().getAttribute("aria-label") === readyLabel,
    "An informational diagnostic must not override ready with a warning or an alarm count");
  scenarios++;

  props.runtime = { ...props.runtime, diagnostics: [...props.runtime.diagnostics,
    { code: "actual_warning", severity: "warning", summary: "Warning fixture", actionable: true }] };
  await act(async () => { render(); });
  check(healthLight().classList.contains("is-warning") && healthLight().getAttribute("aria-label")?.includes("1"),
    "A real warning must still override ready, counting only warning/error diagnostics");
  scenarios++;

  props.runtime = { ...props.runtime, diagnostics: [{ code: "actual_error", severity: "error",
    summary: "Error fixture", actionable: true }] };
  await act(async () => { render(); });
  check(healthLight().classList.contains("is-danger") && healthLight().getAttribute("aria-label") !== readyLabel,
    "A real error must remain visible as danger");
  scenarios++;
  bridge.hold = true;
  await act(async () => { bridge.reset(); });
  const registration = bridge.entries.filter(entry => entry.pending && entry.name === "runtime-log-stream").at(-1);
  check(registration, "Expected a pending native listener registration");
  await act(async () => { registration.deliver({ instance_id: props.details.summary.id,
    log_path: retained.source_path, lines: [], byte_offset: 0, emitted_at_unix_ms: Date.now(),
    stream_error: "EARLY_STREAM_DIAGNOSTIC" }); });
  check(text().includes("EARLY_STREAM_DIAGNOSTIC"), "A diagnostic before registration completion was hidden");
  bridge.hold = false;
  await act(async () => { bridge.resolvePending(); });
  check(text().includes("EARLY_STREAM_DIAGNOSTIC"), "Late listener registration success erased an already received stream diagnostic");
  scenarios++;
  const startupTime = Date.now();
  props.startupPending = true;
  props.startupBoundary = { instanceId: props.details.summary.id, startedAtUnixMs: startupTime,
    previousLogPaths: new Set([retained.source_path]) };
  await act(async () => { render(); });
  await act(async () => {
    bridge.emit({ instance_id: props.details.summary.id, log_path: retained.source_path,
      lines: [], byte_offset: 0, emitted_at_unix_ms: startupTime + 1, stream_error: "OLD_RUN_DIAGNOSTIC" });
    bridge.emit({ instance_id: props.details.summary.id, log_path: "fixture/queued-old.log",
      lines: [], byte_offset: 0, emitted_at_unix_ms: startupTime - 1, stream_error: "QUEUED_OLD_DIAGNOSTIC" });
  });
  check(!/OLD_RUN_DIAGNOSTIC|QUEUED_OLD_DIAGNOSTIC/.test(text()), "An old run's diagnostic polluted the new startup");
  await act(async () => { bridge.emit({ instance_id: props.details.summary.id, log_path: "fixture/new-startup.log",
    lines: [], byte_offset: 0, emitted_at_unix_ms: startupTime + 1, stream_error: "CURRENT_STARTUP_DIAGNOSTIC" }); });
  check(text().includes("CURRENT_STARTUP_DIAGNOSTIC"), "The current startup's diagnostic must remain visible");
  scenarios++;
  await act(async () => { root.unmount(); });
  check(errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  return { status: "passed", scenarios, reads, browser_errors: errors };
}

let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error(`Fixture stalled after ${scenarios} scenarios`)), 25000);
})]).finally(() => clearTimeout(watchdog))
  .catch(error => ({ status: "failed", scenarios, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .then(report => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
