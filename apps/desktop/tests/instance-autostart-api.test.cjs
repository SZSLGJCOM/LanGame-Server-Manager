const assert = require("node:assert/strict");
const fs = require("node:fs");
const Module = require("node:module");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const desktopRoot = path.resolve(__dirname, "..");

function registerTypeScriptRequireExtension(extension) {
  require.extensions[extension] = function compileTypeScript(module, filename) {
    const source = fs.readFileSync(filename, "utf8");
    module._compile(transpileTypeScript(source, filename), filename);
  };
}

registerTypeScriptRequireExtension(".ts");
registerTypeScriptRequireExtension(".tsx");
// Register styles after source extensions so extensionless imports resolve TypeScript first.
require.extensions[".css"] = (module) => module._compile("", module.filename);
const originalResolveFilename = Module._resolveFilename;
Module._resolveFilename = function resolveRawImports(request, parent, isMain, options) {
  if (typeof request === "string" && request.endsWith("?raw")) {
    const resolved = originalResolveFilename.call(this, request.slice(0, -4), parent, isMain, options);
    return `${resolved}?raw`;
  }
  return originalResolveFilename.call(this, request, parent, isMain, options);
};
require.extensions[".toml?raw"] = function compileRawToml(module, filename) {
  const source = fs.readFileSync(filename.slice(0, -4), "utf8");
  module._compile(`module.exports = ${JSON.stringify(source)};`, filename);
};

const { invokeMock } = require(path.join(desktopRoot, "src", "api-mock.ts"));

const { listSettingsModuleIds } = require(path.join(desktopRoot, 'src/views/settings/module-registry.ts'));

test('all 32 modules default autostart off and save it independently from native settings and backup policy', async () => {
  const ids = listSettingsModuleIds();
  assert.equal(ids.length, 32);
  for (const moduleId of ids) {
    const created = await invokeMock('create_instance_record', { input: { name: 'Autostart fixture', module_id: moduleId } });
    const instanceId = created.summary.id;
    const original = await invokeMock('read_instance_details_from_storage', { instanceId });
    assert.equal(original.summary.autostart, false, moduleId);
    const enabled = await invokeMock('update_instance_autostart', { instanceId, autostart: true });
    assert.deepEqual(enabled, { ...original, summary: { ...original.summary, autostart: true } }, moduleId);
    const input = { id: instanceId, bind_ip: original.summary.bind_ip,
      auto_backup_on_stop: true, backup_retention_count: 12,
      settings_json: original.settings_json, ports: original.ports };
    const policy = await invokeMock('update_instance_record_if_current', { input, expectedSettingsJson: original.settings_json });
    assert.equal(policy.summary.autostart, true, moduleId);
    assert.equal(policy.auto_backup_on_stop, true, moduleId);
    const disabled = await invokeMock('update_instance_autostart', { instanceId, autostart: false });
    assert.deepEqual(disabled, { ...policy, summary: { ...policy.summary, autostart: false } }, moduleId);
  }
});

test('autostart rejects non-booleans before mutation', async () => {
  const instanceId = 'srv-dst-1';
  const before = await invokeMock('read_instance_details_from_storage', { instanceId });
  for (const autostart of [undefined, null, 'false', 0]) {
    await assert.rejects(invokeMock('update_instance_autostart', { instanceId, autostart }), /boolean/);
  }
  assert.deepEqual(await invokeMock('read_instance_details_from_storage', { instanceId }), before);
});
