const assert = require("node:assert/strict");
const childProcess = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const root = path.resolve(__dirname, "..", "..", "..");
const desktopRoot = path.join(root, "apps", "desktop");
const modulesDir = path.join(root, "modules");
const settingsDir = path.join(desktopRoot, "src", "views", "settings");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = function compileTypeScript(module, filename) {
    const source = fs.readFileSync(filename, "utf8");
    const outputText = transpileTypeScript(source, filename);
    module._compile(outputText, filename);
  };
}
require.extensions[".css"] = function compileEmptyCss(module) {
  module._compile("", module.filename);
};

const { listSettingsModuleIds } = require(path.join(settingsDir, "module-registry.ts"));

function readBundledModuleIds() {
  return fs.readdirSync(modulesDir)
    .filter((moduleId) =>
      fs.existsSync(path.join(modulesDir, moduleId, "module.toml")) &&
      fs.existsSync(path.join(modulesDir, moduleId, "schema.json"))
    )
    .sort();
}

test("all 32 bundled games resolve through the unified Configuration registry", () => {
  const bundledModuleIds = readBundledModuleIds();
  const auditModuleIds = JSON.parse(childProcess.execFileSync(
    process.execPath,
    [path.join(root, "scripts", "read_settings_module_ids.cjs")],
    { cwd: root, encoding: "utf8" }
  ));
  assert.deepEqual(listSettingsModuleIds(), bundledModuleIds);
  assert.deepEqual(auditModuleIds, bundledModuleIds);
  assert.equal(listSettingsModuleIds().length, 32);
});

test("legacy settings routers and modal shells are absent", () => {
  const legacyPaths = [
    path.join(settingsDir, "SettingsModalRouter.tsx"),
    ...(fs.existsSync(settingsDir)
      ? fs.readdirSync(settingsDir)
        .filter((fileName) => fileName.endsWith("SettingsModal.tsx"))
        .map((fileName) => path.join(settingsDir, fileName))
      : [])
  ];
  assert.deepEqual(legacyPaths.filter((filePath) => fs.existsSync(filePath)), []);
});
