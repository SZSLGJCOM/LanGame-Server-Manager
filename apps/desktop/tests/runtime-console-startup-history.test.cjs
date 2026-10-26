const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (loaded, filename) => loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { createRuntimeConsoleHistoryCache, runtimeConsoleStartupHistoryScope } = require("../src/runtime-console-history.ts");
const { createRuntimeConsoleStartupCapture } = require("../src/runtime-console-startup-history.ts");

function capture(history, instanceId = "a", startedAtUnixMs = 100) {
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
    failed: (...args) => failures.push(args)
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
  a.owner.dispose(); // Both success and failure leave through the same operation cleanup.
  a.owner.dispose();
  a.emit(["retired delivery"]);
  a.callbacks.reset();
  await Promise.resolve();
  assert.equal(history.read(a.scope), null);
  assert.equal(a.released(), 2);
  assert.deepEqual(history.read(b.scope).lines, ["[master] B only"]);
  b.owner.dispose();
  await Promise.resolve();
  assert.equal(b.released(), 2);
  assert.deepEqual([...a.failures, ...b.failures], []);
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
  next.emit([], { snapshot_revision: 8, snapshot: { source_path: "fixture/current.log", lines: ["late revision"], total_lines: 1 } });
  assert.deepEqual(history.read(next.scope).lines, ["new revision"]);
  next.callbacks.reset();
  assert.equal(history.read(next.scope), null);
  next.emit([], { snapshot_revision: 1, snapshot: { source_path: "fixture/current.log", lines: ["service restarted"], total_lines: 1 } });
  assert.deepEqual(history.read(next.scope).lines, ["service restarted"]);
  next.owner.dispose();
  await Promise.resolve();
  assert.equal(history.read(old.scope), null);
  assert.equal(history.read(next.scope), null);
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
  assert.equal(history.read(a.scope), null);
});
