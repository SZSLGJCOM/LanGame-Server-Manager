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
  parseGuidedSettingsSchema, validateGuidedSettingsObject
} = require("../src/views/settings/guided-settings.ts");
const { enshroudedSettingsDefinition } = require("../src/views/settings/modules/enshrouded.ts");
const { buildConfigurationWorkspaceModel } = require("../src/views/settings/configuration-workspace-model.ts");
const { EN_US_MESSAGES } = require("../src/i18n-messages.ts");
const { ZH_CN_MESSAGES } = require("../src/i18n-messages-zh-cn.ts");
const schemaJson = fs.readFileSync(path.resolve(__dirname, "../../../modules/enshrouded/schema.json"), "utf8");

function translator(catalog) {
  return (key, params, fallback) => String(catalog[key] ?? fallback ?? key).replace(
    /\{\s*([\w.]+)\s*\}/g,
    (match, paramKey) => params?.[paramKey] == null ? match : String(params[paramKey])
  );
}

function settingsSchema(locale = "en-US", catalog = EN_US_MESSAGES) {
  const t = translator(catalog);
  const schema = parseGuidedSettingsSchema({
    summary: { id: "enshrouded", name: "Enshrouded" }, schema_json: schemaJson
  }, locale, t);
  assert.equal(schema.parseError, null);
  return { schema, t };
}

test("Enshrouded 1.0 Shroud removal accepts the effective native integer level boundaries", () => {
  const { schema } = settingsSchema();
  const flame = schema.fields.find((field) => field.key === "flame_level_fog_removal");
  assert.ok(flame, "the native Flame level must have a guided editor");
  assert.equal(flame.defaultValue, 11);
  const flameSchema = { ...schema, fields: [flame] };
  for (const [value, accepted] of [[1, true], [11, true], [0, false], [12, false], [20, false], [21, false], [-1, false], [10.5, false]]) {
    const settings = { flame_level_fog_removal: value };
    assert.equal(validateGuidedSettingsObject(flameSchema, settings).length, accepted ? 0 : 1, String(value));
    assert.equal(settings.flame_level_fog_removal, value, "validation must not clamp a stored setting");
  }
});

test("Automatic applies to encounter size while existing enemy defaults and attack limits remain stable", () => {
  const { schema } = settingsSchema();
  const amount = schema.fields.find((field) => field.key === "random_spawner_amount");
  const attacks = schema.fields.find((field) => field.key === "aggro_pool_amount");
  assert.ok(amount);
  assert.ok(attacks);
  assert.equal(amount.defaultValue, "Normal");
  assert.equal(attacks.defaultValue, "Normal");
  const combatSchema = { ...schema, fields: [amount, attacks] };
  for (const value of ["Few", "Normal", "Many", "Extreme", "Automatic"]) {
    assert.deepEqual(validateGuidedSettingsObject(combatSchema, {
      random_spawner_amount: value, aggro_pool_amount: "Normal"
    }), []);
  }
  const issues = validateGuidedSettingsObject(combatSchema, {
    random_spawner_amount: "Normal", aggro_pool_amount: "Automatic"
  });
  assert.equal(issues.length, 1);
  assert.equal(issues[0].fieldKey, "aggro_pool_amount");
  assert.equal(issues[0].reason, "enum");
});

