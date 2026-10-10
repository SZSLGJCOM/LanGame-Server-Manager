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

const contract = JSON.parse(fs.readFileSync(path.resolve(__dirname,
  "../../../modules/runescapedragonwilds/world-settings.json"), "utf8"));
const schemaJson = fs.readFileSync(path.resolve(__dirname, "../../../modules/runescapedragonwilds/schema.json"), "utf8");
const {
  readDragonwildsWorldSettings, writeDragonwildsWorldSettings, isDragonwildsWorldSettingEditable,
  dragonwildsWorldSettingCopyKey, dragonwildsWorldSettingGroup, dragonwildsWorldSettingValue,
  isDragonwildsWorldSettingValueValid, buildDragonwildsWorldSettingsPatch
} = require("../src/dragonwilds-world-settings.ts");
const { parseGuidedSettingsSchema } = require("../src/views/settings/guided-settings.ts");
const { buildConfigurationWorkspaceModel, searchConfigurationItems } = require("../src/views/settings/configuration-workspace-model.ts");
const { runescapedragonwildsSettingsDefinition } = require("../src/views/settings/modules/runescapedragonwilds.ts");
const { EN_US_RUNESCAPE_DRAGONWILDS_MESSAGES } = require("../src/i18n/games/runescapedragonwilds.en.ts");
const { ZH_CN_RUNESCAPE_DRAGONWILDS_MESSAGES } = require("../src/i18n/games/runescapedragonwilds.zh-cn.ts");
const definitions = contract.settings;
const friendlyFire = "Difficulty.Environment.FriendlyFire";
const processing = "Difficulty.Progression.ProcessingSpeedScale";
const hunger = "Difficulty.SurvivalCore.HungerKills";
const prefix = "runescapedragonwilds.settings.worldEditor";
const definition = (tag) => {
  const result = definitions.find((entry) => entry.tag === tag);
  assert.ok(result, `Missing native definition: ${tag}`);
  return result;
};
function snapshot(mode = "Normal", overrides = {}) {
  return {
    instance_id: "dragonwilds-ui-test", status: "ready", world_file: "DedicatedWorld.sav", world_name: "Retained world",
    world_mode: mode, revision: "sha256-original", writable: true, message: null, backup_id: null,
    definitions, overrides: { ...overrides }, values: Object.fromEntries(definitions.map((entry) =>
      [entry.tag, overrides[entry.tag] ?? entry.preset_defaults[mode === "Custom" ? "Normal" : mode]]))
  };
}

test("Dragonwilds exposes native 63 Custom rules and one all-mode friendly-fire rule", () => {
  assert.equal(definitions.length, 66);
  assert.equal(new Set(definitions.map((entry) => entry.tag)).size, 66);
  assert.equal(definitions.filter((entry) => isDragonwildsWorldSettingEditable(entry, "Custom")).length, 63);
  for (const mode of ["Normal", "Hard", "Creative"]) {
    assert.deepEqual(definitions.filter((entry) => isDragonwildsWorldSettingEditable(entry, mode)).map((entry) => entry.tag), [friendlyFire]);
  }
  for (const mode of ["Normal", "Hard", "Creative", "Custom"]) {
    for (const tag of ["Difficulty.Progression.AllSkillsMaxed", "Difficulty.AI.DisableAggressiveAI", "Difficulty.Player.NoBuildingStability"]) {
      assert.equal(isDragonwildsWorldSettingEditable(definition(tag), mode), false, `${mode}: ${tag}`);
    }
  }
});

test("Dragonwilds validates native ranges, precision and booleans for every editable definition", () => {
  for (const entry of definitions.filter((value) => isDragonwildsWorldSettingEditable(value))) {
    const step = 10 ** -entry.decimal_places;
    assert.equal(isDragonwildsWorldSettingValueValid(entry, String(entry.minimum)), true, `${entry.tag}: minimum`);
    assert.equal(isDragonwildsWorldSettingValueValid(entry, String(entry.maximum)), true, `${entry.tag}: maximum`);
    for (const value of ["", " ", "NaN", "Infinity", String(entry.minimum - step), String(entry.maximum + step)]) {
      assert.equal(isDragonwildsWorldSettingValueValid(entry, value), false, `${entry.tag}: ${value}`);
    }
    if (entry.kind === "boolean") assert.equal(isDragonwildsWorldSettingValueValid(entry, "0.5"), false);
    else assert.equal(isDragonwildsWorldSettingValueValid(entry, String(entry.minimum + step / 10)), false,
      `${entry.tag}: unsupported extra decimal place`);
  }
});

