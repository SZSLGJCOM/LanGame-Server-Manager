const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function deferred() {
  let resolve, reject;
  const promise = new Promise((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}
async function flush() { for (let i = 0; i < 20; i++) await Promise.resolve(); }

function harness(overrides = {}) {
  const log = { lines: [], read_error: null, source_path: null };
  const ports = {
    listInstancesFromStorage: async () => [{ id: "a" }],
    readInstanceDetails: async id => ({ summary: { id } }),
    listInstanceBackups: async () => [],
    readInstanceRuntime: async () => ({ log_tail: log }),
    readInstanceRuntimeWindowSnapshot: async () => ({ windows: [] }),
    readInstanceLogDocument: async () => log,
    previewInstanceLaunch: async () => ({ executable: "server" }),
    ...overrides
  };
  const timers = new Map();
  const deadlines = [];
  let nextTimer = 0;
  const filename = path.join(__dirname, "../src/instance-panel-loader.ts");
  const exports = {};
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports, Error, Map, Promise,
    setTimeout: (callback, delay) => { deadlines.push(delay); timers.set(++nextTimer, callback); return nextTimer; },
    clearTimeout: id => timers.delete(id),
    require: id => {
      if (id === "./api") return ports;
      if (id === "./app-state") return {
        describeError: error => error instanceof Error ? error.message : String(error),
        resolveLogDocument: (runtime, document) => document && (document.lines.length || document.read_error || document.source_path) ? document : runtime.log_tail
      };
      throw new Error(`Unexpected dependency ${id}`);
    }
  }, { filename });
  const reader = new exports.InstancePanelReader(ports);
  function observe(id = "a") {
    const controller = new AbortController();
    const updates = [];
    const progress = [];
    const result = reader.load(id, {
      signal: controller.signal,
      onUpdate: patch => updates.push(patch),
      onProgress: state => progress.push(state)
    });
    return { controller, updates, progress, result };
  }
  return { reader, observe, timers, deadlines };
}

test("retirement recovery reads fresh state after a late pre-removal failure without overlapping native reads", async () => {
  const oldDetails = deferred();
  let calls = 0;
  const state = harness({ readInstanceDetails: async id => ++calls === 1 ? oldDetails.promise : ({ summary: { id }, recovered: true }) });
  const old = state.observe();
  await flush();
  state.reader.invalidate("a");
  old.controller.abort();
  const recovered = state.observe();
  await flush();
  assert.equal(calls, 1, "the cancelled native request retains its slot until it settles");
  oldDetails.reject(new Error("Instance directory was moved"));
  const result = await recovered.result;
  await old.result;
  assert.equal(calls, 2);
  assert.equal(result.errors.details, undefined);
  assert.ok(recovered.updates.some(patch => patch.details?.recovered));
  assert.equal(old.updates.some(patch => patch.details), false);
});

test("a cancelled successor never launches a new native request after retirement", async () => {
  const oldDetails = deferred();
  let calls = 0;
  const state = harness({ readInstanceDetails: () => { calls++; return oldDetails.promise; } });
  const old = state.observe();
  await flush();
  state.reader.invalidate("a");
  old.controller.abort();
  const successor = state.observe();
  successor.controller.abort();
  oldDetails.reject(new Error("Missing instance"));
  await Promise.all([old.result, successor.result]);
  await flush();
  assert.equal(calls, 1);
});

test("a second recovery observer keeps the fresh read alive when the first observer leaves", async () => {
  const oldDetails = deferred();
  let calls = 0;
  const state = harness({ readInstanceDetails: async id => ++calls === 1 ? oldDetails.promise : ({ summary: { id }, recovered: true }) });
  const old = state.observe();
  await flush();
  state.reader.invalidate("a");
  old.controller.abort();
  const first = state.observe();
  const second = state.observe();
  first.controller.abort();
  oldDetails.reject(new Error("Instance moved"));
  await Promise.all([old.result, first.result, second.result]);
  assert.equal(calls, 2);
  assert.ok(second.updates.some(patch => patch.details?.recovered));
});

test("a post-retirement inventory observation cannot reuse the pre-removal instance list", async () => {
  const oldList = deferred();
  let calls = 0;
  const state = harness({ listInstancesFromStorage: () => ++calls === 1 ? oldList.promise : Promise.resolve([{ id: "b" }]) });
  const oldController = new AbortController();
  const old = state.reader.readInstances(oldController.signal);
  const cancelled = assert.rejects(old);
  await flush();
  state.reader.invalidate("a");
  oldController.abort();
  const fresh = state.reader.readInstances(new AbortController().signal);
  oldList.resolve([{ id: "a" }, { id: "b" }]);
  await cancelled;
  assert.deepEqual(await fresh, [{ id: "b" }]);
  assert.equal(calls, 2);
});

test("a slow backup list cannot withhold available instance details or runtime", async () => {
  const backups = deferred();
  const state = harness({ listInstanceBackups: () => backups.promise });
  const observation = state.observe();
  await flush();
  assert.ok(observation.updates.some(patch => patch.details?.summary.id === "a"));
  assert.ok(observation.updates.some(patch => patch.runtime));
  assert.deepEqual(Array.from(observation.progress.at(-1).pending), ["backups"]);
  backups.resolve([{ backup_id: "backup" }]);
  const result = await observation.result;
  assert.equal(result.pending.length, 0);
  assert.equal(Object.keys(result.errors).length, 0);
  assert.equal(observation.updates.at(-1).backups[0].backup_id, "backup");
  assert.equal(state.timers.size, 0);
});

