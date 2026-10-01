const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { createRequire } = require("node:module");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { InstanceSettingsSaveCoordinator } = require("../src/views/settings/instance-settings-save-coordinator.ts");
const settle = () => new Promise((resolve) => setImmediate(resolve));
function deferred() {
  let resolve, reject;
  const promise = new Promise((a, b) => { resolve = a; reject = b; });
  return { promise, resolve, reject };
}

// Deterministic React commit/effect and debounce host; persistence and both hooks are real.
function editor(owner, onSave) {
  const slots = [];
  const pending = [];
  const timers = new Map();
  let cursor = 0, nextTimer = 0;
  function memo(factory, deps) {
    const index = cursor++;
    if (!slots[index] || !deps.every((value, key) => Object.is(value, slots[index].deps[key]))) {
      slots[index] = { deps, value: factory() };
    }
    return slots[index].value;
  }
  function effect(setup, deps) {
    const index = cursor++;
    if (!slots[index] || !deps.every((value, key) => Object.is(value, slots[index].deps[key]))) {
      pending.push(() => {
        slots[index]?.cleanup?.();
        slots[index] = { deps, setup, cleanup: setup() };
      });
    }
  }
  const react = {
    useMemo: memo,
    useCallback: (callback, deps) => memo(() => callback, deps),
    useRef: (value) => memo(() => ({ current: value }), []),
    useState: (value) => memo(() => [value, () => {}], []),
    useEffect: effect,
    useLayoutEffect: effect
  };
  const filename = path.resolve(__dirname, "../src/views/settings/useAutoSaveInstanceSettings.ts");
  const actualRequire = createRequire(filename);
  const exports = {};
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports,
    require: (id) => id === "react" ? react
      : id === "./InstanceSettingsSaveContext" ? { useInstanceSettingsSaveCoordinator: () => owner }
        : actualRequire(id),
    window: { setTimeout: (callback) => { timers.set(++nextTimer, callback); return nextTimer; }, clearTimeout: (id) => timers.delete(id) }
  });
  const details = { summary: { id: "server-a", bind_ip: "0.0.0.0" }, settings_json: "base", auto_backup_on_stop: false, backup_retention_count: 3, ports: [] };
  return {
    render(settingsJson, disabled = false, ready = true) {
      cursor = 0;
      exports.useAutoSaveInstanceSettings({ details, bindIp: "0.0.0.0", autoBackupOnStop: false, backupRetentionCount: "3", settingsJson, disabled, ready, onSave });
      while (pending.length) pending.shift()();
    },
    fireTimers() { const callbacks = [...timers.values()]; timers.clear(); callbacks.forEach((callback) => callback()); },
    leave() { for (const slot of slots) slot?.cleanup?.(); },
    replayEffects() {
      for (const slot of slots) slot?.cleanup?.();
      for (const slot of slots) if (slot?.setup) slot.cleanup = slot.setup();
    }
  };
}

function actions(owner) {
  const events = [];
  const startCalls = [];
  const filename = path.resolve(__dirname, "../src/hooks/useDesktopActions.ts");
  const dependencies = {
    react: { useRef: (value) => ({ current: value }),
      useState: (initial) => [typeof initial === "function" ? initial() : initial, () => assert.fail("start must not mutate creation state")] },
    "../api": { startInstance: async (...args) => { startCalls.push(args); events.push(`start:${args[0]}`); }, logFrontendEvent: async () => {} },
    "../app-state": { describeError: (error) => error.message },
    "../app-ui": { message: (key, params) => ({ key, params }) },
    "./useSteamCmdActions": { useSteamCmdActions: () => assert.fail("instance actions must not start library hooks") },
    "../installation-cancellation": {},
    "../i18n": { useI18n: () => ({ t: (key) => key }) },
    "../server-start-error": { readModuleNotReadyError: () => null, serverStartFailureMessage: (error) => ({ error: error.message }) },
    "../views/settings/InstanceSettingsSaveContext": { useInstanceSettingsSaveCoordinator: () => owner },
    "../install-state-presentation": {}, "../instance-panel-refresh": {}
  };
  const exports = {};
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports, require: (id) => { assert.ok(Object.hasOwn(dependencies, id), id); return dependencies[id]; }
  });
  const instanceActions = exports.useInstanceActions({
    setActivity: (value) => { if (value.error) events.push(`error:${value.error}`); },
    reloadBootstrap: async () => {}, setSelectedInstanceId() {}, markRuntimeRefreshed() {}
  });
  return { start: instanceActions.handleStartServer, events, startCalls };
}

test("the final save barrier forwards the confirmed world snapshot to the backend unchanged", async () => {
  const owner = new InstanceSettingsSaveCoordinator();
  const save = deferred();
  const view = editor(owner, () => save.promise);
  view.render("latest");
  const action = actions(owner);
  const expected = { instance_id: "server-a", settings_json: "latest", shards: [] };
  const start = action.start("server-a", expected);
  await settle();
  assert.equal(action.startCalls.length, 0);
  save.resolve({ settings_json: "latest" });
  assert.equal(await start, true);
  assert.deepEqual(action.startCalls, [["server-a", expected]]);
  assert.equal(action.startCalls[0][1], expected);
  view.leave();
});

