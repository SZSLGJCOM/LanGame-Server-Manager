const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (loaded, filename) => loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { createRuntimeConsoleHistoryCache, runtimeConsoleStartupHistoryScope } = require("../src/runtime-console-history.ts");
const { createRuntimeConsoleStartupCapture } = require("../src/runtime-console-startup-history.ts");

function capture(history, instanceId = "a", startedAtUnixMs = 100, readLog) {
  const boundary = { instanceId, startedAtUnixMs, previousLogPaths: new Set(["fixture/old.log"]) };
  const callbacks = {};
  const registrations = [];
  const failures = [];
  let released = 0;
  const register = (name, receive) => {
    callbacks[name] = receive;
    return new Promise(resolve => registrations.push(() => resolve(() => { released++; })));
  };
  const owner = createRuntimeConsoleStartupCapture("dontstarve", boundary, {
    history, listenLog: receive => register("log", receive), listenReset: receive => register("reset", receive),
    failed: (...args) => failures.push(args), readLog
  });
  return { owner, callbacks, failures, scope: runtimeConsoleStartupHistoryScope("dontstarve", boundary),
    released: () => released, registered: () => registrations.forEach(resolve => resolve()),
    emit: (lines, extra = {}) => callbacks.log({ instance_id: instanceId, process_key: "master",
      log_path: "fixture/current.log", lines, byte_offset: 1, emitted_at_unix_ms: startedAtUnixMs + 1, ...extra }) };
}

test("the startup operation captures ordered shard history while its instance page is absent", async () => {
  const history = createRuntimeConsoleHistoryCache();
  const a = capture(history);
  const b = capture(history, "b");
  a.emit(["master first"]);
  a.emit(["caves second"], { process_key: "caves", log_path: "fixture/caves.log" });
  a.emit(["master first"]);
  b.emit(["B only"]);
  a.emit(["foreign"], { instance_id: "b" });
  a.emit(["previous path"], { log_path: "fixture/old.log" });
  a.emit(["queued before startup"], { emitted_at_unix_ms: 99 });
  assert.deepEqual(history.read(a.scope).lines, ["[master] master first", "[caves] caves second", "[master] master first"]);
  assert.deepEqual(history.read(b.scope).lines, ["[master] B only"]);

  a.registered();
  b.registered();
  await Promise.all([a.owner.ready, b.owner.ready]);
  a.owner.dispose(); // Completion releases listeners while retaining this attempt's history.
  a.owner.dispose();
  a.emit(["retired delivery"]);
  a.callbacks.reset();
  await Promise.resolve();
  assert.deepEqual(history.read(a.scope).lines, ["[master] master first", "[caves] caves second", "[master] master first"]);
  assert.equal(a.released(), 2);
  assert.deepEqual(history.read(b.scope).lines, ["[master] B only"]);
  b.owner.dispose();
  await Promise.resolve();
  assert.equal(b.released(), 2);
  assert.deepEqual([...a.failures, ...b.failures], []);
});

const settle = () => new Promise(resolve => setImmediate(resolve));
const consoleSource = (revision, lines, partial_lines = [], extra = {}) => ({ log_path: "fixture/current.log", process_key: "master",
  display_name: "Master", run_id: 1, started_at_unix_ms: 101, stream_id: "stream-1", revision,
  lines, partial_lines, total_lines: lines.length, truncated: false, ...extra });
const consoleDelta = (revision, partial_lines = []) => ({ stream_id: "stream-1", started_at_unix_ms: 101,
  revision, previous_revision: revision - 1, reset: revision === 1, partial_lines });

test("startup capture restores active source snapshots and fills revision gaps while the page is absent", async () => {
  const history = createRuntimeConsoleHistoryCache();
  let source = consoleSource(2, ["one"], ["two pending"]), reads = 0;
  const a = capture(history, "a", 100, async () => { reads++; return { lines: [], console_sources: [source] }; });
  a.registered(); await a.owner.ready; await settle();
  assert.equal(reads, 1);
  assert.deepEqual(history.read(a.scope).lines, ["[master] one"]);
  assert.deepEqual(history.read(a.scope).console_sources[0].partial_lines, ["two pending"]);
  a.emit(["already read"], { console: consoleDelta(2) });
  assert.deepEqual(history.read(a.scope).lines, ["[master] one"]);
  source = consoleSource(4, ["one", "two", "three"], ["four pending"]);
  a.emit(["three"], { console: consoleDelta(4, ["four pending"]) });
  await settle();
  assert.equal(reads, 2);
  assert.deepEqual(history.read(a.scope).lines, ["[master] one", "[master] two", "[master] three"]);
  a.emit(["four pending"], { console: consoleDelta(5) });
  assert.deepEqual(history.read(a.scope).console_sources[0].partial_lines, []);
  assert.equal(history.read(a.scope).total_lines, 4);
  a.owner.dispose();
});

test("service reset replaces an in-flight startup recovery and disposal rejects its late result", async () => {
  const history = createRuntimeConsoleHistoryCache(), pending = [];
  const a = capture(history, "a", 100, () => new Promise(resolve => pending.push(resolve)));
  a.registered(); await a.owner.ready;
  a.emit(["current event"], { console: consoleDelta(1) });
  a.emit(["newer event"], { console: consoleDelta(2) });
  pending[0]({ lines: [], console_sources: [consoleSource(1, ["stale snapshot"])] });
  await settle();
  assert.deepEqual(history.read(a.scope).lines, ["[master] current event", "[master] newer event"]);
  a.callbacks.reset();
  assert.equal(history.read(a.scope), null);
  a.callbacks.reset();
  pending[1]({ lines: [], console_sources: [consoleSource(1, ["obsolete service response"])] });
  await settle();
  assert.equal(history.read(a.scope), null);
  pending[2]({ lines: [], console_sources: [consoleSource(1, ["restarted"], [], { stream_id: "stream-2", started_at_unix_ms: 200 })] });
  await settle();
  assert.deepEqual(history.read(a.scope).lines, ["[master] restarted"]);
  a.callbacks.reset(); a.owner.dispose();
  pending[3]({ lines: [], console_sources: [consoleSource(1, ["late after completion"])] });
  await settle();
  assert.equal(history.read(a.scope), null);
});

