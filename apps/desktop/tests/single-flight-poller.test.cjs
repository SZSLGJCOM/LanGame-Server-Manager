const ts = require("@typescript/typescript6");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { parseSource, sourceText, transpileTypeScript, visitSyntax } = require("../scripts/typescript_source_tools.cjs");

const desktopRoot = path.resolve(__dirname, "..");

function workshopPollingEffect() {
  const filename = path.join(desktopRoot, "src", "views", "servers", "ModWorkbench.tsx");
  const source = fs.readFileSync(filename, "utf8");
  const effects = [];
  visitSyntax(parseSource(source, filename), (node) => {
    if (!ts.isCallExpression(node) || !ts.isIdentifier(node.expression) || node.expression.text !== "useEffect") return;
    const callback = node.arguments[0];
    if (!callback || !ts.isArrowFunction(callback)) return;
    let usesPoller = false;
    visitSyntax(callback.body, (nested) => {
      if (ts.isNewExpression(nested) && ts.isIdentifier(nested.expression) && nested.expression.text === "SingleFlightPoller") usesPoller = true;
    });
    if (usesPoller) effects.push({ source: sourceText(source, node), callback: sourceText(source, callback) });
  });
  assert.equal(effects.length, 1, "Workshop polling must have one effect that owns the shared poller");
  return effects[0];
}

require.extensions[".ts"] = function compileTypeScript(module, filename) {
  const source = fs.readFileSync(filename, "utf8");
  module._compile(transpileTypeScript(source, filename), filename);
};

const { SingleFlightPoller } = require(path.join(
  desktopRoot,
  "src",
  "domain",
  "single-flight-poller.ts"
));

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function flushPromises() {
  return new Promise((resolve) => setImmediate(resolve));
}

class FakeScheduler {
  constructor() {
    this.nextHandle = 1;
    this.tasks = new Map();
  }

  schedule = (callback, delayMs) => {
    const handle = this.nextHandle++;
    this.tasks.set(handle, { callback, delayMs });
    return handle;
  };

  cancel = (handle) => {
    this.tasks.delete(handle);
  };

  runNext() {
    const entry = this.tasks.entries().next().value;
    assert.ok(entry, "a poll must be scheduled");
    const [handle, task] = entry;
    this.tasks.delete(handle);
    task.callback();
    return task.delayMs;
  }
}

test("never overlaps polls and schedules the next read only after completion", async () => {
  const scheduler = new FakeScheduler();
  const first = deferred();
  const second = deferred();
  const values = [];
  let calls = 0;
  const poller = new SingleFlightPoller({
    intervalMs: 350,
    poll: () => {
      calls += 1;
      return calls === 1 ? first.promise : second.promise;
    },
    schedule: scheduler.schedule,
    cancel: scheduler.cancel,
    onValue: (value) => values.push(value)
  });

  poller.start();
  assert.equal(calls, 1);
  assert.equal(await poller.pollNow(), false, "an in-flight poll must reject overlap");
  assert.equal(scheduler.tasks.size, 0);

  first.resolve("first");
  await flushPromises();
  assert.deepEqual(values, ["first"]);
  assert.equal(scheduler.tasks.size, 1);
  assert.equal(scheduler.runNext(), 350);
  assert.equal(calls, 2);
  assert.equal(scheduler.tasks.size, 0);

  second.resolve("second");
  await flushPromises();
  assert.deepEqual(values, ["first", "second"]);
  assert.equal(scheduler.tasks.size, 1);
  poller.dispose();
  assert.equal(scheduler.tasks.size, 0);
});

test("dispose invalidates a late response and prevents rescheduling", async () => {
  const scheduler = new FakeScheduler();
  const pending = deferred();
  const values = [];
  const poller = new SingleFlightPoller({
    intervalMs: 1200,
    poll: () => pending.promise,
    schedule: scheduler.schedule,
    cancel: scheduler.cancel,
    onValue: (value) => values.push(value)
  });

  poller.start();
  poller.dispose();
  pending.resolve("stale");
  await flushPromises();

  assert.deepEqual(values, []);
  assert.equal(scheduler.tasks.size, 0);
  assert.equal(await poller.pollNow(), false);
});

test("a transient failure preserves the loop without overlapping the retry", async () => {
  const scheduler = new FakeScheduler();
  const first = deferred();
  const retry = deferred();
  const errors = [];
  const values = [];
  let calls = 0;
  const poller = new SingleFlightPoller({
    intervalMs: 1200,
    poll: () => {
      calls += 1;
      return calls === 1 ? first.promise : retry.promise;
    },
    schedule: scheduler.schedule,
    cancel: scheduler.cancel,
    onValue: (value) => values.push(value),
    onError: (error) => errors.push(error.message)
  });

  poller.start();
  first.reject(new Error("temporary"));
  await flushPromises();
  assert.deepEqual(errors, ["temporary"]);
  assert.equal(scheduler.tasks.size, 1);

  scheduler.runNext();
  assert.equal(calls, 2);
  assert.equal(await poller.pollNow(), false);
  retry.resolve("recovered");
  await flushPromises();
  assert.deepEqual(values, ["recovered"]);
  poller.dispose();
});

