const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
const { programCleanupDetails, emptyProgramCleanup } = require("./helpers/program-cleanup-fixture.cjs");

function compileTypeScript(module, filename) {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
}

require.extensions[".ts"] = compileTypeScript;
require.extensions[".tsx"] = compileTypeScript;

const {
  LibraryInstallationReader,
  mergeModuleSummaries,
  mergeModuleProgramCounts,
  updateModuleDetailsSummary,
  applyInstallationSnapshot,
  createForegroundInstallationPolling
} = require(path.join(__dirname, "../src/library-installation-refresh.ts"));

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

const flush = () => new Promise((resolve) => setImmediate(resolve));
const moduleSummary = (state) => ({ id: "pz", name: "PZ", version: "1", install_state: state, supported_platforms: ["windows"] });
const preview = (id, exists) => ({ launchPlan: { instance_id: id, executable_exists: exists }, launchPlanError: null });
const installedSnapshot = () => ({ modules: [moduleSummary("Installed")], instanceId: "a", preview: preview("a", true) });

function harness() {
  let modules = [moduleSummary("Installed")];
  let details = { summary: modules[0], process: { executable: "java.exe" } };
  let selectedId = "a";
  let selectedPreview = preview("a", true);
  return {
    get modules() { return modules; },
    get details() { return details; },
    get selectedPreview() { return selectedPreview; },
    select(id) { selectedId = id; selectedPreview = preview(id, true); },
    ports: {
      getCurrentInstanceId: () => selectedId,
      onModules: (next) => { modules = mergeModuleSummaries(modules, next); details = updateModuleDetailsSummary(details, next); },
      onPreview: (next) => { selectedPreview = next; }
    }
  };
}

test("external deletion and restoration update the catalog, open details and preview together", async () => {
  const state = harness();
  let exists = false;
  const reader = new LibraryInstallationReader({
    getCurrentInstanceId: state.ports.getCurrentInstanceId,
    readModules: async () => [moduleSummary(exists ? "Installed" : "NotInstalled")],
    readPreview: async (id) => preview(id, exists)
  });
  applyInstallationSnapshot(await reader.read(), state.ports);
  assert.equal(state.modules[0].install_state, "NotInstalled");
  assert.equal(state.details.summary.install_state, "NotInstalled");
  assert.equal(state.selectedPreview.launchPlan.executable_exists, false);

  exists = true;
  applyInstallationSnapshot(await reader.read(), state.ports);
  assert.equal(state.modules[0].install_state, "Installed");
  assert.equal(state.details.summary.install_state, "Installed");
  assert.equal(state.selectedPreview.launchPlan.executable_exists, true);
});

test("unchanged snapshots preserve module and details identities", () => {
  const state = harness();
  const modules = state.modules;
  const details = state.details;
  state.ports.onModules([moduleSummary("Installed")]);
  assert.equal(state.modules, modules);
  assert.equal(state.details, details);
});

test("retained program counts refresh independently of library installation state", () => {
  const state = harness();
  state.ports.onModules([{ ...moduleSummary("NotInstalled"), instance_program_count: 1, archived_program_count: 0 }]);
  const activeModules = state.modules;
  const activeDetails = state.details;
  state.ports.onModules([{ ...moduleSummary("NotInstalled"), instance_program_count: 0, archived_program_count: 1 }]);
  assert.notEqual(state.modules, activeModules);
  assert.notEqual(state.details, activeDetails);
  assert.equal(state.details.summary.instance_program_count, 0);
  assert.equal(state.details.summary.archived_program_count, 1);
  assert.equal(state.modules[0].install_state, "NotInstalled");
  const archivedModules = state.modules;
  state.ports.onModules([{ ...moduleSummary("NotInstalled"), archived_program_count: 1 }]);
  assert.equal(state.modules, archivedModules, "absent count and zero are equivalent");
});

test("light installation snapshots retain known counts in both catalog and selected details", () => {
  const state = harness();
  state.ports.onModules([{ ...moduleSummary("NotInstalled"), instance_program_count: 2, archived_program_count: 3 }]);
  state.ports.onModules([moduleSummary("Installed")]);
  assert.equal(state.modules[0].install_state, "Installed");
  assert.equal(state.modules[0].instance_program_count, 2);
  assert.equal(state.modules[0].archived_program_count, 3);
  assert.equal(state.details.summary.instance_program_count, 2);
  assert.equal(state.details.summary.archived_program_count, 3);
});

