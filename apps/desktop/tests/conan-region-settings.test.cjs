const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => {
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
require.extensions[".css"] = (module) => module._compile("", module.filename);

const {
  parseGuidedSettingsSchema,
  readGuidedFieldValue,
  writeGuidedFieldValue,
  validateGuidedSettingsObject
} = require("../src/views/settings/guided-settings.ts");
const moduleRoot = path.resolve(__dirname, "../../../modules/conanexiles");
const schemaJson = fs.readFileSync(path.join(moduleRoot, "schema.json"), "utf8");
const nativeSchema = JSON.parse(schemaJson);
const schema = parseGuidedSettingsSchema({
  summary: { id: "conanexiles", name: "Conan Exiles" },
  schema_json: schemaJson
}, "en-US");
const region = schema.fields.find((field) => field.key === "excluded_regions");
// Instance creation supplies a generated admin secret independently of schema defaults.
const persisted = { admin_password: "fixture-admin-password" };

test("Conan default loaded regions pass the real guided form validation", () => {
  assert.equal(schema.parseError, null);
  assert.equal(nativeSchema.properties.excluded_regions.default, "");
  assert.equal(readGuidedFieldValue(region, persisted), "");
  assert.deepEqual(validateGuidedSettingsObject(schema, persisted), []);
  assert.ok(validateGuidedSettingsObject(schema, { ...persisted, server_name: "" })
    .some((issue) => issue.fieldKey === "server_name" && issue.reason === "required"));
});

test("Conan region choices preserve explicit exclusions and switching back to all regions", () => {
  assert.equal(region.control, "select");
  assert.deepEqual(region.enumOptions.map((option) => option.value), ["", "IsleOfSiptah"]);
  let settings = persisted;
  for (const value of ["IsleOfSiptah", ""]) {
    settings = writeGuidedFieldValue(settings, region, value);
    assert.equal(readGuidedFieldValue(region, settings), value);
    assert.equal(settings.excluded_regions, value);
    assert.deepEqual(validateGuidedSettingsObject(schema, settings), []);
  }
});

test("Conan keeps the empty native ExcludedRegions output contract", () => {
  const template = fs.readFileSync(path.join(moduleRoot, "templates/Engine.ini.hbs"), "utf8");
  assert.match(template, /\[Regions\]\r?\nExcludedRegions=\{\{excluded_regions\}\}\r?\n/);
  const fixture = JSON.parse(fs.readFileSync(path.join(
    moduleRoot, "config-fixtures/2026-07-13-steamcmd_anonymous_validate_443030.json"
  ), "utf8"));
  assert.equal(readGuidedFieldValue(region, fixture.settings), "");
  const expected = fixture.expected.files.find((file) => file.path.endsWith("/Engine.ini"));
  assert.equal(expected.keys["Regions.ExcludedRegions"], "");
});
