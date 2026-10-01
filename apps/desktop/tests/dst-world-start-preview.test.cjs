const assert = require("node:assert/strict");
const fs = require("node:fs");
const Module = require("node:module");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = function compileTypeScript(module, filename) {
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
require.extensions[".css"] = (module) => module._compile("", module.filename);
const originalResolveFilename = Module._resolveFilename;
Module._resolveFilename = function resolveRawImports(request, parent, isMain, options) {
  if (typeof request === "string" && request.endsWith("?raw")) {
    return `${originalResolveFilename.call(this, request.slice(0, -4), parent, isMain, options)}?raw`;
  }
  return originalResolveFilename.call(this, request, parent, isMain, options);
};
require.extensions[".toml?raw"] = function compileRawToml(module, filename) {
  module._compile(`module.exports = ${JSON.stringify(fs.readFileSync(filename.slice(0, -4), "utf8"))};`, filename);
};

const { invokeMock } = require("../src/api-mock.ts");
test.before(async () => {
  const status = await invokeMock("ensure_steamcmd_ready", { operationId: "fixture-steamcmd-ready" });
  assert.equal(status.ready, true);
});
const preview = (instanceId) => invokeMock("preview_dontstarve_world_start", { instanceId });

test("DST preview fixtures distinguish each shard and retain saved settings", async () => {
  for (const [fixture, expected] of [
    ["new", ["new", "new"]],
    ["existing", ["existing", "existing"]],
    ["mixed", ["existing", "new"]],
    ["unrecognized", ["unrecognized", "unrecognized"]]
  ]) {
    global.window = { location: { search: `?dstWorld=${fixture}` } };
    const result = await preview("srv-dst-terminal-error");
    const details = await invokeMock("read_instance_details_from_storage", { instanceId: "srv-dst-terminal-error" });
    assert.deepEqual(result.shards.map((shard) => shard.state), expected);
    assert.deepEqual(result.shards.map((shard) => shard.shard), ["Master", "Caves"]);
    assert.equal(result.settings_json, details.settings_json);
    assert.equal(result.instance_id, details.summary.id);
  }
  delete global.window;
});

test("fresh DST mock instances are new and acquire saves after a successful start", async () => {
  const created = await invokeMock("create_instance_record", { input: { module_id: "dontstarve", name: "Preview test" } });
  const instanceId = created.summary.id;
  assert.ok((await preview(instanceId)).shards.every((shard) => shard.state === "new"));
  await invokeMock("install_module_game", { moduleId: "dontstarve" });
  await invokeMock("start_instance_process", { instanceId });
  assert.ok((await preview(instanceId)).shards.every((shard) => !shard.enabled || shard.state === "existing"));
  await invokeMock("delete_instance_record", { instanceId });
});

test("world preview rejects other game instances", async () => {
  await assert.rejects(preview("srv-minecraft-1"), /not a Don't Starve Together server/);
});

test("disabled cave worlds remain new after a master-only mock start", async () => {
  const { MockDstWorldStateStore } = require("../src/api-mock/dst-world-state.ts");
  const store = new MockDstWorldStateStore();
  const details = await invokeMock("read_instance_details_from_storage", { instanceId: "srv-dst-terminal-error" });
  details.settings_json = JSON.stringify({ ...JSON.parse(details.settings_json), enable_caves: false });
  store.create(details.summary.id);
  store.start(details);
  assert.deepEqual(store.preview(details).shards, [
    { shard: "Master", state: "existing", enabled: true },
    { shard: "Caves", state: "new", enabled: false }
  ]);
});

test("Island Adventures mock preview carries all four shards through new-world generation", async () => {
  const { MockDstWorldStateStore } = require("../src/api-mock/dst-world-state.ts");
  const store = new MockDstWorldStateStore();
  const details = await invokeMock("read_instance_details_from_storage", { instanceId: "srv-dst-terminal-error" });
  details.settings_json = JSON.stringify({ shard_layout: "island_adventures", enable_caves: false });
  store.create(details.summary.id);
  assert.deepEqual(store.preview(details).shards.map((shard) => [shard.shard, shard.state, shard.enabled]),
    ["Master", "Caves", "Islands", "Volcano"].map((shard) => [shard, "new", true]));
  store.start(details);
  assert.ok(store.preview(details).shards.every((shard) => shard.state === "existing" && shard.enabled));
});

test("mock start rejects confirmed settings or world state that changed after review", async () => {
  const created = await invokeMock("create_instance_record", { input: { module_id: "dontstarve", name: "Atomic confirmation" } });
  const instanceId = created.summary.id;
  const expectedWorldStart = await preview(instanceId);
  const incorrectSettings = { ...expectedWorldStart, settings_json: "{}" };
  await assert.rejects(invokeMock("start_instance_process", { instanceId, expectedWorldStart: incorrectSettings }), /dst_world_start_changed/);
  const stopped = await invokeMock("read_instance_details_from_storage", { instanceId });
  assert.equal(stopped.summary.status, "Stopped");
  await invokeMock("start_instance_process", { instanceId, expectedWorldStart });
  await invokeMock("stop_instance_process", { instanceId });
  await assert.rejects(invokeMock("start_instance_process", { instanceId, expectedWorldStart }), /dst_world_start_changed/);
  await invokeMock("delete_instance_record", { instanceId });
});

test("native confirmation is validated under the instance lock before materialization", () => {
  const source = fs.readFileSync(require("node:path").join(__dirname, "../src-tauri/src/commands_runtime_lifecycle.rs"), "utf8");
  const start = source.slice(source.indexOf("pub(super) async fn start_instance_process_after_reconcile_reserved"));
  const lock = start.indexOf("state.acquire_instance_mutation(&instance_id)");
  const read = start.indexOf("read_instance_details(&storage.paths, &instance_id)");
  const validate = start.indexOf("validate_dst_world_start_confirmation");
  const materialize = start.indexOf("materialize_runtime_start_configuration");
  assert.ok(lock >= 0 && read > lock && validate > read && materialize > validate);
});