test("late inventory results update only counts without replacing current module state or membership", () => {
  const current = [{ ...moduleSummary("Updating"), instance_program_count: 2, archived_program_count: 3 },
    { ...moduleSummary("Installing"), id: "new-game" }];
  const next = mergeModuleProgramCounts(current, [
    { ...moduleSummary("NotInstalled"), instance_program_count: 1, archived_program_count: 4 },
    { ...moduleSummary("Installed"), id: "removed-game" }
  ]);
  assert.deepEqual(next.map((module) => module.id), ["pz", "new-game"]);
  assert.equal(next[0].install_state, "Updating");
  assert.equal(next[0].instance_program_count, 1);
  assert.equal(next[0].archived_program_count, 4);
  assert.equal(next[1], current[1]);
  assert.equal(mergeModuleProgramCounts(next, [next[0]]), next);
});

test("installation hook keeps one background count request while status refreshes continue and discards disposed counts", async () => {
  const filename = path.join(__dirname, "../src/hooks/useLibraryInstallationRefresh.ts");
  const exports = {};
  const effects = [], reads = [], applied = [], counts = [], failures = [];
  const pendingCounts = deferred();
  const dependencies = {
    react: { useCallback: (callback) => callback, useRef: (value) => ({ current: value }),
      useEffect: (effect) => effects.push(effect) },
    "../api": { refreshModules: (options) => {
      const includeCounts = options?.includePreservedProgramCounts !== false;
      reads.push(includeCounts);
      return includeCounts ? pendingCounts.promise : Promise.resolve([moduleSummary("Installed")]);
    } },
    "../app-state": { loadLaunchPlanPreview: async () => preview("a", true) },
    "../library-installation-refresh": require("../src/library-installation-refresh.ts")
  };
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports, require: (id) => {
      assert.ok(Object.hasOwn(dependencies, id), `unexpected dependency ${id}`);
      return dependencies[id];
    }
  }, { filename });
  const refresh = exports.useLibraryInstallationRefresh({ enabled: false, getCurrentInstanceId: () => null,
    onModules: (value) => applied.push(value), onProgramCounts: (value) => counts.push(value),
    onPreview: () => {}, onError: (error) => failures.push(error) });
  const cleanups = effects.map((effect) => effect());
  await refresh(); await refresh(); await refresh();
  assert.deepEqual(reads, [false, true, false, false], "the queued count reader cannot block or multiply behind status scans");
  assert.equal(applied.length, 3);
  assert.equal(counts.length, 0);
  for (const cleanup of cleanups) cleanup?.();
  pendingCounts.resolve([{ ...moduleSummary("NotInstalled"), archived_program_count: 4 }]);
  await flush();
  assert.deepEqual(counts, []);
  assert.deepEqual(failures, []);
});

test("installation settlement queues one fresh read after an older in-flight poll", async () => {
  const first = deferred();
  const state = harness();
  let calls = 0;
  const reader = new LibraryInstallationReader({
    getCurrentInstanceId: state.ports.getCurrentInstanceId,
    readModules: () => ++calls === 1 ? first.promise : Promise.resolve([moduleSummary("Installed")]),
    readPreview: async (id) => preview(id, calls > 1)
  });
  const stalePoll = reader.read();
  const settled = reader.read();
  const refocused = reader.read();
  assert.equal(settled, refocused, "only one follow-up request is queued");
  assert.equal(calls, 1);
  first.resolve([moduleSummary("NotInstalled")]);
  await stalePoll;
  applyInstallationSnapshot(await settled, state.ports);
  assert.equal(calls, 2);
  assert.equal(state.selectedPreview.launchPlan.executable_exists, true, "repair clears the old warning without server navigation");
});

test("a late installation snapshot updates modules without replacing another instance preview", async () => {
  const pending = deferred();
  const state = harness();
  const reader = new LibraryInstallationReader({
    getCurrentInstanceId: state.ports.getCurrentInstanceId,
    readModules: async () => [moduleSummary("NotInstalled")],
    readPreview: () => pending.promise
  });
  const flight = reader.read();
  state.select("b");
  pending.resolve(preview("a", false));
  applyInstallationSnapshot(await flight, state.ports);
  assert.equal(state.modules[0].install_state, "NotInstalled");
  assert.equal(state.selectedPreview.launchPlan.instance_id, "b");
});

