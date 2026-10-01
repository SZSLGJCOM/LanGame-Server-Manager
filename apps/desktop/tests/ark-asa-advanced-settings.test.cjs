const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function compileTypeScript(module, filename) {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
}
require.extensions[".ts"] = compileTypeScript;
require.extensions[".tsx"] = compileTypeScript;
require.extensions[".css"] = (module) => module._compile("module.exports = {};", module.filename);

const { parseGuidedSettingsSchema, writeGuidedFieldValue } = require("../src/views/settings/guided-settings.ts");
const { buildConfigurationWorkspaceModel } = require("../src/views/settings/configuration-workspace-model.ts");
const { resolveSettingsModuleDefinition } = require("../src/views/settings/module-registry.ts");
const moduleId = "arksurvivalascended";
const schema = JSON.parse(fs.readFileSync(path.resolve(__dirname, "../../../modules", moduleId, "schema.json"), "utf8"));
const translate = (_key, _params, fallback) => fallback ?? "";
const parsed = parseGuidedSettingsSchema({
  summary: { id: moduleId, name: moduleId },
  schema_json: JSON.stringify(schema)
}, "en-US", translate);

const expectedSections = {
  supply_crate_loot_quality_multiplier: "loot",
  override_max_experience_points_player: "experience",
  override_max_experience_points_dino: "experience",
  auto_unlock_all_engrams: "engrams",
  config_override_supply_crate_items: "loot",
  config_override_item_crafting_costs: "crafting",
  config_override_item_max_quantity: "crafting",
  config_override_npc_spawn_entries_container: "spawns",
  config_subtract_npc_spawn_entries_container: "spawns",
  dino_spawn_weight_multipliers: "spawns",
  npc_replacements: "spawns",
  override_player_level_engram_points: "experience",
  engram_entry_auto_unlocks: "engrams",
  per_level_stats_multiplier_dino_tamed_type_integer: "leveling",
  per_level_stats_multiplier_dino_wild_integer: "leveling"
};

test("ASA advanced controls are reachable once in their matching settings category", () => {
  const definition = resolveSettingsModuleDefinition(moduleId);
  const model = buildConfigurationWorkspaceModel(parsed);
  const supplementalFields = parsed.fields.filter((field) => field.sourceId === "asa_advanced_game_ini");
  assert.deepEqual(supplementalFields.map((field) => field.key).sort(), Object.keys(expectedSections).sort());

  for (const [key, sectionId] of Object.entries(expectedSections)) {
    const field = parsed.fields.find((candidate) => candidate.key === key);
    assert.ok(field, key);
    assert.equal(field.sectionId, sectionId, key);
    assert.equal(field.presentation.owner, "configuration", key);
    assert.equal(field.presentation.state, field.type === "string" ? "specialized" : "editable", key);
    if (field.type === "string") {
      assert.equal(definition.specializedRenderers[field.presentation.rendererId].kind, "module-addon", key);
    }
    assert.ok(model.actionableSectionIds.includes(sectionId), key);
    const groups = definition.buildFieldGroups(sectionId,
      parsed.fields.filter((candidate) => candidate.sectionId === sectionId), "en-US", translate);
    assert.equal(groups.flatMap((group) => group.fields).filter((candidate) => candidate.key === key).length, 1, key);
    assert.ok(groups.some((group) => group.id !== "additional" && group.fields.includes(field)), key);
    assert.equal(field.control, field.type === "string" ? "textarea" : field.type === "boolean" ? "checkbox" : "number", key);
  }
  assert.equal(parsed.fields.find((field) => field.key === "per_level_stats_multiplier_player_integer").sectionId, "leveling");
});

test("clearing ASA numeric overrides removes saved keys while zero and false remain explicit", () => {
  for (const key of ["supply_crate_loot_quality_multiplier", "override_max_experience_points_player", "override_max_experience_points_dino"]) {
    const field = parsed.fields.find((candidate) => candidate.key === key);
    assert.equal(field.required, false, key);
    assert.equal(field.defaultValue, undefined, key);
    const filled = writeGuidedFieldValue({ server_name: "ASA" }, field, "2");
    assert.equal(filled[key], 2);
    assert.deepEqual(writeGuidedFieldValue(filled, field, ""), { server_name: "ASA" }, key);
    assert.deepEqual(filled, { server_name: "ASA", [key]: 2 }, "previous state remains immutable");
    assert.equal(writeGuidedFieldValue({}, field, "0")[key], 0, key);
  }
  const toggle = parsed.fields.find((field) => field.key === "auto_unlock_all_engrams");
  assert.deepEqual(writeGuidedFieldValue({ auto_unlock_all_engrams: true }, toggle, false), { auto_unlock_all_engrams: false });
});

test("ASA native multiline editors preserve ordered rules and indexed variants", () => {
  const values = {
    override_player_level_engram_points: "8\n12\n0",
    per_level_stats_multiplier_dino_tamed_type_integer: "[0]=0.2\n_Add[0]=0.14\n_Affinity[0]=0.44",
    npc_replacements: '(FromClassName="Raptor_Character_BP_C",ToClassName="")'
  };
  for (const [key, value] of Object.entries(values)) {
    const field = parsed.fields.find((candidate) => candidate.key === key);
    assert.equal(field.control, "textarea");
    assert.equal(writeGuidedFieldValue({}, field, value)[key], value);
    assert.equal(writeGuidedFieldValue({ [key]: value }, field, "")[key], "");
  }
});

test("ASA loot navigation describes its supported controls while ASE retains fishing", () => {
  const { EN_US_MESSAGES } = require("../src/i18n-messages.ts");
  const { ZH_CN_MESSAGES } = require("../src/i18n-messages-zh-cn.ts");
  for (const catalog of [EN_US_MESSAGES, ZH_CN_MESSAGES]) {
    const translate = (key, _params, fallback) => catalog[key] ?? fallback ?? key;
    const asaLoot = resolveSettingsModuleDefinition(moduleId).getSections(translate, "en-US")
      .find((section) => section.id === "loot");
    const aseLoot = resolveSettingsModuleDefinition("arksurvivalevolved").getSections(translate, "en-US")
      .find((section) => section.id === "loot");
    assert.ok(catalog["arksa.settings.sections.lootDescription"]);
    assert.equal(asaLoot.description, catalog["arksa.settings.sections.lootDescription"]);
    assert.doesNotMatch(asaLoot.description, /fishing|钓鱼/i);
    assert.match(aseLoot.description, /fishing|钓鱼/i);
  }
});
