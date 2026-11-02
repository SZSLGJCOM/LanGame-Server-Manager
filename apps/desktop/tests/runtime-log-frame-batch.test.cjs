const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (loaded, filename) => loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { createRuntimeLogFrameBatch } = require("../src/runtime-log-frame-batch.ts");
const { mountWorkbench, settle } = require("./helpers/runtime-workbench.cjs");
const { runtimeConsoleHistory } = require("../src/runtime-console-history.ts");

test("frame batching publishes one merged document and cancellation rejects an already queued callback", () => {
  const callbacks = new Map(), writes = [], cancelled = [];
  let sequence = 0;
  const batch = createRuntimeLogFrameBatch({ schedule: callback => { callbacks.set(++sequence, callback); return sequence; },
    cancel: handle => cancelled.push(handle), publish: value => writes.push(value) });
  for (let i = 0; i < 1000; i++) batch.push(i);
  assert.equal(callbacks.size, 1);
  callbacks.get(1)();
  assert.deepEqual(writes, [999]);
  batch.push(1000); batch.clear(); batch.push(1001); callbacks.get(2)();
  assert.deepEqual(cancelled, [2]);
  assert.deepEqual(writes, [999]);
  callbacks.get(3)();
  assert.deepEqual(writes, [999, 1001]);
});

test("a late read cannot overwrite console revisions merged before their browser frame publishes", async (t) => {
  const callbacks = new Map(); let sequence = 0, release;
  const previousWindow = globalThis.window;
  globalThis.window = { requestAnimationFrame: callback => { callbacks.set(++sequence, callback); return sequence; },
    cancelAnimationFrame: handle => callbacks.delete(handle) };
  t.after(() => { globalThis.window = previousWindow; });
  const read = new Promise(resolve => { release = resolve; });
  let reads = 0;
  const mounted = mountWorkbench({ promise: Promise.resolve(() => {}) }, { readLogDocument: () => ++reads === 1
    ? Promise.resolve({ source_path: "fixture/run-1-main.log", lines: [] }) : read });
  t.after(mounted.dispose);
  // The receipt must identify this run before runtime events become eligible.
  await settle();
  for (let revision = 1; revision <= 2; revision++) mounted.receive({ lines: [`line ${revision}`], console: {
    stream_id: "stream", started_at_unix_ms: 1, revision, previous_revision: revision - 1, reset: revision === 1, partial_lines: [] } });
  release({ source_path: "fixture/run-1-main.log", lines: ["line 1"], console_sources: [{
    log_path: "fixture/run-1-main.log", process_key: null, display_name: null, run_id: null,
    stream_id: "stream", started_at_unix_ms: 1, revision: 1, lines: ["line 1"], partial_lines: [], total_lines: 1, truncated: false
  }] });
  await settle();
  assert.equal(callbacks.size, 1);
  callbacks.values().next().value();
  assert.deepEqual(mounted.snapshot().lines, ["line 1", "line 2"]);
  assert.equal(mounted.snapshot().console_sources[0].revision, 2);
});

test("the workbench applies a console burst without readbacks and releases a queued frame on unmount", async (t) => {
  const callbacks = new Map(); let sequence = 0, reads = 0;
  const previousWindow = globalThis.window;
  globalThis.window = { requestAnimationFrame: callback => { callbacks.set(++sequence, callback); return sequence; },
    cancelAnimationFrame: handle => callbacks.delete(handle) };
  t.after(() => { globalThis.window = previousWindow; });
  const mounted = mountWorkbench({ promise: Promise.resolve(() => {}) }, { readLogDocument: async () => {
    reads++; return { source_path: "fixture/run-1-main.log", lines: [] };
  } });
  t.after(mounted.dispose);
  await settle();
  const initialReads = reads;
  for (let revision = 1; revision <= 100; revision++) mounted.receive({ lines: revision % 2 === 0 ? ["repeat"] : [],
    console: { stream_id: "stream", started_at_unix_ms: 1, revision, previous_revision: revision - 1,
      reset: revision === 1, partial_lines: revision % 2 ? ["part"] : [] } });
  assert.equal(callbacks.size, 1);
  assert.equal(reads, initialReads);
  const flush = callbacks.values().next().value; callbacks.clear(); flush();
  assert.equal(mounted.snapshot().lines.length, 50);
  assert.ok(mounted.snapshot().lines.every(line => line === "repeat"));
  mounted.receive({ lines: [], console: { stream_id: "stream", started_at_unix_ms: 1, revision: 101, previous_revision: 100,
    reset: false, partial_lines: ["last pending"] } });
  const late = callbacks.values().next().value;
  mounted.dispose(); const writes = mounted.writes.length;
  late();
  assert.equal(callbacks.size, 0);
  assert.equal(mounted.writes.length, writes);
});