test("manual preview success and error cannot overwrite a later selection", async () => {
  for (const result of [preview("a", false), { launchPlan: null, launchPlanError: "missing" }]) {
    const pending = deferred();
    const state = harness();
    const reader = new LibraryInstallationReader({
      getCurrentInstanceId: state.ports.getCurrentInstanceId,
      readModules: async () => [moduleSummary("Installed")],
      readPreview: () => pending.promise
    });
    const activities = [];
    const actions = loadDesktopActions({}).useInstanceActions({
      getCurrentInstanceId: state.ports.getCurrentInstanceId,
      refreshInstallationState: async () => {
        const snapshot = await reader.read();
        applyInstallationSnapshot(snapshot, state.ports);
        return snapshot;
      },
      setActivity: (activity) => activities.push(activity.key)
    });
    const flight = actions.handleRefreshLaunchPreview();
    state.select("b");
    pending.resolve(result);
    await flight;
    assert.equal(state.selectedPreview.launchPlan.instance_id, "b");
    assert.deepEqual(activities, ["activity.generatingLaunchPreview"]);
  }
});

test("manual preview queues behind an old automatic snapshot and remains the final same-instance result", async () => {
  const pendingModules = deferred();
  const state = harness();
  let calls = 0;
  const writes = [];
  const reader = new LibraryInstallationReader({
    getCurrentInstanceId: state.ports.getCurrentInstanceId,
    readModules: () => ++calls === 1 ? pendingModules.promise : Promise.resolve([moduleSummary("Installed")]),
    readPreview: async (id) => preview(id, calls > 1)
  });
  const apply = (snapshot) => {
    applyInstallationSnapshot(snapshot, state.ports);
    writes.push(state.selectedPreview.launchPlan.executable_exists);
    return snapshot;
  };
  const automatic = reader.read().then(apply);
  const actions = loadDesktopActions({}).useInstanceActions({
    getCurrentInstanceId: state.ports.getCurrentInstanceId,
    refreshInstallationState: () => reader.read().then(apply),
    setActivity: () => undefined
  });
  const manual = actions.handleRefreshLaunchPreview();
  await flush();
  assert.equal(calls, 1, "manual preview must not bypass the pending automatic snapshot");
  pendingModules.resolve([moduleSummary("NotInstalled")]);
  await Promise.all([automatic, manual]);
  assert.equal(calls, 2);
  assert.deepEqual(writes, [false, true]);
  assert.equal(state.selectedPreview.launchPlan.executable_exists, true);
});

test("foreground polling checks immediately, serializes focus, and discards hidden late results", async () => {
  const tasks = new Map();
  let nextHandle = 0;
  const pending = deferred();
  const received = [];
  let calls = 0;
  const polling = createForegroundInstallationPolling({
    read: () => { calls += 1; return pending.promise; },
    onValue: (value) => received.push(value),
    onError: assert.fail,
    schedule: (callback, delay) => { const handle = ++nextHandle; tasks.set(handle, { callback, delay }); return handle; },
    cancel: (handle) => tasks.delete(handle)
  });
  polling.setForeground(true);
  polling.setForeground(true);
  assert.equal(calls, 1);
  polling.setForeground(false);
  pending.resolve("hidden response");
  await flush();
  assert.deepEqual(received, []);
  assert.equal(tasks.size, 0);
  polling.setForeground(true);
  await flush();
  assert.equal(calls, 2);
  assert.deepEqual(received, ["hidden response"]);
  assert.equal([...tasks.values()][0].delay, 15000);
  polling.dispose();
  assert.equal(tasks.size, 0);
});

test("a failed scan waits for the pending preview before starting its queued retry", async () => {
  const pending = deferred();
  let calls = 0;
  const reader = new LibraryInstallationReader({
    getCurrentInstanceId: () => "a",
    readModules: () => ++calls === 1 ? Promise.reject(new Error("scan failed")) : Promise.resolve([moduleSummary("Installed")]),
    readPreview: () => calls === 1 ? pending.promise : Promise.resolve(preview("a", true))
  });
  const first = reader.read();
  const rejected = assert.rejects(first, /scan failed/);
  const next = reader.read();
  await flush();
  assert.equal(calls, 1);
  pending.resolve(preview("a", false));
  await rejected;
  assert.equal((await next).modules[0].install_state, "Installed");
  assert.equal(calls, 2);
});

