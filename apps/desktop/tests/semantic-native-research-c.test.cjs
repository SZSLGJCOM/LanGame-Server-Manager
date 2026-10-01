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
const { buildScumNativeMessageCatalog } = require("../src/i18n/games/scum-native-messages.ts");
const root = path.resolve(__dirname, "../../..");
const schema = JSON.parse(fs.readFileSync(path.join(root, "modules/conanexiles/schema.json"), "utf8"));
const fractionalKeys = [
  "pvp_build_from_storage_radius", "building_preload_radius", "player_corpse_life_time", "npc_corpse_life_time",
  "item_repair_durability_loss_penalty_chance", "stamina_static_regen_rate_multiplier", "stamina_moving_regen_rate_multiplier",
  "player_stamina_regen_speed_scale", "stamina_on_exhaustion_regen_pause", "thrall_scouting_time_minutes",
  "thrall_min_distance_away_from_home", "thrall_teleporting_cooldown", "conciousness_damage_multiplier",
  "max_building_decay_time", "max_decay_time_to_auto_demolish", "thrall_decay_time", "building_decay_time_per_score",
  "decay_cleanup_time_multiplier", "decay_bonus_time_rate", "local_nav_mesh_visualization_frequency",
  "local_land_claim_visualization_frequency", "allowed_time_undermesh", "avatar_summon_time"
];

function parsed(locale) {
  const messages = locale === "zh-CN" ? ZH_CN_MESSAGES : EN_US_MESSAGES;
  return parseGuidedSettingsSchema({ summary: { id: "conanexiles", name: "Conan Exiles" }, schema_json: JSON.stringify(schema) },
    locale, (key, _parameters, fallback) => messages[key] ?? fallback ?? key);
}

test("Conan current native float settings accept fractions in the configuration form and INI fixture", () => {
  const settings = parsed("en-US");
  const fixture = JSON.parse(fs.readFileSync(path.join(root, "modules/conanexiles/config-fixtures/2026-07-13-steamcmd_anonymous_validate_443030.json"), "utf8"));
  const values = Object.fromEntries(fractionalKeys.map((key) => [key, 1.25]));
  assert.deepEqual(validateGuidedSettingsObject(settings, values).filter((issue) => fractionalKeys.includes(issue.fieldKey)), []);
  const ini = fixture.expected.files.find((file) => file.path.endsWith("/ServerSettings.ini"));
  for (const key of fractionalKeys) {
    assert.ok(!Number.isInteger(fixture.settings[key]), `${key} must exercise a fraction`);
    assert.equal(ini.keys[schema.properties[key]["x-lsgm-source-key"]], String(fixture.settings[key]));
  }
});

test("Conan defaults use the packaged Blueprint knockout duration and preserve explicit durations", () => {
  assert.equal(schema.properties.unconscious_time_seconds.default, 1800);
  for (const value of [600, 1800, 1800.5]) {
    const issues = validateGuidedSettingsObject(parsed("en-US"), { unconscious_time_seconds: value });
    assert.deepEqual(issues.filter((issue) => issue.fieldKey === "unconscious_time_seconds"), []);
  }
  const fixture = JSON.parse(fs.readFileSync(path.join(root, "modules/conanexiles/config-fixtures/2026-09-30-native_semantics_build_25488622.json"), "utf8"));
  const ini = fixture.expected.files.find((file) => file.path.endsWith("/ServerSettings.ini"));
  assert.equal(ini.keys["ServerSettings.UnconsciousTimeSeconds"], "1800");
});

