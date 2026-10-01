const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const appPath = path.join(__dirname, "../src/App.tsx");
const appSource = fs.readFileSync(appPath, "utf8");
const effectsPath = path.join(__dirname, "../src/hooks/useDesktopEffects.ts");
const flush = () => new Promise(setImmediate);

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

function appFragment(start, end, suffix, context) {
  const first = appSource.indexOf(start);
  const last = appSource.indexOf(end, first + start.length);
  assert.ok(first >= 0 && last > first, `App boundary not found: ${start}`);
  return vm.runInNewContext(transpileTypeScript(appSource.slice(first, last) + suffix, appPath), context);
}

function pollingEnabled(activeView, storageReady, runtimeAutoRefreshPaused, retirementPending = false) {
  return appFragment("const shouldLoadSelectedPanel =", "const runtimePollIntervalMs =",
    "\nsystemPollingEnabled;", { activeView, storageReady, runtimeAutoRefreshPaused, retirementPending,
      retirement: { operations: new Map() }, selectedInstanceId: null });
}

function fixture(bootstrapApp) {
  let now = 0;
  let nextId = 0;
  let dependencies;
  let cleanup;
  const timers = new Map();
  const synced = [];
  const errors = [];
  const requestRef = { current: null };
  let refreshing = false;
  const schedule = (callback, delay, interval = false) => {
    const id = ++nextId;
    timers.set(id, { callback, at: now + delay, interval: interval ? delay : null });
    return id;
  };
  const modules = {
    react: { useRef: () => requestRef, useState: () => [refreshing, (value) => { refreshing = value; }],
      useEffect: (effect, next) => {
      if (dependencies && next.every((value, index) => value === dependencies[index])) return;
      cleanup?.();
      dependencies = next;
      cleanup = effect();
    } },
    "../api": { bootstrapApp },
    "../app-state": { SYSTEM_VIEW_WARMUP_POLL_DELAY_MS: 16000 },
    "../instance-panel-loader": {},
    "../bootstrap-initialization": { createBootstrapInitializationCoordinator: () => ({}) },
    "../domain/single-flight-poller": {},
    "../domain/system-resources": { RESOURCE_STALE_AFTER_MS: 180000 }
  };
  const exports = {};
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(effectsPath, "utf8"), effectsPath), {
    exports,
    Date: { now: () => 1_800_000_000_000 },
    require: (id) => { assert.ok(Object.hasOwn(modules, id), `unexpected import ${id}`); return modules[id]; },
    window: {
      setTimeout: (callback, delay) => schedule(callback, delay),
      setInterval: (callback, delay) => schedule(callback, delay, true),
      clearTimeout: (id) => timers.delete(id),
      clearInterval: (id) => timers.delete(id)
    }
  }, { filename: effectsPath });
  return {
    synced, errors, timers,
    get refreshing() { return refreshing; },
    render: (enabled = true, intervalMs = 60000, requestRevision = 0) => exports.useSystemViewPolling({
      enabled, intervalMs, requestRevision, onSynced: (value) => synced.push(value), onError: (error) => errors.push(error)
    }),
    unmount: () => cleanup?.(),
    async advance(duration) {
      // Immediate async effects settle their microtasks before the next timer.
      await flush();
      const end = now + duration;
      for (;;) {
        const next = [...timers].filter(([, timer]) => timer.at <= end).sort((a, b) => a[1].at - b[1].at)[0];
        if (!next) break;
        const [id, timer] = next;
        now = timer.at;
        if (timer.interval === null) timers.delete(id);
        else timer.at += timer.interval;
        timer.callback();
        await flush();
      }
      now = end;
    }
  };
}

function booted(observedAt = 1_800_000_000_000, overrides = {}) {
  return {
    booted_at_unix_ms: 123,
    state: { modules: [], instances: [], settings: { games_root: "D:/games" }, snapshot: {
      running_instances: 1,
      network_adapters: [{ name: "Ethernet", status: "Up", receive_bps: 100, rate_status: "valid" }],
      telemetry: { observed_at_unix_ms: observedAt, cpu: "valid", cpu_cores: "valid",
        memory: "valid", disk_capacity: "valid", disk_io: "valid", network: "valid", ...overrides }
    } }
  };
}

