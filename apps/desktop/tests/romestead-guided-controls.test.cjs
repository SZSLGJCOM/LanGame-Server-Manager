const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) =>
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
}
require.extensions[".css"] = (module) => module._compile("", module.filename);

const {
  parseGuidedSettingsSchema, readGuidedFieldValue, validateGuidedSettingsObject, writeGuidedFieldValue
} = require("../src/views/settings/guided-settings.ts");
const { buildConfigurationWorkspaceModel } = require("../src/views/settings/configuration-workspace-model.ts");
const { romesteadSettingsDefinition } = require("../src/views/settings/modules/romestead.ts");
const { RomesteadSleepThresholdField } = require("../src/views/settings/RomesteadSleepThresholdField.tsx");
const { EN_US_ROMESTEAD_MESSAGES } = require("../src/i18n/games/romestead.en.ts");
const { ZH_CN_ROMESTEAD_MESSAGES } = require("../src/i18n/games/romestead.zh-cn.ts");
const { EN_US_NIGHTINGALE_MESSAGES } = require("../src/i18n/games/nightingale.en.ts");
const { ZH_CN_NIGHTINGALE_MESSAGES } = require("../src/i18n/games/nightingale.zh-cn.ts");
const schemaJson = fs.readFileSync(path.resolve(__dirname, "../../../modules/romestead/schema.json"), "utf8");

function context(locale) {
  const messages = locale === "zh-CN" ? ZH_CN_ROMESTEAD_MESSAGES : EN_US_ROMESTEAD_MESSAGES;
  const t = (key, _params, fallback) => messages[key] ?? fallback ?? key;
  const schema = parseGuidedSettingsSchema({ summary: { id: "romestead", name: "Romestead" }, schema_json: schemaJson }, locale, t);
  assert.equal(schema.parseError, null);
  return { schema, locale, t };
}

test("Romestead world size offers the three localized native integer choices", () => {
  for (const [locale, expected] of [["en-US", ["Small", "Standard", "Large"]], ["zh-CN", ["小型", "标准", "大型"]]]) {
    const { schema } = context(locale);
    const field = schema.fields.find((candidate) => candidate.key === "auto_create_world_size");
    assert.equal(field.control, "select");
    assert.deepEqual(field.enumOptions, expected.map((label, value) => ({ label, value })));
    assert.equal(readGuidedFieldValue(field, {}), 1);
    for (const value of [0, 1, 2]) {
      assert.deepEqual(writeGuidedFieldValue({ future_setting: "retained" }, field, value), {
        future_setting: "retained", auto_create_world_size: value
      });
      assert.equal(validateGuidedSettingsObject(schema, { auto_create_world_size: value }).length, 0);
    }
    for (const value of [-1, 3, 1.5, "Large"]) {
      assert.ok(validateGuidedSettingsObject(schema, { auto_create_world_size: value })
        .some((issue) => issue.fieldKey === field.key));
    }
  }
});

test("Romestead retains the default and every legal native fractional sleep threshold", () => {
  const { schema, t, locale } = context("en-US");
  const field = schema.fields.find((candidate) => candidate.key === "sleep_threshold_ms");
  assert.equal(readGuidedFieldValue(field, {}), 10);
  for (const value of [-1, 1, 1.00123456789, 10, 13.875, 16.5]) {
    assert.equal(romesteadSettingsDefinition.getFieldValidationMessage({ field, value, settings: {}, t, locale }), undefined);
    const normalized = writeGuidedFieldValue({ future_setting: "retained" }, field, String(value));
    assert.equal(normalized.sleep_threshold_ms, value);
    assert.equal(normalized.future_setting, "retained");
    assert.equal(validateGuidedSettingsObject(schema, normalized).length, 0);
  }
  for (const value of [-2, 0, 0.9999, 16.50001, NaN, Infinity, "", "invalid", true, null, [1]]) {
    assert.ok(romesteadSettingsDefinition.getFieldValidationMessage({ field, value, settings: {}, t, locale }), String(value));
  }
});

test("Romestead controls keep world rules and performance within the shared configuration hierarchy", () => {
  const { schema } = context("zh-CN");
  const model = buildConfigurationWorkspaceModel(schema);
  const world = schema.fields.find((field) => field.key === "auto_create_world_size");
  const sleep = schema.fields.find((field) => field.key === "sleep_threshold_ms");
  const launch = schema.fields.find((field) => field.key === "extra_launch_args");
  assert.equal(world.sectionId, "world");
  assert.equal(sleep.sectionId, "performance");
  assert.equal(launch.sectionId, "advanced");
  assert.equal(schema.sections.find((section) => section.id === "performance").parentId, "runtime");
  assert.equal(sleep.presentation.owner, "configuration");
  assert.equal(sleep.presentation.state, "specialized");
  const renderer = romesteadSettingsDefinition.specializedRenderers[sleep.presentation.rendererId];
  assert.equal(renderer.Renderer, RomesteadSleepThresholdField);
  assert.equal(renderer.sectionId, "performance");
  assert.equal(renderer.keepMounted, true, "the remembered threshold survives configuration section changes");
  assert.equal(renderer.fieldKey, undefined, "the real field owns the focus ID");
  assert.ok(model);
  assert.equal(schema.fields.length, 9, "controls do not add persisted synthetic fields");
});

test("Nightingale advanced help directs operators to the existing status controls", () => {
  for (const catalog of [EN_US_NIGHTINGALE_MESSAGES, ZH_CN_NIGHTINGALE_MESSAGES]) {
    const help = catalog["nightingale.settings.groups.advanced.description"];
    assert.doesNotMatch(help, /-statusPort/);
    assert.match(help, /HTTP/);
    assert.match(help, /Network|网络/);
  }
});
