const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
for (const extension of [".ts", ".tsx"]) require.extensions[extension] = (module, filename) =>
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const { applyArkModChange, applyArkSharedModAction, arkModEnabledMaps, arkModUnionSettings, arkModCompatibilityDiffers,
  readArkModMapOverrides, projectArkModSettings, assertArkModChangeBase } = require("../src/views/servers/ark-map-mods.ts");
const { readAsaModMembership, parseAsaModIds } = require("../src/views/servers/mod-workbench-asa.ts");
const { readAsaModConfigurations } = require("../src/views/servers/asa-mod-configuration.ts");
const { buildArkMapSelectionPatch } = require("../src/views/settings/ark-cluster-maps.ts");
const { parseEvolvedModIds } = require("../src/views/servers/ark-evolved-mod-ids.ts");
const { buildConfiguredEntries } = require("../src/views/servers/mod-workbench-model.ts");
const { buildWorkshopModEnablementPlan, readWorkshopControlStates, buildWorkshopDownloadOwnershipPlan } = require("../src/views/servers/mod-workbench-workshop-controls.ts");
const { buildModSettingsApplyPlan, buildModSettingsRemovePlan } = require("../src/views/servers/mod-workbench-plans.ts");
const ASA = "arksurvivalascended", ASE = "arksurvivalevolved", A = "928708", B = "939713", C = "1254808", P = "500000";
const all = ["main", "map-island", "map-paused"];
const base = () => ({ map_name: "TheCenter_WP", server_name: "Main", additional_maps: [
  { id: "island", map_name: "TheIsland_WP", name: "Island", enabled: true },
  { id: "paused", map_name: "ScorchedEarth_WP", name: "Paused", enabled: false }],
  mod_ids_csv: `${A},${B}`, passive_mod_ids_csv: P, unrelated: "retained",
  asa_mod_configurations: { [A]: { game_ini: "[Fixture]\nValue=1\n", game_user_settings_ini: "[Other]\nKeep=1\n" },
    [B]: { game_ini: "[B]\nKeep=1\n", game_user_settings_ini: "" } } });
const targets = (root, id, maps, moduleId = ASA) => applyArkModChange(root, moduleId, { modId: id, kind: "membership", maps });
const config = (root, maps, text, file = "game_ini", id = A) => applyArkModChange(root, ASA, { modId: id, kind: "configuration", maps, file, text });
const effective = (root, map) => projectArkModSettings(root, ASA, map);

