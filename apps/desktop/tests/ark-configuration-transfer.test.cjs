const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
require.extensions[".ts"] = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
const subjectPath = path.resolve(__dirname, "../src/views/settings/ark-configuration-transfer.ts");
const load = () => fs.existsSync(subjectPath) ? require(subjectPath) : {};
const root = path.resolve(__dirname, "../../..");
const properties = (id) => JSON.parse(fs.readFileSync(path.join(root, "modules", id, "schema.json"), "utf8")).properties;

for (const id of ["arksurvivalascended", "arksurvivalevolved"]) {
  test(`${id}: imports typed INI fields, repeated rules and unknown Mod sections without taking over network ports`, () => {
    const { importArkIniDocuments } = load();
    assert.equal(typeof importArkIniDocuments, "function");
    const result = importArkIniDocuments(properties(id), [
      { name: "GameUserSettings.ini", text: "[SessionSettings]\nSessionName=Imported world\n[ServerSettings]\nserverPVE=True\nTamingSpeedMultiplier=3.5\nRCONPort=29999\n[MyMod]\n; keep this\nCustomRule=one\nCustomRule=two\n" },
      { name: "Game.ini", text: "[/Script/ShooterGame.ShooterGameMode]\nPerLevelStatsMultiplier_Player[7]=2\nPerLevelStatsMultiplier_Player[8]=1.5\nNPCReplacements=(FromClassName=\"A_C\",ToClassName=\"B_C\")\nNPCReplacements=(FromClassName=\"C_C\",ToClassName=\"D_C\")\n" }
    ]);
    assert.deepEqual(result.issues, []);
    assert.equal(result.patch.server_name, "Imported world");
    assert.equal(result.patch.server_pve, true);
    assert.equal(result.patch.taming_speed_multiplier, 3.5);
    assert.match(result.patch.per_level_stats_multiplier_player_integer, /\[7\]=2/);
    assert.equal(result.patch.npc_replacements.split("\n").length, 2);
    assert.match(result.patch.game_user_settings_extra, /\[MyMod\]\n; keep this\nCustomRule=one\nCustomRule=two/);
    assert.doesNotMatch(result.patch.game_user_settings_extra, /RCONPort/);
    assert.ok(result.skippedKeys.includes("RCONPort"));
  });
}

test("invalid scalar values and malformed files produce blocking import issues", () => {
  const { importArkIniDocuments } = load();
  assert.equal(typeof importArkIniDocuments, "function");
  const p = properties("arksurvivalascended");
  assert.ok(importArkIniDocuments(p, [{ name: "GameUserSettings.ini", text: "[ServerSettings]\nTamingSpeedMultiplier=NaN\nserverPVE=perhaps" }]).issues.length === 2);
  assert.ok(importArkIniDocuments(p, [{ name: "Game.ini", text: "[Broken\ninvalid rule" }]).issues.length > 0);
  assert.throws(() => importArkIniDocuments(p, [{ name: "Game.ini", text: "x".repeat(2 * 1024 * 1024 + 1) }]), /size/i);
  assert.equal(importArkIniDocuments(p, [{ name: "GameUserSettings.ini", text: "[SessionSettings]\nMaxPlayers=0" }]).issues.length, 1);
  assert.equal(importArkIniDocuments(p, [{ name: "Game.ini", text: "[/Script/ShooterGame.ShooterGameMode]\nNPCReplacements=not a native rule (" }]).issues.length, 1);
  assert.equal(importArkIniDocuments(p, [{ name: "Game.ini", text: "[/Script/ShooterGame.ShooterGameMode]\nLevelExperienceRampOverrides=(ExperiencePointsForLevel[0]=100,ExperiencePointsForLevel[1]=50)" }]).issues.length, 1);
});

test("INI imports resolve native aliases and indexed keys while leaving Mod installation to its workspace", () => {
  const { importArkIniDocuments } = load();
  const result = importArkIniDocuments(properties("arksurvivalevolved"), [
    { name: "GameUserSettings.ini", text: "[SessionSettings]\nMaxPlayers=42\n[ServerSettings]\nActiveMods=123,456" },
    { name: "Game.ini", text: "[/Script/ShooterGame.ShooterGameMode]\nplayerbasestatmultipliers[7]=2\nperlevelstatsmultiplier_dinotamed_affinity[8]=3\nnpcReplacements=(FromClassName=\"A_C\",ToClassName=\"B_C\")" }
  ]);
  assert.deepEqual(result.issues, []);
  assert.equal(result.patch.max_players, 42);
  assert.equal(result.patch.active_mod_ids, undefined);
  assert.ok(result.skippedKeys.includes("ActiveMods"));
  assert.equal(result.patch.player_base_stat_multipliers_attribute, "PlayerBaseStatMultipliers[7]=2");
  assert.equal(result.patch.per_level_stats_multiplier_dino_tamed_type_integer, "PerLevelStatsMultiplier_DinoTamed_Affinity[8]=3");
  assert.match(result.patch.npc_replacements, /^NPCReplacements=/);
  assert.equal(result.patch.game_ini_extra, undefined);
});

