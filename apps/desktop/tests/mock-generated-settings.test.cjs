const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const Module = require("node:module");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const modulesRoot = path.resolve(__dirname, "../../../modules");
const schemas = Object.fromEntries(fs.readdirSync(modulesRoot)
  .filter((id) => fs.existsSync(path.join(modulesRoot, id, "schema.json")))
  .map((id) => [id, JSON.parse(fs.readFileSync(path.join(modulesRoot, id, "schema.json"), "utf8"))]));
const filename = path.resolve(__dirname, "../src/api-mock/module-settings.ts");
const loaded = new Module(filename, module);
loaded.filename = filename;
loaded.require = (id) => {
  assert.equal(id, "./module-assets");
  return { mockModuleSchemasById: schemas };
};
loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);

test("new Minecraft and HumanitZ defaults preserve each native save transport and explicit RCON choice", () => {
  require.extensions[".ts"] = (sourceModule, sourceFilename) => {
    sourceModule._compile(transpileTypeScript(fs.readFileSync(sourceFilename, "utf8"), sourceFilename), sourceFilename);
  };
  const { resolveWorldSaveAction } = require("../src/views/servers/world-save-action.ts");
  for (const [id, enabled] of [["minecraft", "enable_rcon"], ["humanitz", "rcon_enabled"]]) {
    const settings = loaded.exports.buildMockSettingsForModule(id, "Local save", `${id}-save-default`);
    assert.equal(settings[enabled], true, `${id} uses the declared new-instance default`);
    assert.equal(schemas[id].properties.rcon_password["x-lsgm-default-source"], "generated_secret");
    const source = fs.readFileSync(path.join(modulesRoot, id, "module.toml"), "utf8");
    const block = source.split("[[runtime.player_actions]]").find((value) => /^id = "save_world"$/m.test(value));
    const action = Object.fromEntries([...block.split(/\r?\n\[/)[0].matchAll(/^([a-z_]+) = "([^"]*)"$/gm)]
      .map(([, key, value]) => [key, value]));
    const moduleDetails = { summary: { id }, runtime: { player_actions: [action] } };
    const details = { summary: { id: `${id}-save-default`, module_id: id, status: "Running", active_process_count: 1 },
      active_run: { run_id: 1, processes: [{ process_key: "main", status: "running" }] },
      settings_json: JSON.stringify(settings), ports: [{ name: "rcon", protocol: "tcp", port: 30001 }] };
    assert.equal(resolveWorldSaveAction(details, moduleDetails, "en-US").unavailable, null);
    const disabled = { ...settings, [enabled]: false };
    details.settings_json = JSON.stringify(disabled);
    const save = resolveWorldSaveAction(details, moduleDetails, "en-US");
    if (id === "minecraft") {
      assert.equal(action.transport, "stdin");
      assert.equal(save.unavailable, null, "Minecraft native saving does not require RCON");
    } else {
      assert.equal(action.transport, "source_rcon");
      assert.match(save.unavailable, /Enable RCON/);
    }
    assert.equal(disabled[enabled], false);
    assert.equal(disabled.rcon_password, settings.rcon_password);
    const smoke = fs.readFileSync(path.join(modulesRoot, id, "smoke.toml"), "utf8");
    assert.equal(new RegExp(`^${enabled} = true$`, "m").test(smoke), false,
      "native fixture must exercise the actual default instead of forcing RCON on");
  }
});

test("browser instance creation honors every native generated-secret setting", () => {
  const seen = new Set();
  for (const [id, schema] of Object.entries(schemas)) {
    const fields = Object.entries(schema.properties ?? {}).filter(([, property]) => property["x-lsgm-default-source"] === "generated_secret");
    for (const instanceId of ["instance-one", "instance-two"]) {
      const settings = loaded.exports.buildMockSettingsForModule(id, "Local co-op", instanceId);
      for (const [key, property] of fields) {
        const value = settings[key];
        const expectedLength = Math.min(48, property["x-lsgm-generated-secret-length"] ?? 48, property.maxLength ?? 48);
        assert.match(value, /^[a-f0-9]+$/, `${id}:${key} must initialize an argument-safe secret`);
        assert.equal(value.length, expectedLength, `${id}:${key} must honor its schema length limit`);
        assert.equal(seen.has(value), false, "instances and fields must not share generated credentials");
        seen.add(value);
      }
    }
  }
});

test("browser generated-secret length changes defaults without imposing an input limit", () => {
  const cases = [
    { property: { "x-lsgm-generated-secret-length": 28 }, expected: 28 },
    { property: { "x-lsgm-generated-secret-length": 28, maxLength: 20 }, expected: 20 },
    { property: { "x-lsgm-generated-secret-length": 28, maxLength: 32 }, expected: 28 },
    { property: { maxLength: 28 }, expected: 28 },
    { property: {}, expected: 48 },
    ...[0, -1, 1.5, "28", Number.MAX_SAFE_INTEGER + 1].map((value) => ({
      property: { "x-lsgm-generated-secret-length": value }, expected: 48
    }))
  ];
  schemas.password_limit_fixture = {
    properties: Object.fromEntries(cases.map(({ property }, index) => [
      `password_${index}`, { type: "string", "x-lsgm-default-source": "generated_secret", ...property }
    ]))
  };
  try {
    const settings = loaded.exports.buildMockSettingsForModule("password_limit_fixture", "Local co-op", "bounded-secret");
    for (const [index, { expected }] of cases.entries()) {
      assert.equal(settings[`password_${index}`].length, expected, `length fixture ${index}`);
      assert.match(settings[`password_${index}`], /^[a-f0-9]+$/);
    }
  } finally {
    delete schemas.password_limit_fixture;
  }
});