test("Conan death drops expose the three native numeric modes in survival and preserve their INI values", () => {
  for (const locale of ["en-US", "zh-CN"]) {
    const settings = parsed(locale);
    const field = settings.fields.find((entry) => entry.key === "drop_equipment_on_death");
    assert.ok(field, "Death drops must have one dedicated configuration control");
    assert.equal(field.sectionId, "survival");
    assert.equal(field.control, "select");
    assert.deepEqual(field.enumOptions.map((option) => option.value), [0, 1, 2]);
    assert.deepEqual(field.enumOptions.map((option) => option.label), locale === "zh-CN"
      ? ["保留全部物品", "掉落全部物品", "仅掉落背包物品"]
      : ["Keep all items", "Drop all items", "Drop backpack only"]);
    for (const value of [0, 1, 2]) {
      assert.deepEqual(validateGuidedSettingsObject(settings, { drop_equipment_on_death: value })
        .filter((issue) => issue.fieldKey === field.key), []);
    }
    for (const value of [3, -1, 1.5, "DropEverything", "1"]) {
      assert.ok(validateGuidedSettingsObject(settings, { drop_equipment_on_death: value })
        .some((issue) => issue.fieldKey === field.key), `Reject non-mode ${value}`);
    }
  }
  assert.equal(schema.properties.drop_equipment_on_death.default, 1);
  const template = fs.readFileSync(path.join(root, "modules/conanexiles/templates/ServerSettings.ini.hbs"), "utf8");
  assert.equal(template.split("DropEquipmentOnDeath={{drop_equipment_on_death}}").length, 2);
  for (const [filename, value] of [
    ["2026-09-30-native_semantics_build_25488622.json", 1],
    ["2026-09-30-native_semantics_existing_unconscious.json", 0],
    ["2026-07-13-steamcmd_anonymous_validate_443030.json", 2]
  ]) {
    const fixture = JSON.parse(fs.readFileSync(path.join(root, "modules/conanexiles/config-fixtures", filename), "utf8"));
    assert.equal(fixture.settings.drop_equipment_on_death ?? schema.properties.drop_equipment_on_death.default, value);
    const ini = fixture.expected.files.find((file) => file.path.endsWith("/ServerSettings.ini"));
    assert.equal(ini.keys["ServerSettings.DropEquipmentOnDeath"], String(value));
  }
});

test("Conan time fields explain their verified seconds and HHMM input", () => {
  for (const locale of ["en-US", "zh-CN"]) {
    const settings = parsed(locale);
    for (const key of ["thrall_decay_time", "lqavp_use_time", "lqavp_fade_time", "network_simulated_smooth_rotation_time_with_lqavp"]) {
      const field = settings.fields.find((entry) => entry.key === key);
      assert.match(field.title, locale === "zh-CN" ? /秒/ : /seconds/);
    }
    const schedule = settings.fields.filter((entry) => /^pvp_(time|building_damage_time)_/.test(entry.key));
    assert.equal(schedule.length, 28);
    for (const field of schedule) assert.match(field.description, /HHMM.*1830.*18:30/);
  }
});

test("Conan small-building decay controls are integrated with the native INI and building category", () => {
  const settings = parsed("en-US");
  for (const [key, expectedDefault, native] of [
    ["override_decay_max_building_pieces", 6, "OverrideDecayMaxBuildingPieces"],
    ["override_decay_time", 3600, "OverrideDecayTime"]
  ]) {
    const field = settings.fields.find((entry) => entry.key === key);
    assert.equal(field.sectionId, "building");
    assert.equal(schema.properties[key].default, expectedDefault);
    const template = fs.readFileSync(path.join(root, "modules/conanexiles/templates/ServerSettings.ini.hbs"), "utf8");
    assert.ok(template.includes(`${native}={{${key}}}`));
  }
  assert.match(settings.fields.find((entry) => entry.key === "override_decay_time").description, /10%/);
});

test("Conan advanced controls describe native diagnostics and attribute enforcement", () => {
  const en = parsed("en-US");
  const zh = parsed("zh-CN");
  for (const key of ["local_nav_mesh_visualization_frequency", "local_land_claim_visualization_frequency"]) {
    assert.match(en.fields.find((entry) => entry.key === key).title, /interval \(seconds\)/);
    assert.match(en.fields.find((entry) => entry.key === key).description, /negative value disables/);
    assert.match(zh.fields.find((entry) => entry.key === key).description, /客户端调试/);
    assert.equal(schema.properties[key].default, -1);
  }
  const points = en.fields.find((entry) => entry.key === "validate_player_stats");
  assert.equal(points.sectionId, "access");
  assert.match(points.description, /character level.*resets the attribute points/);
  assert.match(zh.fields.find((entry) => entry.key === "cap_character_layout_scalar_params").description, /三个缩放参数/);
  assert.match(en.fields.find((entry) => entry.key === "lqavp_method").description, /0.*1.*2.*3/);
  assert.equal(schema.properties.lqavp_method.enum, undefined, "Native fallthrough values must remain representable");
});

