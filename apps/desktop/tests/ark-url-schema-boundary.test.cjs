const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const root = path.resolve(__dirname, "..", "..", "..");
const desktopRoot = path.join(root, "apps", "desktop");

function compileTypeScript(module, filename) {
  const source = fs.readFileSync(filename, "utf8");
  module._compile(transpileTypeScript(source, filename), filename);
}

require.extensions[".ts"] = compileTypeScript;
require.extensions[".tsx"] = compileTypeScript;
require.extensions[".css"] = function compileEmptyCss(module) {
  module._compile("", module.filename);
};

const { parseGuidedSettingsSchema, validateGuidedSettingsObject } = require(path.join(
  desktopRoot,
  "src",
  "views",
  "settings",
  "guided-settings.ts"
));

function loadGuidedSchema(moduleId) {
  const schemaJson = fs.readFileSync(path.join(root, "modules", moduleId, "schema.json"), "utf8");
  return parseGuidedSettingsSchema(
    {
      summary: {
        id: moduleId,
        name: moduleId,
        version: "0.0.0",
        description: null,
        steam_app_id: null,
        install_state: "NotInstalled",
        supported_platforms: ["windows"]
      },
      default_ports: [],
      runtime: {},
      schema_json: schemaJson
    },
    "en-US",
    (_key, _params, fallback) => fallback ?? ""
  );
}

function hasPatternIssue(schema, settings, context, fieldKey) {
  return validateGuidedSettingsObject(schema, settings, context).some(
    (issue) => issue.fieldKey === fieldKey && issue.reason === "pattern"
  );
}

test("ARK URL-backed strings reject option and line injection before autosave", () => {
  const asa = loadGuidedSchema("arksurvivalascended");
  const ase = loadGuidedSchema("arksurvivalevolved");

  for (const fieldKey of ["map_name", "server_name"]) {
    for (const unsafeValue of ["safe?Injected=true", "safe\rInjected=true", "safe\nInjected=true"]) {
      assert.equal(hasPatternIssue(asa, { [fieldKey]: unsafeValue }, undefined, fieldKey), true);
    }
  }
  for (const unsafeValue of ["TheIsland?Injected=true", "TheIsland\rInjected", "TheIsland\nInjected"]) {
    assert.equal(hasPatternIssue(ase, { map_name: unsafeValue }, undefined, "map_name"), true);
  }
});

test("ASA INI passwords allow question marks while rejecting line injection", () => {
  const asa = loadGuidedSchema("arksurvivalascended");

  for (const fieldKey of ["server_password", "admin_password"]) {
    assert.equal(hasPatternIssue(asa, { [fieldKey]: "fixture?password" }, undefined, fieldKey), false);
    for (const unsafeValue of ["fixture\rInjected=true", "fixture\nInjected=true"]) {
      assert.equal(hasPatternIssue(asa, { [fieldKey]: unsafeValue }, undefined, fieldKey), true);
    }
  }
});

test("ASA instance-name defaults pass through the same URL boundary", () => {
  const asa = loadGuidedSchema("arksurvivalascended");

  assert.equal(hasPatternIssue(asa, {}, { instanceName: "Public ARK" }, "server_name"), false);
  assert.equal(hasPatternIssue(asa, {}, { instanceName: "Public?ServerAdminPassword=owned" }, "server_name"), true);
});

test("schema validation messages use the active Configuration language", () => {
  const asa = loadGuidedSchema("arksurvivalascended");
  const translate = (key, params, fallback) =>
    key === "settings.configuration.validation.pattern"
      ? `localized:${params.field}`
      : fallback ?? key;

  const issue = validateGuidedSettingsObject(
    asa,
    { server_name: "Public?Injected=true" },
    undefined,
    translate
  ).find((candidate) => candidate.fieldKey === "server_name" && candidate.reason === "pattern");

  const serverNameTitle = asa.fields.find((field) => field.key === "server_name").title;
  assert.equal(issue?.message, `localized:${serverNameTitle}`);
});
