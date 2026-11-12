const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const desktopRoot = path.resolve(__dirname, "..");

function loadSourceModule(relativePath, imports, globals = {}) {
  const filename = path.join(desktopRoot, relativePath);
  const exports = {};
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports, ...globals,
    require: (id) => { assert.ok(Object.hasOwn(imports, id), `Unexpected dependency ${id}`); return imports[id]; }
  }, { filename });
  return exports;
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

function pollingHarness(relativePath, imports) {
  const scheduler = new FakeScheduler();
  const effects = [];
  let effectIndex = 0;
  let readSnapshot;
  const hooks = loadSourceModule(relativePath, { ...imports, react: {
    useCallback: (callback) => callback,
    useSyncExternalStore: (_subscribe, snapshot) => { readSnapshot = snapshot; return snapshot(); },
    useEffect: (create, dependencies) => {
      const index = effectIndex++;
      const previous = effects[index];
      if (previous && dependencies.length === previous.dependencies.length
        && dependencies.every((value, position) => Object.is(value, previous.dependencies[position]))) return;
      previous?.cleanup?.();
      effects[index] = { dependencies, cleanup: create() };
    }
  } }, { window: { setTimeout: scheduler.schedule, clearTimeout: scheduler.cancel } });
  return { scheduler,
    render(name, ...args) { effectIndex = 0; return hooks[name](...args); },
    snapshot: () => readSnapshot(),
    dispose: () => { for (const effect of effects) effect.cleanup?.(); }
  };
}

const workshopOperations = loadSourceModule("src/views/servers/workshop-download-operations.ts", {
  "../../app-state": loadSourceModule("src/app-state.ts", { "./api": {} })
}, { crypto });
const { InstanceSettingsSaveCoordinator } = require("../src/views/settings/instance-settings-save-coordinator.ts");

function workshopPollingHarness(readBackgroundJobs) {
  const owner = new InstanceSettingsSaveCoordinator();
  return pollingHarness("src/views/servers/useWorkshopDownloadOperation.ts", {
    "../../api": { readBackgroundJobs },
    "../../domain/single-flight-poller": { SingleFlightPoller },
    "../settings/InstanceSettingsSaveContext": { useInstanceSettingsSaveCoordinator: () => owner },
    "./workshop-download-operations": workshopOperations
  });
}

function libraryPollingHarness(readBackgroundJobs) {
  return pollingHarness("src/hooks/useDesktopEffects.ts", {
    "../api": { readBackgroundJobs }, "../app-state": {}, "../instance-panel-loader": {},
    "../bootstrap-initialization": { createBootstrapInitializationCoordinator: () => ({}) },
    "../domain/single-flight-poller": { SingleFlightPoller }, "../domain/system-resources": {}
  });
}

const workshopRequest = { ids: ["123456"], contentIds: ["123456"], items: {}, locale: "en-US" };
const workshopJob = (id, overrides = {}) => ({ id: `workshop-download:a:${id}`, target_id: "a",
  kind: "DownloadWorkshop", status: "Running", progress_percent: 42, ...overrides });

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

test("library polling stays single-flight and disposes pending reads", async () => {
  const reads = [], values = [];
  const view = libraryPollingHarness(() => { const read = deferred(); reads.push(read); return read.promise; });
  const options = { enabled: false, onJobsSynced: (jobs) => values.push(jobs) };
  view.render("useLibraryJobPolling", options);
  assert.equal(reads.length, 0);
  options.enabled = true;
  view.render("useLibraryJobPolling", options);
  view.render("useLibraryJobPolling", options);
  assert.equal(reads.length, 1);
  assert.equal(view.scheduler.tasks.size, 0, "Pending reads cannot schedule overlapping polls");
  reads[0].resolve([]);
  await flushPromises();
  assert.deepEqual(values, [[]]);
  assert.equal(view.scheduler.runNext(), 1200);
  assert.equal(reads.length, 2);
  view.dispose();
  reads[1].resolve([workshopJob("late")]);
  await flushPromises();
  assert.deepEqual(values, [[]]);
  assert.equal(view.scheduler.tasks.size, 0);
});