test("both languages explain party scaling and show the Flame requirement in progression", () => {
  for (const [locale, catalog, expectedTitle, expectedScaling, expectedAutomatic] of [
    ["en-US", EN_US_MESSAGES, /Flame Level/, /nearby active players/, "Automatic"],
    ["zh-CN", ZH_CN_MESSAGES, /灵火等级/, /附近实际参与的玩家人数/, "自动"]
  ]) {
    const { schema, t } = settingsSchema(locale, catalog);
    const flame = schema.fields.find((field) => field.key === "flame_level_fog_removal");
    const amount = schema.fields.find((field) => field.key === "random_spawner_amount");
    assert.ok(flame);
    assert.ok(amount);
    assert.match(flame.title, expectedTitle);
    assert.match(flame.description, /11/);
    assert.match(amount.description, expectedScaling);
    assert.equal(amount.enumOptions.find((option) => option.value === "Automatic")?.label, expectedAutomatic);
    const groups = enshroudedSettingsDefinition.buildFieldGroups("progression", [flame], locale, t);
    assert.equal(groups.length, 1);
    assert.equal(groups[0].id, "progression");
    assert.equal(groups[0].fields[0].key, flame.key);
  }
});

test("every Enshrouded gameplay category explains the Custom preset requirement", () => {
  for (const [locale, catalog] of [["en-US", EN_US_MESSAGES], ["zh-CN", ZH_CN_MESSAGES]]) {
    const { schema, t } = settingsSchema(locale, catalog);
    const requirement = t("enshrouded.settings.customPresetRequirement");
    assert.notEqual(requirement, "enshrouded.settings.customPresetRequirement");
    for (const id of ["players", "survival", "world", "progression", "combat"]) {
      const section = schema.sections.find((entry) => entry.id === id);
      assert.ok(section?.description?.includes(requirement), `${locale}.${id}: explain when custom values apply`);
    }
  }
});

test("Enshrouded keeps one editor per setting and player bans outside configuration", () => {
  const { schema, t } = settingsSchema();
  const properties = JSON.parse(schemaJson).properties;
  assert.equal(Object.keys(properties).length, 75);
  assert.equal(schema.fields.length, 74);
  assert.equal(schema.fields.filter((field) => field.sourceKey?.startsWith("gameSettings.")).length, 38);
  assert.equal(schema.presentationFields.find((field) => field.key === "banned_player_ids")?.presentation.owner, "player_access");
  assert.equal(schema.fields.some((field) => field.key === "banned_player_ids"), false);
  assert.equal(schema.fields.some((field) => field.presentation.owner === "mods"), false);
  for (const section of schema.sections) {
    const fields = schema.fields.filter((field) => field.sectionId === section.id);
    const groups = enshroudedSettingsDefinition.buildFieldGroups(section.id, fields, "en-US", t);
    const groupedKeys = groups.flatMap((group) => group.fields.map((field) => field.key));
    assert.equal(new Set(groupedKeys).size, groupedKeys.length, `${section.id}: no duplicate editor`);
    assert.deepEqual(groupedKeys.slice().sort(), fields.map((field) => field.key).sort());
  }
  const model = buildConfigurationWorkspaceModel(schema);
  assert.equal(model.actionableSectionIds.includes("moderation"), false);
  for (const role of ["admin", "friend", "guest", "visitor"]) {
    const fields = schema.fields.filter((field) => field.sectionId === `${role}_role`);
    assert.equal(fields.length, 7);
    assert.equal(schema.sections.find((section) => section.id === `${role}_role`)?.parentId, "access");
  }
});

test("Enshrouded time editors use minutes and production wording describes elapsed duration", () => {
  for (const [locale, catalog, minutes, duration] of [
    ["en-US", EN_US_MESSAGES, /minutes/i, /how long.*production takes/i],
    ["zh-CN", ZH_CN_MESSAGES, /分钟/, /生产.*耗时/]
  ]) {
    const { schema } = settingsSchema(locale, catalog);
    for (const [key, defaultValue] of [["hunger_to_starving_minutes", 10], ["day_time_minutes", 30], ["night_time_minutes", 12]]) {
      const field = schema.fields.find((entry) => entry.key === key);
      assert.ok(field);
      assert.match(field.title, minutes);
      assert.equal(field.defaultValue, defaultValue);
      assert.equal(field.type, "integer");
    }
    assert.match(schema.fields.find((field) => field.key === "factory_production_speed_factor").description, duration);
  }
});