test("Dragonwilds preset previews retain saved overrides, and entering Custom retains effective rules", () => {
  const retained = snapshot("Hard", { [processing]: 1.7, [hunger]: 0, "Difficulty.Future.UnknownRule": 19 });
  for (const mode of ["Normal", "Hard", "Creative", "Custom"]) {
    assert.equal(dragonwildsWorldSettingValue(retained, mode, definition(processing), {}), "1.7");
    assert.equal(dragonwildsWorldSettingValue(retained, mode, definition(hunger), {}), "0");
  }
  assert.equal(dragonwildsWorldSettingValue(retained, "Custom", definition("Difficulty.AI.Goblin.Health"), {}), "2");
  const partialCustom = snapshot("Custom", { [processing]: 0.8 });
  assert.equal(dragonwildsWorldSettingValue(partialCustom, "Custom", definition("Difficulty.AI.Goblin.Health"), {}), "1");
  assert.deepEqual(retained.overrides, { [processing]: 1.7, [hunger]: 0, "Difficulty.Future.UnknownRule": 19 });
});

test("Dragonwilds patches only edited and mode-permitted rules without serializing defaults", () => {
  const retained = snapshot("Normal", { [processing]: 1.7, "Difficulty.Future.UnknownRule": 19 });
  const draft = { [processing]: "1.7", [friendlyFire]: "1", [hunger]: "1",
    "Difficulty.Progression.AllSkillsMaxed": "1", "Difficulty.Player.NoBuildingStability": "1",
    "Difficulty.Future.UnknownRule": "23" };
  assert.deepEqual(buildDragonwildsWorldSettingsPatch(retained, "Normal", draft), { [friendlyFire]: 1 });
  assert.deepEqual(buildDragonwildsWorldSettingsPatch(retained, "Custom", draft), { [friendlyFire]: 1, [hunger]: 1 });
  for (const mode of ["Normal", "Hard", "Creative", "Custom"]) {
    assert.deepEqual(buildDragonwildsWorldSettingsPatch(retained, mode, {}), {});
  }
  assert.throws(() => buildDragonwildsWorldSettingsPatch(retained, "Custom", { [processing]: "3.1" }), /Invalid world setting/u);
  assert.throws(() => buildDragonwildsWorldSettingsPatch(retained, "Custom", { [processing]: "1.23" }), /Invalid world setting/u);
  assert.deepEqual(buildDragonwildsWorldSettingsPatch(retained, "Normal", { [processing]: "3.1" }), {});
  assert.deepEqual(retained.overrides, { [processing]: 1.7, "Difficulty.Future.UnknownRule": 19 });
});

test("Dragonwilds handles native float round-trip noise without false changes", () => {
  const retained = snapshot("Custom", { [processing]: Math.fround(1.2) });
  assert.equal(dragonwildsWorldSettingValue(retained, "Custom", definition(processing), {}), "1.2");
  assert.deepEqual(buildDragonwildsWorldSettingsPatch(retained, "Custom", { [processing]: "1.2" }), {});
  assert.deepEqual(buildDragonwildsWorldSettingsPatch(retained, "Custom", { [processing]: "1.3" }), { [processing]: 1.3 });
});

test("Dragonwilds virtual world editor enters navigation without creating instance settings fields", () => {
  for (const [locale, catalog] of [["en-US", EN_US_RUNESCAPE_DRAGONWILDS_MESSAGES], ["zh-CN", ZH_CN_RUNESCAPE_DRAGONWILDS_MESSAGES]]) {
    const schema = parseGuidedSettingsSchema({ summary: { id: "runescapedragonwilds", name: "Dragonwilds" }, schema_json: schemaJson },
      locale, (key, _params, fallback) => catalog[key] ?? fallback ?? key);
    assert.equal(schema.parseError, null);
    assert.equal(schema.fields.some((entry) => entry.key === "world_settings" || entry.key.startsWith("Difficulty.")), false);
    const model = buildConfigurationWorkspaceModel(schema);
    assert.ok(model.actionableSectionIds.includes("world"));
    const virtualItems = model.items.filter((entry) => entry.fieldKey === "world_settings");
    assert.equal(virtualItems.length, 1);
    assert.equal(virtualItems[0].field.presentation.rendererId, "dragonwilds-world");
    const registration = runescapedragonwildsSettingsDefinition.specializedRenderers["dragonwilds-world"];
    assert.equal(registration.kind, "module-addon");
    assert.equal(registration.saveMode, "native-settings");
    assert.equal(typeof runescapedragonwildsSettingsDefinition.workspaceProvider, "function");
    assert.equal(registration.fieldKey, undefined, "Virtual presentation is not a schema property");
    assert.equal(registration.sectionId, "world");
    const rules = model.items.filter((entry) => entry.fieldKey.startsWith("Difficulty."));
    assert.equal(rules.length, 63, "Every supported rule is individually searchable");
    assert.equal(rules.find((entry) => entry.fieldKey === processing).sectionId, "world_crafting");
    assert.equal(rules.find((entry) => entry.fieldKey === friendlyFire).sectionId, "world_player");
    assert.equal(rules.find((entry) => entry.fieldKey === hunger).sectionId, "world_survival");
    assert.equal(rules.some((entry) => entry.fieldKey === "Difficulty.Player.NoBuildingStability"), false);
    const peerIds = ["world", ...["survival", "player", "death", "magic", "building", "crafting", "progression", "creatures"]
      .map((category) => `world_${category}`)];
    assert.deepEqual(model.roots.filter((entry) => peerIds.includes(entry.id)).map((entry) => entry.id), peerIds,
      "World and gameplay categories are directly actionable navigation peers in their intended order");
    for (const section of ["survival", "player", "death", "magic", "building", "crafting", "progression", "creatures"]) {
      assert.ok(model.actionableSectionIds.includes(`world_${section}`));
      assert.equal(schema.sections.find((entry) => entry.id === `world_${section}`).parentId, undefined);
      assert.equal(model.roots.find((entry) => entry.id === `world_${section}`).children.length, 0);
    }
    assert.equal(model.roots.find((entry) => entry.id === "world").children.length, 0);
    assert.equal(schema.sections.some((entry) => entry.id === "world_environment"), false);
    assert.equal(Object.hasOwn(runescapedragonwildsSettingsDefinition.specializedRenderers, "dragonwilds-world_environment"), false);
    assert.deepEqual(rules.filter((entry) => entry.sectionId === "world").map((entry) => entry.fieldKey).sort(),
      ["Difficulty.WorldEvents.MajorWorldEventFrequencyScale", "Difficulty.WorldEvents.MinorWorldEventFrequencyScale"].sort(),
      "World owns only its mode and two world events");
    assert.equal(new Set(rules.map((entry) => entry.fieldKey)).size, 63, "Native rules have no duplicate search entries");
    assert.deepEqual(searchConfigurationItems(model, friendlyFire, locale).map((entry) => entry.sectionId), ["world_player"],
      "Friendly fire search navigates to player rules rather than an obsolete environmental branch");
  }
});