test("backup rejection is localized and does not discard successful core reads", async () => {
  const state = harness({ listInstanceBackups: async () => { throw new Error("Backup unavailable"); } });
  const observation = state.observe();
  const result = await observation.result;
  assert.equal(result.errors.backups, "Backup unavailable");
  assert.ok(observation.updates.some(patch => patch.details));
  assert.ok(observation.updates.some(patch => patch.runtime));
  assert.equal(Object.keys(result.errors).length, 1);
});

test("changing selection suppresses all late updates from the old instance", async () => {
  const oldDetails = deferred();
  const state = harness({ readInstanceDetails: id => id === "a" ? oldDetails.promise : Promise.resolve({ summary: { id } }) });
  const first = state.observe("a");
  await flush();
  first.controller.abort();
  const firstUpdateCount = first.updates.length;
  const firstProgressCount = first.progress.length;
  const second = state.observe("b");
  await second.result;
  oldDetails.resolve({ summary: { id: "a" } });
  await first.result;
  await flush();
  assert.equal(first.updates.length, firstUpdateCount);
  assert.equal(first.progress.length, firstProgressCount);
  assert.ok(second.updates.some(patch => patch.details?.summary.id === "b"));
  assert.ok(second.updates.every(patch => !patch.details || patch.details.summary.id === "b"));
  assert.equal(state.timers.size, 0);
});

test("a deadline reports an error and retry shares the still-running native read", async () => {
  const backupRead = deferred();
  let reads = 0;
  const state = harness({ listInstanceBackups: () => { reads++; return backupRead.promise; } });
  const first = state.observe();
  await flush();
  assert.equal(state.timers.size, 1);
  for (const callback of [...state.timers.values()]) callback();
  assert.match((await first.result).errors.backups, /timed out/);
  const count = first.updates.length;
  const retry = state.observe();
  await flush();
  assert.equal(reads, 1);
  backupRead.resolve([{ backup_id: "recovered" }]);
  assert.equal(Object.keys((await retry.result).errors).length, 0);
  assert.equal(first.updates.length, count, "the timed-out observation must not later publish success");
  assert.equal(retry.updates.at(-1).backups[0].backup_id, "recovered");
  assert.equal(state.timers.size, 0);
});

test("a failed read starts a fresh request on retry", async () => {
  let reads = 0;
  const state = harness({ readInstanceDetails: async () => {
    if (++reads === 1) throw new Error("Transient read failure");
    return { summary: { id: "a" } };
  } });
  assert.equal((await state.observe().result).errors.details, "Transient read failure");
  const recovered = state.observe();
  await recovered.result;
  assert.ok(recovered.updates.some(patch => patch.details));
  assert.equal(reads, 2);
});

test("runtime and dedicated log arrival orders retain the richer log document", async () => {
  for (const runtimeFirst of [true, false]) {
    const runtime = deferred();
    const document = deferred();
    const richLog = { lines: ["first", "second"], source_path: "server.log", read_error: null };
    const state = harness({ readInstanceRuntime: () => runtime.promise, readInstanceLogDocument: () => document.promise });
    const observation = state.observe();
    const resolveRuntime = () => runtime.resolve({ log_tail: { lines: ["second"] } });
    if (runtimeFirst) resolveRuntime(); else document.resolve(richLog);
    await flush();
    if (runtimeFirst) document.resolve(richLog); else resolveRuntime();
    await observation.result;
    const delivered = observation.updates.filter(patch => patch.logDocument);
    assert.ok(Object.is(delivered.at(-1).logDocument, richLog));
  }
});

test("isolation reads use a 15-second deadline and retry the same pending IPC", async () => {
  const read = deferred();
  let calls = 0;
  const state = harness({ readInstanceIsolation: () => { calls++; return read.promise; } });
  const first = state.reader.readIsolation("a", new AbortController().signal);
  const firstFailed = assert.rejects(first, /timed out/);
  await flush();
  assert.deepEqual(state.deadlines, [15_000]);
  for (const timer of [...state.timers.values()]) timer();
  await firstFailed;
  const retry = state.reader.readIsolation("a", new AbortController().signal);
  await flush();
  assert.equal(calls, 1);
  const report = { instance_id: "a", mode: "damaged", issues: ["Runtime is missing"] };
  read.resolve(report);
  assert.equal(await retry, report);
  assert.equal(state.timers.size, 0);
});

test("aborting an isolation observer releases its timer without starting another native read", async () => {
  const read = deferred();
  let calls = 0;
  const state = harness({ readInstanceIsolation: () => { calls++; return read.promise; } });
  const owner = new AbortController();
  const observed = state.reader.readIsolation("a", owner.signal);
  const aborted = assert.rejects(observed, /Selection changed/);
  await flush();
  owner.abort(new Error("Selection changed"));
  await aborted;
  assert.equal(state.timers.size, 0);
  const retry = state.reader.readIsolation("a", new AbortController().signal);
  const report = { instance_id: "a", mode: "private", issues: [] };
  read.resolve(report);
  assert.equal(await retry, report);
  assert.equal(calls, 1);
  assert.equal(state.timers.size, 0);
});
