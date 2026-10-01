const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const root = path.resolve(__dirname, "..", "..", "..");
const desktopRoot = path.join(root, "apps", "desktop");

require.extensions[".ts"] = function compileTypeScript(module, filename) {
  const source = fs.readFileSync(filename, "utf8");
  const outputText = transpileTypeScript(source, filename);
  module._compile(outputText, filename);
};

const { changedSettingKeys, mergeSettingPatch } = require(path.join(
  desktopRoot,
  "src",
  "views",
  "servers",
  "mod-settings-patch.ts"
));

test("merges only dirty configuration fields onto the latest settings", () => {
  const persisted = {
    auto_update_mods: false,
    mod_package_names: "ExistingPackage",
    server_name: "Original"
  };
  const draft = {
    ...persisted,
    auto_update_mods: true
  };
  const latest = {
    ...persisted,
    mod_package_names: "ExistingPackage\nNewPackage",
    server_name: "Changed elsewhere"
  };
  const dirtyKeys = changedSettingKeys(persisted, draft, [
    "auto_update_mods",
    "mod_package_names"
  ]);

  assert.deepEqual(dirtyKeys, ["auto_update_mods"]);
  assert.deepEqual(mergeSettingPatch(latest, draft, dirtyKeys), {
    auto_update_mods: true,
    mod_package_names: "ExistingPackage\nNewPackage",
    server_name: "Changed elsewhere"
  });
});

test("supports field deletion and ignores object key order", () => {
  const persisted = { options: { alpha: 1, beta: 2 }, obsolete: true };
  const draft = { options: { beta: 2, alpha: 1 } };
  const dirtyKeys = changedSettingKeys(persisted, draft, ["options", "obsolete"]);

  assert.deepEqual(dirtyKeys, ["obsolete"]);
  assert.deepEqual(mergeSettingPatch({ ...persisted, external: "kept" }, draft, dirtyKeys), {
    options: { alpha: 1, beta: 2 },
    external: "kept"
  });
});