function loadDesktopActions(api) {
  const filename = path.join(__dirname, "../src/hooks/useDesktopActions.ts");
  const exports = {};
  const dependencies = {
    react: {
      useRef: (value) => ({ current: value }),
      useState(initial) {
        let value = typeof initial === "function" ? initial() : initial;
        return [value, (update) => { value = typeof update === "function" ? update(value) : update; }];
      }
    },
    "../api": api,
    "../app-state": { describeError: (error) => error.message },
    "../app-ui": { message: (key, params, extra) => ({ key, params, ...extra }), programCleanupDetails },
    "../steamcmd-ui": {},
    "./useSteamCmdActions": { useSteamCmdActions: () => ({}) },
    "../i18n": { useI18n: () => ({ t: (key) => key }) },
    "../install-state-presentation": { formatInstallState: (value) => value },
    "../installation-cancellation": require("../src/installation-cancellation.ts"),
    "../instance-panel-refresh": {},
    "../server-start-error": require("../src/server-start-error.ts"),
    "../library-installation-refresh": {},
    "../views/settings/InstanceSettingsSaveContext": { useInstanceSettingsSaveCoordinator: () => ({ flush: async () => {} }) }
  };
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports,
    require: (id) => {
      assert.ok(Object.hasOwn(dependencies, id), `unexpected dependency ${id}`);
      return dependencies[id];
    }
  }, { filename });
  return exports;
}

test("partial library uninstall exposes cleanup details instead of reporting the retained library state as completion", async () => {
  const activities = [];
  const { useLibraryActions } = loadDesktopActions({ uninstallModule: async () => ({
    module_id: "pz", install_state: "Installed", executable_exists: true,
    cleanup: { removed_install_roots: ["fixture/extra"], preserved_data_paths: [],
      retained_installs: [{ install_root: "fixture/base", reason: "archive_dependency" }] }
  }) });
  const actions = useLibraryActions({ setActivity: (value) => activities.push(value), setLibraryTaskPolling: () => {},
    reloadBootstrap: async () => {}, refreshInstallationState: async () => installedSnapshot(), refreshSteamCmdStatus: async () => {} });
  await actions.handleUninstallModule("pz");
  const completed = activities.find((value) => value.key === "activity.uninstallCompletedWithRetained");
  assert.equal(completed?.params.count, 1);
  assert.equal(completed?.tone, "warning");
  assert.ok(completed.params.details);
  assert.equal(activities.some((value) => value.key === "activity.uninstallCompleted"), false);
});

for (const operation of ["install", "validate", "uninstall"]) {
  for (const fails of [false, true]) {
    test(`${operation} ${fails ? "failure" : "success"} awaits fresh installation and preview state`, async () => {
      const mutation = deferred();
      const refresh = deferred();
      const events = [];
      const { useLibraryActions } = loadDesktopActions({
        installModule: () => mutation.promise,
        uninstallModule: () => mutation.promise
      });
      const actions = useLibraryActions({
        refreshSteamCmdStatus: async () => { events.push("steamcmd"); },
        setActivity: (value) => events.push(value.key),
        setLibraryTaskPolling: (enabled) => events.push(`polling:${enabled}`),
        refreshInstallationState: () => { events.push("refresh"); return refresh.promise; },
        reloadBootstrap: async (preferred) => {
          assert.equal(preferred, undefined, "bootstrap must preserve the current selection");
          events.push("bootstrap");
        },
        setSelectedModuleId: () => assert.fail("a late mutation must preserve the current module selection")
      });
      const flight = operation === "uninstall" ? actions.handleUninstallModule("pz") : actions.handleInstallModule("pz", operation === "validate");
      if (fails) mutation.reject(new Error("installation failed"));
      else mutation.resolve({ install_state: "Installed", cleanup: emptyProgramCleanup() });
      await flush();
      assert.ok(events.includes("refresh"));
      assert.ok(events.indexOf("bootstrap") < events.indexOf("refresh"), "refresh jobs before applying the fresh filesystem snapshot");
      assert.equal(events.includes("polling:false"), false, "keep tracking until the state refresh finishes");
      refresh.resolve(installedSnapshot());
      await flight;
      assert.ok(events.includes("steamcmd"), "success and failure both refresh the SteamCMD dependency");
      assert.equal(events.at(-1), "polling:false");
    });
  }
}