test("server failures do not disable home telemetry, while view and storage still gate it", () => {
  assert.equal(pollingEnabled("system", true, true), true);
  assert.equal(pollingEnabled("system", true, false), true);
  assert.equal(pollingEnabled("servers", true, true), false);
  assert.equal(pollingEnabled("system", false, false), false);
});

test("system sync does not clear or resume a failed server poller", () => {
  let serverResumes = 0;
  let issue = { message: "system failed" };
  let current = booted();
  const sync = appFragment("const handleSystemViewSynced =", "const handleRuntimeInstancesSynced =",
    "\nhandleSystemViewSynced;", {
      useEffectEvent: (callback) => callback,
      retirement: { isPending: () => false, currentRevision: () => 0, mergeInstances: (_current, incoming) => incoming },
      setBootstrap: (value) => { current = typeof value === "function" ? value(current) : value; },
      setSelectedModuleId: () => {}, setSelectedInstanceId: () => {},
      setSystemRefreshIssue: (value) => { issue = value; },
      markRuntimeRefreshed: () => { serverResumes += 1; },
      mergeBootstrapSnapshot: (previous, latest) => latest
    });
  sync(booted(), 0);
  assert.equal(serverResumes, 0);
  assert.equal(issue, null);
});

test("a rejected telemetry request retries at the existing cadence and recovers", async () => {
  let calls = 0;
  const input = booted();
  const view = fixture(async () => {
    calls += 1;
    if (calls <= 3) throw new Error("temporary telemetry failure");
    return input;
  });
  view.render();
  await view.advance(180000);
  assert.equal(calls, 4);
  assert.equal(view.errors.length, 3);
  assert.equal(view.synced.at(-1), input);
  view.unmount();
  assert.equal(view.timers.size, 0);
});

test("entering and returning to System starts collection without an artificial delay", async () => {
  let calls = 0;
  const view = fixture(async () => { calls += 1; return booted(); });
  view.render();
  assert.equal(calls, 1);
  await flush();
  view.render(false);
  await view.advance(240000);
  assert.equal(calls, 1, "other views must not continue polling");
  view.render();
  assert.equal(calls, 2, "returning to stale readings must request immediately");
  await flush();
  view.unmount();
});

test("returning while collection is pending reuses that flight and receives its fresh result", async () => {
  const flight = deferred();
  let calls = 0;
  const view = fixture(() => { calls += 1; return flight.promise; });
  view.render();
  await view.advance(5000);
  assert.equal(view.refreshing, true);
  view.render(false);
  assert.equal(view.refreshing, false);
  view.render();
  await view.advance(5000);
  assert.equal(calls, 1, "navigation must not submit a second native collection");
  assert.equal(view.refreshing, true);
  const fresh = booted();
  flight.resolve(fresh);
  await flush();
  assert.equal(view.synced.length, 1, "only the active view subscription receives the result");
  assert.equal(view.synced[0], fresh);
  assert.equal(view.refreshing, false);
  view.unmount();
  assert.equal(view.timers.size, 0);
});

test("failed collection ends Updating and remains retryable after navigation", async () => {
  const flight = deferred();
  let calls = 0;
  const view = fixture(() => ++calls === 1 ? flight.promise : Promise.resolve(booted()));
  view.render();
  assert.equal(view.refreshing, true);
  flight.reject(new Error("offline"));
  await flush();
  assert.equal(view.refreshing, false);
  assert.equal(view.errors.length, 1);
  view.render(false);
  view.render();
  await flush();
  assert.equal(view.synced.length, 1);
  assert.equal(view.refreshing, false);
  view.unmount();
});