test("the default ARK game log accepts native patches, rejects another map and reads only on a gap", async (t) => {
  const callbacks = new Map(); let sequence = 0, reads = 0;
  const previousWindow = globalThis.window;
  globalThis.window = { requestAnimationFrame: callback => { callbacks.set(++sequence, callback); return sequence; },
    cancelAnimationFrame: handle => callbacks.delete(handle) };
  t.after(() => { globalThis.window = previousWindow; });
  let retained = { source_path: "fixture/native.log", snapshot_revision: 5, lines: ["native", "part"] };
  const readArguments = [];
  const mounted = mountWorkbench({ promise: Promise.resolve(() => {}) }, {
    details: { summary: { id: "fixture", module_id: "arksurvivalascended", status: "running", active_process_count: 1 },
      active_run: { run_id: 7, log_path: "fixture/console.log", processes: [{ run_id: 7, process_key: "theisland", is_primary: true,
        display_name: "The Island", log_path: "fixture/console.log", status: "running" }] } },
    readLogDocument: async (...args) => {
      reads++; readArguments.push(args);
      if (args[2] === undefined) {
        assert.deepEqual(args, ["fixture", 400, undefined, undefined]);
        return { source_path: "fixture/console.log", lines: [] };
      }
      assert.deepEqual(args, ["fixture", 400, 7, "game"]);
      return retained;
    }
  });
  t.after(mounted.dispose);
  await settle();
  const initialReads = reads;
  assert.deepEqual(readArguments[0], ["fixture", 400, undefined, undefined], "Read the startup receipt before selecting the native map log");
  assert.deepEqual(readArguments.at(-1), ["fixture", 400, 7, "game"]);
  const event = { log_path: "fixture/native.log", process_key: "theisland", run_id: 7, lines: [], document_patch: {
    previous_revision: 5, revision: 6, start: 1, delete_count: 1, lines: ["partial complete", "repeat", "repeat"],
    total_lines: 4, truncated: false, read_error: null } };
  mounted.receive({ ...event, process_key: "scorched-earth", run_id: 8 });
  assert.equal(callbacks.size, 0);
  mounted.receive(event);
  assert.equal(callbacks.size, 1);
  callbacks.values().next().value(); callbacks.clear();
  assert.equal(reads, initialReads);
  assert.deepEqual(mounted.snapshot().lines, ["native", "partial complete", "repeat", "repeat"]);
  retained = { source_path: "fixture/native.log", snapshot_revision: 8, lines: ["native", "missed", "recovered"] };
  mounted.receive({ ...event, document_patch: { ...event.document_patch, previous_revision: 7, revision: 8 } });
  await settle();
  assert.equal(reads, initialReads + 1);
  assert.deepEqual(mounted.snapshot().lines, ["native", "missed", "recovered"]);
});

test("a failed partial stream accepts the later complete disk tail and retains the producer diagnostic", async (t) => {
  const callbacks = new Map(); let sequence = 0;
  const previousWindow = globalThis.window;
  globalThis.window = { requestAnimationFrame: callback => { callbacks.set(++sequence, callback); return sequence; },
    cancelAnimationFrame: handle => callbacks.delete(handle) };
  t.after(() => { globalThis.window = previousWindow; });
  let retained = { source_path: "fixture/run-1-main.log", lines: [] }, hold = false, release;
  const mounted = mountWorkbench({ promise: Promise.resolve(() => {}) }, { readLogDocument: async () => {
    const response = retained;
    if (hold) await new Promise(resolve => { release = resolve; });
    return response;
  } });
  t.after(mounted.dispose);
  await settle();
  mounted.receive({ lines: [], console: { stream_id: "stream", started_at_unix_ms: 1,
    revision: 1, previous_revision: 0, reset: true, partial_lines: ["incomplete"] } });
  retained = { source_path: "fixture/run-1-main.log", lines: ["OLD_READ"] };
  hold = true;
  mounted.receive({ lines: [] });
  retained = { source_path: "fixture/run-1-main.log", lines: ["complete final tail"] };
  mounted.receive({ lines: [], stream_error: "PRODUCER_DRAIN_FAILED" });
  hold = false; release();
  await settle();
  assert.deepEqual(mounted.snapshot().lines, ["complete final tail"]);
  assert.equal(mounted.snapshot().console_sources, undefined);
  assert.equal(mounted.diagnostic().message, "PRODUCER_DRAIN_FAILED");
  assert.ok(!mounted.writes.some(write => write?.lines?.includes("OLD_READ")), "an old read cannot confirm a stream after its failure");
});

test("a restored console cursor does not block a new mount's successful disk fallback", async (t) => {
  const scope = { instanceId: "fixture", moduleId: "dontstarve", processKey: null, runId: 1, source: "default" };
  runtimeConsoleHistory.write(scope, { source_path: "fixture/run-1-main.log", lines: [], console_sources: [{
    log_path: "fixture/run-1-main.log", process_key: null, display_name: null, run_id: 1,
    stream_id: "retired", started_at_unix_ms: 1, revision: 8, lines: [], partial_lines: ["cached partial"], total_lines: 0, truncated: false
  }] });
  t.after(() => runtimeConsoleHistory.clearInstance("fixture"));
  const mounted = mountWorkbench({ promise: Promise.resolve(() => {}) }, {
    details: { summary: { id: "fixture", module_id: "dontstarve", status: "stopped", active_process_count: 0 },
      active_run: { run_id: 1, log_path: "fixture/run-1-main.log" } },
    readLogDocument: async () => ({ source_path: "fixture/run-1-main.log", lines: ["complete retained file"] })
  });
  t.after(mounted.dispose);
  await settle();
  assert.deepEqual(mounted.snapshot().lines, ["complete retained file"]);
  assert.equal(mounted.snapshot().console_sources, undefined);
});
