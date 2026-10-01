const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const desktopRoot = path.resolve(__dirname, "..");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = function compileTypeScript(module, filename) {
    const source = fs.readFileSync(filename, "utf8");
    const outputText = transpileTypeScript(source, filename);
    module._compile(outputText, filename);
  };
}
require.extensions[".css"] = (module) => module._compile("", module.filename);

const { vrisingSettingsDefinition } = require(path.join(
  desktopRoot,
  "src",
  "views",
  "settings",
  "modules",
  "vrising.ts"
));

const context = {
  locale: "en-US",
  t: (key) => `translated:${key}`
};

function parseRaw(settings) {
  return JSON.parse(settings.server_game_settings_json);
}

test("V Rising initializes typed controls from raw JSON without losing unknown settings", () => {
  const unknownTopLevel = { keep: true };
  const input = {
    unknown_top_level: unknownTopLevel,
    server_game_settings_json: JSON.stringify({
      GameDifficulty: 2,
      UnsupportedNativeBlock: { KeepMe: true }
    })
  };
  const snapshot = structuredClone(input);

  assert.equal(typeof vrisingSettingsDefinition.initializeSettings, "function");
  const initialized = vrisingSettingsDefinition.initializeSettings(input, context);

  assert.deepEqual(input, snapshot, "initialization must not mutate its input");
  assert.strictEqual(initialized.unknown_top_level, unknownTopLevel);
  assert.equal(initialized.game_difficulty, "Brutal");
  assert.deepEqual(parseRaw(initialized).UnsupportedNativeBlock, { KeepMe: true });
});

test("V Rising applies typed and raw patches bidirectionally while preserving unrelated keys", () => {
  const baseline = vrisingSettingsDefinition.initializeSettings({
    unknown_top_level: "keep-me",
    server_game_settings_json: JSON.stringify({
      GameDifficulty: 1,
      UnsupportedNativeBlock: { KeepMe: true }
    })
  }, context);

  assert.equal(typeof vrisingSettingsDefinition.applySettingsPatch, "function");
  const typedPatch = vrisingSettingsDefinition.applySettingsPatch(
    baseline,
    { game_difficulty: "Easy", new_unknown_key: 42 },
    context
  );
  assert.equal(typedPatch.unknown_top_level, "keep-me");
  assert.equal(typedPatch.new_unknown_key, 42);
  assert.equal(parseRaw(typedPatch).GameDifficulty, 0);
  assert.deepEqual(parseRaw(typedPatch).UnsupportedNativeBlock, { KeepMe: true });

  const rawPatchValue = JSON.stringify({
    GameDifficulty: 2,
    UnsupportedNativeBlock: { KeepMe: "still-here" }
  });
  const rawPatch = vrisingSettingsDefinition.applySettingsPatch(
    typedPatch,
    { server_game_settings_json: rawPatchValue },
    context
  );
  assert.equal(rawPatch.game_difficulty, "Brutal");
  assert.equal(rawPatch.server_game_settings_json, rawPatchValue);
  assert.equal(rawPatch.unknown_top_level, "keep-me");
});

test("V Rising synchronizes exact-build nested modifiers and preserves specialized arrays", () => {
  const initialized = vrisingSettingsDefinition.initializeSettings({
    server_game_settings_json: JSON.stringify({
      VampireStatModifiers: { MaxHealthModifier: 1.25 },
      CastleStatModifiers_Global: {
        HeartLimits: { Level5: { FloorLimit: 777, FutureLimit: 9 } }
      },
      VBloodUnitSettings: [{ UnitId: 42 }],
      UnlockedAchievements: [1],
      UnlockedResearchs: [2]
    })
  }, context);

  assert.equal(initialized.vampire_max_health_modifier, 1.25);
  assert.equal(initialized.castle_heart_level_5_floor_limit, 777);

  const patched = vrisingSettingsDefinition.applySettingsPatch(
    initialized,
    {
      vampire_max_health_modifier: 1.5,
      castle_heart_level_5_floor_limit: 800
    },
    context
  );
  const raw = parseRaw(patched);
  assert.equal(raw.VampireStatModifiers.MaxHealthModifier, 1.5);
  assert.equal(raw.CastleStatModifiers_Global.HeartLimits.Level5.FloorLimit, 800);
  assert.equal(raw.CastleStatModifiers_Global.HeartLimits.Level5.FutureLimit, 9);
  assert.deepEqual(raw.VBloodUnitSettings, [{ UnitId: 42 }]);
  assert.deepEqual(raw.UnlockedAchievements, [1]);
  assert.deepEqual(raw.UnlockedResearchs, [2]);
});