test("library and workshop job polling use the shared single-flight lifecycle", () => {
  const hooksSource = fs.readFileSync(path.join(
    desktopRoot,
    "src",
    "hooks",
    "useDesktopEffects.ts"
  ), "utf8");
  const libraryPolling = hooksSource.slice(
    hooksSource.indexOf("export function useLibraryJobPolling"),
    hooksSource.indexOf("export function useRuntimeViewPolling")
  );
  const workshopPolling = workshopPollingEffect().source;

  for (const source of [libraryPolling, workshopPolling]) {
    assert.match(source, /new SingleFlightPoller</);
    assert.match(source, /poller\.dispose\(\)/);
    assert.doesNotMatch(source, /setInterval\(/);
  }
});

test("workshop polling respects read-only and idle states and discards a response after cleanup", async () => {
  const vm = require("node:vm");
  const callback = transpileTypeScript(`const effect = ${workshopPollingEffect().callback};`, "workshop-polling.ts");
  for (const [readOnly, downloadState] of [[true, "running"], [false, "idle"], [false, "running"]]) {
    const scheduler = new FakeScheduler();
    const pending = deferred();
    const values = [];
    let calls = 0;
    const context = { readOnly, downloadState, props: { details: { summary: { id: "a" } } },
      SingleFlightPoller, readBackgroundJobs: () => { calls++; return pending.promise; },
      isActiveJobStatus: (status) => status === "Running", setActiveJob: (value) => values.push(value),
      window: { setTimeout: scheduler.schedule, clearTimeout: scheduler.cancel } };
    const cleanup = vm.runInNewContext(`${callback}\neffect();`, context);
    const active = !readOnly && downloadState === "running";
    assert.equal(calls, active ? 1 : 0);
    assert.deepEqual(values, [null]);
    if (active) {
      assert.equal(typeof cleanup, "function");
      cleanup();
      pending.resolve([{ target_id: "a", kind: "DownloadWorkshop", status: "Running" }]);
      await flushPromises();
      assert.deepEqual(values, [null], "A disposed instance must not receive the late Workshop response");
    } else assert.equal(cleanup, undefined);
    assert.equal(scheduler.tasks.size, 0);
  }
});

test("server workspace discovers autostart jobs enqueued after an initially empty bootstrap", async () => {
  const vm = require("node:vm");
  const appSource = fs.readFileSync(path.join(desktopRoot, "src", "App.tsx"), "utf8");
  const condition = appSource.match(/useLibraryJobPolling\(\{\s*enabled:\s*([^,]+),/)[1];
  const evaluate = (overrides = {}) => vm.runInNewContext(condition, {
    storageReady: true, activeView: "servers", libraryTaskPolling: false, hasActiveJobs: false,
    creatingModuleIds: new Set(), ...overrides
  });
  assert.equal(evaluate(), true, "an empty bootstrap must not disable discovery on the server workspace");
  assert.equal(evaluate({ storageReady: false }), false);
  assert.equal(evaluate({ activeView: "system" }), false);
  assert.equal(evaluate({ activeView: "system", hasActiveJobs: true }), true);
  assert.equal(evaluate({ activeView: "system", creatingModuleIds: new Set(["minecraft"]) }), true);
  const hooksSource = fs.readFileSync(path.join(desktopRoot, "src", "hooks", "useDesktopEffects.ts"), "utf8");
  const source = hooksSource.slice(hooksSource.indexOf("export function useLibraryJobPolling"), hooksSource.indexOf("export function useRuntimeViewPolling"));
  const scheduler = new FakeScheduler();
  const values = [];
  let result = [];
  let cleanup;
  const context = { exports: {}, useEffect: create => { cleanup = create(); }, SingleFlightPoller,
    readBackgroundJobs: async () => result, window: { setTimeout: scheduler.schedule, clearTimeout: scheduler.cancel } };
  vm.runInNewContext(transpileTypeScript(source, "useDesktopEffects.ts"), context);
  context.exports.useLibraryJobPolling({ enabled: evaluate(), onJobsSynced: jobs => values.push(jobs) });
  await flushPromises();
  assert.deepEqual(values, [[]]);
  result = [{ id: "auto-1", label: "Autostart Server", kind: "StartInstance", status: "Running" }];
  assert.equal(scheduler.runNext(), 1200);
  await flushPromises();
  assert.deepEqual(values[1], result);
  cleanup();
  assert.equal(scheduler.tasks.size, 0);
});
