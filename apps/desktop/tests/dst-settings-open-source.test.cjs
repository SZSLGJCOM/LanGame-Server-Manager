const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const root = path.resolve(__dirname, "..", "..", "..");
const desktopRoot = path.join(root, "apps", "desktop");

function registerTypeScriptRequireExtension(extension) {
  require.extensions[extension] = function compileTypeScript(module, filename) {
    const source = fs.readFileSync(filename, "utf8");
    module._compile(transpileTypeScript(source, filename), filename);
  };
}

registerTypeScriptRequireExtension(".ts");
registerTypeScriptRequireExtension(".tsx");
require.extensions[".css"] = function compileEmptyCss(module, filename) {
  module._compile("", filename);
};

function t(_key, _vars, fallback) {
  return fallback ?? "";
}

test("DST guided fields do not depend on extracted game artwork", () => {
  const { parseGuidedSettingsSchema } = require(path.join(
    desktopRoot,
    "src",
    "views",
    "settings",
    "guided-settings.ts"
  ));
  const schema = parseGuidedSettingsSchema(
    {
      summary: {
        id: "dontstarve",
        name: "Don't Starve Together",
        version: "0.0.0",
        description: null,
        steam_app_id: null,
        install_state: "NotInstalled",
        supported_platforms: ["windows"]
      },
      default_ports: [],
      runtime: {},
      schema_json: fs.readFileSync(path.join(root, "modules", "dontstarve", "schema.json"), "utf8")
    },
    "zh-CN",
    t
  );

  for (const fieldKey of ["world_autumn", "master_deerclops", "master_trees", "caves_bunnymen"]) {
    const field = schema.fields.find((candidate) => candidate.key === fieldKey);
    assert.ok(field, `${fieldKey} should remain available`);
    assert.equal(field.icon, null, `${fieldKey} should not reference bundled Klei artwork`);
  }
  assert.equal(
    fs.existsSync(path.join(desktopRoot, "public", "game-assets")),
    false,
    "third-party game assets must not be vendored"
  );
});