test("V Rising keeps invalid raw drafts and reports a field-level validation issue", () => {
  assert.equal(typeof vrisingSettingsDefinition.getSettingsValidationIssues, "function");
  const cases = [
    ["", "empty", "settings.vrising.serverGameSettingsEmpty"],
    ["{broken", "json", "settings.vrising.serverGameSettingsInvalid"],
    ["[]", "object", "settings.vrising.serverGameSettingsObject"],
    ["null", "object", "settings.vrising.serverGameSettingsObject"]
  ];

  for (const [raw, reason, messageKey] of cases) {
    const settings = {
      unrelated: "keep-me",
      game_difficulty: "Normal",
      server_game_settings_json: raw
    };
    const patched = vrisingSettingsDefinition.applySettingsPatch(
      settings,
      { server_game_settings_json: raw },
      context
    );
    assert.equal(patched.server_game_settings_json, raw, `${reason} draft must remain editable`);
    assert.equal(patched.unrelated, "keep-me");
    assert.deepEqual(
      vrisingSettingsDefinition.getSettingsValidationIssues(patched, context),
      [{
        fieldKey: "server_game_settings_json",
        reason,
        message: `translated:${messageKey}`
      }]
    );
  }
});

test("V Rising validation accepts a JSON object and initialization preserves invalid drafts", () => {
  assert.deepEqual(
    vrisingSettingsDefinition.getSettingsValidationIssues(
      { server_game_settings_json: "{\n  \"Unknown\": true\n}" },
      context
    ),
    []
  );

  const invalid = {
    unrelated: true,
    game_difficulty: "Normal",
    server_game_settings_json: "{broken"
  };
  assert.deepEqual(
    vrisingSettingsDefinition.initializeSettings(invalid, context),
    invalid,
    "initialization must not normalize an invalid draft before validation can report it"
  );

  const initializedMissingRaw = vrisingSettingsDefinition.initializeSettings(
    { unrelated: "keep-me" },
    context
  );
  assert.equal(initializedMissingRaw.unrelated, "keep-me");
  assert.deepEqual(parseRaw(initializedMissingRaw), {});
});

