const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const Module = require("node:module");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => {
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
require.extensions[".css"] = (module) => module._compile("", module.filename);
require.extensions[".png"] = (module, filename) => module._compile(`module.exports = ${JSON.stringify(filename)};`, filename);

function captureSaveOptions(moduleDetails, settingsJson = "{}") {
  const filename = path.resolve(__dirname, "../src/views/settings/ConfigurationWorkspace.tsx");
  const loaded = new Module(filename, module);
  loaded.filename = filename;
  const load = Module.createRequire(filename);
  let saveOptions;
  loaded.require = (id) => id === "react" ? {
    useMemo: (factory) => factory(),
    useState: (initial) => [typeof initial === "function" ? initial() : initial, () => {}],
    useRef: (initial) => ({ current: initial }),
    useLayoutEffect() {}
  } : id === "../../i18n" ? {
    useI18n: () => ({ locale: "en-US", t: (key, _params, fallback) => fallback ?? key })
  } : id === "./useAutoSaveInstanceSettings" ? {
    useAutoSaveInstanceSettings(options) { saveOptions = options; return { status: { state: "saved" }, retry() {} }; }
  } : id === "./useModuleConfigurationIcons" ? {
    useModuleConfigurationIcons: () => ({ icons: {}, loading: false })
  } : id === "./useInstancePortRegistration" ? {
    useInstancePortRegistration: () => ({ ports: [], defaultPorts: [], setPorts() {} })
  } : load(id);
  loaded._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  loaded.exports.ConfigurationWorkspace({
    details: {
      summary: { id: "server-a", module_id: "dontstarve", bind_ip: "0.0.0.0", name: "DST" },
      settings_json: settingsJson, ports: [], backup_retention_count: 3
    },
    moduleDetails, bindAddressCandidates: [], runtime: null, launchPlan: null,
    launchPlanError: null, onSave() {}
  });
  return saveOptions;
}

test("missing or mismatched module metadata marks the editor unavailable separately from invalid settings", () => {
  assert.equal(captureSaveOptions(null).ready, false);
  assert.equal(captureSaveOptions({ summary: { id: "valheim", name: "Valheim" }, schema_json: "{}" }).ready, false);
});

test("a loaded schema with invalid settings remains a real draft validation blocker", () => {
  const schemaJson = fs.readFileSync(path.resolve(__dirname, "../../../modules/dontstarve/schema.json"), "utf8");
  const options = captureSaveOptions({ summary: { id: "dontstarve", name: "DST" }, schema_json: schemaJson }, "{ invalid JSON");
  assert.equal(options.ready, true);
  assert.equal(options.disabled, true);
});
