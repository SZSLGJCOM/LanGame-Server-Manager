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
const { parseGuidedSettingsSchema } = require("../src/views/settings/guided-settings.ts");
const { buildConfigurationWorkspaceModel, resolveConfigurationSectionId } = require("../src/views/settings/configuration-workspace-model.ts");
const { resolveSettingsModuleDefinition } = require("../src/views/settings/module-registry.ts");
const { EN_US_MESSAGES } = require("../src/i18n-messages.ts");
const { ZH_CN_MESSAGES } = require("../src/i18n-messages-zh-cn.ts");
function schemaFor(id, locale, catalog = {}) {
  const schema = parseGuidedSettingsSchema({
    summary: { id, name: id },
    schema_json: fs.readFileSync(path.resolve(__dirname, "../../../modules", id, "schema.json"), "utf8")
  }, locale, (key, _params, fallback) => catalog[key] ?? fallback ?? key);
  assert.equal(schema.parseError, null);
  return schema;
}
function fieldSections(model) {
  return Object.fromEntries(model.items.map((item) => [item.fieldKey, item.sectionId]));
}

for (const id of ["arksurvivalascended", "arksurvivalevolved"]) {
  test(`${id} keeps identity together and separates world, permission and host controls`, () => {
    const model = buildConfigurationWorkspaceModel(schemaFor(id));
    const sections = fieldSections(model);
    assert.equal(resolveConfigurationSectionId(model), "room");
    const mapKey = id === "arksurvivalascended" ? "active_map_mod" : "map_mod_id";
    assert.deepEqual(model.items.filter((item) => item.sectionId === "room").map((item) => item.fieldKey).sort(),
      ["server_name", "map_name", "max_players", "server_password", "message", "duration", "custom_notification_url", mapKey].sort());
    assert.equal(sections.active_event, "world");
    assert.equal(sections.admin_password, "admin");
    assert.equal(sections.battleye_enabled, "join");
    assert.equal(sections.server_hardcore, "gameplay");
    const all = model.roots.flatMap(function flatten(node) { return [node, ...node.children.flatMap(flatten)]; });
    assert.equal(all.find((node) => node.id === "admin").parentId, "access");
    assert.equal(all.find((node) => node.id === "operations").parentId, "runtime");
    assert.equal(all.some((node) => node.id === "session"), false);
    assert.equal(model.items.find((item) => item.fieldKey === "admin_account_ids").owner, "player_access");
  });
  test(`${id} exposes the announcement source once in the localized room form`, () => {
    for (const [locale, catalog, title] of [
      ["zh-CN", ZH_CN_MESSAGES, "公告来源 URL"],
      ["en-US", EN_US_MESSAGES, "Announcement source URL"]
    ]) {
      const schema = schemaFor(id, locale, catalog);
      const t = (key, _params, fallback) => catalog[key] ?? fallback ?? key;
      const definition = resolveSettingsModuleDefinition(id);
      const roomGroups = definition.buildFieldGroups("room", schema.fields.filter((field) => field.sectionId === "room"), locale, t);
      const announcements = roomGroups.flatMap((group) => group.fields).filter((field) => field.key === "custom_notification_url");
      assert.equal(announcements.length, 1);
      assert.equal(announcements[0].title, title);
      const logGroups = definition.buildFieldGroups("logs", schema.fields.filter((field) => field.sectionId === "logs"), locale, t);
      assert.equal(logGroups.flatMap((group) => group.fields).some((field) => field.key === "custom_notification_url"), false);
    }
  });
}