test("installation still refreshes file and preview state when bootstrap refresh fails", async () => {
  const events = [];
  const { useLibraryActions } = loadDesktopActions({ installModule: async () => ({ install_state: "Installed" }) });
  const actions = useLibraryActions({
    refreshSteamCmdStatus: async () => { events.push("steamcmd"); },
    setActivity: (value) => events.push(value),
    setLibraryTaskPolling: () => undefined,
    reloadBootstrap: async () => { throw new Error("bootstrap unavailable"); },
    refreshInstallationState: async () => { events.push("fresh snapshot"); return installedSnapshot(); }
  });
  await actions.handleInstallModule("pz", false);
  assert.ok(events.includes("fresh snapshot"));
  assert.ok(events.includes("steamcmd"));
  assert.equal(events.at(-1).key, "activity.refreshLibraryFailed");
  assert.equal(events.at(-1).params.message, "bootstrap unavailable");
});

test("backend cancellation reports a normal stopped result and still refreshes installation state", async () => {
  const events = [];
  const { useLibraryActions } = loadDesktopActions({ installModule: async () => { throw "installation_cancelled"; } });
  const actions = useLibraryActions({
    refreshSteamCmdStatus: async () => { events.push("steamcmd"); },
    setActivity: (value) => events.push(value.key),
    setLibraryTaskPolling: (enabled) => events.push(`polling:${enabled}`),
    reloadBootstrap: async () => { events.push("bootstrap"); },
    refreshInstallationState: async () => { events.push("refresh"); return installedSnapshot(); }
  });
  await actions.handleInstallModule("pz", false);
  assert.ok(events.includes("activity.installationStopped"));
  assert.equal(events.includes("activity.installFailed"), false);
  assert.equal(events.includes("activity.installCompleted"), false);
  assert.ok(events.includes("refresh"));
  assert.equal(events.at(-1), "polling:false");
});

test("details polling skips unchanged summaries but reloads schema and process fields for a new summary snapshot", async () => {
  const filename = path.join(__dirname, "../src/hooks/useDesktopEffects.ts");
  const exports = {};
  let dependencies = null;
  let cleanup;
  let version = "before";
  let reads = 0;
  let loaded;
  const modules = [moduleSummary("Installed")];
  const injected = {
    react: { useEffect: (effect, nextDependencies) => {
      if (dependencies && dependencies.every((item, index) => item === nextDependencies[index])) return;
      cleanup?.();
      dependencies = nextDependencies;
      cleanup = effect();
    } },
    "../api": { readModuleDetails: async (_id, options) => {
      assert.equal(options.includePreservedProgramCounts, false, "configuration must not wait for unrelated archive inventory");
      reads += 1;
      return { summary: moduleSummary("Installed"), schema_json: version, process: { executable: version } };
    } },
    "../app-state": {},
    "../instance-panel-loader": {},
    "../bootstrap-initialization": { createBootstrapInitializationCoordinator: () => ({}) },
    "../domain/single-flight-poller": {},
    "../domain/system-resources": {}
  };
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports,
    require: (id) => {
      assert.ok(Object.hasOwn(injected, id), `unexpected dependency ${id}`);
      return injected[id];
    }
  }, { filename });
  const render = (nextModules) => exports.useSelectedModuleDetailsSync({
    enabled: true, selectedModuleId: "pz", modules: nextModules, onLoaded: (details) => { loaded = details; }
  });
  render(modules);
  await flush();
  version = "after";
  render(mergeModuleSummaries(modules, [moduleSummary("Installed")]));
  await flush();
  assert.equal(reads, 1, "automatic unchanged summaries must not reload details");
  render([moduleSummary("Installed")]);
  await flush();
  assert.equal(reads, 2);
  assert.equal(loaded.schema_json, "after");
  assert.equal(loaded.process.executable, "after");
  cleanup?.();
});