for (const id of ["arksurvivalascended", "arksurvivalevolved"]) {
  test(`${id}: padded section names cannot bypass managed ports and Mod ownership`, () => {
    const result = load().importArkIniDocuments(properties(id), [{ name: "GameUserSettings.ini", text:
      "[ ServerSettings ]\nRCONPort=29999\n+ActiveMods=123\nTamingSpeedMultiplier=3\n[ /Script/Engine.GameSession ]\nMaxPlayers=42" }]);
    assert.deepEqual(result.issues, []);
    assert.equal(result.patch.taming_speed_multiplier, 3);
    assert.equal(result.patch.max_players, 42);
    assert.equal(result.patch.game_user_settings_extra, undefined);
    assert.deepEqual(result.skippedKeys, ["RCONPort", "+ActiveMods"]);
    assert.equal(load().importArkIniDocuments(properties(id), [{ name: "Game.ini", text: "[   ]\nValue=1" }]).issues.length, 1);
  });
}

test("portable presets exclude credentials, identity, paths, network and independent workspace settings", () => {
  const { createArkPreset, readArkPreset } = load();
  assert.equal(typeof createArkPreset, "function");
  const fields = [
    ["taming_speed_multiplier", "rates", "configuration", "plain"],
    ["server_name", "room", "configuration", "plain"],
    ["admin_password", "access", "configuration", "secret"],
    ["cluster_directory", "transfer", "configuration", "path"],
    ["mod_ids_csv", "mods", "mods", "plain"],
    ["game_ini_extra", "advanced", "configuration", "raw"]
  ].map(([key, sectionId, owner, behavior]) => ({ key, sectionId, presentation: { owner, behavior } }));
  const preset = createArkPreset("arksurvivalascended", { taming_speed_multiplier: 3, server_name: "Private", admin_password: "secret", cluster_directory: "D:/private", mod_ids_csv: "123", game_ini_extra: "[Mod]\nApiKey=secret" }, fields, ["rates", "room", "access", "transfer", "mods", "advanced"]);
  assert.deepEqual(preset.settings, { taming_speed_multiplier: 3 });
  assert.deepEqual(readArkPreset(JSON.stringify(preset), "arksurvivalascended", properties("arksurvivalascended")), { taming_speed_multiplier: 3 });
  assert.throws(() => readArkPreset(JSON.stringify(preset), "arksurvivalevolved", properties("arksurvivalevolved")), /edition/i);
  assert.throws(() => readArkPreset('{"format":"lgsm-ark-preset","moduleId":"arksurvivalascended","settings":{"__proto__":{}}}', "arksurvivalascended", properties("arksurvivalascended")), /field/i);
  assert.throws(() => readArkPreset('{"format":"lgsm-ark-preset","moduleId":"arksurvivalascended","settings":{"admin_password":"hidden"}}', "arksurvivalascended", properties("arksurvivalascended")), /field/i);
  assert.throws(() => readArkPreset(JSON.stringify({ ...preset, settings: { npc_replacements: "broken (" } }), "arksurvivalascended", properties("arksurvivalascended")), /field/i);
});

test("copying a category resets omitted overrides instead of retaining stale target values", () => {
  const { createArkPreset, readArkPreset } = load();
  const fields = [{ key: "taming_speed_multiplier", sectionId: "rates", presentation: { owner: "configuration" } }];
  const preset = createArkPreset("arksurvivalascended", {}, fields, ["rates"]);
  assert.deepEqual(preset.reset, ["taming_speed_multiplier"]);
  const patch = readArkPreset(JSON.stringify(preset), "arksurvivalascended", properties("arksurvivalascended"));
  assert.ok(Object.hasOwn(patch, "taming_speed_multiplier"));
  assert.equal(patch.taming_speed_multiplier, undefined);
});