test("SCUM current defaults match unconditional native constructor values in schema and inventory", () => {
  const scum = JSON.parse(fs.readFileSync(path.join(root, "modules/scum/schema.json"), "utf8"));
  for (const [section, key, current] of [
    ["general", "max_ping", 200], ["world", "max_allowed_killbox_keycards", 4],
    ["world", "max_allowed_killbox_keycards_police_station", 3]
  ]) {
    const field = scum.properties[`server_${section}`].properties[key];
    const inventory = JSON.parse(fs.readFileSync(path.join(root, `modules/scum/server-settings-v7/${section}.json`), "utf8"));
    assert.equal(field.default, current);
    assert.equal(inventory.find((entry) => entry.key === key).default, current);
    assert.equal(field.const, undefined);
    assert.equal(field.enum, undefined);
  }
});

test("SCUM explains purchase availability, seasonal ghosts and native distance/time units", () => {
  const en = buildScumNativeMessageCatalog("en-US");
  const zh = buildScumNativeMessageCatalog("zh-CN");
  const message = (catalog, key, part) => catalog[`scum.settings.native.${key}.${part}`];
  for (const key of ["item_virtualization_visitor_distance_travelled_for_update", "item_virtualization_visitor_bounds", "virtualized_item_bounds"]) {
    assert.match(message(en, key, "title"), /cm/);
    assert.match(message(zh, key, "title"), /厘米/);
  }
  assert.match(message(en, "logout_timer_in_bunker", "description"), /seconds/i);
  assert.match(message(zh, "logout_timer_in_bunker", "title"), /秒/);
  assert.match(message(zh, "disable_examine_ghost", "title"), /节日幽灵/);
  const vehicleKeys = Object.keys(en).filter((key) => key.endsWith("_min_purchased_amount.title"));
  assert.equal(vehicleKeys.length, 15);
  for (const key of vehicleKeys) {
    assert.match(en[key], /available to buy/);
    assert.match(zh[key], /最低可购数量/);
  }
  assert.match(message(en, "laika_min_purchased_amount", "description"), /engines/);
  assert.doesNotMatch(message(en, "dirtbike_min_purchased_amount", "description"), /engines/);
});


test("Conan packaged defaults preserve explicit settings across native fixtures", () => {
  const expected = { unconscious_time_seconds: 1800, everybody_can_loot_corpse: false, client_catch_up_time: 10, max_nudity: 0 };
  for (const [key, value] of Object.entries(expected)) assert.equal(schema.properties[key].default, value);
  const custom = JSON.parse(fs.readFileSync(path.join(root, "modules/conanexiles/config-fixtures/2026-09-30-native_semantics_existing_unconscious.json"), "utf8"));
  const native = custom.expected.files[0].keys;
  for (const [key, value] of Object.entries({ unconscious_time_seconds: 600, everybody_can_loot_corpse: true, client_catch_up_time: 14, max_nudity: 2 })) {
    assert.equal(custom.settings[key], value);
    assert.equal(native[schema.properties[key]["x-lsgm-source-key"]], String(value));
  }
});

test("Conan Blueprint consumption controls have distinct semantics and native output", () => {
  assert.equal(schema.properties.disabled_knowledge_ids["x-lsgm-source-key"], "ServerSettings.FeatsBlacklist");
  const values = { player_offline_hunger_multiplier: 1.25, player_offline_thirst_multiplier: 1.25, player_water_multiplier: 1.25, shield_durability_multiplier: 1.25, disabled_knowledge_ids: '("1","2","2")', enable_building_destruction_capsules: true };
  const settings = parsed("en-US");
  assert.deepEqual(validateGuidedSettingsObject(settings, values).filter((issue) => issue.fieldKey in values), []);
  const fixture = JSON.parse(fs.readFileSync(path.join(root, "modules/conanexiles/config-fixtures/2026-09-30-native_semantics_existing_unconscious.json"), "utf8"));
  const native = fixture.expected.files[0].keys;
  for (const [key, value] of Object.entries(values)) {
    assert.equal(fixture.settings[key], value);
    assert.equal(native[schema.properties[key]["x-lsgm-source-key"]], String(value));
    const field = settings.fields.find((field) => field.key === key);
    assert.equal(field.sectionId, key === "disabled_knowledge_ids" ? "rates" : key === "enable_building_destruction_capsules" ? "building" : key === "shield_durability_multiplier" ? "combat" : "survival");
  }
  assert.match(settings.fields.find((field) => field.key === "player_water_multiplier").description, /water restored/);
  assert.match(settings.fields.find((field) => field.key === "shield_durability_multiplier").description, /weapon blocks/);
  assert.match(settings.fields.find((field) => field.key === "enable_building_destruction_capsules").description, /capsule-shaped.*destroy buildings/);
  for (const value of ["()", '("1")', '("1", "2")', '("1","1")']) {
    assert.deepEqual(validateGuidedSettingsObject(settings, { disabled_knowledge_ids: value }).filter((issue) => issue.fieldKey === "disabled_knowledge_ids"), []);
  }
  for (const value of ["1,2", '("abc")', '("1")\nInjected=True']) {
    assert.ok(validateGuidedSettingsObject(settings, { disabled_knowledge_ids: value }).some((issue) => issue.fieldKey === "disabled_knowledge_ids"));
  }
  for (const key of ["server_blacklisted_feat_ids", "server_building_pvp_whitelist_ids", "player_movement_acceleration_multiplier", "validate_phys_nav_walk_with_raycast"]) assert.equal(schema.properties[key], undefined);
});

