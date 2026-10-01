const assert = require("node:assert/strict");
const fs = require("node:fs");
const Module = require("node:module");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const filename = path.resolve(__dirname, "../src/views/servers/AiBroadcastWorkbench.tsx");
const loaded = new Module(filename, module);
loaded.filename = filename;
loaded.require = () => ({});
loaded._compile(transpileTypeScript(
  fs.readFileSync(filename, "utf8") + "\nexport { normalizeRules, normalizePolicy };\n", filename
), filename);
const { normalizeRules, normalizePolicy } = loaded.exports;

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, sourcePath) => {
    module._compile(transpileTypeScript(fs.readFileSync(sourcePath, "utf8"), sourcePath), sourcePath);
  };
}
const originalResolveFilename = Module._resolveFilename;
Module._resolveFilename = function resolveRawImports(request, parent, isMain, options) {
  if (typeof request === "string" && request.endsWith("?raw")) {
    return `${originalResolveFilename.call(this, request.slice(0, -4), parent, isMain, options)}?raw`;
  }
  return originalResolveFilename.call(this, request, parent, isMain, options);
};
require.extensions[".toml?raw"] = (module, sourcePath) => {
  module._compile(`module.exports = ${JSON.stringify(fs.readFileSync(sourcePath.slice(0, -4), "utf8"))};`, sourcePath);
};
const { invokeMock } = require("../src/api-mock.ts");

test("a zero-minute broadcast cooldown survives edit normalization and persisted policy loading", () => {
  const edited = normalizeRules({ cooldown_minutes: 0 });
  assert.equal(edited.cooldown_minutes, 0);
  const reloaded = normalizePolicy({ enabled: true, rules: edited, updated_at_unix_ms: 1 }, "server-a");
  assert.equal(reloaded.instance_id, "server-a");
  assert.equal(reloaded.rules.cooldown_minutes, 0);
  assert.equal(normalizeRules(reloaded.rules).cooldown_minutes, 0);
});

test("broadcast cooldown retains valid durations and constrains out-of-range input", () => {
  for (const [value, expected] of [[1, 1], [10, 10], [1440, 1440], [-1, 0], [1441, 1440]]) {
    assert.equal(normalizeRules({ cooldown_minutes: value }).cooldown_minutes, expected);
  }
});

test("missing and non-finite broadcast cooldown values use the documented default", () => {
  for (const value of [undefined, null, NaN, Infinity, -Infinity]) {
    assert.equal(normalizeRules({ cooldown_minutes: value }).cooldown_minutes, 10);
  }
});

test("the browser mock preserves zero cooldown across update and read commands", async () => {
  const instanceId = "maintenance-broadcast-zero-cooldown";
  const initial = await invokeMock("read_instance_broadcast_policy", { instanceId });
  assert.equal(initial.rules.cooldown_minutes, 10);
  const input = {
    instance_id: instanceId,
    enabled: true,
    rules: { ...initial.rules, cooldown_minutes: 0 }
  };
  const saved = await invokeMock("update_instance_broadcast_policy", { input });
  assert.equal(saved.rules.cooldown_minutes, 0);
  saved.rules.cooldown_minutes = 99;
  const reloaded = await invokeMock("read_instance_broadcast_policy", { instanceId });
  assert.equal(reloaded.enabled, true);
  assert.equal(reloaded.rules.cooldown_minutes, 0, "returned policy objects cannot mutate the saved value");
  assert.equal(normalizePolicy(reloaded, instanceId).rules.cooldown_minutes, 0);
});

test("browser mock cooldown boundaries match the editor through update and read", async () => {
  const instanceId = "maintenance-broadcast-cooldown-boundaries";
  for (const value of [undefined, null, NaN, Infinity, -Infinity, -1, 1, 10, 1440, 1441]) {
    const saved = await invokeMock("update_instance_broadcast_policy", {
      input: { instance_id: instanceId, enabled: false, rules: { cooldown_minutes: value } }
    });
    const expected = normalizeRules({ cooldown_minutes: value }).cooldown_minutes;
    assert.equal(saved.rules.cooldown_minutes, expected, String(value));
    const read = await invokeMock("read_instance_broadcast_policy", { instanceId });
    assert.equal(read.rules.cooldown_minutes, expected, String(value));
  }
});
