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

const { parseGuidedSettingsSchema, validateGuidedSettingsObject } = require("../src/views/settings/guided-settings.ts");
const { valheimSettingsDefinition } = require("../src/views/settings/modules/valheim.ts");
const { ValheimWorldRulesPanel } = require("../src/views/settings/ValheimWorldRulesPanel.tsx");
const {
  readValheimWorldRuleContract, parseValheimRuleEntries, readValheimModifier,
  replaceValheimModifier, replaceValheimWorldKey, isValheimModifierEntry,
  valheimPresetKeys, readSavedValheimPreset, readSavedValheimModifier, resolveSavedValheimKeys
} = require("../src/views/settings/valheim-world-rules.ts");
const { EN_US_VALHEIM_MESSAGES } = require("../src/i18n/games/valheim.en.ts");
const { ZH_CN_VALHEIM_MESSAGES } = require("../src/i18n/games/valheim.zh-cn.ts");
const schemaJson = fs.readFileSync(path.resolve(__dirname, "../../../modules/valheim/schema.json"), "utf8");
const { parseValheimWorldRules } = require("../src/valheim-world.ts");
const { ValheimWorldRulesProvider } = require("../src/views/settings/ValheimWorldRulesContext.tsx");

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
    const renderer = valheimSettingsDefinition.specializedRenderers[field.presentation.rendererId];
    assert.equal(renderer.Renderer, ValheimWorldRulesPanel);
    assert.equal(renderer.fieldKey, "world_preset");
    assert.deepEqual(readValheimWorldRuleContract(schemaJson).presets, field.enumOptions.map((option) => option.value));
  }
});

test("Valheim guided choices use the native schema and translate every supported pair", () => {
  const contract = readValheimWorldRuleContract(schemaJson);
  assert.deepEqual(contract.modifiers, {
    combat: ["veryeasy", "easy", "hard", "veryhard"],
    deathpenalty: ["casual", "veryeasy", "easy", "hard", "hardcore"],
    resources: ["muchless", "less", "more", "muchmore", "most"],
    raids: ["none", "muchless", "less", "more", "muchmore"],
    portals: ["casual", "hard", "veryhard"]
  });
  assert.deepEqual(contract.keys, ["nobuildcost", "playerevents", "passivemobs", "nomap"]);
  for (const [name, choices] of Object.entries(contract.modifiers)) {
    for (const choice of choices) {
      assert.equal(isValheimModifierEntry(`${name} ${choice}`, contract), true);
      for (const messages of [EN_US_VALHEIM_MESSAGES, ZH_CN_VALHEIM_MESSAGES]) {
        assert.ok(messages[`valheim.settings.rules.${name}.option.${choice}`]);
      }
    }
  }
  const reducedSchema = JSON.parse(schemaJson);
  reducedSchema.properties.world_modifiers.pattern = reducedSchema.properties.world_modifiers.pattern.replace("|veryhard", "");
  assert.deepEqual(readValheimWorldRuleContract(JSON.stringify(reducedSchema)).modifiers.combat, ["veryeasy", "easy", "hard"]);
});

test("Valheim rule updates preserve unrelated overrides and never invent a preset", () => {
  const original = "combat easy\r\nresources most;raids none,combat hard\nfuture_rule retained";
  assert.deepEqual(parseValheimRuleEntries(original), ["combat easy", "resources most", "raids none", "combat hard", "future_rule retained"]);
  assert.equal(readValheimModifier(original, "combat"), "hard");
  assert.equal(replaceValheimModifier(original, "combat", "veryhard"), "resources most\nraids none\nfuture_rule retained\ncombat veryhard");
  assert.equal(replaceValheimModifier(original, "combat", ""), "resources most\nraids none\nfuture_rule retained");
  assert.equal(replaceValheimModifier(undefined, "combat", ""), "");
  const keys = "nomap,playerevents;nomap\r\nfuture_key";
  assert.equal(replaceValheimWorldKey(keys, "nomap", false), "playerevents\nfuture_key");
  assert.equal(replaceValheimWorldKey(keys, "nomap", true), "playerevents\nfuture_key\nnomap");
  assert.equal(replaceValheimWorldKey(undefined, "nomap", false), "");
});

