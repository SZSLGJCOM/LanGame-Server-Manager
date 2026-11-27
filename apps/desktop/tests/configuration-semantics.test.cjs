const assert = require("node:assert/strict");
const fs = require("node:fs");
const Module = require("node:module");
const path = require("node:path");
const test = require("node:test");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

for (const extension of [".ts", ".tsx"]) {
  require.extensions[extension] = (module, filename) => {
    module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
  };
}
require.extensions[".css"] = (module) => module._compile("module.exports = {};", module.filename);
require.extensions[".png"] = (module, filename) => { module.exports = filename; };

const root = path.resolve(__dirname, "../../..");
const settingsRoot = path.join(root, "apps/desktop/src/views/settings");
const { parseGuidedSettingsSchema, validateGuidedSettingsObject } = require("../src/views/settings/guided-settings.ts");
const { buildConfigurationWorkspaceModel, resolveConfigurationSectionId } = require("../src/views/settings/configuration-workspace-model.ts");
const { resolveSettingsModuleDefinition } = require("../src/views/settings/module-registry.ts");
const { EN_US_MESSAGES } = require("../src/i18n-messages.ts");
const { ZH_CN_MESSAGES } = require("../src/i18n-messages-zh-cn.ts");
const { I18nContext } = require("../src/i18n-context.ts");
const { buildScumNativeMessageCatalog } = require("../src/i18n/games/scum-native-messages.ts");

function readSchema(moduleId) {
  return JSON.parse(fs.readFileSync(path.join(root, "modules", moduleId, "schema.json"), "utf8"));
}

function schemaFor(moduleId, locale = "en-US", catalog = locale === "zh-CN" ? ZH_CN_MESSAGES : EN_US_MESSAGES) {
  const schema = parseGuidedSettingsSchema({
    summary: { id: moduleId, name: moduleId },
    schema_json: JSON.stringify(readSchema(moduleId))
  }, locale, (key, _params, fallback) => catalog[key] ?? fallback ?? key);
  assert.equal(schema.parseError, null, moduleId);
  return schema;
}

function modelFor(moduleId, locale = "en-US", catalog) {
  return buildConfigurationWorkspaceModel(schemaFor(moduleId, locale, catalog));
}

function fieldSections(model) {
  return Object.fromEntries(model.items.map((item) => [item.fieldKey, item.sectionId]));
}

