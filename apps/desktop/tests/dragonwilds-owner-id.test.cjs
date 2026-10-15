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

const { ConfigurationField } = require("../src/views/settings/ConfigurationField.tsx");
const {
  parseGuidedSettingsSchema, readGuidedFieldValue, serializeSettingsObject,
  validateGuidedSettingsObject, writeGuidedFieldValue
} = require("../src/views/settings/guided-settings.ts");
const { EN_US_MESSAGES } = require("../src/i18n-messages.ts");
const { ZH_CN_MESSAGES } = require("../src/i18n-messages-zh-cn.ts");
const schemaJson = fs.readFileSync(path.resolve(__dirname, "../../../modules/runescapedragonwilds/schema.json"), "utf8");

function translator(catalog) {
  return (key, params, fallback) => String(catalog[key] ?? fallback ?? key).replace(
    /\{\s*([\w.]+)\s*\}/g,
    (match, paramKey) => params?.[paramKey] == null ? match : String(params[paramKey])
  );
}

function ownerSchema(locale = "en-US", catalog = EN_US_MESSAGES) {
  const t = translator(catalog);
  const parsed = parseGuidedSettingsSchema({
    summary: { id: "runescapedragonwilds", name: "Dragonwilds" }, schema_json: schemaJson
  }, locale, t);
  assert.equal(parsed.parseError, null);
  const owner = parsed.fields.find((field) => field.key === "owner_id");
  assert.ok(owner, "the native OwnerId must have a guided editor");
  return { schema: { ...parsed, fields: [owner] }, owner, t };
}

test("Dragonwilds OwnerId applies its native UTF-16 boundary without a standard character limit", () => {
  const { schema, owner } = ownerSchema();
  assert.equal(owner.maxUtf16Length, 32);
  assert.equal(owner.maxLength, undefined, "native identity limits must not truncate pasted input");
  for (const [value, accepted] of [
    ["", true], ["a".repeat(32), true], ["a".repeat(33), false],
    ["😀".repeat(16), true], ["😀".repeat(17), false]
  ]) {
    const settings = { owner_id: value };
    const issues = validateGuidedSettingsObject(schema, settings);
    assert.deepEqual(issues.map((issue) => issue.reason), accepted ? [] : ["maxUtf16Length"]);
    assert.equal(settings.owner_id, value, "validation must not alter an account identity");
  }
});

test("editing an overlong OwnerId retains the complete value and renders an error without maxlength", () => {
  const { schema, owner, t } = ownerSchema();
  for (const value of ["a".repeat(33), "😀".repeat(17)]) {
    const original = { owner_id: "previous-id", future_setting: { retain: true } };
    const edited = writeGuidedFieldValue(original, owner, value);
    const persisted = JSON.parse(serializeSettingsObject(edited));
    assert.equal(readGuidedFieldValue(owner, persisted), value);
    assert.equal(original.owner_id, "previous-id");
    assert.deepEqual(persisted.future_setting, original.future_setting);
    const issues = validateGuidedSettingsObject(schema, persisted, undefined, t);
    assert.equal(issues.length, 1);
    const html = renderToStaticMarkup(React.createElement(ConfigurationField, {
      copy: { concealSecret: "Hide value", revealSecret: "Show value", restartScopes: {
        cluster: "Restart cluster", none: "No restart", server: "Restart server", world: "Restart world"
      } },
      field: owner, onPatch: () => undefined, settings: persisted, value,
      validationMessage: issues[0].message
    }));
    assert.doesNotMatch(html, /\bmaxlength=/i);
    assert.ok(html.includes(`value="${value}"`), "the input must retain every pasted character");
    assert.match(html, /aria-invalid="true"/);
  }
});

test("native length errors use the active language without exposing a truncation action", () => {
  for (const [locale, catalog, expected] of [
    ["en-US", EN_US_MESSAGES, /game.*32/i],
    ["zh-CN", ZH_CN_MESSAGES, /游戏.*32/]
  ]) {
    const { schema, t } = ownerSchema(locale, catalog);
    const [issue] = validateGuidedSettingsObject(schema, { owner_id: "a".repeat(33) }, undefined, t);
    assert.ok(issue);
    assert.match(issue.message, expected);
  }
});

test("standard schema maxLength keeps Unicode code-point semantics", () => {
  const schema = parseGuidedSettingsSchema({
    summary: { id: "unicode-contract", name: "Unicode contract" },
    schema_json: JSON.stringify({ type: "object", properties: {
      text: { type: "string", maxLength: 1 }
    } })
  }, "en-US", translator({}));
  assert.equal(schema.parseError, null);
  assert.deepEqual(validateGuidedSettingsObject(schema, { text: "😀" }), []);
  assert.deepEqual(validateGuidedSettingsObject(schema, { text: "😀😀" }).map((issue) => issue.reason), ["maxLength"]);
});