test("an expired native wait joins collection once more without waiting a polling interval", async () => {
  const completion = deferred();
  let calls = 0;
  const view = fixture(() => ++calls === 1 ? Promise.resolve(booted(1_799_999_760_000)) : completion.promise);
  view.render();
  await flush();
  assert.equal(calls, 2);
  assert.equal(view.refreshing, true);
  assert.equal(view.synced.length, 0);
  completion.resolve(booted());
  await flush();
  assert.equal(view.synced.length, 1);
  assert.equal(view.refreshing, false);
  view.unmount();
});

test("expired responses cannot create an unbounded refresh loop", async () => {
  let calls = 0;
  const view = fixture(async () => { calls += 1; return booted(1_799_999_760_000); });
  view.render();
  await flush();
  assert.equal(calls, 2);
  assert.equal(view.refreshing, false);
  assert.equal(view.synced.length, 1, "staleness remains visible after the bounded attempt");
  await view.advance(59999);
  assert.equal(calls, 2);
  view.unmount();
});

test("foreground/background changes share the same pending collection", async () => {
  const flight = deferred();
  let calls = 0;
  const view = fixture(() => { calls += 1; return flight.promise; });
  view.render(true, 120000);
  view.render(true, 60000);
  assert.equal(calls, 1);
  flight.resolve(booted());
  await flush();
  assert.equal(view.synced.length, 1);
  assert.equal(view.refreshing, false);
  view.unmount();
});

test("retirement revision changes cannot reuse a response with old instance metadata", async () => {
  const old = deferred();
  const current = deferred();
  let calls = 0;
  const view = fixture(() => ++calls === 1 ? old.promise : current.promise);
  view.render(true, 60000, 0);
  view.render(false, 60000, 1);
  view.render(true, 60000, 2);
  assert.equal(calls, 2, "a new inventory revision needs a newly captured bootstrap");
  old.resolve(booted());
  await flush();
  assert.equal(view.synced.length, 0);
  assert.equal(view.refreshing, true, "the old request cannot clear the newer request's pending state");
  const latest = booted();
  latest.state.instances = [];
  current.resolve(latest);
  await flush();
  assert.equal(view.synced[0], latest);
  assert.equal(view.refreshing, false);
  view.unmount();
});

test("retirement keeps unrelated system observations and errors visible without reviving old instance metadata", () => {
  const requested = booted(1000);
  requested.state.instances = [{ id: "removed" }, { id: "other", status: "Stopped" }];
  let current = { ...requested, state: { ...requested.state, instances: [{ id: "other", status: "Running" }] } };
  let issue;
  const context = {
    useEffectEvent: (callback) => callback,
    retirement: { mergeInstances: (_current, incoming) => incoming },
    setBootstrap: (update) => { current = update(current); },
    setSystemRefreshIssue: (update) => { issue = typeof update === "function" ? update(issue) : update; },
    mergeBootstrapSnapshot: mergeSnapshots,
    formatDesktopError: (_t, error) => error.message,
    t: (key) => key
  };
  const sync = appFragment("const handleSystemViewSynced =", "const handleSystemPollingError =",
    "\nhandleSystemViewSynced;", context);
  const error = appFragment("const handleSystemPollingError =", "const handleRuntimeInstancesSynced =",
    "\nhandleSystemPollingError;", context);
  const response = { ...requested, state: { ...requested.state, snapshot: booted(2000).state.snapshot } };
  sync(response, 0, requested);
  assert.equal(current.state.snapshot.telemetry.observed_at_unix_ms, 2000);
  assert.deepEqual(current.state.instances, [{ id: "other", status: "Running" }]);
  error(new Error("old error"), 0);
  assert.equal(issue.message, "old error");
  assert.equal(pollingEnabled("system", true, false, true), true);
});

test("a pending request cannot accumulate probes and unmount discards its late result", async () => {
  const flight = deferred();
  let calls = 0;
  const view = fixture(() => { calls += 1; return flight.promise; });
  view.render();
  await view.advance(180000);
  assert.equal(calls, 1);
  view.unmount();
  flight.resolve(booted());
  await flush();
  assert.equal(view.synced.length, 0);
  assert.equal(view.errors.length, 0);
  assert.equal(view.timers.size, 0);
});

