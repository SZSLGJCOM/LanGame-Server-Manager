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
const { EN_US_MESSAGES } = require("../src/i18n-messages.ts");
const { ZH_CN_MESSAGES } = require("../src/i18n-messages-zh-cn.ts");
const root = path.resolve(__dirname, "../../..");
const read = (file) => fs.readFileSync(path.join(root, file), "utf8");

function parsed(moduleId, locale = "en-US") {
  const messages = locale === "zh-CN" ? ZH_CN_MESSAGES : EN_US_MESSAGES;
  return parseGuidedSettingsSchema({ summary: { id: moduleId, name: moduleId },
    schema_json: read(`modules/${moduleId}/schema.json`) }, locale,
  (key, _parameters, fallback) => messages[key] ?? fallback ?? key);
}

test("Astroneer help describes the semaphore-specific shutdown path and inactivity seconds", () => {
  for (const locale of ["en-US", "zh-CN"]) {
    const fields = parsed("astroneer", locale).fields;
    const field = (key) => fields.find((item) => item.key === key);
    assert.match(field("wait_for_players_before_shutdown").description, /ExitSemaphore/);
    assert.match(field("disable_server_travel").description, /ServerTravel/);
    assert.match(field("player_activity_timeout_seconds").description, locale === "zh-CN" ? /0.*关闭/ : /0.*disable/);
    assert.match(field("player_activity_timeout_seconds").title, locale === "zh-CN" ? /秒/ : /Seconds/);
    assert.deepEqual(validateGuidedSettingsObject(parsed("astroneer", locale), { player_activity_timeout_seconds: 1 })
      .filter((issue) => issue.fieldKey === "player_activity_timeout_seconds"), []);
  }
});

test("Palworld distinguishes building limits and the verified boundary-effect polarity", () => {
  for (const locale of ["en-US", "zh-CN"]) {
    const settings = parsed("palworld", locale);
    const fields = settings.fields;
    assert.match(fields.find((item) => item.key === "max_building_limit_num").title, locale === "zh-CN" ? /基地/ : /Base/);
    assert.match(fields.find((item) => item.key === "max_building_limit_num_per_player").title, locale === "zh-CN" ? /玩家/ : /Player/);
    assert.match(fields.find((item) => item.key === "active_unko").description, /Unko_S.*Unko_L/);
    const boundary = fields.find((item) => item.key === "invisible_other_guild_base_camp_area_fx");
    assert.match(boundary.title, locale === "zh-CN" ? /隐藏其他公会/ : /Hide Other Guild/);
    assert.match(boundary.description, locale === "zh-CN" ? /不隐藏.*建筑.*地图标记/ : /does not hide.*structures.*map markers/);
  }
});

const retired = {
  humanitz: {
    only_allowed_players: "OnlyAllowedPlayers", clear_infection_on_respawn: "ClearInfection",
    human_difficulty: "HumanDifficulty", loot_rarity: "LootRarity", eagle_eye_enabled: "EagleEye",
    void_enabled: "Void", vehicle_decay_hours: "VehicleDecayHours",
    owned_vehicle_decay_hours: "OwnedVehicleDecayHours", vehicle_decay_duration_seconds: "VehicleDecayDuration",
    max_world_vehicles: "MaxWorldVehicles", max_vehicles_per_player: "MaxVehiclesPerPlayer",
    vehicle_respawn_enabled: "VehicleRespawnEnabled", vehicle_respawn_interval_seconds: "VehicleRespawnInterval",
    ambient_vehicle_damage: "AmbientVehicleDamage", ambient_damage_rate: "AmbientDamageRate",
    zombies_target_vehicles: "ZombiesTargetVehicles",
  },
  palworld: {
    coop_player_max_num: "CoopPlayerMaxNum", is_multiplay: "bIsMultiplay", difficulty: "Difficulty",
    enable_defense_other_guild_player: "bEnableDefenseOtherGuildPlayer",
    enable_non_login_penalty: "bEnableNonLoginPenalty",
  },
};

test("unsupported dedicated controls are absent from schemas, groups and renderer sources", () => {
  for (const [moduleId, keys] of Object.entries(retired)) {
    const template = moduleId === "humanitz" ? "modules/humanitz/templates/GameServerSettings.ini.hbs"
      : "crates/app-storage/src/templates_render_palworld.rs";
    for (const [key, nativeKey] of Object.entries(keys)) {
    for (const locale of ["en-US", "zh-CN"]) {
      assert.ok(!parsed(moduleId, locale).fields.some((field) => field.key === key));
    }
    assert.ok(!read(`apps/desktop/src/views/settings/modules/${moduleId}.ts`).includes(`"${key}"`));
    assert.doesNotMatch(read(template), new RegExp(`(?:^|["\\n])${nativeKey}=`), `${moduleId}: ${nativeKey}`);
    const fixtureName = fs.readdirSync(path.join(root, "modules", moduleId, "config-fixtures"))
      .find((name) => name.startsWith("2026-07-13-"));
    const fixture = JSON.parse(read(`modules/${moduleId}/config-fixtures/${fixtureName}`));
    assert.ok(fixture.classifications.excluded.some((entry) => entry.endsWith(`.${nativeKey}`)));
    assert.ok(!fixture.classifications.editable.some((entry) => entry.endsWith(`.${nativeKey}`)));
    }
  }
});

test("HumanitZ retains confirmed grid units and current threat and loot controls", () => {
  for (const locale of ["en-US", "zh-CN"]) {
    const settings = parsed("humanitz", locale);
    const field = (key) => settings.fields.find((item) => item.key === key);
    assert.match(field("map_segment_2").description, /60.*60/);
    for (const key of ["human_health", "human_speed", "human_damage", "rarity_food", "rarity_drink",
      "rarity_melee", "rarity_ranged", "rarity_ammo", "rarity_armor", "rarity_resources", "rarity_other",
      "max_owned_cars", "recycle_car_days"]) assert.ok(field(key), `${locale}: ${key}`);
  }
});

test("HumanitZ exposes current NetID rosters and retains the former MVP file only as user data", () => {
  const schema = JSON.parse(read("modules/humanitz/schema.json"));
  assert.ok(!Object.hasOwn(schema.properties, "allowed_player_steam_ids"));
  assert.ok(!fs.existsSync(path.join(root, "modules/humanitz/templates/F_MVPAccess.txt.hbs")));
  for (const key of ["admin_steam_ids", "reserved_player_steam_ids", "banned_player_steam_ids"]) {
    assert.equal(schema.properties[key]["x-lsgm-player-access-codec"], "humanitz_net_id");
    for (const messages of [EN_US_MESSAGES, ZH_CN_MESSAGES]) {
      assert.match(messages[`settings.schema.humanitz.${key}.title`], /NetID/);
      assert.match(messages[`settings.schema.humanitz.${key}.description`], /PlayerIDMapped\.txt/);
      assert.match(messages[`settings.schema.humanitz.${key}.description`], /\|/);
    }
  }
  const moduleText = read("modules/humanitz/module.toml");
  assert.match(moduleText, /runtime_copy_exclusions = .*HumanitZServer\/F_MVPAccess\.txt/);
  assert.match(moduleText, /retained_paths = \[[\s\S]*?HumanitZServer\/F_MVPAccess\.txt/);
  assert.doesNotMatch(moduleText, /target_label = "Steam64 ID"/);
});
