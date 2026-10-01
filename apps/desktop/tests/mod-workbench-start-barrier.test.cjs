const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
require.extensions[".tsx"] = require.extensions[".ts"];
const { InstanceSettingsSaveCoordinator } = require("../src/views/settings/instance-settings-save-coordinator.ts");
const policy = require("../src/views/servers/mod-workbench-dst-policy.ts");
const settle = () => new Promise(resolve => setImmediate(resolve));
function deferred() {
  let resolve, reject;
  const promise = new Promise((a, b) => { resolve = a; reject = b; });
  return { promise, resolve, reject };
}
function mutations(owner, settings = {}) {
  const filename = path.resolve(__dirname, "../src/views/servers/useModWorkbenchMutations.ts");
  const exports = {};
  const dependencies = {
    react: { useCallback: fn => fn, useRef: current => ({ current }), useState: value => [value, () => {}] },
    "../../desktop-error-message": require("../src/desktop-error-message.ts"),
    "../settings/InstanceSettingsSaveContext": { useInstanceSettingsSaveCoordinator: () => owner },
    "./mod-workbench-dst-policy": policy
  };
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports, require: id => { assert.ok(Object.hasOwn(dependencies, id), id); return dependencies[id]; }
  });
  return exports.useModWorkbenchMutations("server-a", "dontstarve", { current: false }, { current: settings },
    (key, params, fallback) => fallback ?? key);
}

test("ModWorkbench's actual operation hook blocks immediate Start before the preliminary read completes", async () => {
  const owner = new InstanceSettingsSaveCoordinator();
  const view = mutations(owner);
  const read = deferred();
  const events = [];
  view.launchModMutation("configuration", async () => { events.push("read"); await read.promise; events.push("saved"); });
  const start = owner.flush("server-a").then(() => events.push("start"));
  await settle();
  assert.deepEqual(events, ["read"]);
  read.resolve();
  await start;
  assert.deepEqual(events, ["read", "saved", "start"]);
});

test("a failed Mod save remains a blocker after the view reference is discarded", async () => {
  const owner = new InstanceSettingsSaveCoordinator();
  const save = deferred();
  mutations(owner).launchModMutation("configuration", () => save.promise);
  save.reject(new Error("precondition failed"));
  await settle();
  await assert.rejects(owner.flush("server-a"), /precondition failed/);
});

test("raw-owned Mod enablement is rejected without invoking its write", async () => {
  const view = mutations(new InstanceSettingsSaveCoordinator(), { master_modoverrides_lua: "return {custom=true}" });
  await assert.rejects(view.runModMutation("enablement", async () => assert.fail("must not write")), /modoverrides/);
});

test("one shard's raw Lua does not disable configuration changes owned by the other shard", async () => {
  const view = mutations(new InstanceSettingsSaveCoordinator(), { caves_modoverrides_lua: "return {custom=true}" });
  let saved = false;
  await view.runModMutation("configuration", async () => { saved = true; });
  assert.equal(saved, true);
});