test("background telemetry keeps its 120-second cadence", async () => {
  let calls = 0;
  const view = fixture(async () => { calls += 1; return booted(); });
  view.render(true, 120000);
  await view.advance(119999);
  assert.equal(calls, 1);
  await view.advance(1);
  assert.equal(calls, 2);
  view.unmount();
});

test("cold counters receive one follow-up after the 15-second backend cache expires", async () => {
  let calls = 0;
  const view = fixture(async () => { calls += 1; return booted(undefined, { cpu: "warming_up" }); });
  view.render();
  await flush();
  assert.equal(calls, 1);
  await view.advance(15000);
  assert.equal(calls, 1, "the follow-up must not reuse the initial cached sample");
  await view.advance(1000);
  assert.equal(calls, 2, "warm counters should not stay blank for the entire 60-second interval");
  await view.advance(16000);
  assert.equal(calls, 2, "warming follow-ups must be bounded");
  await view.advance(28000);
  assert.equal(calls, 3, "the ordinary cadence remains active");
  view.unmount();
});

function mergeSnapshots(current, latest, requested) {
  const filename = path.join(__dirname, "../src/bootstrap-snapshot.ts");
  const exports = {};
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), { exports });
  return exports.mergeBootstrapSnapshot(current, latest, requested);
}

test("late bootstrap preserves independently updated jobs, modules and instances while accepting telemetry", () => {
  const requested = booted(1000);
  requested.state.jobs = [{ id: "download", status: "Running", progress: 10 }];
  requested.state.modules = [{ id: "game", install_state: "Installing" }];
  requested.state.instances = [{ id: "retired" }];
  const current = { ...requested, state: { ...requested.state,
    jobs: [{ id: "download", status: "Completed", progress: 100 }],
    modules: [{ id: "game", install_state: "Installed" }], instances: [] } };
  const latest = { ...requested, state: { ...requested.state, snapshot: booted(2000).state.snapshot } };
  const result = mergeSnapshots(current, latest, requested);
  for (const key of ["jobs", "modules", "instances"]) assert.equal(result.state[key], current.state[key]);
  assert.equal(result.state.snapshot, latest.state.snapshot);
  const next = { ...current, state: { ...current.state, instances: [{ id: "other", status: "Running" }] } };
  const fresh = mergeSnapshots(current, next, current);
  for (const key of ["jobs", "modules", "instances"]) assert.equal(fresh.state[key], next.state[key]);
  assert.equal(fresh.state.instances.some((item) => item.id === "retired"), false);
});

test("a late runtime list cannot revive a retired instance or undo a newer selection", () => {
  const requestedInstances = [{ id: "removed" }, { id: "other" }];
  let current = { ...booted(), state: { ...booted().state, instances: [{ id: "other" }] } };
  let selection = "other";
  const sync = appFragment("const handleRuntimeInstancesSynced =", "const handleRuntimeSelectionCleared =",
    "\nhandleRuntimeInstancesSynced;", {
      useEffectEvent: (fn) => fn,
      retirement: { isPending: () => false, mergeInstances: (_current, incoming) => incoming },
      getCurrentBootstrap: () => current,
      selectedInstanceCursor: { current: () => selection },
      setBootstrap: (update) => { current = update(current); },
      setSelectedInstanceId: (id) => { selection = id; }
    });
  sync({ instances: requestedInstances, selectedInstanceId: "removed", requestedInstances });
  assert.deepEqual(current.state.instances, [{ id: "other" }]);
  assert.equal(selection, "other");
});

