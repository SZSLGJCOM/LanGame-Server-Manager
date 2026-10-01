const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
const { programCleanupDetails, emptyProgramCleanup } = require("./helpers/program-cleanup-fixture.cjs");

function load(relative, dependencies) {
  const filename = path.join(__dirname, "../src", relative);
  const exports = {};
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports, require: (id) => {
      assert.ok(Object.hasOwn(dependencies, id), `Unexpected dependency ${id}`);
      return dependencies[id];
    }
  }, { filename });
  return exports;
}

function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

test("retirement protects only the owned instance while merging unrelated runtime updates", () => {
  const react = { useRef: (value) => ({ current: value }), useCallback: (fn) => fn,
    useState: (initial) => [initial(), () => {}] };
  const retirement = load("hooks/useInstanceRetirement.ts", { react }).useInstanceRetirement();
  retirement.begin("a", "archive");
  const a = { id: "a", status: "Stopped" }, b = { id: "b", status: "Stopped" };
  const latestB = { id: "b", status: "Running" };
  const merged = retirement.mergeInstances([a, b], [latestB]);
  assert.equal(merged.find((item) => item.id === "a"), a);
  assert.equal(merged.find((item) => item.id === "b"), latestB);
  assert.equal(retirement.mergeInstances([b], [a, latestB]).some((item) => item.id === "a"), false);
  assert.equal(retirement.mergeInstances([a, b], [latestB], "a").some((item) => item.id === "a"), false);
  retirement.finish("a");
  const after = retirement.mergeInstances(merged, [latestB]);
  assert.equal(after.some((item) => item.id === "a"), false, "completed retirement cannot retain its earlier protected card");
  assert.equal(after[0], latestB);
});

function harness(kind) {
  const react = {
    useRef: (value) => ({ current: value }),
    useState: (initial) => [typeof initial === "function" ? initial() : initial, () => {}],
    useCallback: (callback) => callback
  };
  const retirement = load("hooks/useInstanceRetirement.ts", { react }).useInstanceRetirement();
  const native = deferred();
  const refreshed = deferred();
  const refreshStarted = deferred();
  const state = {
    selectedId: "a", panel: "a", nativeCalls: 0,
    bootstrap: { state: { instances: [{ id: "a" }, { id: "b" }] } },
    details: { a: {}, b: {} }, backups: { a: [], b: [] }, runtimes: { a: {}, b: {} }, activities: []
  };
  const invoke = (id) => {
    assert.equal(retirement.isPending(id), true, "retirement owns the instance before native IPC starts");
    state.nativeCalls++;
    return native.promise;
  };
  const actions = load("hooks/useDesktopActions.ts", {
    react,
    "../api": { archiveInstance: invoke, deleteInstance: invoke },
    "../app-state": { describeError: (error) => error.message },
    "../app-ui": { message: (key, params) => ({ key, params }), programCleanupDetails },
    "../i18n": { useI18n: () => ({ t: (key, params) => `${key}: ${JSON.stringify(params)}` }) },
    "./useSteamCmdActions": {}, "../installation-cancellation": {}, "../install-state-presentation": {},
    "../instance-panel-refresh": {}, "../server-start-error": {},
    "../views/settings/InstanceSettingsSaveContext": { useInstanceSettingsSaveCoordinator: () => ({}) }
  }).useInstanceActions({
    retirement,
    getCurrentInstanceId: () => state.selectedId,
    setSelectedInstanceId: (id) => { state.selectedId = id; },
    clearSelectedInstancePanel: () => { state.panel = null; },
    setBootstrap: (update) => { state.bootstrap = update(state.bootstrap); },
    setInstanceDetailsById: (update) => { state.details = update(state.details); },
    setInstanceBackupsById: (update) => { state.backups = update(state.backups); },
    setInstanceRuntimesById: (update) => { state.runtimes = update(state.runtimes); },
    setActivity: (activity) => state.activities.push(activity),
    reloadBootstrap: () => { refreshStarted.resolve(); return refreshed.promise; }
  });
  const result = { instance_name: "A", previous_instance_root: "fixture/a", archived_instance_root: "fixture/archive/a",
    saves_archived_with_instance_root: true, program_cleanup: emptyProgramCleanup() };
  return { state, retirement, native, refreshed, refreshStarted, result,
    execute: () => kind === "delete" ? actions.handleDeleteInstance("a") : actions.handleArchiveInstance("a") };
}

for (const kind of ["delete", "archive"]) {
  test(`${kind}: pending spans native cleanup and refresh, and duplicate submission does not run twice`, async () => {
    const h = harness(kind);
    const revision = h.retirement.currentRevision();
    const pending = h.execute();
    assert.equal(h.retirement.isPending("a"), true);
    assert.notEqual(h.retirement.currentRevision(), revision);
    assert.deepEqual(h.state.bootstrap.state.instances.map(({ id }) => id), ["a", "b"]);
    await h.execute();
    assert.equal(h.state.nativeCalls, 1);
    h.native.resolve(h.result);
    await h.refreshStarted.promise;
    assert.equal(h.retirement.isPending("a"), true);
    assert.deepEqual(h.state.bootstrap.state.instances.map(({ id }) => id), ["b"]);
    assert.equal(h.state.panel, null);
    assert.equal(h.state.selectedId, null);
    assert.equal(Object.hasOwn(h.state.details, "a"), false);
    h.refreshed.resolve();
    await pending;
    assert.equal(h.retirement.isPending(), false);
    assert.match(h.state.activities.at(-1).key, /instance(?:Deleted|Archived)$/);
  });

  test(`${kind}: completion preserves a later selection and still removes the card when refresh fails`, async () => {
    const h = harness(kind);
    const pending = h.execute();
    h.state.selectedId = "b";
    h.state.panel = "b";
    h.native.resolve(h.result);
    await h.refreshStarted.promise;
    h.refreshed.reject(new Error("Inventory temporarily unavailable"));
    await pending;
    assert.equal(h.state.selectedId, "b");
    assert.equal(h.state.panel, "b");
    assert.deepEqual(h.state.bootstrap.state.instances.map(({ id }) => id), ["b"]);
    assert.equal(h.state.activities.at(-1).key, "activity.instanceRemovalRefreshFailed");
    assert.equal(h.retirement.isPending(), false);
  });

  test(`${kind}: actual failure remains actionable and releases pending only after reconciliation`, async () => {
    const h = harness(kind);
    const error = new Error("Owned file could not be removed");
    const pending = h.execute();
    const rejected = assert.rejects(pending, (actual) => Object.is(actual, error));
    h.native.reject(error);
    await h.refreshStarted.promise;
    assert.equal(h.retirement.isPending("a"), true);
    assert.equal(h.state.selectedId, "a");
    assert.equal(Object.hasOwn(h.state.details, "a"), true);
    h.refreshed.resolve();
    await rejected;
    assert.equal(h.retirement.isPending(), false);
    assert.equal(h.state.activities.at(-1).params.message, error.message);
  });
}