test("V Rising rejects invalid managed native values without losing the raw draft or typed settings", () => {
  const readableContext = { locale: "en-US", t: (key, _params, fallback) => fallback ?? key };
  const baseline = vrisingSettingsDefinition.initializeSettings({
    unrelated: "keep-me",
    server_game_settings_json: '{"ClanSize":4,"BloodDrainModifier":1}'
  }, readableContext);
  for (const [raw, nativePath] of [
    ['{ "ClanSize": "oops" }', "ClanSize"],
    ['{ "ClanSize": null }', "ClanSize"],
    ['{ "ClanSize": 1.5 }', "ClanSize"],
    ['{ "ClanSize": 0 }', "ClanSize"],
    ['{ "ClanSize": 9007199254740992 }', "ClanSize"],
    ['{ "BloodDrainModifier": 1e400 }', "BloodDrainModifier"],
    ['{ "CanLootEnemyContainers": "true" }', "CanLootEnemyContainers"],
    ['{ "GameModeType": "unrecognized" }', "GameModeType"],
    ['{ "GameDifficulty": 99 }', "GameDifficulty"],
    ['{ "GameTimeModifiers": { "DayStartHour": 24 } }', "GameTimeModifiers.DayStartHour"],
    ['{ "WarEventGameSettings": { "Interval": "oops" } }', "WarEventGameSettings.Interval"],
    ['{ "WarEventGameSettings": { "Interval": 45 } }', "WarEventGameSettings.Interval"],
    ['{ "WarEventGameSettings": { "Interval": 60 } }', "WarEventGameSettings.Interval"],
    ['{ "VampireStatModifiers": [] }', "VampireStatModifiers"],
    ['{ "CastleStatModifiers_Global": { "HeartLimits": false } }', "CastleStatModifiers_Global.HeartLimits"]
  ]) {
    const patched = vrisingSettingsDefinition.applySettingsPatch(baseline, { server_game_settings_json: raw }, readableContext);
    assert.deepEqual(patched, { ...baseline, server_game_settings_json: raw }, nativePath);
    const issues = vrisingSettingsDefinition.getSettingsValidationIssues(patched, readableContext);
    assert.equal(issues.length, 1, nativePath);
    assert.equal(issues[0].fieldKey, "server_game_settings_json", nativePath);
    assert.equal(issues[0].reason, "nativeValue", nativePath);
    assert.ok(issues[0].message.includes(nativePath), nativePath);
    assert.deepEqual(vrisingSettingsDefinition.initializeSettings(patched, readableContext), patched, nativePath);
    const unrelatedEdit = vrisingSettingsDefinition.applySettingsPatch(patched, { blood_drain_modifier: 2 }, readableContext);
    assert.equal(unrelatedEdit.server_game_settings_json, raw, nativePath);
    assert.equal(unrelatedEdit.clan_size, baseline.clan_size, nativePath);
    assert.equal(unrelatedEdit.blood_drain_modifier, 2, nativePath);
  }
});

test("V Rising resumes bidirectional editing when the managed raw value is corrected", () => {
  const invalid = {
    clan_size: 4,
    server_game_settings_json: '{"ClanSize":"oops"}'
  };
  const correctedRaw = JSON.stringify({
    ClanSize: 6,
    GameDifficulty: 2,
    PlayerInteractionSettings: { TimeZone: 1 },
    CastleStatModifiers_Global: { CastleHeartLimitType: 1, FutureNative: [true] }
  });
  const corrected = vrisingSettingsDefinition.applySettingsPatch(invalid, { server_game_settings_json: correctedRaw }, context);
  assert.deepEqual(vrisingSettingsDefinition.getSettingsValidationIssues(corrected, context), []);
  assert.equal(corrected.server_game_settings_json, correctedRaw);
  assert.equal(corrected.clan_size, 6);
  assert.equal(corrected.game_difficulty, "Brutal");
  assert.equal(corrected.player_interaction_time_zone, "UTC");
  assert.equal(corrected.castle_heart_limit_type, "Clan");
  const typedEdit = vrisingSettingsDefinition.applySettingsPatch(corrected, { clan_size: 8 }, context);
  assert.equal(parseRaw(typedEdit).ClanSize, 8);
  assert.deepEqual(parseRaw(typedEdit).CastleStatModifiers_Global.FutureNative, [true]);
});

test("V Rising synchronizes War Event paths from the canonical schema in both directions", () => {
  const raw = JSON.stringify({ WarEventGameSettings: {
    Interval: 3,
    WeekdayTime: { StartHour: 6 },
    ScalingPlayers4: { PointsModifier: 1.5, DropModifier: 2.5, FutureRule: "keep-me" }
  } });
  assert.deepEqual(vrisingSettingsDefinition.getSettingsValidationIssues({ server_game_settings_json: raw }, context), []);
  const initialized = vrisingSettingsDefinition.initializeSettings({ server_game_settings_json: raw }, context);
  assert.equal(initialized.war_event_interval, 3);
  assert.equal(initialized.war_event_weekday_start_hour, 6);
  assert.equal(initialized.war_event_scaling_players_4_points_modifier, 1.5);
  assert.equal(initialized.war_event_scaling_players_4_drop_modifier, 2.5);
  const patched = vrisingSettingsDefinition.applySettingsPatch(initialized, {
    war_event_interval: 4,
    war_event_weekday_start_hour: 7,
    war_event_scaling_players_4_drop_modifier: 3.5
  }, context);
  assert.deepEqual(parseRaw(patched).WarEventGameSettings, {
    Interval: 4,
    WeekdayTime: { StartHour: 7 },
    ScalingPlayers4: { PointsModifier: 1.5, DropModifier: 3.5, FutureRule: "keep-me" }
  });
  assert.deepEqual(vrisingSettingsDefinition.getSettingsValidationIssues(patched, context), []);
});