test("ordinary bootstrap updates do not erase a measured snapshot", async () => {
  let current = booted();
  const measured = current.state.snapshot;
  const latest = booted();
  latest.state.settings.games_root = "E:/games";
  latest.state.snapshot = { running_instances: 2, network_adapters: [], telemetry: null };
  const reload = appFragment("async function reloadBootstrap(", "function clearSelectedInstancePanel()",
    "\nreloadBootstrap;", {
      retirement: { isPending: () => false, currentRevision: () => 0, mergeInstances: (_current, incoming) => incoming },
      getCurrentBootstrap: () => current,
      bootstrapApp: async () => latest,
      setBootstrap: (value) => { current = typeof value === "function" ? value(current) : value; },
      setSelectedModuleId: () => {}, setSelectedInstanceId: () => {}, setOverlays: () => {},
      mergeBootstrapSnapshot: mergeSnapshots
    });
  await reload();
  assert.equal(current.state.settings.games_root, "E:/games");
  assert.equal(current.state.snapshot.running_instances, 2);
  assert.equal(current.state.snapshot.telemetry, measured.telemetry);
  assert.equal(current.state.snapshot.network_adapters, measured.network_adapters);
});

test("system errors use their own feedback and never request a server pause", () => {
  let issue = null;
  let serverFailures = 0;
  const context = {
    useEffectEvent: (callback) => callback,
    retirement: { isPending: () => false, currentRevision: () => 0 },
    setSystemRefreshIssue: (update) => { issue = update(issue); },
    describeError: (error) => error.message,
    formatDesktopError: (_t, error) => error.message,
    t: (key) => key,
    markRuntimeRefreshFailed: () => { serverFailures += 1; }
  };
  const onError = appFragment("const handleSystemPollingError =", "const handleRuntimeInstancesSynced =",
    "\nhandleSystemPollingError;", context);
  for (let index = 0; index < 3; index += 1) onError(new Error("system unavailable"), 0);
  assert.equal(issue.consecutiveFailures, 3);
  assert.equal(issue.message, "system unavailable");
  assert.equal(serverFailures, 0);
  const boundError = appFragment("useSystemViewPolling({", "function handleResumeRuntimeAutoRefresh()",
    "", {
      useSystemViewPolling: (options) => options.onError,
      systemPollingEnabled: true, systemPollIntervalMs: 60000,
      retirement: { currentRevision: () => 0 },
      getCurrentBootstrap: () => booted(),
      handleSystemViewSynced: () => {}, handleSystemPollingError: onError,
      handleRuntimePollingError: () => { serverFailures += 1; }
    });
  assert.equal(boundError, onError, "the real App binding must use the isolated handler");
});

test("leaving the home page cancels an outstanding warm-up timer", async () => {
  let calls = 0;
  const view = fixture(async () => { calls += 1; return booted(undefined, { network: "warming_up" }); });
  view.render();
  await view.advance(5000);
  view.render(false);
  await view.advance(120000);
  assert.equal(calls, 1);
  assert.equal(view.timers.size, 0);
});

test("absent or disconnected adapters do not schedule a counter warm-up", async () => {
  for (const adapters of [null, undefined, [{ status: "Down", rate_status: "warming_up" }]]) {
    let calls = 0;
    const input = booted();
    input.state.snapshot.network_adapters = adapters;
    const view = fixture(async () => { calls += 1; return input; });
    view.render();
    await view.advance(59999);
    assert.equal(calls, 1);
    assert.equal(view.errors.length, 0);
    view.unmount();
  }
});

test("late cached responses cannot rewind observed time, but a new backend session can", () => {
  const current = booted(2000);
  const older = booted(1000);
  older.state.snapshot.running_instances = 4;
  assert.equal(mergeSnapshots(current, older).state.snapshot.telemetry.observed_at_unix_ms, 2000);
  assert.equal(mergeSnapshots(current, older).state.snapshot.running_instances, 4);
  const restarted = { ...older, booted_at_unix_ms: 124 };
  assert.equal(mergeSnapshots(current, restarted).state.snapshot.telemetry.observed_at_unix_ms, 1000);
  const newFailure = booted(3000, { network: "unavailable" });
  assert.equal(mergeSnapshots(current, newFailure).state.snapshot.telemetry.network, "unavailable");
});