// Structured native controls must remain in their gameplay or operational domain.
const nativeSemanticGroups = {
  arksurvivalascended: {
    b_allow_flyer_speed_leveling: "leveling/rules",
    b_allow_speed_leveling: "leveling/rules",
    b_allow_unlimited_respecs: "leveling/rules",
    b_disable_friendly_fire: "gameplay/combat-and-carry",
    b_pv_edisable_friendly_fire: "gameplay/combat-and-carry",
    b_disable_photo_mode: "gameplay/camera-and-map",
    photo_mode_range_limit: "gameplay/camera-and-map",
    hair_growth_speed_multiplier: "gameplay/appearance",
    disable_custom_cosmetics: "gameplay/appearance",
    tribe_tower_bonus_multiplier: "gameplay/tribes",
    b_disable_wireless_crafting: "crafting/wireless",
    b_disable_wireless_crafting_for_dinos: "crafting/wireless",
    b_disable_wireless_crafting_for_players: "crafting/wireless",
    b_disable_wireless_crafting_for_structures: "crafting/wireless",
    wireless_crafting_range_override: "crafting/wireless",
    crafting_skill_bonus_multiplier: "crafting/recipes",
    global_spoiling_time_multiplier: "crafting/item-rules",
    b_ignore_structures_prevention_volumes: "building/structure-rules",
    destroy_tames_over_level_clamp: "limits/tame-levels",
    limit_generators_num: "limits/generators",
    limit_generators_range: "limits/generators",
    exclude_item_indices: "loot/overrides",
    global_item_decomposition_time_multiplier: "loot/lifetime",
    craft_xpmultiplier: "rates/progression",
    generic_xpmultiplier: "rates/progression",
    harvest_xpmultiplier: "rates/progression",
    kill_xpmultiplier: "rates/progression",
    special_xpmultiplier: "rates/progression",
    harvest_resource_item_amount_class_multipliers: "rates/progression",
    wild_dino_character_food_drain_multiplier: "rates/survivor-pacing",
    base_hexagon_reward_multiplier: "rates/hexagon-economy",
    hexagon_cost_multiplier: "rates/hexagon-economy",
    max_fall_speed_multiplier: "balance/player-combat",
    prevent_breeding_for_class_names: "breeding/rules",
    valguero_memorial_entries: "world/environment",
    cheat_teleport_locations: "admin/teleport",
    server_platform: "join/gate",
    kick_idle_players_period: "join/gate",
    enable_idle_player_kick: "join/gate",
    enable_steel_shield: "network/protection",
  },
  arksurvivalevolved: {
    b_allow_flyer_speed_leveling: "leveling/rules",
    b_allow_unlimited_respecs: "leveling/rules",
    per_level_stats_multiplier_player_integer: "leveling/player",
    player_base_stat_multipliers_attribute: "leveling/player",
    per_level_stats_multiplier_dino_wild_integer: "leveling/wild",
    per_level_stats_multiplier_dino_tamed_type_integer: "leveling/tamed",
    mutagen_level_boost_stat_id: "leveling/tamed",
    mutagen_level_boost_bred_stat_id: "leveling/tamed",
    b_disable_friendly_fire: "gameplay/combat-and-carry",
    b_pv_edisable_friendly_fire: "gameplay/combat-and-carry",
    b_allow_unclaim_dinos: "gameplay/combat-and-carry",
    b_disable_dino_riding: "gameplay/combat-and-carry",
    b_disable_dino_taming: "gameplay/combat-and-carry",
    b_passive_defenses_damage_riderless_dinos: "gameplay/combat-and-carry",
    prevent_dino_tame_class_names: "gameplay/combat-and-carry",
    b_use_corpse_locator: "gameplay/camera-and-map",
    hair_growth_speed_multiplier: "gameplay/appearance",
    max_alliances_per_tribe: "gameplay/tribes",
    max_number_of_players_in_tribe: "gameplay/tribes",
    max_tribe_logs: "gameplay/tribes",
    max_tribes_per_alliance: "gameplay/tribes",
    tribe_slot_reuse_cooldown: "gameplay/tribes",
    b_pv_eallow_tribe_war: "gameplay/tribes",
    b_pv_eallow_tribe_war_cancel: "gameplay/tribes",
    auto_pv_estart_time_seconds: "gameplay/pve-schedule",
    auto_pv_estop_time_seconds: "gameplay/pve-schedule",
    b_auto_pv_etimer: "gameplay/pve-schedule",
    b_auto_pv_euse_system_time: "gameplay/pve-schedule",
    b_increase_pv_prespawn_interval: "gameplay/respawn",
    increase_pv_prespawn_interval_base_amount: "gameplay/respawn",
    increase_pv_prespawn_interval_check_period: "gameplay/respawn",
    increase_pv_prespawn_interval_multiplier: "gameplay/respawn",
    b_disable_genesis_missions: "gameplay/missions",
    b_disable_world_buffs: "gameplay/missions",
    b_enable_world_buff_scaling: "gameplay/missions",
    world_buff_scaling_efficacy: "gameplay/missions",
    b_disable_hexagon_store: "gameplay/hexagon-store",
    b_hex_store_allow_only_engram_trade_option: "gameplay/hexagon-store",
    crafting_skill_bonus_multiplier: "crafting/recipes",
    global_spoiling_time_multiplier: "crafting/item-rules",
    item_stat_clamps_attribute: "crafting/item-rules",
    b_ignore_structures_prevention_volumes: "building/structure-rules",
    b_allow_platform_saddle_multi_floors: "building/structure-rules",
    b_flyer_platform_allow_unaligned_dino_basing: "building/structure-rules",
    b_genesis_use_structures_prevention_volumes: "building/structure-rules",
    structure_damage_repair_cooldown: "building/structure-rules",
    fuel_consumption_interval_multiplier: "building/power",
    global_powered_battery_durability_decrease_per_second: "building/power",
    destroy_tames_over_level_clamp: "limits/tame-levels",
    b_use_tame_limit_for_structures_only: "limits/tame-levels",
    fast_decay_interval: "limits/abandoned-cleanup",
    limit_non_player_dropped_items_count: "limits/dropped-items",
    limit_non_player_dropped_items_range: "limits/dropped-items",
    use_item_dupe_check: "limits/integrity",
    enable_victory_core_dupe_check: "limits/integrity",
    exclude_item_indices: "loot/overrides",
    global_item_decomposition_time_multiplier: "loot/lifetime",
    global_corpse_decomposition_time_multiplier: "loot/lifetime",
    use_corpse_life_span_multiplier: "loot/lifetime",
    b_disable_default_map_item_sets: "loot/spawn-equipment",
    craft_xpmultiplier: "rates/progression",
    generic_xpmultiplier: "rates/progression",
    harvest_xpmultiplier: "rates/progression",
    kill_xpmultiplier: "rates/progression",
    special_xpmultiplier: "rates/progression",
    harvest_resource_item_amount_class_multipliers: "rates/progression",
    dino_harvesting_damage_multiplier: "rates/progression",
    player_harvesting_damage_multiplier: "rates/progression",
    passive_tame_interval_multiplier: "rates/progression",
    wild_dino_character_food_drain_multiplier: "rates/survivor-pacing",
    tamed_dino_character_food_drain_multiplier: "rates/survivor-pacing",
    tamed_dino_torpor_drain_multiplier: "rates/survivor-pacing",
    wild_dino_torpor_drain_multiplier: "rates/survivor-pacing",
    base_hexagon_reward_multiplier: "rates/hexagon-economy",
    hexagon_cost_multiplier: "rates/hexagon-economy",
    max_fall_speed_multiplier: "balance/player-combat",
    prevent_offline_pv_pconnection_invincible_interval: "balance/player-combat",
    dino_turret_damage_multiplier: "balance/wild-combat",
    pv_pzone_structure_damage_multiplier: "balance/structure-combat",
    prevent_breeding_for_class_names: "breeding/rules",
    b_disable_dino_breeding: "breeding/rules",
    override_max_experience_points_dino: "experience/curve",
    override_max_experience_points_player: "experience/curve",
    b_only_allow_specified_engrams: "engrams/posture",
    override_engram_entries: "engrams/indexed-overrides",
    base_temperature_multiplier: "world/environment",
    adjustable_mutagen_spawn_delay_multiplier: "farming/mutagen",
    crossplay: "join/gate",
    epiconly: "join/gate",
    kick_idle_players_period: "join/gate",
    enable_idle_player_kick: "join/gate",
    secure_send_ar_kpayload: "transfer/uploads",
    use_secure_spawn_rules: "spawns/integrity",
  },
};
for (const [id, expectedGroups] of Object.entries(nativeSemanticGroups)) {
  test(`${id} native controls have one semantic group and preserve workspace ownership`, () => {
    for (const [locale, catalog] of [["en-US", EN_US_MESSAGES], ["zh-CN", ZH_CN_MESSAGES]]) {
      const schema = schemaFor(id, locale, catalog);
      const model = buildConfigurationWorkspaceModel(schema);
      const definition = resolveSettingsModuleDefinition(id);
      const t = (key, _params, fallback) => catalog[key] ?? fallback ?? key;
      assert.deepEqual(schema.fields.filter((field) => field.sectionId === "advanced").map((field) => field.key).sort(),
        ["custom_launch_flags", "game_ini_extra", "game_user_settings_extra"]);
      for (const [key, expectedGroup] of Object.entries(expectedGroups)) {
        const [section, groupId] = expectedGroup.split("/");
        const item = model.items.find((candidate) => candidate.fieldKey === key);
        assert.equal(item?.sectionId, section, `${id}.${key}`);
        assert.equal(item?.owner, "configuration", `${id}.${key}`);
        const groups = definition.buildFieldGroups(section, schema.fields.filter((field) => field.sectionId === section), locale, t);
        const owners = groups.filter((group) => group.fields.some((field) => field.key === key));
        assert.equal(owners.length, 1, `${id}.${key} group count`);
        assert.equal(owners[0].id, groupId, `${id}.${key} group`);
        const titleKey = `ark.settings.groups.${section}.${groupId}.title`;
        assert.ok(catalog[titleKey], `${locale} ${titleKey} translated`);
        assert.equal(owners[0].title, catalog[titleKey], `${id}.${key} localized group`);
      }
      assert.equal(model.items.find((item) => item.fieldKey === "admin_account_ids")?.owner, "player_access");
      for (const section of new Set(schema.fields.map((field) => field.sectionId))) {
        const fields = schema.fields.filter((field) => field.sectionId === section);
        const groups = definition.buildFieldGroups(section, fields, locale, t);
        assert.deepEqual(groups.flatMap((group) => group.fields).map((field) => field.key).sort(), fields.map((field) => field.key).sort(), `${id}.${section} exact field coverage`);
        assert.equal(new Set(fields.map((field) => field.sortWeight)).size, fields.length, `${id}.${section} unique order`);
      }
    }
  });
}