test("V Rising retains supported native numeric enum aliases and canonical typed serialization", () => {
  for (const [native, fieldKey, label, expected] of [
    [{ GameDifficulty: 0 }, "game_difficulty", "Easy", { GameDifficulty: 0 }],
    [{ GameDifficulty: 1 }, "game_difficulty", "Normal", { GameDifficulty: 1 }],
    [{ GameDifficulty: 2 }, "game_difficulty", "Brutal", { GameDifficulty: 2 }],
    [{ CastleStatModifiers_Global: { CastleHeartLimitType: 0 } }, "castle_heart_limit_type", "User", { CastleStatModifiers_Global: { CastleHeartLimitType: "User" } }],
    [{ CastleStatModifiers_Global: { CastleHeartLimitType: 1 } }, "castle_heart_limit_type", "Clan", { CastleStatModifiers_Global: { CastleHeartLimitType: "Clan" } }],
    [{ PlayerInteractionSettings: { TimeZone: 0 } }, "player_interaction_time_zone", "Local", { PlayerInteractionSettings: { TimeZone: "Local" } }],
    [{ PlayerInteractionSettings: { TimeZone: 1 } }, "player_interaction_time_zone", "UTC", { PlayerInteractionSettings: { TimeZone: "UTC" } }]
  ]) {
    const settings = { server_game_settings_json: JSON.stringify(native) };
    assert.deepEqual(vrisingSettingsDefinition.getSettingsValidationIssues(settings, context), []);
    const initialized = vrisingSettingsDefinition.initializeSettings(settings, context);
    assert.equal(initialized[fieldKey], label);
    const typed = vrisingSettingsDefinition.applySettingsPatch(initialized, { [fieldKey]: label }, context);
    assert.deepEqual(parseRaw(typed), expected);
    assert.deepEqual(vrisingSettingsDefinition.getSettingsValidationIssues(typed, context), []);
  }
});

test("V Rising managed raw errors navigate to the visible JSON editor", () => {
  const { parseGuidedSettingsSchema } = require("../src/views/settings/guided-settings.ts");
  const { buildConfigurationWorkspaceModel } = require("../src/views/settings/configuration-workspace-model.ts");
  const { resolveConfigurationFieldNavigation } = require("../src/views/settings/configuration-workspace-state.ts");
  const schema = parseGuidedSettingsSchema({
    summary: { id: "vrising", name: "V Rising" },
    schema_json: fs.readFileSync(path.resolve(desktopRoot, "../../modules/vrising/schema.json"), "utf8")
  });
  const issues = vrisingSettingsDefinition.getSettingsValidationIssues({ server_game_settings_json: '{"ClanSize":"oops"}' }, context);
  assert.equal(issues.length, 1);
  const target = resolveConfigurationFieldNavigation(buildConfigurationWorkspaceModel(schema), issues[0].fieldKey, "configuration-vrising");
  assert.equal(target.sectionId, "raw");
  assert.equal(target.inputId, "configuration-vrising-server-game-settings-json-input");
});