for (const id of ["arksurvivalascended", "arksurvivalevolved"]) {
  test(`${id} keeps identity together and separates world, permission and host controls`, () => {
    const model = modelFor(id);
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

const nativeArkSemanticGroups = {
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
    enable_steel_shield: "network/protection"
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
    use_secure_spawn_rules: "spawns/integrity"
  }
};

for (const [id, expectedGroups] of Object.entries(nativeArkSemanticGroups)) {
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

test("DST room excludes rules and credentials while preserving native cluster controls", () => {
  const model = modelFor("dontstarve");
  const sections = fieldSections(model);
  assert.deepEqual(model.items.filter((item) => item.sectionId === "room").map((item) => item.fieldKey).sort(),
    ["cluster_name", "cluster_description", "max_players", "cluster_intention", "cluster_password", "cluster_language", "lan_only_cluster"].sort());
  for (const key of ["game_mode", "pvp", "vote_enabled"]) assert.equal(sections[key], "cluster-rules", key);
  for (const key of ["pause_when_empty", "tick_rate", "autosaver_enabled", "max_snapshots"]) assert.equal(sections[key], "cluster-runtime", key);
  for (const key of ["cluster_token", "friends_only", "whitelist_slots", "steam_group_only", "steam_group_id", "steam_group_admins"]) assert.equal(sections[key], "access", key);
  assert.equal(sections.shard_bind_ip, "network");
  assert.equal(sections.offline_cluster, "network");
  for (const key of ["admin_list", "whitelist", "blocklist"]) assert.equal(model.items.find((item) => item.fieldKey === key).owner, "player_access");
});

test("SCUM separates native network, permissions and maintenance and renders every native control once", () => {
  const model = modelFor("scum");
  const sections = fieldSections(model);
  for (const key of ["max_ping_check_enabled", "max_ping", "max_number_of_consecutive_high_ping_readings", "master_server_update_send_interval"]) {
    assert.equal(sections[`server_general.${key}`], "network", key);
  }
  assert.equal(sections["server_general.play_safe_id_protection"], "access");
  for (const key of ["min_server_tick_rate", "max_server_tick_rate", "log_suicides", "item_virtualization_event_processing_time_budget"]) {
    assert.equal(sections[`server_general.${key}`], "runtime", key);
  }
  assert.equal(sections["server_features.enable_net_watchdog"], "runtime");
  assert.equal(sections["server_general.allow_voting"], "general");
  assert.equal(sections.extra_launch_args, "runtime");
  for (const key of ["partial_wipe", "gold_wipe", "full_wipe"]) {
    assert.equal(sections[`server_general.${key}`], "maintenance", `${key} belongs to maintenance`);
    assert.equal(model.items.find((item) => item.fieldKey === `server_general.${key}`).owner, "maintenance");
    assert.ok(!model.actionableSectionIds.includes("maintenance"));
  }

  const { scumSettingsDefinition } = require("../src/views/settings/modules/scum.ts");
  const { SCUM_EDITABLE_SETTINGS, initializeScumSettings } = require("../src/views/settings/scum-server-settings-inventory.ts");
  const settings = initializeScumSettings({});
  const nativeKeys = Object.entries(scumSettingsDefinition.specializedRenderers)
    .filter(([id]) => id.startsWith("scum-server-"))
    .flatMap(([, renderer]) => {
      const html = renderToStaticMarkup(React.createElement(I18nContext.Provider, {
        value: { locale: "en-US", setLocale() {}, t: (key, _params, fallback) => fallback ?? key }
      }, React.createElement(renderer.Renderer, {
        sectionId: renderer.sectionId, fieldKey: renderer.fieldKey,
        details: { summary: { status: "stopped" } }, settings, disabled: false, onPatch() {}
      })));
      return [...html.matchAll(/data-scum-native-key="([^"]+)"/gu)].map((match) => match[1]);
    });
  assert.equal(nativeKeys.length, 433);
  assert.equal(new Set(nativeKeys).size, 433);
  assert.deepEqual(nativeKeys.sort(), SCUM_EDITABLE_SETTINGS.filter((setting) => !["partial_wipe", "gold_wipe", "full_wipe"].includes(setting.key)).map((setting) => setting.nativeKey).sort());
});

test("7 Days to Die validates sandbox codes, separates semantic sections and renders command permissions cleanly", () => {
  const schema = schemaFor("sevendaystodie");
  const issuesFor = (sandbox_code) => validateGuidedSettingsObject(schema, { sandbox_code })
    .filter((issue) => issue.fieldKey === "sandbox_code");
  for (const code of ["A", "AAAJABJACJADJARFBNC", "AAAJ"]) {
    assert.deepEqual(issuesFor(code), [], code);
  }
  for (const code of ["B", "BAAJ", "AA", "AAA", "AAAJZ", "AAAJZZ", "AAAJ1"]) {
    assert.ok(issuesFor(code).some((issue) => issue.reason === "pattern"), code);
  }

  const model = buildConfigurationWorkspaceModel(schema);
  const sections = fieldSections(model);
  assert.deepEqual(model.items.filter((item) => item.sectionId === "room").map((item) => item.fieldKey).sort(),
    ["server_name", "server_description", "server_website_url", "server_login_confirmation_text", "server_password", "max_players", "visibility", "region", "language", "game_world", "world_name"].sort());
  assert.equal(sections.command_permissions, "access");
  assert.equal(model.items.find((item) => item.fieldKey === "command_permissions").owner, "configuration");
  for (const key of ["telnet_enabled", "telnet_password", "server_allow_crossplay", "web_dashboard_enabled"]) assert.equal(sections[key], "network", key);
  for (const key of ["ignore_eos_sanctions", "eac_enabled", "twitch_server_permission"]) assert.equal(sections[key], "access", key);
  assert.equal(sections.terminal_window_enabled, "runtime");
  assert.equal(sections.game_mode, "world");

  const { ZH_CN_SEVEN_DAYS_TO_DIE_MESSAGES } = require("../src/i18n/games/sevendaystodie.zh-cn.ts");
  const { SevenDaysServerAdminPanel } = require("../src/views/settings/SevenDaysServerAdminPanel.tsx");
  const t = (key, _params, fallback) => ZH_CN_SEVEN_DAYS_TO_DIE_MESSAGES[key] ?? fallback ?? key;
  const html = renderToStaticMarkup(React.createElement(I18nContext.Provider, {
    value: { locale: "zh-CN", setLocale() {}, t }
  }, React.createElement(SevenDaysServerAdminPanel, {
    sectionId: "access", settings: { command_permissions: [] }, disabled: false, onPatch() {}
  })));
  assert.equal((html.match(/<h4\b/gu) ?? []).length, 1);
  assert.match(html, />控制台命令权限<\/h4>/u);
  assert.doesNotMatch(html, /<div class="detail-label">命令<\/div>|玩家名单|玩家访问/u);
});

const roomInventories = {
  abioticfactor: ["server_name", "world_save_name", "max_server_players", "server_password", "lan_only"],
  astroneer: ["server_name", "max_players", "server_password", "active_save_file_name"],
  barotrauma: ["server_name", "server_message", "server_password", "max_players", "public_server", "play_style", "language"],
  conanexiles: ["server_name", "server_message_of_the_day", "excluded_regions", "max_players", "server_region", "public_server", "server_password", "show_online_players"],
  corekeeper: ["server_name", "game_id", "max_players", "world_index", "join_password"],
  enshrouded: ["server_name", "max_players", "admin_password", "friend_password", "guest_password", "visitor_password", "server_tags"],
  humanitz: ["server_name", "max_players", "search_id", "welcome_message", "server_password", "save_name"],
  minecraft: ["motd", "max_players", "enable_status", "hide_online_players", "level_name", "bug_report_link"],
  necesse: ["world_name", "max_slots", "motd", "password", "language"],
  nightingale: ["server_password", "max_players"],
  palworld: ["server_name", "server_description", "max_players", "server_password", "community_server", "region", "show_player_list"],
  projectzomboid: ["server_name", "server_description", "welcome_message", "max_players", "public_server", "server_password"],
  returntomoria: ["server_name", "server_password", "world_file_name", "rules_message"],
  rimworld: ["server_name", "server_description", "server_image_url", "max_players", "enable_server_browser", "discord_url", "steam_workshop_url", "server_password", "chat_login_notifications", "chat_disconnect_notifications", "chat_enable_mo_td", "chat_message_of_the_day"],
  romestead: ["auto_start_world_name", "password", "max_players"],
  runescapedragonwilds: ["server_name", "default_world_name", "world_password"],
  rust: ["server_name", "server_description", "max_players", "tags", "website_url", "header_image_url", "logo_image_url", "level", "level_url", "censor_player_list"],
  satisfactory: ["max_players", "native_room"],
  sonsoftheforest: ["server_name", "max_players", "server_password", "lan_only", "save_slot"],
  soulmask: ["server_name", "server_password", "max_players"],
  squad: ["server_name", "max_players", "advertise", "lan_only", "map_rotation", "layer_rotation", "level_rotation", "motd_cfg", "server_message", "server_tags", "server_rules", "server_message_interval_seconds"],
  terraria: ["world_name", "world_file", "motd", "max_players", "password", "lobby", "language"],
  theforest: ["server_name", "max_players", "server_password", "save_slot"],
  unturned: ["server_name", "internet_server", "map", "max_players", "password", "welcome_message", "browser_icon_url", "browser_desc_hint", "browser_desc_full", "browser_links_json", "native_browser_desc_server_list", "native_browser_monetization", "native_browser_thumbnail"],
  valheim: ["server_name", "world_name", "server_password", "public_server"],
  vrising: ["server_name", "server_description", "max_players", "server_password", "list_on_eos", "save_name", "list_on_steam", "safe_reconnect_slots", "safe_reconnect_time"],
  windrose: ["server_name", "server_password", "invite_code", "max_players", "user_selected_region", "world_island_id", "world_name"]
};

test("module room sections own only their supported identity, join, listing and map selection fields", () => {
  for (const [moduleId, keys] of Object.entries(roomInventories)) {
    const model = modelFor(moduleId);
    assert.deepEqual(
      model.items.filter((item) => item.sectionId === "room" && item.owner === "configuration").map((item) => item.fieldKey).sort(),
      [...keys].sort(),
      moduleId
    );
  }
});

test("survival modules keep gameplay, operator credentials and network controls in dedicated reachable sections", () => {
  const expected = {
    astroneer: { console_password: "network", server_owner_display_name: "access", owner_guid: "access" },
    barotrauma: { tick_rate: "runtime", lines_per_log_file: "runtime", randomize_seed: "round", kill_disconnected_time: "round", despawn_disconnected_permadeath_time: "round", max_transport_time: "round", allow_remote_campaign_interactions: "campaign", karma_enabled: "gameplay", allow_spectating: "gameplay" },
    corekeeper: { world_seed: "world", world_mode: "world", allowed_platform_code: "access" },
    enshrouded: { voice_chat_mode: "communication", enable_voice_chat: "communication", enable_text_chat: "communication", game_settings_preset: "world" },
    humanitz: { pvp_enabled: "world", limited_spawns: "world", no_death_feedback: "world", no_join_feedback: "world", map_segment_0: "world", map_segment_1: "world", map_segment_2: "world", voip_enabled: "network" },
    minecraft: { log_ips: "advanced", allow_flight: "access", chat_spam_threshold_seconds: "access", command_spam_threshold_seconds: "access", text_filtering_config: "access", text_filtering_version: "access", initial_enabled_packs: "world", initial_disabled_packs: "world", resource_pack: "world", resource_pack_id: "world", resource_pack_sha1: "world", resource_pack_prompt: "world", require_resource_pack: "world" },
    necesse: { ignore_seasons: "world", strict_server_authority: "access", max_client_latency_seconds: "network", unload_levels_cooldown: "host", unload_settlements: "host", max_settlements_per_player: "world", world_border_size: "world" },
    nightingale: { admin_password: "access", enable_cheats: "access" },
    palworld: { base_camp_max_num: "world", base_camp_max_num_in_guild: "world", base_camp_worker_max_num: "world", guild_player_max_num: "world", guild_rejoin_cooldown_minutes: "world", max_building_limit_num: "world", max_building_limit_num_per_player: "world", drop_item_alive_max_hours: "world", join_left_message: "world", enable_building_player_uid_display: "world", chat_post_limit_per_minute: "access", enable_voice_chat: "network", voice_chat_max_volume_distance: "network", voice_chat_zero_volume_distance: "network" },
    projectzomboid: { pause_empty: "services", global_chat: "moderation", chat_streams: "moderation", chat_message_character_limit: "moderation", admin_username: "access", admin_password: "access", upnp: "network", max_packets_per_second: "network", login_queue_connect_timeout: "network", voice_enable: "network", voice_min_distance: "network", voice_max_distance: "network", voice_3d: "network", anti_cheat_speed: "anticheat", war: "safehouses", disable_vehicle_towing: "world", display_user_name: "world", show_first_and_last_name: "world", steam_scoreboard: "world", mouse_over_to_see_display_name: "world", hide_players_behind_you: "world", login_queue_enabled: "join" },
    returntomoria: { world_seed: "world", console_enabled: "runtime", advertise_address: "network", advertise_port: "network" },
    rimworld: { verbosity: "runtime", display_chat_in_console: "runtime", sync_local_save: "runtime" },
    romestead: { enable_cheats: "access", auto_create_world_seed: "world" },
    runescapedragonwilds: { owner_id: "access", admin_password: "access" },
    rust: { server_gamemode: "gamemode", pve: "gamemode", favorites_endpoint: "network", seed: "world", bans_server_endpoint: "access", bans_server_failure_mode: "access", bans_server_timeout_seconds: "access", reports_print_to_console: "access", reports_server_endpoint: "access", reports_server_endpoint_key: "access" },
    satisfactory: { disable_seasonal_events: "world", net_connection_timeout_seconds: "network", net_initial_connect_timeout_seconds: "network", net_max_client_rate: "network", net_max_internet_client_rate: "network" },
    sonsoftheforest: { game_mode: "world", save_mode: "world", idle_day_cycle_speed: "world" },
    soulmask: { pvp_mode_arg: "world", admin_password: "access" },
    squad: { enforce_team_balance: "world", tk_auto_kick_enabled: "world", reserved_slots: "access", public_queue_limit: "access", joining_player_timeout_seconds: "network", remote_admin_hosts: "access", remote_ban_hosts: "access", excluded_factions: "world", excluded_layers: "world", excluded_levels: "world" },
    terraria: { upnp: "network", steam: "network", seed: "world", disableannouncementbox: "world", announcementboxrange: "world" },
    theforest: { admin_password: "access", allow_cheats: "access" },
    unturned: { timeout_seconds: "network", max_ping: "network" },
    valheim: { crossplay_enabled: "network", instance_id: "network", custom_launch_flags: "advanced", log_file: "advanced" },
    vrising: { max_admins: "access", server_fps: "advanced", hide_ip_address: "network", admin_only_debug_events: "access", api_enabled: "network" },
    windrose: { allow_multiple_server_instances: "advanced", auto_load_latest_backup_if_has_broken: "advanced" }
  };

  for (const [moduleId, fields] of Object.entries(expected)) {
    const model = modelFor(moduleId);
    for (const [key, sectionId] of Object.entries(fields)) {
      const item = model.items.find((entry) => entry.fieldKey === key);
      assert.equal(item?.sectionId, sectionId, `${moduleId}.${key}`);
      assert.equal(item?.owner, "configuration", `${moduleId}.${key}`);
      assert.ok(model.actionableSectionIds.includes(sectionId), `${moduleId}.${key} reachable`);
    }
  }

  const enshroudedSections = resolveSettingsModuleDefinition("enshrouded").getSections((_key, _params, fallback) => fallback);
  for (const id of ["admin_role", "friend_role", "guest_role", "visitor_role"]) {
    assert.equal(enshroudedSections.find((section) => section.id === id)?.parentId, "access", id);
  }
  const terrariaSections = resolveSettingsModuleDefinition("terraria").getSections((key, _params, fallback) => fallback ?? key);
  assert.equal(terrariaSections.find((section) => section.id === "journey")?.parentId, "access");
});

test("Windrose world identity renderer follows the room, enforces process guards and renders unique DOM IDs", () => {
  const { windroseSettingsDefinition } = require(path.join(settingsRoot, "modules/windrose.ts"));
  const { fieldPresentationOverrides, specializedRenderers } = windroseSettingsDefinition;
  const name = fieldPresentationOverrides.world_name;
  assert.equal(name.sectionId, "room");
  assert.equal(specializedRenderers[name.rendererId].sectionId, "room");
  assert.equal(specializedRenderers[name.rendererId].fieldKey, undefined);
  for (const [key, presentation] of Object.entries(fieldPresentationOverrides)) {
    if (key === "world_name") continue;
    assert.equal(presentation.sectionId, "world", key);
    assert.equal(specializedRenderers[presentation.rendererId].sectionId, "world", key);
  }

  const panelFile = path.join(settingsRoot, "WindroseWorldSettingsPanel.tsx");
  const loadedPanel = new Module(panelFile, module);
  const panelRequire = Module.createRequire(panelFile);
  loadedPanel.filename = panelFile;
  loadedPanel.require = (id) => id === "../../i18n" ? {
    useI18n: () => ({ t: (key, _params, fallback) => fallback ?? key })
  } : id === "./ConfigurationFieldHelp" ? {
    useConfigurationFieldHelp: () => ({ anchorRef() {}, interactionProps: {}, helpNode: null })
  } : panelRequire(id);
  loadedPanel._compile(transpileTypeScript(fs.readFileSync(panelFile, "utf8"), panelFile), panelFile);
  function findInput(element) {
    if (!React.isValidElement(element)) return null;
    if (element.type === "input") return element;
    if (typeof element.type === "function") return findInput(element.type(element.props));
    for (const child of React.Children.toArray(element.props.children)) {
      const input = findInput(child);
      if (input) return input;
    }
    return null;
  }
  for (const [status, worldId, disabled, expected] of [
    ["stopped", "island-one", false, false],
    ["stopped", "", false, true],
    ["starting", "island-one", false, true],
    ["running", "island-one", false, true],
    ["stopping", "island-one", false, true],
    ["stopped", "island-one", true, true]
  ]) {
    let patch;
    const input = findInput(loadedPanel.exports.WindroseWorldNameField({
      details: { summary: { status } }, settings: { world_island_id: worldId, world_name: "Existing world" },
      disabled, onPatch: (value) => { patch = value; }
    }));
    assert.ok(input);
    assert.equal(input.props.disabled, expected, `${status}/${worldId}/${disabled}`);
    assert.equal(input.props.value, "Existing world");
    if (!expected) {
      input.props.onChange({ target: { value: "Harbor" } });
      assert.deepEqual(patch, { world_name: "Harbor" });
    }
  }

  const workspaceFile = path.join(settingsRoot, "ConfigurationWorkspace.tsx");
  const loadedWorkspace = new Module(workspaceFile, module);
  const workspaceRequire = Module.createRequire(workspaceFile);
  loadedWorkspace.filename = workspaceFile;
  loadedWorkspace.require = (id) => id === "./useAutoSaveInstanceSettings" ? {
    useAutoSaveInstanceSettings: () => ({ status: { state: "saved" }, retry() {} })
  } : id === "./useInstancePortRegistration" ? {
    useInstancePortRegistration: () => ({ ports: [], defaultPorts: [], setPorts() {} })
  } : id === "./useModuleConfigurationIcons" ? {
    useModuleConfigurationIcons: () => ({ icons: {} })
  } : workspaceRequire(id);
  loadedWorkspace._compile(transpileTypeScript(fs.readFileSync(workspaceFile, "utf8"), workspaceFile), workspaceFile);
  const windroseSchema = readSchema("windrose");
  const defaults = Object.fromEntries(Object.entries(windroseSchema.properties).map(([key, property]) => [key, property.default]));
  const html = renderToStaticMarkup(React.createElement(I18nContext.Provider, {
    value: { locale: "en-US", setLocale() {}, t: (key, _params, fallback) => fallback ?? key }
  }, React.createElement(loadedWorkspace.exports.ConfigurationWorkspace, {
    details: {
      summary: { id: "windrose-semantic", module_id: "windrose", name: "Harbor", status: "stopped", bind_ip: "127.0.0.1" },
      settings_json: JSON.stringify({ ...defaults, world_island_id: "island-one" }),
      auto_backup_on_stop: false, backup_retention_count: 3
    },
    moduleDetails: { summary: { id: "windrose", name: "Windrose" }, schema_json: JSON.stringify(windroseSchema) },
    moduleDetailsError: null,
    onRetryModuleDetails() {},
    bindAddressCandidates: [],
    launchPlanError: null,
    onSave() {}
  })));
  const fieldKeys = [...html.matchAll(/data-field-key="([^"]+)"/g)].map((match) => match[1]);
  assert.deepEqual(fieldKeys.sort(), [...roomInventories.windrose].sort());
  const ids = [...html.matchAll(/\bid="([^"]+)"/g)].map((match) => match[1]);
  assert.equal(ids.length, new Set(ids).size, "all room DOM IDs are unique");
  assert.equal((html.match(/<input[^>]*id="configuration-windrose-world-name-input"/g) ?? []).length, 1);
  assert.doesNotMatch(html, /A specialized editor is unavailable/);
});

test("Astroneer, Palworld, HumanitZ, V Rising, Core Keeper, Terraria and Romestead preserve verified native semantics", () => {
  for (const locale of ["en-US", "zh-CN"]) {
    const astroneer = schemaFor("astroneer", locale);
    const astroneerField = (key) => astroneer.fields.find((item) => item.key === key);
    assert.match(astroneerField("wait_for_players_before_shutdown").description, /ExitSemaphore/);
    assert.match(astroneerField("disable_server_travel").description, /ServerTravel/);
    assert.match(astroneerField("player_activity_timeout_seconds").description, locale === "zh-CN" ? /0.*关闭/ : /0.*disable/);
    assert.deepEqual(validateGuidedSettingsObject(astroneer, { player_activity_timeout_seconds: 1 })
      .filter((issue) => issue.fieldKey === "player_activity_timeout_seconds"), []);

    const palworld = schemaFor("palworld", locale);
    assert.match(palworld.fields.find((item) => item.key === "max_building_limit_num").title, locale === "zh-CN" ? /基地/ : /Base/);
    assert.match(palworld.fields.find((item) => item.key === "max_building_limit_num_per_player").title, locale === "zh-CN" ? /玩家/ : /Player/);
    const boundary = palworld.fields.find((item) => item.key === "invisible_other_guild_base_camp_area_fx");
    assert.match(boundary.title, locale === "zh-CN" ? /隐藏其他公会/ : /Hide Other Guild/);
    assert.match(boundary.description, locale === "zh-CN" ? /不隐藏.*建筑.*地图标记/ : /does not hide.*structures.*map markers/);
    for (const retiredKey of ["coop_player_max_num", "is_multiplay", "difficulty", "enable_defense_other_guild_player", "enable_non_login_penalty"]) {
      assert.equal(palworld.fields.some((item) => item.key === retiredKey), false, retiredKey);
    }

    const humanitz = schemaFor("humanitz", locale);
    assert.match(humanitz.fields.find((item) => item.key === "map_segment_2").description, /60.*60/);
    for (const retiredKey of ["only_allowed_players", "clear_infection_on_respawn", "human_difficulty", "loot_rarity", "eagle_eye_enabled"]) {
      assert.equal(humanitz.fields.some((item) => item.key === retiredKey), false, retiredKey);
    }
  }

  const vrisingProps = readSchema("vrising").properties;
  const vrisingDef = resolveSettingsModuleDefinition("vrising");
  const vrisingCases = {
    war_event_interval: [0, ["30 minutes", "1 hour", "1.5 hours", "2 hours", "4 hours", "8 hours", "12 hours", "24 hours"], ["30 分钟", "1 小时", "1.5 小时", "2 小时", "4 小时", "8 小时", "12 小时", "24 小时"]],
    war_event_major_duration: [1, ["15 minutes", "20 minutes", "25 minutes", "30 minutes", "35 minutes", "45 minutes", "1 hour", "2 hours"], ["15 分钟", "20 分钟", "25 分钟", "30 分钟", "35 分钟", "45 分钟", "1 小时", "2 小时"]],
    war_event_minor_duration: [1, ["15 minutes", "20 minutes", "25 minutes", "30 minutes", "35 minutes", "45 minutes", "1 hour", "2 hours"], ["15 分钟", "20 分钟", "25 分钟", "30 分钟", "35 分钟", "45 分钟", "1 小时", "2 小时"]]
  };
  for (const [key, [defaultValue, enLabels, zhLabels]] of Object.entries(vrisingCases)) {
    assert.deepEqual(vrisingProps[key].enum, [0, 1, 2, 3, 4, 5, 6, 7]);
    assert.equal(vrisingProps[key].default, defaultValue);
    for (const [locale, catalog, expected] of [["en-US", EN_US_MESSAGES, enLabels], ["zh-CN", ZH_CN_MESSAGES, zhLabels]]) {
      const translate = (messageKey, _params, fallback = messageKey) => catalog[messageKey] ?? fallback;
      assert.deepEqual(vrisingProps[key].enum.map((value) => vrisingDef.getEnumOptionLabel(key, value, locale, translate)), expected);
    }
  }

  assert.match(EN_US_MESSAGES["settings.schema.corekeeper.max_packets_per_frame.description"], /1,200[\s\S]*9,440/);
  assert.match(ZH_CN_MESSAGES["settings.schema.corekeeper.max_packets_per_frame.description"], /1,200[\s\S]*9,440/);
  assert.doesNotMatch(EN_US_MESSAGES["terraria.settings.validation.npcstreamNonNegative"], /radius/i);
  assert.doesNotMatch(ZH_CN_MESSAGES["terraria.settings.validation.npcstreamNonNegative"], /半径/);
  assert.match(EN_US_MESSAGES["settings.schema.romestead.enable_cheats.description"], /spawn[\s\S]*weather/);
  assert.match(ZH_CN_MESSAGES["settings.schema.romestead.enable_cheats.description"], /生成[\s\S]*天气/);
});

test("Conan Exiles and SCUM preserve fractional validation, enum modes, localized sections and native units", () => {
  const conanEn = schemaFor("conanexiles", "en-US");
  const conanZh = schemaFor("conanexiles", "zh-CN");
  const fractionalValues = {
    pvp_build_from_storage_radius: 1.25,
    player_stamina_regen_speed_scale: 1.25,
    action_stamina_cost_multiplier: 1.25,
    unconscious_time_seconds: 1800.5,
    player_offline_hunger_multiplier: 1.25,
    player_water_multiplier: 1.25,
    shield_durability_multiplier: 1.25
  };
  assert.deepEqual(validateGuidedSettingsObject(conanEn, fractionalValues).filter((issue) => issue.fieldKey in fractionalValues), []);

  for (const [settings, labels] of [
    [conanEn, ["Keep all items", "Drop all items", "Drop backpack only"]],
    [conanZh, ["保留全部物品", "掉落全部物品", "仅掉落背包物品"]]
  ]) {
    const field = settings.fields.find((entry) => entry.key === "drop_equipment_on_death");
    assert.equal(field.sectionId, "survival");
    assert.equal(field.control, "select");
    assert.deepEqual(field.enumOptions.map((option) => option.value), [0, 1, 2]);
    assert.deepEqual(field.enumOptions.map((option) => option.label), labels);
    for (const value of [0, 1, 2]) {
      assert.deepEqual(validateGuidedSettingsObject(settings, { drop_equipment_on_death: value }).filter((issue) => issue.fieldKey === field.key), []);
    }
    for (const value of [3, -1, 1.5, "DropEverything", "1"]) {
      assert.ok(validateGuidedSettingsObject(settings, { drop_equipment_on_death: value }).some((issue) => issue.fieldKey === field.key));
    }
  }

  for (const value of ["()", '("1")', '("1", "2")']) {
    assert.deepEqual(validateGuidedSettingsObject(conanEn, { disabled_knowledge_ids: value }).filter((issue) => issue.fieldKey === "disabled_knowledge_ids"), []);
  }
  for (const value of ["1,2", '("abc")', '("1")\nInjected=True']) {
    assert.ok(validateGuidedSettingsObject(conanEn, { disabled_knowledge_ids: value }).some((issue) => issue.fieldKey === "disabled_knowledge_ids"));
  }

  const conanSections = new Map(conanZh.sections.map((section) => [section.id, section]));
  for (const [id, title] of Object.entries({
    pvp_schedule: "PvP 与神祇召唤时段",
    survival: "生存与耐力",
    building: "建筑与衰退",
    followers: "随从与奴隶",
    purge: "大清洗（Purge）",
    advanced: "高级设置"
  })) {
    assert.equal(conanSections.get(id)?.title, title, id);
    assert.ok(conanSections.get(id)?.description, id);
  }

  const scumEn = buildScumNativeMessageCatalog("en-US");
  const scumZh = buildScumNativeMessageCatalog("zh-CN");
  for (const key of ["item_virtualization_visitor_distance_travelled_for_update", "item_virtualization_visitor_bounds", "virtualized_item_bounds"]) {
    assert.match(scumEn[`scum.settings.native.${key}.title`], /cm/);
    assert.match(scumZh[`scum.settings.native.${key}.title`], /厘米/);
  }
  const vehicleKeys = Object.keys(scumEn).filter((key) => key.endsWith("_min_purchased_amount.title"));
  assert.equal(vehicleKeys.length, 15);
  for (const key of vehicleKeys) {
    assert.match(scumEn[key], /available to buy/);
    assert.match(scumZh[key], /最低可购数量/);
  }
});
