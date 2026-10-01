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
  parseGuidedSettingsSchema, readGuidedFieldValue, validateGuidedSettingsObject,
  writeGuidedFieldValue, serializeSettingsObject, resolveGuidedFieldDefaultValue
} = require("../src/views/settings/guided-settings.ts");
const { satisfactorySettingsDefinition } = require("../src/views/settings/modules/satisfactory.ts");
const { translate } = require("../src/i18n.tsx");
const { EN_US_MESSAGES } = require("../src/i18n-messages.ts");
const { ZH_CN_MESSAGES } = require("../src/i18n-messages-zh-cn.ts");
const details = {
  summary: { id: "satisfactory", name: "Satisfactory" },
  schema_json: fs.readFileSync(path.resolve(__dirname, "../../../modules/satisfactory/schema.json"), "utf8")
};
const nativeOptions = ["auto_pause_when_empty", "network_quality", "send_gameplay_data", "weather_preset"];

function parse(locale, surface = "general") {
  const catalogs = { "en-US": EN_US_MESSAGES, "zh-CN": ZH_CN_MESSAGES };
  const t = (key, params, fallback) => translate(locale, key, params, fallback, catalogs);
  const schema = parseGuidedSettingsSchema(details, locale, t, { surface });
  assert.equal(schema.parseError, null);
  return { schema, t };
}

for (const locale of ["en-US", "zh-CN"]) {
  test(`Satisfactory native options have one ${locale} control in the correct purpose group`, () => {
    const { schema, t } = parse(locale);
    for (const [key, sectionId, groupId] of [
      ["auto_pause_when_empty", "world", "simulation"],
      ["weather_preset", "world", "simulation"],
      ["network_quality", "network", "engine-networking"],
      ["send_gameplay_data", "advanced", "privacy"]
    ]) {
      const fields = schema.fields.filter((field) => field.key === key);
      assert.equal(fields.length, 1, key);
      const field = fields[0];
      assert.equal(field.sectionId, sectionId);
      assert.equal(field.sourceSurface, "materializer");
      assert.match(field.sourceKey, /^\/Script\/FactoryGame\.FGGameUserSettings\.mIntValues\.FG\./);
      const groups = satisfactorySettingsDefinition.buildFieldGroups(sectionId,
        schema.fields.filter((entry) => entry.sectionId === sectionId), locale, t);
      assert.equal(groups.find((group) => group.id === groupId).fields.filter((entry) => entry.key === key).length, 1);
      if (locale === "zh-CN") assert.match(field.title, /[\u3400-\u9fff]/);
    }
    const privacy = schema.fields.find((field) => field.key === "send_gameplay_data");
    assert.match(privacy.description, locale === "zh-CN" ? /下次.*启动/ : /next server start/i);
    assert.ok(!schema.fields.some((field) => field.key === "rotating_autosaves"));
    const maintenance = parse(locale, "maintenance").schema;
    assert.ok(maintenance.fields.some((field) => field.key === "rotating_autosaves"));
    for (const surface of ["maintenance", "mods", "player_access"]) {
      assert.deepEqual(parse(locale, surface).schema.fields.filter((field) => nativeOptions.includes(field.key)), []);
    }
    assert.ok(!schema.fields.some((field) => /autosave|restart.*(?:interval|time)|game_mode/i.test(field.key)));
  });
}