test("editing then immediately starting persists the draft before invoking start", async () => {
  const owner = new InstanceSettingsSaveCoordinator();
  const save = deferred();
  const writes = [];
  const view = editor(owner, (input) => { writes.push(input.settings_json); return save.promise; });
  view.render("latest");
  const action = actions(owner);
  const start = action.start("server-a");
  await settle();
  assert.deepEqual(action.events, [], "server must wait for the save, even before debounce fires");
  assert.deepEqual(writes, ["latest"]);
  save.resolve({ settings_json: "latest" });
  assert.equal(await start, true);
  assert.deepEqual(action.events, ["start:server-a"]);
  view.leave();
});

test("an edit arriving during the startup save barrier is persisted too", async () => {
  const owner = new InstanceSettingsSaveCoordinator();
  const first = deferred(), latest = deferred();
  const writes = [];
  const view = editor(owner, (input, options) => {
    writes.push([input.settings_json, options.expectedSettingsJson]);
    return writes.length === 1 ? first.promise : latest.promise;
  });
  view.render("first");
  view.fireTimers();
  await settle();
  const action = actions(owner);
  const start = action.start("server-a");
  view.render("latest");
  first.resolve({ settings_json: "first" });
  await settle();
  assert.deepEqual(action.events, []);
  assert.deepEqual(writes, [["first", "base"], ["latest", "first"]]);
  latest.resolve({ settings_json: "latest" });
  assert.equal(await start, true);
  view.leave();
});

test("leaving settings while saving still delays the real start action", async () => {
  const owner = new InstanceSettingsSaveCoordinator();
  const save = deferred();
  const view = editor(owner, () => save.promise);
  view.render("latest");
  view.leave();
  const action = actions(owner);
  const start = action.start("server-a");
  await settle();
  assert.deepEqual(action.events, []);
  save.resolve({ settings_json: "latest" });
  assert.equal(await start, true);
});

test("StrictMode effect replay does not retain a cancelled simulated closing save", async () => {
  const owner = new InstanceSettingsSaveCoordinator();
  const writes = [];
  const view = editor(owner, async (input) => {
    writes.push(input.settings_json);
    return { settings_json: input.settings_json };
  });
  view.render("latest");
  view.replayEffects();
  await settle();
  const action = actions(owner);
  assert.equal(await action.start("server-a"), true);
  assert.deepEqual(writes, ["latest"]);
  view.leave();
});

test("leaving settings during a start barrier waits for the final write without a stale editor error", async () => {
  const owner = new InstanceSettingsSaveCoordinator();
  const save = deferred();
  const view = editor(owner, () => save.promise);
  view.render("latest");
  const action = actions(owner);
  const start = action.start("server-a");
  await settle();
  view.leave();
  save.resolve({ settings_json: "latest" });
  assert.equal(await start, true);
  assert.deepEqual(action.events, ["start:server-a"]);
});

test("correcting an invalid draft allows the newly valid settings to save and start", async () => {
  const owner = new InstanceSettingsSaveCoordinator();
  const writes = [];
  const view = editor(owner, async (input) => {
    writes.push(input.settings_json);
    return { settings_json: input.settings_json };
  });
  const action = actions(owner);
  view.render("invalid", true);
  assert.equal(await action.start("server-a"), false);
  view.render("corrected");
  assert.equal(await action.start("server-a"), true);
  assert.deepEqual(writes, ["corrected"]);
  view.leave();
});

test("leaving before module metadata loads does not register an invalid or initialized draft", async () => {
  const owner = new InstanceSettingsSaveCoordinator();
  const writes = [];
  const view = editor(owner, async (input) => {
    writes.push(input.settings_json);
    return { settings_json: input.settings_json };
  });
  // Module initialization can change the projection even though controls remain disabled.
  view.render("initialized projection", true, false);
  view.leave();
  const action = actions(owner);
  assert.equal(await action.start("server-a"), true);
  assert.deepEqual(writes, [], "an unavailable editor cannot commit an unvalidated projection");
  assert.deepEqual(action.events, ["start:server-a"]);
});

test("metadata readiness activates autosave without requiring another field edit", async () => {
  const owner = new InstanceSettingsSaveCoordinator();
  const writes = [];
  const view = editor(owner, async (input) => {
    writes.push(input.settings_json);
    return { settings_json: input.settings_json };
  });
  view.render("initialized projection", true, false);
  view.fireTimers();
  await settle();
  assert.deepEqual(writes, []);
  view.render("initialized projection", false, true);
  const action = actions(owner);
  assert.equal(await action.start("server-a"), true);
  assert.deepEqual(writes, ["initialized projection"]);
  view.leave();
});

test("temporary metadata unavailability cannot clear an already invalid edited draft", async () => {
  const owner = new InstanceSettingsSaveCoordinator();
  const view = editor(owner, () => assert.fail("invalid edits must never persist"));
  view.render("invalid edited draft", true, true);
  view.render("unavailable projection", true, false);
  view.leave();
  const action = actions(owner);
  assert.equal(await action.start("server-a"), false);
  assert.match(action.events.at(-1), /invalid/);
});

for (const reason of ["disk is read-only", "precondition failed", "invalid"]) {
  for (const leave of [false, true]) {
    test(`${reason} blocks start ${leave ? "after leaving" : "inside"} settings`, async () => {
      const owner = new InstanceSettingsSaveCoordinator();
      const view = editor(owner, () => { throw new Error(reason); });
      view.render("latest", reason === "invalid");
      if (leave) view.leave();
      const action = actions(owner);
      assert.equal(await action.start("server-a"), false);
      assert.ok(action.events.every((event) => !event.startsWith("start:")));
      assert.match(action.events.at(-1), new RegExp(reason));
      if (!leave) view.leave();
      await settle();
    });
  }
}