test("Valheim retains malformed rules until an explicit repair and routes originals to guided panels", () => {
  const contract = readValheimWorldRuleContract(schemaJson);
  for (const entry of ["combat", "combat hard extra", "combat normal", "future easy", "-password secret"]) {
    assert.equal(isValheimModifierEntry(entry, contract), false, entry);
  }
  for (const invalid of [null, 5, ["combat hard", "resources most"], { rule: "nomap" }]) {
    assert.throws(() => replaceValheimModifier(invalid, "combat", "easy"), /must be stored as text/);
    assert.throws(() => replaceValheimWorldKey(invalid, "nomap", false), /must be stored as text/);
  }
  for (const key of ["world_preset", "world_modifiers", "world_set_keys"]) {
    const field = readSchema("zh-CN").fields.find((candidate) => candidate.key === key);
    const renderer = valheimSettingsDefinition.specializedRenderers[field.presentation.rendererId];
    assert.equal(field.presentation.state, "specialized");
    assert.equal(renderer.kind, "module-addon");
    assert.equal(renderer.fieldKey, key);
    assert.equal(renderer.placement, "before-fields");
    assert.equal(field.editorVariant, undefined);
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

test("Valheim saved rule display is based on native serialized preset and modifier keys", () => {
  assert.equal(valheimSettingsDefinition.workspaceProvider, ValheimWorldRulesProvider);
  const sections = valheimSettingsDefinition.getSections((key, _args, fallback) => ZH_CN_VALHEIM_MESSAGES[key] ?? fallback);
  assert.equal(sections.find((section) => section.id === "world").title, "世界规则");
  assert.equal(readSavedValheimPreset([]), "normal");
  assert.equal(readSavedValheimPreset(valheimPresetKeys("casual")), "casual");
  assert.equal(readSavedValheimPreset(["preset hard"]), "hard");
  assert.equal(readSavedValheimModifier(["preset combat_hard:resources_most", "future_rule 99"], "combat"), "hard");
  assert.equal(readSavedValheimModifier(["preset combat_hard:resources_most", "future_rule 99"], "resources"), "most");
  assert.ok(resolveSavedValheimKeys(["preset hammer", "future_rule 99"]).includes("nobuildcost"));
  assert.ok(resolveSavedValheimKeys(["preset hard", "nomap"]).includes("nomap"));
  assert.equal(readSavedValheimPreset(["future_rule 99"]), "custom");
  assert.equal(valheimPresetKeys("future"), null);
  assert.equal(readSavedValheimModifier([], "combat"), "normal");
  assert.equal(readSavedValheimModifier(valheimPresetKeys("hard"), "combat"), "hard");
  assert.equal(readSavedValheimModifier(["preset custom", "ResourceRate 300", "nomap"], "resources"), "most");
  assert.equal(readSavedValheimModifier(["resourcerate 400"], "resources"), "custom");
  assert.equal(readSavedValheimModifier(["enemydamage 150"], "combat"), "custom");
  for (const [name, choices] of Object.entries(readValheimWorldRuleContract(schemaJson).modifiers)) {
    assert.ok(!choices.includes("normal") && !choices.includes("default"), `${name}: unverified standard-reset options are not exposed`);
  }
});

test("Valheim metadata responses keep saved, new and missing worlds separate and reject stale identity", () => {
  const ready = { instance_id: "id", world_name: "world", source: "saved", world_version: 41, saved_keys: ["nomap", "future_rule 99"] };
  assert.deepEqual(parseValheimWorldRules(ready, "id", "world"), ready);
  for (const source of ["new_world", "missing_metadata"]) {
    const response = { ...ready, source, world_version: null, saved_keys: [] };
    assert.deepEqual(parseValheimWorldRules(response, "id", "world"), response);
  }
  for (const response of [{ ...ready, instance_id: "peer" }, { ...ready, world_name: "peer" },
    { ...ready, source: "guess" }, { ...ready, source: "saved", world_version: null },
    { ...ready, source: "missing_metadata" }, { ...ready, world_version: 42 },
    { ...ready, saved_keys: ["nomap\nunsafe"] }, { ...ready, saved_keys: Array(513).fill("nomap") }]) {
    assert.throws(() => parseValheimWorldRules(response, "id", "world"), /Invalid Valheim/);
  }
});
