const assert = require("node:assert/strict");
const fs = require("node:fs");
const Module = require("node:module");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
for (const extension of [".ts", ".tsx"]) require.extensions[extension] = (loaded, filename) => {
  loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
require.extensions[".css"] = () => {};
const resolveFilename = Module._resolveFilename;
Module._resolveFilename = function(request, parent, isMain, options) {
  return typeof request === "string" && request.endsWith("?raw")
    ? `${resolveFilename.call(this, request.slice(0, -4), parent, isMain, options)}?raw`
    : resolveFilename.call(this, request, parent, isMain, options);
};
require.extensions[".toml?raw"] = (loaded, filename) => { loaded.exports = fs.readFileSync(filename.slice(0, -4), "utf8"); };
const { invokeMock } = require("../src/api-mock.ts");
const map = (id, enabled = true) => ({ id, enabled, name: id, map_name: `${id}_Package` });

for (const moduleId of ["arksurvivalevolved", "arksurvivalascended"]) test(`${moduleId}: preview persists endpoints, starts every enabled map and isolates CMD logs`, async () => {
  await invokeMock("ensure_steamcmd_ready");
  await invokeMock("install_module_game", { moduleId });
  const created = await invokeMock("create_instance_record", { input: { module_id: moduleId, name: "Map cluster preview" } });
  const instanceId = created.summary.id;
  const read = () => invokeMock("read_instance_details_from_storage", { instanceId });
  let details = await read();
  const save = async (settings, ports = details.ports) => {
    const saved = await invokeMock("update_instance_record_if_current", { expectedSettingsJson: details.settings_json,
      input: { id: instanceId, settings_json: JSON.stringify(settings), ports, bind_ip: details.summary.bind_ip,
        auto_backup_on_stop: false, backup_retention_count: details.backup_retention_count } });
    details = saved; return saved;
  };
  let settings = { ...JSON.parse(details.settings_json), rcon_enabled: true, admin_password: "fixture-admin-password", additional_maps: [map("scorched"), map("center"), map("paused", false)] };
  await save(settings);
  assert.equal(details.ports.length, 16);
  assert.equal(details.summary.port_count, 16);
  const pausedPorts = details.ports.filter((port) => port.name.startsWith("map-paused-"));
  const existingEndpoints = details.ports.map((port) => `${port.protocol}:${port.port}`);
  assert.equal(new Set(existingEndpoints).size, existingEndpoints.length);
  if (moduleId === "arksurvivalevolved") assert.equal(details.ports.find((port) => port.name === "map-scorched-peer").port, details.ports.find((port) => port.name === "map-scorched-game").port + 1);
  await invokeMock("start_instance_process", { instanceId }); details = await read();
  assert.equal(details.summary.active_process_count, 3);
  assert.deepEqual(details.active_run.processes.map((process) => process.process_key), ["main", "map-scorched", "map-center"]);
  await assert.rejects(save({ ...settings, additional_maps: [] }), /Stop the ARK instance/);
  const send = (processKey, command, transport = "source_rcon") => invokeMock("send_instance_runtime_command", { input: {
    instanceId, processKey, command, transport, portName: "rcon", passwordSettingKey: "admin_password", enabledSettingKey: "rcon_enabled"
  } });
  const result = await send("map-scorched", "SaveWorld");
  assert.equal(result.process_key, "map-scorched"); assert.equal(result.display_name, "scorched");
  const scorchedLog = await invokeMock("read_instance_log_document_from_storage", { instanceId, runId: 2 });
  const centerLog = await invokeMock("read_instance_log_document_from_storage", { instanceId, runId: 3 });
  const primaryLog = await invokeMock("read_instance_log_document_from_storage", { instanceId, runId: 1 });
  assert.notEqual(scorchedLog.source_path, centerLog.source_path);
  assert.ok(scorchedLog.lines.some((line) => line.includes("SaveWorld")));
  assert.ok(!centerLog.lines.some((line) => line.includes("SaveWorld")));
  assert.ok(!primaryLog.lines.some((line) => line.includes("SaveWorld")));
  const broadcast = await invokeMock("send_instance_runtime_command", { input: { instanceId, runtimeActionId: "broadcast", runtimeActionTarget: "Hello",
    processKey: "map-center" } });
  assert.equal(broadcast.process_key, "map-center");
  const primaryBroadcast = await invokeMock("send_instance_runtime_command", { input: { instanceId, runtimeActionId: "broadcast", runtimeActionTarget: "Hello" } });
  assert.equal(primaryBroadcast.process_key, "main");
  await assert.rejects(send("map-paused", "SaveWorld"), /not running/);
  await assert.rejects(send("map-scorched", "SaveWorld", "stdin"), /Source RCON/);
  await save({ ...settings, rcon_enabled: false });
  await assert.rejects(send("map-center", "SaveWorld"), /Enable RCON/);
  await invokeMock("stop_instance_process", { instanceId }); details = await read();
  assert.equal(details.active_run, null); assert.equal(details.summary.active_process_count, 0);
  assert.deepEqual(JSON.parse(details.settings_json).additional_maps, settings.additional_maps);
  await assert.rejects(save({ ...settings, additional_maps: [map("scorched") , { ...map("center"), map_name: "DifferentPackage" }, map("paused", false)] }), /package cannot change/);
  await assert.rejects(save(settings, details.ports.map((port) => port.name === "map-center-rcon" ? { ...port, port: 0 } : port)), /nonzero tcp/);
  settings = { ...settings, additional_maps: [map("new"), map("center"), map("paused", false)] };
  await save(settings);
  assert.ok(!details.ports.some((port) => port.name.startsWith("map-scorched-")));
  assert.deepEqual(details.ports.filter((port) => port.name.startsWith("map-paused-")), pausedPorts);
  assert.deepEqual((await read()).ports, details.ports);
});