test("startup history stays bounded and resets revisions without reviving a completed operation", async () => {
  const history = createRuntimeConsoleHistoryCache();
  const old = capture(history);
  old.emit(Array.from({ length: 500 }, (_, index) => "line " + index));
  assert.equal(history.read(old.scope).lines.length, 400);
  assert.equal(history.read(old.scope).lines[0], "[master] line 100");
  old.owner.dispose(); // Registration may finish after leaving the owning view.
  old.registered();
  await old.owner.ready;
  assert.equal(old.released(), 2);

  const next = capture(history, "a", 200);
  next.registered();
  await next.owner.ready;
  assert.equal(history.read(next.scope), null);
  old.emit(["old operation"]);
  next.emit([], { snapshot_revision: 9, snapshot: { source_path: "fixture/current.log", lines: ["new revision"], total_lines: 1 } });
  assert.equal(history.read(next.scope), null, "native game documents have no baseline in the operation-owned startup console");
  const stream = { ...consoleDelta(1), stream_id: "new-startup", started_at_unix_ms: 201 };
  next.emit(["new revision"], { console: stream });
  next.emit(["late revision"], { console: stream });
  assert.deepEqual(history.read(next.scope).lines, ["[master] new revision"]);
  next.callbacks.reset();
  assert.equal(history.read(next.scope), null);
  next.emit(["service restarted"], { console: { ...stream, stream_id: "restarted-startup", started_at_unix_ms: 300 } });
  assert.deepEqual(history.read(next.scope).lines, ["[master] service restarted"]);
  next.owner.dispose();
  await Promise.resolve();
  assert.equal(history.read(old.scope).lines.at(-1), "[master] line 499");
  assert.deepEqual(history.read(next.scope).lines, ["[master] service restarted"]);
  assert.equal(next.released(), 2);
});

test("metadata-only diagnostics survive navigation and only the same producer can clear them", async () => {
  const history = createRuntimeConsoleHistoryCache();
  const a = capture(history);
  a.registered();
  await a.owner.ready;
  a.emit([], { run_id: 23, stream_error: "STARTUP_READER_FAILED" });
  assert.equal(history.read(a.scope)?.startup_stream_diagnostic?.message, "STARTUP_READER_FAILED");
  assert.deepEqual(history.read(a.scope).lines, []);
  a.emit(["other shard is healthy"], { process_key: "caves", run_id: 24, log_path: "fixture/caves.log" });
  a.emit(["another run is healthy"], { run_id: 99 });
  a.emit([], { run_id: 23, log_path: "fixture/old.log", stream_error: "OLD_DIAGNOSTIC" });
  assert.equal(history.read(a.scope).startup_stream_diagnostic.message, "STARTUP_READER_FAILED");
  a.emit(["same reader recovered"], { run_id: 23 });
  assert.equal(history.read(a.scope).startup_stream_diagnostic, null);
  assert.equal(history.read(a.scope).lines.some(line => line.includes("STARTUP_READER_FAILED")), false);
  a.owner.dispose();
  await Promise.resolve();
  assert.equal(history.read(a.scope).startup_stream_diagnostic, null);
  assert.equal(history.read(a.scope).lines.at(-1), "[master] same reader recovered");
});

test("disposal reads the final durable outcome and cannot import a newer attempt's receipt", async () => {
  const history = createRuntimeConsoleHistoryCache();
  const boundary = { instanceId: "a", startedAtUnixMs: 100, startupAttemptId: "attempt-a", previousLogPaths: new Set() };
  const scope = runtimeConsoleStartupHistoryScope("terraria", boundary);
  let status = "starting", readCount = 0;
  const session = () => ({ attempt_id: "attempt-a", status, started_at_unix_ms: 101,
    finished_at_unix_ms: status === "starting" ? null : 200, source_path: "fixture/startup.log", run_ids: [],
    error: status === "failed" ? "TEST_FAILURE" : null, reader_error: null, retained_start_cursor: "0", before_cursor: null, line_generation: "retained:0",
    console_source: null, snapshot: { source_path: "fixture/startup.log", total_lines: 2, lines: ["precheck", status] } });
  const owner = createRuntimeConsoleStartupCapture("terraria", boundary, { history,
    listenLog: async () => () => {}, listenReset: async () => () => {}, failed: () => assert.fail("Unexpected capture failure"),
    readLog: async () => { readCount++; return { lines: [], startup_session: session() }; } });
  await owner.ready; await settle();
  status = "failed";
  await owner.dispose();
  assert.equal(readCount, 2, "Completion performs a separate final read after cancelling the regular refresh");
  assert.equal(history.read(scope).startup_session.status, "failed");
  assert.deepEqual(history.read(scope).startup_session.snapshot.lines, ["precheck", "failed"]);
  const nextScope = runtimeConsoleStartupHistoryScope("terraria", { ...boundary, startupAttemptId: "attempt-b" });
  assert.equal(history.read(nextScope), null, "Even attempts created in the same millisecond remain distinct");
});
