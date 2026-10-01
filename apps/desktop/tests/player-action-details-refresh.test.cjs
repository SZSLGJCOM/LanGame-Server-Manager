const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((accept, fail) => { resolve = accept; reject = fail; });
  return { promise, resolve, reject };
}

function loadModule(relativePath, dependencies = {}) {
  const filename = path.join(__dirname, "../src", relativePath);
  const exports = {};
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports, require: (id) => {
      assert.ok(Object.hasOwn(dependencies, id), `unexpected dependency ${id}`);
      return dependencies[id];
    }
  }, { filename });
  return exports;
}

function harness() {
  const command = deferred();
  const panel = deferred();
  const refreshStarted = deferred();
  const input = { instance_id: "a", snapshot_id: "snapshot-a", player_key: "alice", action_id: "ban_player" };
  const result = { action_id: "ban_player", status: "sent", executed_at_unix_ms: 1, summary: "sent" };
  const refreshed = { details: { summary: { id: "a" }, settings_json: '{"blacklist_entries":[{"platform":"Steam","userid":"76561198000000001"}]}' } };
  const state = { selectedId: "a", selected: null, cache: {}, refreshes: [], activity: null, bootstrapRefreshes: 0 };
  const dependencies = {
    react: { useRef: (value) => ({ current: value }), useState: (initial) => [typeof initial === "function" ? initial() : initial, () => {}] },
    "../api": { executeInstancePlayerAction: (value) => { assert.deepEqual(value, input); return command.promise; } },
    "../app-state": { describeError: (error) => error.message, loadInstancePanelData: (id) => {
      state.refreshes.push(id); refreshStarted.resolve(); return panel.promise;
    } },
    "../app-ui": { message: (key, params) => ({ key, params }) },
    "./useSteamCmdActions": {}, "../installation-cancellation": {}, "../install-state-presentation": {}, "../server-start-error": {},
    "../i18n": { useI18n: () => ({ t: (key, params) => `${key}: ${params.error}` }) },
    "../instance-panel-refresh": loadModule("instance-panel-refresh.ts"),
    "../views/settings/InstanceSettingsSaveContext": { useInstanceSettingsSaveCoordinator: () => ({}) }
  };
  const actions = loadModule("hooks/useDesktopActions.ts", dependencies).useInstanceActions({
    getCurrentInstanceId: () => state.selectedId,
    cacheInstancePanel: (id, value) => { state.cache[id] = value; },
    replaceSelectedInstancePanel: (value) => { state.selected = value; },
    reloadBootstrap: async () => { state.bootstrapRefreshes++; },
    setActivity: (value) => { state.activity = value; }
  });
  return { state, command, panel, refreshStarted, result, refreshed, execute: () => actions.handleExecutePlayerAction(input) };
}

test("online action waits for authoritative details and publishes the newly persisted roster", async () => {
  const h = harness();
  let completed = false;
  const pending = h.execute().then((result) => { completed = true; return result; });
  assert.deepEqual(h.state.refreshes, []);
  h.command.resolve(h.result);
  await h.refreshStarted.promise;
  assert.equal(completed, false);
  assert.deepEqual(h.state.refreshes, ["a"]);
  h.panel.resolve(h.refreshed);
  assert.equal(await pending, h.result);
  assert.equal(h.state.selected, h.refreshed);
  assert.equal(h.state.cache.a, h.refreshed);
  assert.equal(h.state.bootstrapRefreshes, 1);
});

test("online action refresh caches its original instance without replacing a later selection", async () => {
  const h = harness();
  const pending = h.execute();
  h.command.resolve(h.result);
  await h.refreshStarted.promise;
  h.state.selectedId = "b";
  const otherPanel = { details: { summary: { id: "b" } } };
  h.state.selected = otherPanel;
  h.panel.resolve(h.refreshed);
  await pending;
  assert.equal(h.state.cache.a, h.refreshed);
  assert.equal(h.state.selected, otherPanel);
});

test("native readback failure still refreshes authoritative details and preserves the original rejection", async () => {
  const h = harness();
  const failure = new Error("Ban sent but native roster readback failed");
  const pending = h.execute();
  h.command.reject(failure);
  await h.refreshStarted.promise;
  h.panel.resolve(h.refreshed);
  await assert.rejects(pending, (error) => error === failure);
  assert.equal(h.state.selected, h.refreshed);
});

test("a second refresh failure cannot replace the original player action failure", async () => {
  const h = harness();
  const failure = new Error("Native readback failed");
  const pending = h.execute();
  h.command.reject(failure);
  await h.refreshStarted.promise;
  h.panel.reject(new Error("Details unavailable"));
  await assert.rejects(pending, (error) => error === failure);
  assert.equal(h.state.activity.key, "activity.playerAccessRefreshFailed");
  assert.equal(h.state.activity.params.message, "Details unavailable");
});

test("successful dispatch with failed details refresh reports partial failure instead of success", async () => {
  const h = harness();
  const pending = h.execute();
  h.command.resolve(h.result);
  await h.refreshStarted.promise;
  h.panel.reject(new Error("Details unavailable"));
  await assert.rejects(pending, /servers\.playerCenter\.member\.refreshFailed: Details unavailable/);
  assert.equal(h.state.activity.key, "activity.playerAccessRefreshFailed");
});