test("Mods inherit shared settings until that specific Mod receives an override", () => {
  const root = base();
  for (const map of all) assert.deepEqual(effective(root, map), root);
  assert.deepEqual(arkModEnabledMaps(root, ASA, A), all);
  assert.equal(arkModCompatibilityDiffers(root, ASA), false);
});
test("one Mod can target two maps while every other Mod retains shared membership and options", () => {
  const original = base(), saved = targets(original, A, ["main", "map-island"]);
  assert.deepEqual(arkModEnabledMaps(saved, ASA, A), ["main", "map-island"]);
  assert.deepEqual(arkModEnabledMaps(saved, ASA, B), all);
  assert.deepEqual(arkModEnabledMaps(saved, ASA, P), all);
  for (const map of Object.values(saved.ark_mod_map_overrides)) assert.deepEqual(Object.keys(map.mods), [A]);
  assert.equal(saved.mod_ids_csv, original.mod_ids_csv);
  assert.deepEqual(saved.asa_mod_configurations, original.asa_mod_configurations);
  assert.deepEqual(original, base());
  assert.equal(arkModCompatibilityDiffers(saved, ASA), false, "paused maps alone do not generate a live-cluster warning");
  assert.equal(arkModCompatibilityDiffers(targets(saved, A, ["main"]), ASA), true);
});
test("zero targets disables only the selected Mod everywhere; a shared toggle cannot resurrect it", () => {
  const saved = targets(base(), A, []);
  assert.deepEqual(arkModEnabledMaps(saved, ASA, A), []);
  assert.deepEqual(arkModEnabledMaps(saved, ASA, B), all);
  const changedB = applyArkSharedModAction(saved, ASA, [B], "disable");
  assert.deepEqual(arkModEnabledMaps(changedB, ASA, A), []);
  assert.equal(changedB.mod_ids_csv, A);
  assert.ok(readAsaModMembership(arkModUnionSettings(changedB, ASA)).owned.has(A));
});
test("restoring membership inheritance preserves per-Mod parameters and loading order", () => {
  let saved = config(targets(base(), A, ["main"]), ["map-island"], "[Fixture]\nValue=2\n");
  saved.ark_mod_map_overrides["map-island"].active_order = [B, A];
  saved = applyArkModChange(saved, ASA, { modId: A, kind: "inherit-membership" });
  assert.deepEqual(arkModEnabledMaps(saved, ASA, A), all);
  assert.equal(readAsaModConfigurations(effective(saved, "map-island"))[A].game_ini, "[Fixture]\nValue=2\n");
  assert.deepEqual(readAsaModMembership(effective(saved, "map-island")).active, [B, A]);
  assert.deepEqual(Object.keys(saved.ark_mod_map_overrides["map-island"].mods[A]), ["configuration"]);
});
test("parameter saves patch only the current Mod, selected maps and current INI file", () => {
  const root = targets(base(), A, ["map-island"]), saved = config(root, ["main", "map-island"], "[Fixture]\nValue=2\n");
  for (const map of ["main", "map-island"]) {
    const configs = readAsaModConfigurations(effective(saved, map));
    assert.equal(configs[A].game_ini, "[Fixture]\nValue=2\n");
    assert.equal(configs[A].game_user_settings_ini, "[Other]\nKeep=1\n");
    assert.deepEqual(configs[B], root.asa_mod_configurations[B]);
  }
  assert.deepEqual(readAsaModConfigurations(effective(saved, "map-paused")), root.asa_mod_configurations);
  assert.deepEqual(arkModEnabledMaps(saved, ASA, A), ["map-island"]);
  assert.deepEqual(saved.asa_mod_configurations, root.asa_mod_configurations);
});
test("sparse file inheritance distinguishes absent, an explicit empty file and null", () => {
  let saved = config(base(), ["main"], "");
  assert.equal(readAsaModConfigurations(effective(saved, "main"))[A].game_ini, "");
  assert.equal(readAsaModConfigurations(effective(saved, "main"))[A].game_user_settings_ini, "[Other]\nKeep=1\n");
  saved.asa_mod_configurations[A].game_user_settings_ini = "[Other]\nKeep=2\n";
  assert.equal(readAsaModConfigurations(effective(saved, "main"))[A].game_user_settings_ini, "[Other]\nKeep=2\n");
  saved.ark_mod_map_overrides.main.mods[A].configuration = null;
  assert.equal(readAsaModConfigurations(effective(saved, "main"))[A], undefined);
  saved = config(saved, ["main"], "[Fixture]\nValue=3\n");
  assert.equal(readAsaModConfigurations(effective(saved, "main"))[A].game_user_settings_ini, "", "editing one file after null retains the other file's explicit clear");
});
test("an empty per-Mod configuration object is a no-op and does not invent an empty configuration record", () => {
  const root = { ...base(), ark_mod_map_overrides: { main: { mods: { [C]: { configuration: {} } } } } };
  assert.equal(readAsaModConfigurations(effective(root, "main"))[C], undefined);
  assert.deepEqual(readAsaModConfigurations(effective(root, "main")), root.asa_mod_configurations);
});
test("restoring configuration inheritance does not reset the selected Mod's enabled maps", () => {
  const root = config(targets(base(), A, ["map-island"]), ["main", "map-island"], "[Fixture]\nValue=2\n");
  const saved = applyArkModChange(root, ASA, { modId: A, kind: "inherit-configuration", maps: ["map-island"] });
  assert.deepEqual(arkModEnabledMaps(saved, ASA, A), ["map-island"]);
  assert.equal(readAsaModConfigurations(effective(saved, "map-island"))[A].game_ini, base().asa_mod_configurations[A].game_ini);
  assert.equal(readAsaModConfigurations(effective(saved, "main"))[A].game_ini, "[Fixture]\nValue=2\n");
});
test("map-only Mod ordering matches native lexical keys and preserves explicit map order", () => {
  const root = { ...base(), mod_ids_csv: "", passive_mod_ids_csv: "", ark_mod_map_overrides: { main: { mods: {
    [A]: { active: true }, [C]: { active: true } } } } };
  assert.deepEqual(readAsaModMembership(effective(root, "main")).active, [C, A]);
  root.ark_mod_map_overrides.main.active_order = [A, C];
  assert.deepEqual(readAsaModMembership(effective(root, "main")).active, [A, C]);
  assert.deepEqual(parseAsaModIds(arkModUnionSettings(root, ASA).mod_ids_csv), [A, C]);
  root.ark_mod_map_overrides.main.active_order = [A, A];
  assert.throws(() => readArkModMapOverrides(root, ASA), (error) => error.code === "shape");
});
test("removing a map-only Mod clears every membership and cannot be revived by retained options or cache", () => {
  const root = { ...base(), curseforge_removed_mod_ids: [C], ark_mod_map_overrides: {
    "map-island": { mods: { [C]: { active: true, removed: false, configuration: { game_ini: "[C]\nKeep=1\n" } } }, active_order: [C, A, B] } } };
  assert.ok(readAsaModMembership(arkModUnionSettings(root, ASA)).owned.has(C));
  const saved = applyArkSharedModAction(root, ASA, [C], "remove");
  assert.deepEqual(arkModEnabledMaps(saved, ASA, C), []);
  assert.equal(readAsaModMembership(arkModUnionSettings(saved, ASA)).owned.has(C), false);
  assert.ok(readAsaModMembership(arkModUnionSettings(saved, ASA)).removed.has(C));
  assert.equal(saved.ark_mod_map_overrides["map-island"].mods[C].configuration.game_ini, "[C]\nKeep=1\n");
  assert.deepEqual(saved.ark_mod_map_overrides["map-island"].active_order, [A, B]);
});
test("shared additions still reach every map when another Mod has local overrides", () => {
  const root = targets(base(), A, ["main"]), saved = applyArkSharedModAction(root, ASA, [C], "add");
  assert.deepEqual(arkModEnabledMaps(saved, ASA, C), all);
  assert.deepEqual(arkModEnabledMaps(saved, ASA, A), ["main"]);
  assert.deepEqual(saved.ark_mod_map_overrides, root.ark_mod_map_overrides);
});
test("semantic CAS merges other Mods and the other INI file but detects local inheritance intent changes", () => {
  const root = config(base(), ["main"], base().asa_mod_configurations[A].game_ini);
  const change = { kind: "configuration", modId: A, maps: ["main"], file: "game_ini", text: "[Fixture]\nValue=2\n" };
  const other = config(config(root, ["main"], "[B]\nKeep=2\n", "game_ini", B), ["main"], "[Other]\nKeep=3\n", "game_user_settings_ini");
  assert.doesNotThrow(() => assertArkModChangeBase(root, other, ASA, change));
  const saved = applyArkModChange(other, ASA, change);
  assert.equal(readAsaModConfigurations(effective(saved, "main"))[B].game_ini, "[B]\nKeep=2\n");
  assert.equal(readAsaModConfigurations(effective(saved, "main"))[A].game_user_settings_ini, "[Other]\nKeep=3\n");
  const inherited = applyArkModChange(root, ASA, { kind: "inherit-configuration", modId: A, maps: ["main"] });
  assert.throws(() => assertArkModChangeBase(root, inherited, ASA, change), (error) => error.code === "changed");
  const enabled = targets(base(), A, all), reset = applyArkModChange(enabled, ASA, { kind: "inherit-membership", modId: A });
  assert.throws(() => assertArkModChangeBase(enabled, reset, ASA, { kind: "membership", modId: A, maps: ["main"] }), (error) => error.code === "changed");
});
test("map selection preserves passive-only and combined membership when disabling and restoring", () => {
  let root = { ...base(), mod_ids_csv: B, passive_mod_ids_csv: `${A},${P}` };
  root = targets(root, A, ["main", "map-island"]);
  assert.equal(readAsaModMembership(effective(root, "main")).active.includes(A), false);
  assert.equal(readAsaModMembership(effective(root, "main")).passive.includes(A), true);
  root = targets(targets({ ...base(), passive_mod_ids_csv: `${A},${P}` }, A, []), A, ["main"]);
  assert.ok(readAsaModMembership(effective(root, "main")).active.includes(A));
  assert.ok(readAsaModMembership(effective(root, "main")).passive.includes(A));
});
test("ASE union-only membership never becomes a shared active Mod during another shared edit", () => {
  const root = { ...base(), active_mod_ids: B, auto_managed_mod_ids: B };
  let saved = targets(root, C, ["map-island"], ASE);
  assert.equal(saved.active_mod_ids, B);
  assert.ok(arkModUnionSettings(saved, ASE).active_mod_ids.includes(C));
  saved = applyArkSharedModAction(saved, ASE, [C], "remove");
  assert.equal(saved.active_mod_ids, B); assert.equal(arkModUnionSettings(saved, ASE).active_mod_ids.includes(C), false);
});
test("ASE references follow native ActiveMods separators, id= priority and stable deduplication", () => {
  assert.deepEqual(parseEvolvedModIds("111111;222222 333333\tworkshop-444444、555555，111111\u0085666666"),
    ["111111", "222222", "333333", "444444", "555555", "666666"]);
  assert.deepEqual(parseEvolvedModIds("https://steamcommunity.com/123456/filedetails/?id=777777,ref-888888 #999999 --000000 ignored\n777777"),
    ["777777", "888888"]);
  assert.deepEqual(parseEvolvedModIds("ref-123456?id=12&id=777777,111111；222222"), ["123456", "111111"],
    "the first id= and native delimiter set determine interpretation");
});
test("ASE map edits and shared removal retain every native shared ID", () => {
  const root = { ...base(), active_mod_ids: "111111;222222", auto_managed_mod_ids: "111111 222222",
    ark_mod_map_overrides: { main: { mods: { "333333": { active: true } } } } };
  assert.deepEqual(parseEvolvedModIds(projectArkModSettings(root, ASE, "main").active_mod_ids), ["111111", "222222", "333333"]);
  assert.deepEqual(arkModEnabledMaps(root, ASE, "222222"), all);
  const saved = applyArkSharedModAction(root, ASE, ["333333"], "remove");
  assert.deepEqual(parseEvolvedModIds(saved.active_mod_ids), ["111111", "222222"]);
  assert.deepEqual(parseEvolvedModIds(saved.auto_managed_mod_ids), ["111111", "222222"]);
  assert.deepEqual(parseEvolvedModIds(arkModUnionSettings(saved, ASE).active_mod_ids), ["111111", "222222"]);
  assert.throws(() => readArkModMapOverrides({ ...root, ark_mod_map_overrides: { main: { mods: { "12345": { active: true } } } } }, ASE),
    (error) => error.code === "shape");
  assert.throws(() => readArkModMapOverrides({ ...root, ark_mod_map_overrides: { main: { mods: {}, active_order: ["12345"] } } }, ASE),
    (error) => error.code === "shape");
});
test("ASE shared display, toggles and download ownership use the same native parser", () => {
  const root = { active_mod_ids: "111111;https://steamcommunity.com/123456/filedetails/?id=222222", auto_managed_mod_ids: "111111 222222" };
  const details = { workshop: { provider: "steam" }, mods: { enablement: { setting_key: "active_mod_ids", setting_label: "Mods" } } };
  assert.deepEqual(buildConfiguredEntries(ASE, root, details)[0].ids, ["111111", "222222"]);
  assert.equal(readWorkshopControlStates(ASE, root).get("222222").enabled, true);
  const disabled = buildWorkshopModEnablementPlan(ASE, root, ["111111"], false).nextSettings;
  assert.deepEqual(parseEvolvedModIds(disabled.active_mod_ids), ["222222"]);
  assert.deepEqual(parseEvolvedModIds(buildWorkshopModEnablementPlan(ASE, disabled, ["111111"], true).nextSettings.active_mod_ids), ["222222", "111111"]);
  assert.deepEqual(parseEvolvedModIds(buildWorkshopDownloadOwnershipPlan(ASE, root, ["333333"]).auto_managed_mod_ids), ["111111", "222222", "333333"]);
  const item = { id: "333333", status: "resolved", item_kind: "item", consumer_app_id: 346110, children: [] };
  const imported = buildModSettingsApplyPlan(ASE, root, [item.id], { [item.id]: item }, 346110);
  assert.deepEqual(parseEvolvedModIds(imported.nextSettings.active_mod_ids), ["111111", "222222", "333333"]);
  const removed = buildModSettingsRemovePlan(ASE, imported.nextSettings, [item.id], { [item.id]: item }, 346110, null);
  assert.deepEqual(parseEvolvedModIds(removed.nextSettings.active_mod_ids), ["111111", "222222"]);
});
test("choosing a downloaded map Mod enables only its destination without changing unrelated shared loading", () => {
  const root = base();
  const patch = buildArkMapSelectionPatch(root, "ModMap_WP", [{ value: "ModMap_WP", label: "Mod Map", modId: C }],
    { additional_maps: root.additional_maps.map((map) => map.id === "island" ? { ...map, map_name: "ModMap_WP" } : map) }, "map-island");
  const saved = { ...root, ...patch };
  assert.equal(patch.mod_ids_csv, undefined);
  assert.deepEqual(arkModEnabledMaps(saved, ASA, C), ["map-island"]);
  assert.deepEqual(readAsaModMembership(effective(saved, "map-island")).active, [A, B, C]);
  assert.deepEqual(root, base());
});
