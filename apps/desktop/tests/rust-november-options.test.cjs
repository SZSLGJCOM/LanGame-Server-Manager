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

const {
  parseGuidedSettingsSchema,
  readGuidedFieldValue,
  writeGuidedFieldValue,
  validateGuidedSettingsObject
} = require("../src/views/settings/guided-settings.ts");
const { isOptionalBooleanOverride } = require("../src/views/settings/guided-setting-values.ts");
const { ConfigurationField } = require("../src/views/settings/ConfigurationField.tsx");
const { rustSettingsDefinition } = require("../src/views/settings/modules/rust.ts");
const schemaText = fs.readFileSync(path.resolve(__dirname, "../../../modules/rust/schema.json"), "utf8");
const performanceKeys = ["network_position_lod", "network_position_lod_players", "batch_packets", "player_update_jobs"];

function parsed(locale = "en-US") {
  const suffix = locale === "zh-CN" ? "zh-cn" : "en";
  const exports = require(`../src/i18n/games/rust.${suffix}.ts`);
  const catalog = Object.values(exports)[0];
  const t = (key, _params, fallback) => catalog[key] ?? fallback ?? key;
  const schema = parseGuidedSettingsSchema({
    summary: { id: "rust", name: "Rust" }, schema_json: schemaText
  }, locale, t);
  assert.equal(schema.parseError, null);
  return { schema, t };
}

test("Rust navigation shows the new default while preserving saved old-navigation choices", () => {
  const { schema } = parsed();
  const field = schema.fields.find((entry) => entry.key === "use_new_navmesh");
  assert.equal(isOptionalBooleanOverride(field), false);
  assert.equal(field.defaultValue, true);
  assert.equal(readGuidedFieldValue(field, {}), true);
  for (const value of [true, false]) {
    const saved = { use_new_navmesh: value };
    assert.equal(readGuidedFieldValue(field, saved), value);
    assert.deepEqual(writeGuidedFieldValue({}, field, value), saved);
  }
  const render = (settings) => renderToStaticMarkup(React.createElement(ConfigurationField, {
    field, value: readGuidedFieldValue(field, settings), settings, onPatch() {},
    copy: { concealSecret: "Hide", revealSecret: "Show", restartScopes: {} }
  }));
  assert.match(render({}), /type="checkbox"[^>]*checked=""/);
  assert.doesNotMatch(render({ use_new_navmesh: false }), /checked=""/);
  assert.match(render({ use_new_navmesh: true }), /type="checkbox"[^>]*checked=""/);
  assert.match(field.description, /-useOldNavmesh/);
  assert.doesNotMatch(field.description, /opt in|opt-in|-useNewNavmesh/i);
});

test("Rust performance controls preserve unset, zero and false independently", () => {
  const { schema, t } = parsed();
  for (const key of performanceKeys) {
    const field = schema.fields.find((entry) => entry.key === key);
    assert.ok(field, key);
    assert.equal(field.defaultValue, undefined, key);
    assert.equal(field.preserveNativeWhenUnset, true, key);
    assert.equal(readGuidedFieldValue(field, {}), undefined, key);
  }
  const batch = schema.fields.find((entry) => entry.key === "batch_packets");
  assert.ok(isOptionalBooleanOverride(batch));
  assert.deepEqual(writeGuidedFieldValue({}, batch, false), { batch_packets: false });
  assert.deepEqual(writeGuidedFieldValue({ batch_packets: false }, batch, undefined), {});
  for (const key of ["network_position_lod", "network_position_lod_players"]) {
    const field = schema.fields.find((entry) => entry.key === key);
    for (const value of [0, 1, 2]) {
      assert.deepEqual(validateGuidedSettingsObject(schema, { [key]: value }).filter((issue) => issue.fieldKey === key), []);
    }
    for (const value of [-1, 3, 1.5, "1"]) {
      assert.ok(validateGuidedSettingsObject(schema, { [key]: value }).some((issue) => issue.fieldKey === key), `${key}: ${value}`);
    }
    assert.deepEqual(writeGuidedFieldValue({ [key]: 0 }, field, undefined), {});
  }
  for (const value of [3, 4]) {
    assert.deepEqual(validateGuidedSettingsObject(schema, { player_update_jobs: value }).filter((issue) => issue.fieldKey === "player_update_jobs"), []);
  }
  for (const value of [0, 2, 5, 3.5, "4"]) {
    assert.ok(validateGuidedSettingsObject(schema, { player_update_jobs: value }).some((issue) => issue.fieldKey === "player_update_jobs"), `player_update_jobs: ${value}`);
  }
  const groups = rustSettingsDefinition.buildFieldGroups("advanced", schema.fields.filter((field) => field.sectionId === "advanced"), "en-US", t);
  assert.deepEqual(groups.find((group) => group.id === "server-performance").fields.map((field) => field.key), performanceKeys);
});

test("Rust November settings and choices have authored English and Chinese copy", () => {
  for (const locale of ["en-US", "zh-CN"]) {
    const { schema } = parsed(locale);
    for (const key of [...performanceKeys, "use_new_navmesh"]) {
      const field = schema.fields.find((entry) => entry.key === key);
      assert.ok(field.title.length > 2, key);
      assert.ok(field.description.length > 20, key);
      if (locale === "zh-CN") assert.match(field.title, /[\u4e00-\u9fff]/);
      for (const option of field.enumOptions ?? []) {
        assert.ok(option.label.length > 0, `${key}: ${option.value}`);
        if (locale === "zh-CN") assert.match(option.label, /[\u4e00-\u9fff]/);
      }
    }
  }
});