test("Dragonwilds localizes all native controls and explains processing as time rather than speed", () => {
  for (const entry of definitions) {
    const key = dragonwildsWorldSettingCopyKey(entry.tag);
    const group = dragonwildsWorldSettingGroup(entry.tag);
    for (const catalog of [EN_US_RUNESCAPE_DRAGONWILDS_MESSAGES, ZH_CN_RUNESCAPE_DRAGONWILDS_MESSAGES]) {
      assert.ok(catalog[`${prefix}.fields.${key}.title`], entry.tag);
      assert.ok(catalog[`${prefix}.fields.${key}.description`], entry.tag);
      assert.ok(catalog[`${prefix}.groups.${group}`], group);
    }
    assert.match(ZH_CN_RUNESCAPE_DRAGONWILDS_MESSAGES[`${prefix}.fields.${key}.title`], /[\u3400-\u9fff]/u);
  }
  assert.match(EN_US_RUNESCAPE_DRAGONWILDS_MESSAGES[`${prefix}.fields.ProcessingSpeedScale.title`], /time/iu);
  assert.match(ZH_CN_RUNESCAPE_DRAGONWILDS_MESSAGES[`${prefix}.fields.ProcessingSpeedScale.description`], /越低.*越快/u);
});

test("Dragonwilds API invokes the real native transport with world identity and compare-and-swap revision", async () => {
  const previousWindow = globalThis.window;
  const hadIsTauri = Object.hasOwn(globalThis, "isTauri");
  const previousIsTauri = globalThis.isTauri;
  const calls = [];
  const retained = snapshot();
  globalThis.isTauri = true;
  globalThis.window = { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command, args) => {
    calls.push({ command, args: structuredClone(args) });
    return structuredClone(retained);
  } } };
  try {
    assert.deepEqual(await readDragonwildsWorldSettings(retained.instance_id), retained);
    const input = { instance_id: retained.instance_id, world_file: retained.world_file,
      expected_revision: retained.revision, world_mode: "Custom", values: { [processing]: 0.8 } };
    assert.deepEqual(await writeDragonwildsWorldSettings(input), retained);
    assert.deepEqual(calls, [
      { command: "read_dragonwilds_world_settings", args: { instanceId: retained.instance_id } },
      { command: "write_dragonwilds_world_settings", args: { input } }
    ]);
  } finally {
    if (previousWindow === undefined) delete globalThis.window; else globalThis.window = previousWindow;
    if (hadIsTauri) globalThis.isTauri = previousIsTauri; else delete globalThis.isTauri;
  }
});

test("Dragonwilds preview preserves internal Creative values when switching to Custom", () => {
  const { MockDragonwildsWorldSettings } = require("../src/api-mock/dragonwilds-world-settings.ts");
  const worlds = new MockDragonwildsWorldSettings();
  const details = { summary: { id: "world-preview", module_id: "runescapedragonwilds", name: "Preview world",
    status: "Stopped", active_process_count: 0 }, active_run: null };
  const first = worlds.read(details);
  const creative = worlds.write(details, { instance_id: details.summary.id, world_file: first.world_file,
    expected_revision: first.revision, world_mode: "Creative", values: {} });
  const custom = worlds.write(details, { instance_id: details.summary.id, world_file: creative.world_file,
    expected_revision: creative.revision, world_mode: "Custom", values: {} });
  assert.deepEqual(custom.values, creative.values);
  assert.equal(custom.values["Difficulty.Progression.AllSkillsMaxed"], 1);
  assert.equal(custom.values["Difficulty.AI.DisableAggressiveAI"], 1);
});