test("Satisfactory native defaults remain documented without pretending to read unmanaged values", () => {
  const { schema } = parse("en-US");
  const initial = { max_players: 8, future_setting: { retained: true } };
  for (const [key, fallback, changed] of [
    ["auto_pause_when_empty", true, false], ["network_quality", 1, 2], ["send_gameplay_data", true, false], ["weather_preset", undefined, 6]
  ]) {
    const field = schema.fields.find((entry) => entry.key === key);
    assert.ok(field, key);
    assert.equal(field.defaultValue, fallback);
    assert.equal(field.preserveNativeWhenUnset, true);
    assert.equal(readGuidedFieldValue(field, initial), undefined);
    assert.equal(resolveGuidedFieldDefaultValue(field), undefined);
    assert.equal(Object.hasOwn(initial, key), false);
    const persisted = JSON.parse(serializeSettingsObject(writeGuidedFieldValue(initial, field, changed)));
    assert.equal(persisted[key], changed);
    assert.deepEqual(persisted.future_setting, initial.future_setting);
    assert.equal(persisted.max_players, 8);
    assert.equal(readGuidedFieldValue(field, persisted), changed);
    const released = writeGuidedFieldValue(persisted, field, undefined);
    assert.equal(Object.hasOwn(released, key), false);
    assert.equal(Object.hasOwn(JSON.parse(serializeSettingsObject(released)), key), false);
    assert.deepEqual(released.future_setting, initial.future_setting);
  }
  assert.deepEqual(schema.fields.filter((field) => field.preserveNativeWhenUnset).map((field) => field.key).sort(),
    nativeOptions.slice().sort());
  assert.equal(satisfactorySettingsDefinition.initializeSettings, undefined);
});

test("native preservation metadata rejects fields without an optional scalar materializer contract", () => {
  for (const invalid of [
    { type: "string", "x-lsgm-source-surface": "materializer" },
    { type: "boolean", "x-lsgm-source-surface": "config_file" },
    { type: "integer", "x-lsgm-source-surface": "materializer" }
  ]) {
    const schema = parseGuidedSettingsSchema({ ...details, schema_json: JSON.stringify({ type: "object", properties: {
      auto_pause_when_empty: { ...invalid, "x-lsgm-section": "world", "x-lsgm-preserve-native-when-unset": true }
    } }) }, "en-US");
    assert.match(schema.parseError, /Native preservation requires/);
  }
  const original = JSON.parse(details.schema_json);
  original.required = ["auto_pause_when_empty"];
  assert.match(parseGuidedSettingsSchema({ ...details, schema_json: JSON.stringify(original) }, "en-US").parseError,
    /Native preservation requires/);
});

test("Satisfactory network quality accepts only the four native integer presets", () => {
  const { schema } = parse("en-US");
  const field = schema.fields.find((entry) => entry.key === "network_quality");
  assert.ok(field);
  assert.equal(field.control, "select");
  assert.deepEqual(field.enumOptions.map((option) => option.value), [0, 1, 2, 3]);
  for (const value of [0, 1, 2, 3]) {
    assert.deepEqual(validateGuidedSettingsObject(schema, { network_quality: value })
      .filter((issue) => issue.fieldKey === "network_quality"), []);
  }
  for (const value of [-1, 4, 1.5]) {
    assert.ok(validateGuidedSettingsObject(schema, { network_quality: value })
      .some((issue) => issue.fieldKey === "network_quality"));
  }
});

test("Satisfactory weather offers the seven native presets and rejects other values", () => {
  for (const locale of ["en-US", "zh-CN"]) {
    const { schema } = parse(locale);
    const field = schema.fields.find((entry) => entry.key === "weather_preset");
    assert.equal(field.control, "select");
    assert.deepEqual(field.enumOptions.map((option) => option.value), [0, 1, 2, 3, 4, 5, 6]);
    assert.match(field.description, locale === "zh-CN" ? /重启后生效/ : /after the server restarts/);
    for (const value of [0, 1, 2, 3, 4, 5, 6]) {
      assert.deepEqual(validateGuidedSettingsObject(schema, { weather_preset: value })
        .filter((issue) => issue.fieldKey === "weather_preset"), []);
    }
    for (const value of [-1, 7, 1.5]) {
      assert.ok(validateGuidedSettingsObject(schema, { weather_preset: value })
        .some((issue) => issue.fieldKey === "weather_preset"));
    }
  }
});
