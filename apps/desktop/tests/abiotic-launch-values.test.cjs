const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => {
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
require.extensions[".css"] = (module) => module._compile("", module.filename);

const { parseGuidedSettingsSchema, readGuidedFieldValue, writeGuidedFieldValue } = require("../src/views/settings/guided-settings.ts");
const { resolveSettingsModuleDefinition } = require("../src/views/settings/module-registry.ts");
const { ConfigurationField } = require("../src/views/settings/ConfigurationField.tsx");
const { EN_US_ABIOTIC_FACTOR_MESSAGES } = require("../src/i18n/games/abioticfactor.en.ts");
const { ZH_CN_ABIOTIC_FACTOR_MESSAGES } = require("../src/i18n/games/abioticfactor.zh-cn.ts");
const schemaJson = fs.readFileSync(path.resolve(__dirname, "../../../modules/abioticfactor/schema.json"), "utf8");
const definition = resolveSettingsModuleDefinition("abioticfactor");
const launchKeys = ["server_name", "world_save_name", "server_password", "admin_password"];

function fixture(locale, catalog) {
  const t = (key, _params, fallback) => catalog[key] ?? fallback ?? key;
  const schema = parseGuidedSettingsSchema({ summary: { id: "abioticfactor", name: "Abiotic Factor" }, schema_json: schemaJson }, locale, t);
  assert.equal(schema.parseError, null);
  const fields = new Map(schema.fields.map((field) => [field.key, field]));
  const validate = (key, value, settings = {}) => definition.getFieldValidationMessage({
    field: fields.get(key), value, settings: { ...settings, [key]: value }, locale, t
  });
  return { t, fields, validate };
}

test("Abiotic's real field validation rejects unrepresentable quotes with bilingual non-secret feedback", () => {
  for (const [locale, catalog] of [["en-US", EN_US_ABIOTIC_FACTOR_MESSAGES], ["zh-CN", ZH_CN_ABIOTIC_FACTOR_MESSAGES]]) {
    const { t, fields, validate } = fixture(locale, catalog);
    const input = 'synthetic"test-only';
    for (const key of launchKeys) {
      const message = validate(key, input);
      assert.equal(message, catalog["abiotic.settings.validation.launchDoubleQuote"], key);
      assert.equal(typeof message, "string");
      assert.ok(message.length > 0);
      assert.ok(!message.includes(input), "feedback must not echo a credential");
      const markup = renderToStaticMarkup(React.createElement(ConfigurationField, {
        field: fields.get(key), value: input, settings: { [key]: input }, validationMessage: message,
        onPatch() {}, t, copy: { revealSecret: "Show value", concealSecret: "Hide value",
          restartScopes: { none: "No restart", server: "Restart server", world: "Restart world", cluster: "Restart cluster" } }
      }));
      assert.match(markup, /aria-invalid="true"/);
      assert.match(markup, /aria-errormessage="[^"]+-error"/);
      assert.ok(markup.includes(message));
      if (key.endsWith("password")) assert.match(markup, /type="password"/);
    }
  }
});

test("Abiotic accepts native quoted-value punctuation and retains exact settings drafts", () => {
  const { fields, validate } = fixture("en-US", EN_US_ABIOTIC_FACTOR_MESSAGES);
  for (const key of launchKeys) {
    for (const value of ["synthetic-test-only room", "synthetic-test-only,room", "synthetic-test-only)room", "合成世界 (one,two)"]) {
      assert.equal(validate(key, value), undefined, key);
      assert.equal(writeGuidedFieldValue({}, fields.get(key), value)[key], value);
    }
  }
  for (const key of ["server_password", "admin_password"]) {
    for (const value of [" synthetic-test-only ", " ", "synthetic-test-only room\\"]) {
      assert.equal(validate(key, value), undefined);
      assert.equal(writeGuidedFieldValue({}, fields.get(key), value)[key], value);
    }
  }
  assert.equal(validate("admin_password", " synthetic-test-only ", { server_password: "synthetic-test-only" }), undefined);
});

test("Abiotic world names retain a single Windows-directory boundary", () => {
  const { fields, validate } = fixture("en-US", EN_US_ABIOTIC_FACTOR_MESSAGES);
  for (const value of ["", ".", "..", "../other", "nested/world", "nested\\world", "CON", "NUL", "COM1.txt", "world.", "world ", "world:stream"]) {
    assert.ok(validate("world_save_name", value), value);
  }
  assert.equal(readGuidedFieldValue(fields.get("world_save_name"), {}, { instanceId: "synthetic-test-only" }), "synthetic-test-only");
  assert.equal(definition.getFieldValidationMessage({ field: fields.get("world_save_name"), value: undefined,
    settings: {}, locale: "en-US", t: (_key, _params, fallback) => fallback }), undefined);
  assert.equal(validate("admin_password", "synthetic-test-only", { server_password: "synthetic-test-only" }),
    EN_US_ABIOTIC_FACTOR_MESSAGES["abiotic.settings.groups.useADifferentAdminPasswordSoTheJoinGateAndTheOperatorLaneStaySeparate"]);
});