test("workshop polling respects read-only, idle and queued states and discards a response after cleanup", async () => {
  for (const [readOnly, stage] of [[true, "running"], [false, "idle"], [false, "queued"], [false, "preparing"], [false, "running"]]) {
    const pending = deferred(), finished = deferred();
    let calls = 0;
    const view = workshopPollingHarness(() => { calls++; return pending.promise; });
    const initial = view.render("useWorkshopDownloadOperation", "a", false);
    let completion;
    if (stage !== "idle") {
      completion = initial.enqueue(workshopRequest, (operation) => {
        if (stage === "running") operation.beginAttempt();
        return finished.promise;
      });
      if (stage !== "queued") await flushPromises();
      assert.equal(view.snapshot().requests[0].status, stage === "queued" ? "queued" : "preparing");
    }
    view.render("useWorkshopDownloadOperation", "a", readOnly);
    const active = !readOnly && stage === "running";
    assert.equal(calls, active ? 1 : 0);
    assert.equal(view.snapshot().requests[0]?.job ?? null, null);
    view.dispose();
    pending.resolve([workshopJob("late")]);
    await flushPromises();
    assert.equal(view.snapshot().requests[0]?.job ?? null, null, "A disposed instance must not receive the late Workshop response");
    assert.equal(view.scheduler.tasks.size, 0);
    finished.resolve();
    await completion;
  }
});

test("workshop polling records only its own accepted attempt and stops when it becomes read-only", async () => {
  const reads = [], finished = deferred();
  const view = workshopPollingHarness(() => { const read = deferred(); reads.push(read); return read.promise; });
  const existing = workshopJob("existing");
  const completion = view.render("useWorkshopDownloadOperation", "a", false).enqueue(workshopRequest, (operation) => {
    operation.beginAttempt();
    return finished.promise;
  });
  await flushPromises();
  const own = workshopJob(view.snapshot().requests[0].attemptToken);
  view.render("useWorkshopDownloadOperation", "a", false);
  view.render("useWorkshopDownloadOperation", "a", false);
  assert.equal(reads.length, 1);
  assert.equal(view.scheduler.tasks.size, 0);
  reads[0].resolve([existing, workshopJob("other", { target_id: "b" }), own]);
  await flushPromises();
  assert.equal(view.snapshot().requests[0].job, own);
  view.render("useWorkshopDownloadOperation", "a", false);
  assert.equal(reads.length, 1, "Progress updates must not restart an unchanged polling effect");
  assert.equal(view.scheduler.runNext(), 350);
  assert.equal(reads.length, 2);
  view.render("useWorkshopDownloadOperation", "a", true);
  reads[1].resolve([{ ...own, progress_percent: 99 }]);
  await flushPromises();
  assert.equal(view.snapshot().requests[0].job, own, "A response started before read-only must not update progress");
  assert.equal(view.scheduler.tasks.size, 0);
  view.dispose();
  finished.resolve();
  await completion;
});

test("server workspace discovers autostart jobs enqueued after an initially empty bootstrap", async () => {
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
  const values = [];
  let result = [];
  const view = libraryPollingHarness(async () => result);
  view.render("useLibraryJobPolling", { enabled: evaluate(), onJobsSynced: jobs => values.push(jobs) });
  await flushPromises();
  assert.deepEqual(values, [[]]);
  result = [{ id: "auto-1", label: "Autostart Server", kind: "StartInstance", status: "Running" }];
  assert.equal(view.scheduler.runNext(), 1200);
  await flushPromises();
  assert.deepEqual(values[1], result);
  view.dispose();
  assert.equal(view.scheduler.tasks.size, 0);
});
