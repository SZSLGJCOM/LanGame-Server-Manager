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

const { parseGuidedSettingsSchema, validateGuidedSettingsObject } = require("../src/views/settings/guided-settings.ts");
const { ConfigurationField } = require("../src/views/settings/ConfigurationField.tsx");
const { EN_US_VALHEIM_MESSAGES } = require("../src/i18n/games/valheim.en.ts");
const { ZH_CN_VALHEIM_MESSAGES } = require("../src/i18n/games/valheim.zh-cn.ts");
const schemaJson = fs.readFileSync(path.resolve(__dirname, "../../../modules/valheim/schema.json"), "utf8");

function readSchema(locale) {
  const messages = locale === "zh-CN" ? ZH_CN_VALHEIM_MESSAGES : EN_US_VALHEIM_MESSAGES;
  const t = (key, _params, fallback) => messages[key] ?? fallback ?? key;
  return parseGuidedSettingsSchema({ summary: { id: "valheim", name: "Valheim" }, schema_json: schemaJson }, locale, t);
}

test("Valheim offers an explicit Normal preset separately from preserving saved rules", () => {
  for (const [locale, keepLabel, normalLabel] of [
    ["en-US", "Keep current rules", "Normal"],
    ["zh-CN", "保留当前规则", "标准"]
  ]) {
    const schema = readSchema(locale);
    assert.equal(schema.parseError, null);
    const field = schema.fields.find((entry) => entry.key === "world_preset");
    assert.equal(field.defaultValue, "");
    assert.equal(field.enumOptions.find((option) => option.value === "").label, keepLabel);
    assert.equal(field.enumOptions.find((option) => option.value === "normal").label, normalLabel);
    assert.equal(validateGuidedSettingsObject(schema, { world_preset: "normal" }).some((issue) => issue.fieldKey === "world_preset"), false);
    const html = renderToStaticMarkup(React.createElement(ConfigurationField, {
      field, value: "normal", settings: { world_preset: "normal" }, onPatch() {},
      copy: { concealSecret: "Hide", revealSecret: "Show", restartScopes: { none: "", server: "Restart", world: "New world", cluster: "Restart" } }
    }));
    assert.ok(html.includes(keepLabel));
    assert.match(html, new RegExp(`selected="">${normalLabel}</option>`));
  }
});

test("Valheim validates documented modifier pairs and world keys before saving", () => {
  const schema = readSchema("en-US");
  const cases = [
    ["world_modifiers", "", true],
    ["world_modifiers", "combat hard\r\ndeathpenalty casual;resources most,raids none\nportals veryhard", true],
    ["world_modifiers", "combat", false],
    ["world_modifiers", "combat hard extra", false],
    ["world_modifiers", "combat most", false],
    ["world_modifiers", "combat hard\n-password secret", false],
    ["world_set_keys", "playerevents\nnomap", true],
    ["world_set_keys", "players=8", false],
    ["world_set_keys", "nomap unexpected", false]
  ];
  for (const [key, value, valid] of cases) {
    const issues = validateGuidedSettingsObject(schema, { [key]: value });
    assert.equal(issues.some((issue) => issue.fieldKey === key), !valid, `${key}: ${value}`);
  }
});