test("V Rising typed range errors retain valid raw values and recover when corrected", () => {
  const { parseGuidedSettingsSchema, validateGuidedSettingsObject } = require("../src/views/settings/guided-settings.ts");
  const schema = parseGuidedSettingsSchema({
    summary: { id: "vrising", name: "V Rising" },
    schema_json: fs.readFileSync(path.resolve(desktopRoot, "../../modules/vrising/schema.json"), "utf8")
  });
  for (const [fieldKey, native, invalidValue, correctedValue] of [
    ["clan_size", { ClanSize: 4 }, 0, 4],
    ["war_event_interval", { WarEventGameSettings: { Interval: 3 } }, -1, 5],
    ["war_event_weekday_start_hour", { WarEventGameSettings: { WeekdayTime: { StartHour: 6 } } }, 24, 7]
  ]) {
    const baseline = vrisingSettingsDefinition.initializeSettings({ server_game_settings_json: JSON.stringify(native) }, context);
    const invalid = vrisingSettingsDefinition.applySettingsPatch(baseline, { [fieldKey]: invalidValue }, context);
    assert.equal(invalid[fieldKey], invalidValue, "the typed draft stays available for correction");
    assert.deepEqual(parseRaw(invalid), native, fieldKey);
    assert.deepEqual(vrisingSettingsDefinition.getSettingsValidationIssues(invalid, context), [], "only the typed control is invalid");
    assert.ok(validateGuidedSettingsObject(schema, invalid).some((issue) => issue.fieldKey === fieldKey), fieldKey);
    const unrelated = vrisingSettingsDefinition.applySettingsPatch(invalid, { blood_drain_modifier: 2 }, context);
    assert.equal(unrelated[fieldKey], invalidValue);
    assert.deepEqual(parseRaw(unrelated), { ...native, BloodDrainModifier: 2 });
    const corrected = vrisingSettingsDefinition.applySettingsPatch(unrelated, { [fieldKey]: correctedValue }, context);
    assert.deepEqual(vrisingSettingsDefinition.getSettingsValidationIssues(corrected, context), []);
    assert.ok(!validateGuidedSettingsObject(schema, corrected).some((issue) => issue.fieldKey === fieldKey));
    const reopened = vrisingSettingsDefinition.initializeSettings(corrected, context);
    assert.equal(reopened[fieldKey], correctedValue, "corrected typed value must survive a raw round trip");
  }
});

test("V Rising incomplete typed numbers and invalid enums never delete or replace valid native values", () => {
  for (const [fieldKey, native, invalidValue, correctedValue] of [
    ["clan_size", { ClanSize: 4 }, "-", 6],
    ["clan_size", { ClanSize: 4 }, 1.5, 6],
    ["war_event_interval", { WarEventGameSettings: { Interval: 3 } }, "1e", 5],
    ["game_difficulty", { GameDifficulty: 1 }, "invalid", "Brutal"],
    ["game_mode_type", { GameModeType: "PvP" }, "invalid", "PvE"]
  ]) {
    const baseline = vrisingSettingsDefinition.initializeSettings({ server_game_settings_json: JSON.stringify(native) }, context);
    const invalid = vrisingSettingsDefinition.applySettingsPatch(baseline, { [fieldKey]: invalidValue }, context);
    assert.equal(invalid[fieldKey], invalidValue);
    assert.deepEqual(parseRaw(invalid), native, `${fieldKey}: ${invalidValue}`);
    const corrected = vrisingSettingsDefinition.applySettingsPatch(invalid, { [fieldKey]: correctedValue }, context);
    assert.equal(vrisingSettingsDefinition.initializeSettings(corrected, context)[fieldKey], correctedValue);
    assert.deepEqual(vrisingSettingsDefinition.getSettingsValidationIssues(corrected, context), []);
  }
});

test("V Rising clearing a typed override still removes that managed native value", () => {
  const baseline = vrisingSettingsDefinition.initializeSettings({
    server_game_settings_json: '{"ClanSize":4,"WarEventGameSettings":{"Interval":3,"FutureRule":true}}'
  }, context);
  const cleared = vrisingSettingsDefinition.applySettingsPatch(baseline, {
    clan_size: undefined, war_event_interval: undefined
  }, context);
  assert.deepEqual(parseRaw(cleared), { WarEventGameSettings: { FutureRule: true } });
});
