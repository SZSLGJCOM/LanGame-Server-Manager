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
  validateGuidedSettingsObject,
  writeGuidedFieldValue
} = require("../src/views/settings/guided-settings.ts");
const schemaJson = fs.readFileSync(path.resolve(__dirname, "../../../modules/humanitz/schema.json"), "utf8");
const schema = parseGuidedSettingsSchema({
  summary: { id: "humanitz", name: "HumanitZ" },
  schema_json: schemaJson
});

function field(key) {
  assert.equal(schema.parseError, null);
  const result = schema.fields.find((candidate) => candidate.key === key);
  assert.ok(result, `${key} must be available in the guided configuration`);
  return result;
}

test("HumanitZ new native settings remain unset until the user chooses an override", () => {
  for (const [key, section, nativeKey] of [
    ["backpack_cleanup_days", "loot", "BackpackCleanup"],
    ["weapon_durability_multiplier", "world", "WeaponDurabilityMulti"]
  ]) {
    const value = field(key);
    assert.equal(value.sectionId, section);
    assert.equal(value.sourceKey, nativeKey);
    assert.equal(readGuidedFieldValue(value, {}), null);
    assert.equal(value.maximum, undefined, "the reference does not establish an upper limit");
    const cleared = writeGuidedFieldValue({ [key]: 2 }, value, "");
    assert.equal(Object.hasOwn(cleared, key), false);
    assert.equal(readGuidedFieldValue(value, cleared), null);
  }
  const headshot = field("headshot_only_mode");
  assert.equal(headshot.sectionId, "difficulty");
  assert.equal(headshot.control, "select");
  assert.equal(readGuidedFieldValue(headshot, {}), "preserve");
  assert.deepEqual(headshot.enumOptions.map((option) => option.value), ["preserve", "enabled", "disabled"]);
});

test("HumanitZ durability supports the official fractional example and cleanup supports zero", () => {
  const durability = field("weapon_durability_multiplier");
  const backpack = field("backpack_cleanup_days");
  let settings = writeGuidedFieldValue({ rcon_password: "fixture-password" }, durability, "0.5");
  settings = writeGuidedFieldValue(settings, backpack, "0");
  assert.equal(settings.weapon_durability_multiplier, 0.5);
  assert.equal(settings.backpack_cleanup_days, 0);
  assert.deepEqual(validateGuidedSettingsObject(schema, settings), []);
  for (const candidate of [durability, backpack]) {
    const invalid = writeGuidedFieldValue({}, candidate, "-1");
    assert.ok(validateGuidedSettingsObject(schema, invalid).some((issue) => issue.fieldKey === candidate.key));
  }
});

test("HumanitZ existing weapon breakage and car recycling remain independent settings", () => {
  assert.equal(field("weapon_break").defaultValue, true);
  assert.equal(field("recycle_car_days").defaultValue, 14);
  assert.equal(field("recycle_car_days").sourceKey, "RecycleCar");
  assert.match(field("recycle_car_days").description, /last login/i);
});

test("HumanitZ optional controls render in their gameplay groups in both languages", () => {
  const React = require("react");
  const { renderToStaticMarkup } = require("react-dom/server");
  const { ConfigurationField } = require("../src/views/settings/ConfigurationField.tsx");
  const { humanitzSettingsDefinition } = require("../src/views/settings/modules/humanitz.ts");
  const { EN_US_HUMANITZ_MESSAGES } = require("../src/i18n/games/humanitz.en.ts");
  const { ZH_CN_HUMANITZ_MESSAGES } = require("../src/i18n/games/humanitz.zh-cn.ts");
  for (const [locale, catalog, preserveLabel] of [
    ["en-US", EN_US_HUMANITZ_MESSAGES, "Keep native setting"],
    ["zh-CN", ZH_CN_HUMANITZ_MESSAGES, "保持原生设置"]
  ]) {
    const t = (key, _params, fallback) => catalog[key] ?? fallback ?? key;
    const localized = parseGuidedSettingsSchema({ summary: { id: "humanitz", name: "HumanitZ" }, schema_json: schemaJson }, locale, t);
    assert.equal(localized.parseError, null);
    for (const [key, groupId] of [
      ["backpack_cleanup_days", "loot-rules"],
      ["weapon_durability_multiplier", "survival-rules"],
      ["headshot_only_mode", "threat-scaling"]
    ]) {
      const candidate = localized.fields.find((value) => value.key === key);
      const groups = humanitzSettingsDefinition.buildFieldGroups(candidate.sectionId, localized.fields.filter((value) => value.sectionId === candidate.sectionId), locale, t);
      assert.ok(groups.find((group) => group.id === groupId).fields.includes(candidate));
      const markup = renderToStaticMarkup(React.createElement(ConfigurationField, {
        field: candidate, settings: {}, value: readGuidedFieldValue(candidate, {}),
        onPatch() {}, t, copy: { restartScopes: { server: "Restart server" } }
      }));
      assert.ok(markup.includes(candidate.title));
      if (key === "headshot_only_mode") {
        assert.match(markup, /<select/);
        assert.ok(markup.includes(preserveLabel));
        assert.match(markup, /<option value="0" selected=""/);
      } else {
        assert.match(markup, /<input/);
        assert.match(markup, /value=""/);
      }
    }
  }
});