test("Conan storm windows use four HHMM fields in the existing Maelstrom category", () => {
  const settings = parsed("en-US");
  for (const period of ["weekday", "weekend"]) for (const edge of ["start", "end"]) {
    const key = `storm_time_${period}_${edge}`;
    const field = settings.fields.find((entry) => entry.key === key);
    assert.equal(field.sectionId, "world");
    assert.equal(field.type, "integer");
    assert.equal(schema.properties[key].default, edge === "start" ? 0 : 2359);
    assert.match(field.description, /server local time.*HHMM.*1830.*excludes its end.*cannot cross midnight/);
  }
});


test("Conan action stamina permits fractions and the unsupported Blueprint control is absent", () => {
  const settings = parsed("en-US");
  const values = { action_stamina_cost_multiplier: 1.25 };
  assert.equal(schema.properties.player_stamina_cost_multiplier, undefined);
  assert.deepEqual(validateGuidedSettingsObject(settings, values).filter((issue) => issue.fieldKey in values), []);
  const fixture = JSON.parse(fs.readFileSync(path.join(root, "modules/conanexiles/config-fixtures/2026-09-30-native_semantics_existing_unconscious.json"), "utf8"));
  for (const [key, value] of Object.entries(values)) {
    assert.equal(fixture.settings[key], value);
    assert.equal(fixture.expected.files[0].keys[schema.properties[key]["x-lsgm-source-key"]], "1.25");
  }
  assert.match(settings.fields.find((field) => field.key === "action_stamina_cost_multiplier").description, /attacks, jumps and dodges.*Sprinting/);
});


test("Conan authentication ban policy is distinct from individual player lists", () => {
  const settings = parsed("en-US");
  const field = settings.fields.find((entry) => entry.key === "enable_ban_check");
  assert.equal(field.sectionId, "access");
  assert.equal(schema.properties.enable_ban_check.default, true);
  assert.match(field.description, /authentication response reports IsBanned/);
  for (const [suffix, value] of [["build_25488622", true], ["existing_unconscious", false]]) {
    const fixture = JSON.parse(fs.readFileSync(path.join(root, `modules/conanexiles/config-fixtures/2026-09-30-native_semantics_${suffix}.json`), "utf8"));
    assert.equal(fixture.expected.files[0].keys["ServerSettings.EnableBanCheck"], String(value));
  }
});

test("Conan section labels use complete Chinese gameplay names", () => {
  const sections = new Map(parsed("zh-CN").sections.map((section) => [section.id, section]));
  for (const [id, title] of Object.entries({
    pvp_schedule: "PvP 与神祇召唤时段",
    survival: "生存与耐力",
    building: "建筑与衰退",
    followers: "随从与奴隶",
    purge: "大清洗（Purge）",
    advanced: "高级设置"
  })) {
    assert.equal(sections.get(id)?.title, title, id);
    assert.ok(sections.get(id)?.description, `${id} must explain its setting scope`);
    assert.doesNotMatch(sections.get(id).description, /逃生口|原生配置：|分组中的设置/);
  }
  assert.match(sections.get("pvp_schedule").description, /建筑攻防.*神祇召唤/);
  assert.match(sections.get("followers").description, /救援.*人口上限/);
  assert.match(sections.get("building").description, /领地.*衰退/);
  assert.match(sections.get("survival").description, /饥饿.*耐力/);
  assert.match(sections.get("advanced").description, /运动预测.*INI.*启动参数/);
});
