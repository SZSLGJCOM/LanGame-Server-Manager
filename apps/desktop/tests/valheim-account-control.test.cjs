const assert = require("node:assert/strict");
const fs = require("node:fs");
const Module = require("node:module");
const path = require("node:path");
const vm = require("node:vm");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) require.extensions[extension] = (module, filename) => {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
const resolveFilename = Module._resolveFilename;
Module._resolveFilename = function resolveRaw(request, parent, isMain, options) {
  return typeof request === "string" && request.endsWith("?raw")
    ? `${resolveFilename.call(this, request.slice(0, -4), parent, isMain, options)}?raw`
    : resolveFilename.call(this, request, parent, isMain, options);
};
require.extensions[".toml?raw"] = (module, filename) => {
  module._compile(`module.exports = ${JSON.stringify(fs.readFileSync(filename.slice(0, -4), "utf8"))};`, filename);
};

test("Valheim preparation API sends only the selected instance ID to the explicit command", async () => {
  const filename = path.resolve(__dirname, "../src/api.ts");
  const calls = [];
  const result = { summary: { id: "valheim-fixture", module_id: "valheim" } };
  const dependencies = {
    "@tauri-apps/api/core": {}, "./locale-preference": {},
    "./storage-management-requests": {}, "./ark-map-availability": {},
    "./api-transport": { invokeOrMock: async (command, args) => { calls.push({ command, args }); return result; } }
  };
  const exports = {};
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports, require: (name) => { assert.ok(name in dependencies, name); return dependencies[name]; }
  });
  assert.ok(Object.is(await exports.prepareValheimPlayerControl("valheim-fixture"), result));
  assert.deepEqual(JSON.parse(JSON.stringify(calls)), [{ command: "prepare_valheim_player_control", args: { instanceId: "valheim-fixture" } }]);
});

test("preview Valheim preparation requires the stopped game instance and preserves all saved settings", async () => {
  const { invokeMock } = require("../src/api-mock.ts");
  await invokeMock("ensure_steamcmd_ready", { operationId: "valheim-account-control-fixture" });
  await invokeMock("install_module_game", { moduleId: "valheim" });
  const create = async (name, module_id = "valheim") => (await invokeMock("create_instance_record", { input: { name, module_id } })).summary.id;
  const instanceId = await create("Synthetic Valheim control");
  const otherId = await create("Unchanged neighboring Valheim");
  const before = await invokeMock("read_instance_details_from_storage", { instanceId });
  const otherBefore = await invokeMock("read_instance_isolation", { input: { instance_id: otherId } });
  const prepared = await invokeMock("prepare_valheim_player_control", { instanceId });
  assert.deepEqual(prepared, before, "Preparation must not modify settings, world rules, saves or status");
  const isolation = await invokeMock("read_instance_isolation", { input: { instance_id: instanceId } });
  assert.equal(isolation.mode, "private");
  assert.deepEqual(await invokeMock("read_instance_isolation", { input: { instance_id: otherId } }), otherBefore);
  await invokeMock("start_instance_process", { instanceId });
  await assert.rejects(invokeMock("prepare_valheim_player_control", { instanceId }), /Stop the server/);
  await assert.rejects(invokeMock("prepare_valheim_player_control", { instanceId: "unknown-instance" }), /Unknown Valheim/);
  await invokeMock("install_module_game", { moduleId: "dontstarve" });
  const wrongGame = await create("Other game", "dontstarve");
  await assert.rejects(invokeMock("prepare_valheim_player_control", { instanceId: wrongGame }), /requires a Valheim/);
});
